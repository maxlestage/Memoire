//! MySQL and MariaDB backend (feature `mysql`, requires a Tokio runtime).
//!
//! Built on `mysql_async`, a pure Rust implementation of the MySQL protocol, with
//! its connection pool and per-connection prepared statement cache. Primary-key
//! CRUD uses the SQL generated at compile time by `#[derive(Model)]`.
//!
//! Tables created by Memoire use `utf8mb4` with the binary collation
//! (`utf8mb4_bin`), so comparisons, `LIKE` and sorting are case-sensitive and
//! behave exactly like on the other backends.

use std::fmt;

use async_lock::Mutex;
use mysql_async::consts::ColumnType as MyType;
use mysql_async::prelude::Queryable;
use mysql_async::{Column, Opts, OptsBuilder, Params, Pool, PoolOpts, Row, TxOpts, Value as MyValue};

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::{FromRow, RowSource};
use crate::schema::{Dialect, TableSchema};
use crate::sql;
use crate::value::Value;

const BINARY_CHARSET: u16 = 63;

fn query_table(query: &Query<'_>) -> Option<&'static TableSchema> {
    match *query {
        Query::Select(s) => Some(s.from),
        Query::Insert(i) => Some(i.table),
        Query::Update(u) => Some(u.table),
        Query::Delete(d) => Some(d.table),
        Query::FindByKey { table, .. }
        | Query::InsertRow { table, .. }
        | Query::UpsertRow { table, .. }
        | Query::UpdateRow { table, .. }
        | Query::DeleteByKey { table, .. }
        | Query::CreateTable { table, .. } => Some(table),
        _ => None,
    }
}

/// Text between the first and the last single quote after `marker`.
fn quoted_after<'m>(msg: &'m str, marker: &str) -> Option<&'m str> {
    let rest = &msg[msg.find(marker)? + marker.len()..];
    let start = rest.find('\'')? + 1;
    let end = start + rest[start..].find('\'')?;
    Some(&rest[start..end])
}

fn map_err(err: mysql_async::Error, table: Option<&'static TableSchema>) -> Error {
    if let mysql_async::Error::Server(e) = &err {
        match e.code {
            // ER_DUP_ENTRY: "Duplicate entry 'x' for key 'users.email'" (MySQL 8)
            // or "... for key 'email'" (MariaDB).
            1062 => {
                let key = quoted_after(&e.message, "for key").unwrap_or_default();
                let key = key.rsplit('.').next().unwrap_or(key);
                let column = match (key, table) {
                    ("PRIMARY", Some(t)) => t.pk().name.to_owned(),
                    (k, _) => k.to_owned(),
                };
                return Error::UniqueViolation { table: table.map(|t| t.name.to_owned()).unwrap_or_default(), column };
            }
            // ER_NO_SUCH_TABLE: "Table 'db.users' doesn't exist"
            1146 => {
                let name = quoted_after(&e.message, "Table").unwrap_or_default();
                return Error::NoSuchTable(name.rsplit('.').next().unwrap_or(name).to_owned());
            }
            // ER_BAD_TABLE_ERROR (DROP of a missing table): "Unknown table 'db.users'"
            1051 => {
                let name = quoted_after(&e.message, "table").unwrap_or_default();
                return Error::NoSuchTable(name.rsplit('.').next().unwrap_or(name).to_owned());
            }
            // ER_TABLE_EXISTS_ERROR: "Table 'users' already exists"
            1050 => {
                let name = quoted_after(&e.message, "Table").unwrap_or_default();
                return Error::TableExists(name.rsplit('.').next().unwrap_or(name).to_owned());
            }
            // NOT NULL, foreign keys, CHECK (MySQL 3819, MariaDB 4025).
            1048 | 1216 | 1217 | 1451 | 1452 | 3819 | 4025 => return Error::Constraint(e.message.clone()),
            // Deadlock / lock wait timeout: retrying the transaction is safe.
            1205 | 1213 => return Error::Conflict(table.map(|t| t.name.to_owned()).unwrap_or_default()),
            _ => {}
        }
    }
    Error::Database(Box::new(err))
}

