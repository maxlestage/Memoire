//! PostgreSQL backend (feature `postgres`, requires a Tokio runtime).
//!
//! Built on `tokio-postgres` (a pure Rust implementation of the PostgreSQL wire
//! protocol) and the `deadpool` connection pool. Every statement is prepared once
//! per connection and cached; primary-key CRUD uses SQL generated at compile time.

use std::error::Error as StdError;
use std::fmt;

use bytes::{BufMut, BytesMut};
use deadpool_postgres::{ClientWrapper, Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use tokio_postgres::error::SqlState;
use tokio_postgres::types::{FromSql, IsNull, Kind, ToSql, Type, to_sql_checked};
use tokio_postgres::{NoTls, Row};

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::{FromRow, RowSource};
use crate::schema::Dialect;
use crate::sql;
use crate::value::Value;

type BoxError = Box<dyn StdError + Sync + Send>;

/// Statements cached per connection before the cache is reset (protects
/// long-running processes that build many distinct dynamic queries).
const STATEMENT_CACHE_LIMIT: usize = 1024;

fn map_err(err: tokio_postgres::Error) -> Error {
    if let Some(db) = err.as_db_error() {
        let code = db.code();
        if *code == SqlState::UNIQUE_VIOLATION {
            // detail: `Key (email)=(a@b.c) already exists.`
            let column = db
                .detail()
                .and_then(|d| d.strip_prefix("Key ("))
                .and_then(|d| d.split_once(")="))
                .map(|(c, _)| c.to_owned())
                .or_else(|| db.constraint().map(str::to_owned))
                .unwrap_or_default();
            return Error::UniqueViolation { table: db.table().unwrap_or_default().to_owned(), column };
        }
        if *code == SqlState::NOT_NULL_VIOLATION
            || *code == SqlState::FOREIGN_KEY_VIOLATION
            || *code == SqlState::CHECK_VIOLATION
        {
            return Error::Constraint(db.message().to_owned());
        }
        if *code == SqlState::UNDEFINED_TABLE {
            let name = db.message().split('"').nth(1).unwrap_or_default();
            return Error::NoSuchTable(name.to_owned());
        }
        if *code == SqlState::DUPLICATE_TABLE {
            let name = db.message().split('"').nth(1).unwrap_or_default();
            return Error::TableExists(name.to_owned());
        }
        if *code == SqlState::T_R_SERIALIZATION_FAILURE || *code == SqlState::T_R_DEADLOCK_DETECTED {
            return Error::Conflict(db.table().unwrap_or_default().to_owned());
        }
    }
    Error::Database(Box::new(err))
}

fn pool_err(err: impl fmt::Display) -> Error {
    Error::Pool(err.to_string())
}

// ---------------------------------------------------------------------------
// NUMERIC binary format
// ---------------------------------------------------------------------------

const NUMERIC_NEG: u16 = 0x4000;
const NUMERIC_NAN: u16 = 0xC000;
const NUMERIC_PINF: u16 = 0xD000;
const NUMERIC_NINF: u16 = 0xF000;

fn decode_numeric(raw: &[u8]) -> std::result::Result<String, BoxError> {
    if raw.len() < 8 {
        return Err("invalid NUMERIC value".into());
    }
    let rd = |i: usize| u16::from_be_bytes([raw[i], raw[i + 1]]);
    let ndigits = rd(0) as usize;
    let weight = rd(2) as i16 as i32;
    let sign = rd(4);
    let dscale = rd(6) as usize;
    match sign {
        NUMERIC_NAN => return Ok("NaN".into()),
        NUMERIC_PINF => return Ok("Infinity".into()),
        NUMERIC_NINF => return Ok("-Infinity".into()),
        _ => {}
    }
    if raw.len() < 8 + ndigits * 2 {
        return Err("invalid NUMERIC value".into());
    }
    let digit = |i: i32| -> u16 { if i < 0 || i as usize >= ndigits { 0 } else { rd(8 + i as usize * 2) } };
    let mut s = String::new();
    if sign == NUMERIC_NEG {
        s.push('-');
    }
    if weight < 0 {
        s.push('0');
    } else {
        for i in 0..=weight {
            let d = digit(i);
            if i == 0 {
                s.push_str(&d.to_string());
            } else {
                s.push_str(&format!("{d:04}"));
            }
        }
    }
    if dscale > 0 {
        s.push('.');
        let mut frac = String::new();
        let groups = dscale.div_ceil(4) as i32;
        for k in 0..groups {
            frac.push_str(&format!("{:04}", digit(weight + 1 + k)));
        }
        frac.truncate(dscale);
        s.push_str(&frac);
    }
    Ok(s)
}

fn encode_numeric(text: &str, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
    let t = text.trim();
    let header = |out: &mut BytesMut, nd: u16, w: i16, sign: u16, scale: u16| {
        out.put_u16(nd);
        out.put_i16(w);
        out.put_u16(sign);
        out.put_u16(scale);
    };
    match t {
        "NaN" => {
            header(out, 0, 0, NUMERIC_NAN, 0);
            return Ok(IsNull::No);
        }
        "Infinity" | "inf" => {
            header(out, 0, 0, NUMERIC_PINF, 0);
            return Ok(IsNull::No);
        }
        "-Infinity" | "-inf" => {
            header(out, 0, 0, NUMERIC_NINF, 0);
            return Ok(IsNull::No);
        }
        _ => {}
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (int_part, frac_part) = body.split_once('.').unwrap_or((body, ""));
    if int_part.is_empty() && frac_part.is_empty()
        || !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("`{text}` is not a valid NUMERIC").into());
    }
    let int_part = int_part.trim_start_matches('0');
    let dscale = frac_part.len();
    // Pad to whole base-10000 groups.
    let int_pad = (4 - int_part.len() % 4) % 4;
    let frac_pad = (4 - frac_part.len() % 4) % 4;
    let digits_str = format!("{}{}{}{}", "0".repeat(int_pad), int_part, frac_part, "0".repeat(frac_pad));
    let mut groups: Vec<u16> =
        digits_str.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or("0").parse().unwrap_or(0)).collect();
    let mut weight = ((int_part.len() + int_pad) / 4) as i32 - 1;
    while groups.first() == Some(&0) {
        groups.remove(0);
        weight -= 1;
    }
    while groups.last() == Some(&0) {
        groups.pop();
    }
    if groups.is_empty() {
        weight = 0;
    }
    header(
        out,
        groups.len() as u16,
        weight as i16,
        if neg && !groups.is_empty() { NUMERIC_NEG } else { 0 },
        dscale as u16,
    );
    for g in groups {
        out.put_u16(g);
    }
    Ok(IsNull::No)
}