fn to_mysql(v: &Value) -> MyValue {
    match v {
        Value::Null => MyValue::NULL,
        Value::Bool(b) => MyValue::Int(*b as i64),
        Value::I32(i) => MyValue::Int(*i as i64),
        Value::I64(i) => MyValue::Int(*i),
        Value::F64(f) => MyValue::Double(*f),
        Value::Text(s) => MyValue::Bytes(s.as_bytes().to_vec()),
        Value::Bytes(b) => MyValue::Bytes(b.clone()),
        #[cfg(feature = "uuid")]
        Value::Uuid(u) => MyValue::Bytes(u.hyphenated().to_string().into_bytes()),
        #[cfg(feature = "chrono")]
        Value::Timestamp(t) => datetime(*t),
        #[cfg(feature = "chrono")]
        Value::TimestampTz(t) => datetime(t.naive_utc()),
        #[cfg(feature = "chrono")]
        Value::Date(d) => {
            use chrono::Datelike;
            MyValue::Date(d.year() as u16, d.month() as u8, d.day() as u8, 0, 0, 0, 0)
        }
        #[cfg(feature = "chrono")]
        Value::Time(t) => {
            use chrono::Timelike;
            MyValue::Time(false, 0, t.hour() as u8, t.minute() as u8, t.second() as u8, t.nanosecond() / 1000)
        }
        #[cfg(feature = "json")]
        Value::Json(j) => MyValue::Bytes(j.to_string().into_bytes()),
    }
}

#[cfg(feature = "chrono")]
fn datetime(t: chrono::NaiveDateTime) -> MyValue {
    use chrono::{Datelike, Timelike};
    MyValue::Date(
        t.year() as u16,
        t.month() as u8,
        t.day() as u8,
        t.hour() as u8,
        t.minute() as u8,
        t.second() as u8,
        t.nanosecond() / 1000,
    )
}

fn params(values: &[&Value]) -> Params {
    if values.is_empty() { Params::Empty } else { Params::Positional(values.iter().map(|v| to_mysql(v)).collect()) }
}

fn from_mysql(v: MyValue, col: &Column) -> Result<Value> {
    let text = |b: Vec<u8>| {
        String::from_utf8(b).map(Value::Text).map_err(|e| Error::Conversion(format!("invalid UTF-8 text: {e}")))
    };
    Ok(match v {
        MyValue::NULL => Value::Null,
        MyValue::Int(i) => Value::I64(i),
        MyValue::UInt(u) => i64::try_from(u).map(Value::I64).unwrap_or_else(|_| Value::Text(u.to_string())),
        MyValue::Float(f) => Value::F64(f as f64),
        MyValue::Double(d) => Value::F64(d),
        MyValue::Bytes(b) => match col.column_type() {
            MyType::MYSQL_TYPE_DECIMAL | MyType::MYSQL_TYPE_NEWDECIMAL => text(b)?,
            #[cfg(feature = "json")]
            MyType::MYSQL_TYPE_JSON => Value::Json(serde_json::from_slice(&b).map_err(Error::conversion)?),
            MyType::MYSQL_TYPE_TINY_BLOB
            | MyType::MYSQL_TYPE_MEDIUM_BLOB
            | MyType::MYSQL_TYPE_LONG_BLOB
            | MyType::MYSQL_TYPE_BLOB
            | MyType::MYSQL_TYPE_VAR_STRING
            | MyType::MYSQL_TYPE_STRING
            | MyType::MYSQL_TYPE_VARCHAR
                if col.character_set() == BINARY_CHARSET =>
            {
                Value::Bytes(b)
            }
            _ => text(b)?,
        },
        MyValue::Date(y, mo, d, h, mi, s, us) => date_value(col, y, mo, d, h, mi, s, us),
        MyValue::Time(neg, days, h, mi, s, us) => time_value(neg, days, h, mi, s, us),
    })
}