// ---------------------------------------------------------------------------
// Value <-> PostgreSQL
// ---------------------------------------------------------------------------

fn is_text_like(ty: &Type) -> bool {
    matches!(*ty, Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN)
        || matches!(ty.kind(), Kind::Enum(_))
        || matches!(ty.name(), "citext" | "ltree" | "lquery")
}

fn int_to_sql(v: i64, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
    match *ty {
        Type::INT2 => i16::try_from(v).map_err(|_| format!("{v} does not fit in SMALLINT"))?.to_sql(ty, out),
        Type::INT4 => i32::try_from(v).map_err(|_| format!("{v} does not fit in INTEGER"))?.to_sql(ty, out),
        Type::INT8 => v.to_sql(ty, out),
        Type::OID => u32::try_from(v).map_err(|_| format!("{v} does not fit in OID"))?.to_sql(ty, out),
        Type::FLOAT4 => (v as f32).to_sql(ty, out),
        Type::FLOAT8 => (v as f64).to_sql(ty, out),
        Type::NUMERIC => encode_numeric(&v.to_string(), out),
        Type::BOOL => (v != 0).to_sql(ty, out),
        _ if is_text_like(ty) => v.to_string().as_str().to_sql(&Type::TEXT, out),
        _ => Err(format!("cannot bind an integer to a {ty} parameter").into()),
    }
}

impl ToSql for Value {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
        match self {
            Value::Null => Ok(IsNull::Yes),
            Value::Bool(b) => match *ty {
                Type::BOOL => b.to_sql(ty, out),
                _ => int_to_sql(*b as i64, ty, out),
            },
            Value::I32(v) => int_to_sql(*v as i64, ty, out),
            Value::I64(v) => int_to_sql(*v, ty, out),
            Value::F64(v) => match *ty {
                Type::FLOAT8 => v.to_sql(ty, out),
                Type::FLOAT4 => (*v as f32).to_sql(ty, out),
                Type::NUMERIC => encode_numeric(&v.to_string(), out),
                Type::INT2 | Type::INT4 | Type::INT8 if v.fract() == 0.0 => int_to_sql(*v as i64, ty, out),
                _ if is_text_like(ty) => v.to_string().as_str().to_sql(&Type::TEXT, out),
                _ => Err(format!("cannot bind a float to a {ty} parameter").into()),
            },
            Value::Text(s) => match *ty {
                Type::NUMERIC => encode_numeric(s, out),
                Type::BYTEA => s.as_bytes().to_sql(ty, out),
                Type::JSON | Type::JSONB => {
                    if *ty == Type::JSONB {
                        out.put_u8(1);
                    }
                    out.extend_from_slice(s.as_bytes());
                    Ok(IsNull::No)
                }
                #[cfg(feature = "uuid")]
                Type::UUID => uuid::Uuid::parse_str(s)?.to_sql(ty, out),
                _ if is_text_like(ty) => {
                    out.extend_from_slice(s.as_bytes());
                    Ok(IsNull::No)
                }
                _ => Err(format!("cannot bind text to a {ty} parameter").into()),
            },
            Value::Bytes(b) => b.as_slice().to_sql(ty, out),
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => {
                if is_text_like(ty) {
                    u.to_string().as_str().to_sql(&Type::TEXT, out)
                } else {
                    u.to_sql(ty, out)
                }
            }
            #[cfg(feature = "chrono")]
            Value::Timestamp(t) => match *ty {
                Type::TIMESTAMPTZ => t.and_utc().to_sql(ty, out),
                Type::DATE => t.date().to_sql(ty, out),
                _ => t.to_sql(ty, out),
            },
            #[cfg(feature = "chrono")]
            Value::TimestampTz(t) => match *ty {
                Type::TIMESTAMP => t.naive_utc().to_sql(ty, out),
                Type::DATE => t.date_naive().to_sql(ty, out),
                _ => t.to_sql(ty, out),
            },
            #[cfg(feature = "chrono")]
            Value::Date(d) => d.to_sql(ty, out),
            #[cfg(feature = "chrono")]
            Value::Time(t) => t.to_sql(ty, out),
            #[cfg(feature = "json")]
            Value::Json(j) => {
                if is_text_like(ty) {
                    j.to_string().as_str().to_sql(&Type::TEXT, out)
                } else {
                    j.to_sql(ty, out)
                }
            }
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for Value {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        Ok(match *ty {
            Type::BOOL => Value::Bool(bool::from_sql(ty, raw)?),
            Type::INT2 => Value::I32(i16::from_sql(ty, raw)? as i32),
            Type::INT4 => Value::I32(i32::from_sql(ty, raw)?),
            Type::INT8 => Value::I64(i64::from_sql(ty, raw)?),
            Type::OID => Value::I64(u32::from_sql(ty, raw)? as i64),
            Type::FLOAT4 => Value::F64(f32::from_sql(ty, raw)? as f64),
            Type::FLOAT8 => Value::F64(f64::from_sql(ty, raw)?),
            Type::NUMERIC => Value::Text(decode_numeric(raw)?),
            Type::BYTEA => Value::Bytes(raw.to_vec()),
            #[cfg(feature = "uuid")]
            Type::UUID => Value::Uuid(uuid::Uuid::from_sql(ty, raw)?),
            #[cfg(not(feature = "uuid"))]
            Type::UUID => Value::Text(
                raw.iter()
                    .enumerate()
                    .map(|(i, b)| if matches!(i, 4 | 6 | 8 | 10) { format!("-{b:02x}") } else { format!("{b:02x}") })
                    .collect(),
            ),
            #[cfg(feature = "chrono")]
            Type::TIMESTAMP => Value::Timestamp(chrono::NaiveDateTime::from_sql(ty, raw)?),
            #[cfg(feature = "chrono")]
            Type::TIMESTAMPTZ => Value::TimestampTz(chrono::DateTime::<chrono::Utc>::from_sql(ty, raw)?),
            #[cfg(feature = "chrono")]
            Type::DATE => Value::Date(chrono::NaiveDate::from_sql(ty, raw)?),
            #[cfg(feature = "chrono")]
            Type::TIME => Value::Time(chrono::NaiveTime::from_sql(ty, raw)?),
            #[cfg(feature = "json")]
            Type::JSON | Type::JSONB => Value::Json(serde_json::Value::from_sql(ty, raw)?),
            #[cfg(not(feature = "json"))]
            Type::JSON => Value::Text(std::str::from_utf8(raw)?.to_owned()),
            #[cfg(not(feature = "json"))]
            Type::JSONB => Value::Text(std::str::from_utf8(raw.get(1..).unwrap_or_default())?.to_owned()),
            _ if is_text_like(ty) => Value::Text(std::str::from_utf8(raw)?.to_owned()),
            _ => {
                return Err(format!(
                    "unsupported PostgreSQL type `{ty}` (enable the `chrono`, `uuid` or `json` feature, or cast it in SQL)"
                )
                .into());
            }
        })
    }

    fn from_sql_null(_: &Type) -> std::result::Result<Self, BoxError> {
        Ok(Value::Null)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

/// Probe used to test for NULL without decoding.
struct NullProbe(bool);

impl<'a> FromSql<'a> for NullProbe {
    fn from_sql(_: &Type, _: &'a [u8]) -> std::result::Result<Self, BoxError> {
        Ok(NullProbe(false))
    }
    fn from_sql_null(_: &Type) -> std::result::Result<Self, BoxError> {
        Ok(NullProbe(true))
    }
    fn accepts(_: &Type) -> bool {
        true
    }
}

struct PgRow<'a>(&'a Row);

impl RowSource for PgRow<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn take(&mut self, index: usize) -> Result<Value> {
        self.0.try_get::<_, Value>(index).map_err(|e| Error::Conversion(e.to_string()))
    }

    fn is_null(&self, index: usize) -> bool {
        self.0.try_get::<_, NullProbe>(index).map(|p| p.0).unwrap_or(true)
    }
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

async fn prepare(client: &ClientWrapper, sql: &str) -> Result<tokio_postgres::Statement> {
    if client.statement_cache.size() > STATEMENT_CACHE_LIMIT {
        client.statement_cache.clear();
    }
    client.prepare_cached(sql).await.map_err(map_err)
}

fn as_params<'a>(params: &'a [&'a Value]) -> Vec<&'a (dyn ToSql + Sync)> {
    params.iter().map(|v| *v as &(dyn ToSql + Sync)).collect()
}

async fn pg_fetch<O: FromRow>(client: &ClientWrapper, query: &Query<'_>) -> Result<Vec<O>> {
    let r = sql::render(query, Dialect::Postgres)?;
    let stmt = prepare(client, &r.sql).await?;
    let rows = client.query(&stmt, &as_params(&r.params)).await.map_err(map_err)?;
    rows.iter().map(|row| O::from_row(&mut PgRow(row), 0)).collect()
}

async fn pg_execute(client: &ClientWrapper, query: &Query<'_>) -> Result<ExecResult> {
    match query {
        Query::CreateTable { .. } | Query::DropTable { .. } => {
            let stmts = sql::ddl(query, Dialect::Postgres)?;
            client.batch_execute(&stmts.join(";\n")).await.map_err(map_err)?;
            Ok(ExecResult::default())
        }
        Query::Script { sql } => {
            client.batch_execute(sql).await.map_err(map_err)?;
            Ok(ExecResult::default())
        }
        Query::InsertRow { table, .. } if table.auto_increment() => {
            let r = sql::render(query, Dialect::Postgres)?;
            let stmt = prepare(client, &r.sql).await?;
            let row = client.query_one(&stmt, &as_params(&r.params)).await.map_err(map_err)?;
            let id = PgRow(&row).take(0)?;
            Ok(ExecResult { rows_affected: 1, last_insert_id: Some(id) })
        }
        _ => {
            let r = sql::render(query, Dialect::Postgres)?;
            let stmt = prepare(client, &r.sql).await?;
            let n = client.execute(&stmt, &as_params(&r.params)).await.map_err(map_err)?;
            Ok(ExecResult { rows_affected: n, last_insert_id: None })
        }
    }
}

/// A pool of PostgreSQL connections.
///
/// ```no_run
/// # async fn demo() -> memoire_orm::Result<()> {
/// use memoire_orm::prelude::*;
/// let db = PgPool::connect("postgres://user:password@localhost/app").await?;
/// # Ok(()) }
/// ```
///
/// Connections are opened lazily and recycled. Cloning the pool is cheap.
/// TLS is not built in: for managed databases that require it, build a
/// `deadpool_postgres::Pool` with the TLS connector of your choice and wrap it
/// with [`PgPool::from_pool`].
#[derive(Clone)]
pub struct PgPool {
    pool: Pool,
}

impl fmt::Debug for PgPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.pool.status();
        f.debug_struct("PgPool").field("size", &s.size).field("available", &s.available).finish()
    }
}

impl PgPool {
    /// Connects with a `postgres://` URL (or a `key=value` connection string)
    /// and a pool of up to `4 × CPU cores` connections. One connection is
    /// opened immediately to validate the settings.
    pub async fn connect(url: &str) -> Result<Self> {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        Self::connect_with(url, cores * 4).await
    }