#[allow(clippy::too_many_arguments)]
fn date_value(col: &Column, y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8, us: u32) -> Value {
    #[cfg(feature = "chrono")]
    {
        if let Some(date) = chrono::NaiveDate::from_ymd_opt(y as i32, mo as u32, d as u32) {
            if col.column_type() == MyType::MYSQL_TYPE_DATE {
                return Value::Date(date);
            }
            if let Some(t) = date.and_hms_micro_opt(h as u32, mi as u32, s as u32, us) {
                return Value::Timestamp(t);
            }
        }
    }
    if col.column_type() == MyType::MYSQL_TYPE_DATE {
        Value::Text(format!("{y:04}-{mo:02}-{d:02}"))
    } else {
        Value::Text(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}"))
    }
}

fn time_value(neg: bool, days: u32, h: u8, mi: u8, s: u8, us: u32) -> Value {
    #[cfg(feature = "chrono")]
    if !neg
        && days == 0
        && let Some(t) = chrono::NaiveTime::from_hms_micro_opt(h as u32, mi as u32, s as u32, us)
    {
        return Value::Time(t);
    }
    let hours = days * 24 + h as u32;
    Value::Text(format!("{}{hours:02}:{mi:02}:{s:02}.{us:06}", if neg { "-" } else { "" }))
}

struct MyRow(Row);

impl RowSource for MyRow {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn take(&mut self, index: usize) -> Result<Value> {
        let col = self
            .0
            .columns_ref()
            .get(index)
            .cloned()
            .ok_or_else(|| Error::Conversion(format!("column index {index} out of range")))?;
        let v: MyValue = self.0.take(index).unwrap_or(MyValue::NULL);
        from_mysql(v, &col)
    }

    fn is_null(&self, index: usize) -> bool {
        matches!(self.0.as_ref(index), Some(MyValue::NULL) | None)
    }
}

async fn my_fetch<O: FromRow, Q: Queryable>(conn: &mut Q, query: &Query<'_>) -> Result<Vec<O>> {
    let table = query_table(query);
    let r = sql::render(query, Dialect::MySql)?;
    let rows: Vec<Row> = conn.exec(r.sql.as_ref(), params(&r.params)).await.map_err(|e| map_err(e, table))?;
    rows.into_iter().map(|row| O::from_row(&mut MyRow(row), 0)).collect()
}

async fn my_execute<Q: Queryable>(conn: &mut Q, query: &Query<'_>) -> Result<ExecResult> {
    let table = query_table(query);
    match query {
        Query::CreateTable { .. } | Query::DropTable { .. } => {
            for stmt in sql::ddl(query, Dialect::MySql)? {
                conn.query_drop(stmt).await.map_err(|e| map_err(e, table))?;
            }
            Ok(ExecResult::default())
        }
        Query::Script { sql } => {
            conn.query_drop(*sql).await.map_err(|e| map_err(e, table))?;
            Ok(ExecResult::default())
        }
        _ => {
            let r = sql::render(query, Dialect::MySql)?;
            let result = conn.exec_iter(r.sql.as_ref(), params(&r.params)).await.map_err(|e| map_err(e, table))?;
            let rows_affected = result.affected_rows();
            let last_insert_id = result.last_insert_id();
            result.drop_result().await.map_err(|e| map_err(e, table))?;
            let last_insert_id = match query {
                Query::InsertRow { table, .. } if table.auto_increment() => {
                    Some(Value::I64(last_insert_id.and_then(|id| i64::try_from(id).ok()).ok_or_else(|| {
                        Error::Conversion(format!("MySQL did not return the generated key of `{}`", table.name))
                    })?))
                }
                _ => None,
            };
            Ok(ExecResult { rows_affected, last_insert_id })
        }
    }
}

/// A pool of MySQL / MariaDB connections.
///
/// ```no_run
/// # async fn demo() -> memoire_orm::Result<()> {
/// use memoire_orm::prelude::*;
/// let db = MySqlPool::connect("mysql://user:password@localhost:3306/app").await?;
/// # Ok(()) }
/// ```
///
/// The pool opens connections lazily, caches up to 256 prepared statements per
/// connection and reports *matched* rows for `UPDATE` (like every other
/// backend). Cloning it is cheap.
#[derive(Clone)]
pub struct MySqlPool {
    pool: Pool,
}