    /// Like [`connect`](Self::connect) with an explicit maximum pool size.
    pub async fn connect_with(url: &str, max_size: usize) -> Result<Self> {
        let cfg: tokio_postgres::Config = url.parse().map_err(map_err)?;
        let manager = Manager::from_config(cfg, NoTls, ManagerConfig { recycling_method: RecyclingMethod::Fast });
        let pool = Pool::builder(manager).max_size(max_size.max(1)).build().map_err(pool_err)?;
        drop(pool.get().await.map_err(pool_err)?);
        Ok(PgPool { pool })
    }

    /// Wraps an existing `deadpool_postgres::Pool` (custom TLS, timeouts...).
    pub fn from_pool(pool: Pool) -> Self {
        PgPool { pool }
    }

    /// The underlying pool.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

impl Executor for PgPool {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::Postgres)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let client = self.pool.get().await.map_err(pool_err)?;
        pg_fetch(&client, &query).await
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let client = self.pool.get().await.map_err(pool_err)?;
        pg_execute(&client, &query).await
    }
}

impl Database for PgPool {
    type Tx<'c> = PgTransaction;

    async fn begin(&self) -> Result<PgTransaction> {
        let client = self.pool.get().await.map_err(pool_err)?;
        client.batch_execute("BEGIN").await.map_err(map_err)?;
        Ok(PgTransaction { client: Some(client) })
    }
}

/// A PostgreSQL transaction holding one pooled connection.
///
/// Dropping it without committing rolls it back (asynchronously, on the Tokio
/// runtime; if no runtime is available the connection is closed instead, which
/// also aborts the transaction).
pub struct PgTransaction {
    client: Option<Object>,
}

impl fmt::Debug for PgTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgTransaction").field("open", &self.client.is_some()).finish()
    }
}

impl PgTransaction {
    fn client(&self) -> Result<&ClientWrapper> {
        self.client.as_deref().ok_or(Error::TransactionFinished)
    }

    async fn finish(mut self, sql: &str) -> Result<()> {
        let client = self.client.take().ok_or(Error::TransactionFinished)?;
        match client.batch_execute(sql).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Never return a connection in an unknown transaction state.
                let _ = Object::take(client);
                Err(map_err(e))
            }
        }
    }
}

impl Drop for PgTransaction {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(async move {
                        if client.batch_execute("ROLLBACK").await.is_err() {
                            let _ = Object::take(client);
                        }
                    });
                }
                Err(_) => {
                    let _ = Object::take(client);
                }
            }
        }
    }
}

impl Executor for PgTransaction {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::Postgres)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        pg_fetch(self.client()?, &query).await
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        pg_execute(self.client()?, &query).await
    }
}

impl Transaction for PgTransaction {
    fn commit(self) -> impl Future<Output = Result<()>> + Send {
        self.finish("COMMIT")
    }

    fn rollback(self) -> impl Future<Output = Result<()>> + Send {
        self.finish("ROLLBACK")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(s: &str) -> String {
        let mut buf = BytesMut::new();
        encode_numeric(s, &mut buf).unwrap();
        decode_numeric(&buf).unwrap()
    }

    #[test]
    fn numeric_codec() {
        assert_eq!(roundtrip("0"), "0");
        assert_eq!(roundtrip("12345678"), "12345678");
        assert_eq!(roundtrip("-42.5"), "-42.5");
        assert_eq!(roundtrip("0.0001"), "0.0001");
        assert_eq!(roundtrip("10000.00"), "10000.00");
        assert_eq!(roundtrip("123456789.123456789"), "123456789.123456789");
        assert!(encode_numeric("abc", &mut BytesMut::new()).is_err());
    }
}