impl fmt::Debug for MySqlPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MySqlPool").finish_non_exhaustive()
    }
}

impl MySqlPool {
    /// Connects with a `mysql://user:password@host:port/database` URL. One
    /// connection is opened immediately to validate the settings.
    pub async fn connect(url: &str) -> Result<Self> {
        let opts = Opts::from_url(url).map_err(|e| Error::Pool(format!("invalid MySQL URL: {e}")))?;
        Self::connect_with(OptsBuilder::from_opts(opts)).await
    }

    /// Connects with explicit `mysql_async` options (TLS, pool size, ...).
    /// Memoire enables `client_found_rows` and a 256-entry statement cache.
    pub async fn connect_with(builder: OptsBuilder) -> Result<Self> {
        let builder = builder
            .client_found_rows(true)
            .stmt_cache_size(256)
            .pool_opts(PoolOpts::default().with_reset_connection(false));
        let pool = Pool::new(builder);
        let conn = pool.get_conn().await.map_err(|e| map_err(e, None))?;
        drop(conn);
        Ok(MySqlPool { pool })
    }

    /// The underlying `mysql_async` pool.
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Closes every connection of the pool.
    pub async fn close(self) -> Result<()> {
        self.pool.disconnect().await.map_err(|e| map_err(e, None))
    }
}

impl Executor for MySqlPool {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::MySql)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let mut conn = self.pool.get_conn().await.map_err(|e| map_err(e, None))?;
        my_fetch(&mut conn, &query).await
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let mut conn = self.pool.get_conn().await.map_err(|e| map_err(e, None))?;
        my_execute(&mut conn, &query).await
    }
}

impl Database for MySqlPool {
    type Tx<'c> = MySqlTransaction;

    async fn begin(&self) -> Result<MySqlTransaction> {
        let tx = self.pool.start_transaction(TxOpts::default()).await.map_err(|e| map_err(e, None))?;
        Ok(MySqlTransaction { tx: Mutex::new(Some(tx)) })
    }
}

/// A MySQL / MariaDB transaction holding one pooled connection.
///
/// Dropping it without committing rolls it back when the connection returns to
/// the pool.
///
/// **Note:** MySQL commits implicitly before and after DDL statements
/// (`CREATE TABLE`, `DROP TABLE`...), so schema changes cannot be rolled back.
pub struct MySqlTransaction {
    tx: Mutex<Option<mysql_async::Transaction<'static>>>,
}

impl fmt::Debug for MySqlTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MySqlTransaction").finish_non_exhaustive()
    }
}

impl Executor for MySqlTransaction {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::MySql)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let mut guard = self.tx.lock().await;
        let tx = guard.as_mut().ok_or(Error::TransactionFinished)?;
        my_fetch(tx, &query).await
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let mut guard = self.tx.lock().await;
        let tx = guard.as_mut().ok_or(Error::TransactionFinished)?;
        my_execute(tx, &query).await
    }
}

impl Transaction for MySqlTransaction {
    async fn commit(self) -> Result<()> {
        let tx = self.tx.into_inner().ok_or(Error::TransactionFinished)?;
        tx.commit().await.map_err(|e| map_err(e, None))
    }

    async fn rollback(self) -> Result<()> {
        let tx = self.tx.into_inner().ok_or(Error::TransactionFinished)?;
        tx.rollback().await.map_err(|e| map_err(e, None))
    }
}

#[cfg(test)]
mod tests {
    use super::quoted_after;

    #[test]
    fn parses_server_messages() {
        assert_eq!(quoted_after("Duplicate entry 'a@b.c' for key 'users.email'", "for key"), Some("users.email"));
        assert_eq!(quoted_after("Table 'db.tags' doesn't exist", "Table"), Some("db.tags"));
        assert_eq!(quoted_after("Table 'users' already exists", "Table"), Some("users"));
    }
}
