//! SQLite backend (feature `sqlite`).
//!
//! Statements are prepared once and cached per connection; primary-key CRUD uses
//! the SQL generated at compile time by `#[derive(Model)]`. The connection runs
//! SQLite in-process, so queries complete without any I/O wait: the futures are
//! ready immediately and work with any executor (Tokio, async-std,
//! [`block_on`](crate::block_on)...).

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};

use async_lock::{Mutex, MutexGuardArc};
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{Connection, ToSql};

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::{FromRow, RowSource};
use crate::schema::Dialect;
use crate::sql;
use crate::value::Value;

const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;
const SQLITE_CONSTRAINT_UNIQUE: i32 = 2067;

fn map_err(err: rusqlite::Error) -> Error {
    let failure = match &err {
        rusqlite::Error::SqliteFailure(e, msg) => Some((e, msg.clone().unwrap_or_default())),
        rusqlite::Error::SqlInputError { error, msg, .. } => Some((error, msg.clone())),
        _ => None,
    };
    if let Some((e, msg)) = failure {
        if e.code == rusqlite::ErrorCode::ConstraintViolation {
            if e.extended_code == SQLITE_CONSTRAINT_UNIQUE || e.extended_code == SQLITE_CONSTRAINT_PRIMARYKEY {
                // "UNIQUE constraint failed: users.email"
                let target = msg.rsplit(": ").next().unwrap_or("").split(", ").next().unwrap_or("");
                let (table, column) = target.split_once('.').unwrap_or(("", target));
                return Error::UniqueViolation { table: table.to_owned(), column: column.to_owned() };
            }
            return Error::Constraint(msg);
        }
        if let Some(t) = msg.strip_prefix("no such table: ") {
            return Error::NoSuchTable(t.trim().to_owned());
        }
        if let Some(rest) = msg.strip_prefix("table ")
            && let Some(name) = rest.strip_suffix(" already exists")
        {
            return Error::TableExists(name.trim_matches(['"', '`']).to_owned());
        }
    }
    Error::Database(Box::new(err))
}

struct Param<'a>(&'a Value);

impl ToSql for Param<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        use rusqlite::types::Value as Sv;
        Ok(match self.0 {
            Value::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Value::Bool(b) => ToSqlOutput::Owned(Sv::Integer(*b as i64)),
            Value::I32(v) => ToSqlOutput::Owned(Sv::Integer(*v as i64)),
            Value::I64(v) => ToSqlOutput::Owned(Sv::Integer(*v)),
            Value::F64(v) => ToSqlOutput::Owned(Sv::Real(*v)),
            Value::Text(s) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
            Value::Bytes(b) => ToSqlOutput::Borrowed(ValueRef::Blob(b)),
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => ToSqlOutput::Owned(Sv::Text(u.hyphenated().to_string())),
            #[cfg(feature = "chrono")]
            Value::Timestamp(t) => ToSqlOutput::Owned(Sv::Text(t.format("%Y-%m-%d %H:%M:%S%.f").to_string())),
            #[cfg(feature = "chrono")]
            Value::TimestampTz(t) => {
                ToSqlOutput::Owned(Sv::Text(t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)))
            }
            #[cfg(feature = "chrono")]
            Value::Date(d) => ToSqlOutput::Owned(Sv::Text(d.format("%Y-%m-%d").to_string())),
            #[cfg(feature = "chrono")]
            Value::Time(t) => ToSqlOutput::Owned(Sv::Text(t.format("%H:%M:%S%.f").to_string())),
            #[cfg(feature = "json")]
            Value::Json(j) => ToSqlOutput::Owned(Sv::Text(j.to_string())),
        })
    }
}

struct SqliteRow<'r, 's>(&'r rusqlite::Row<'s>);

impl RowSource for SqliteRow<'_, '_> {
    fn len(&self) -> usize {
        self.0.as_ref().column_count()
    }

    fn take(&mut self, index: usize) -> Result<Value> {
        Ok(match self.0.get_ref(index).map_err(map_err)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(i) => Value::I64(i),
            ValueRef::Real(f) => Value::F64(f),
            ValueRef::Text(t) => Value::Text(
                std::str::from_utf8(t).map_err(|e| Error::Conversion(format!("invalid UTF-8 text: {e}")))?.to_owned(),
            ),
            ValueRef::Blob(b) => Value::Bytes(b.to_vec()),
        })
    }

    fn is_null(&self, index: usize) -> bool {
        matches!(self.0.get_ref(index), Ok(ValueRef::Null) | Err(_))
    }
}

fn run_fetch<O: FromRow>(conn: &Connection, query: &Query<'_>) -> Result<Vec<O>> {
    let r = sql::render(query, Dialect::Sqlite)?;
    let mut stmt = conn.prepare_cached(&r.sql).map_err(map_err)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(r.params.iter().map(|v| Param(v)))).map_err(map_err)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(map_err)? {
        out.push(O::from_row(&mut SqliteRow(row), 0)?);
    }
    Ok(out)
}

fn run_execute(conn: &Connection, query: &Query<'_>) -> Result<ExecResult> {
    match query {
        Query::CreateTable { .. } | Query::DropTable { .. } => {
            for stmt in sql::ddl(query, Dialect::Sqlite)? {
                conn.execute_batch(&stmt).map_err(map_err)?;
            }
            Ok(ExecResult::default())
        }
        Query::Script { sql } => {
            conn.execute_batch(sql).map_err(map_err)?;
            Ok(ExecResult { rows_affected: conn.changes(), last_insert_id: None })
        }
        Query::InsertRow { table, .. } if table.auto_increment() => {
            let r = sql::render(query, Dialect::Sqlite)?;
            let mut stmt = conn.prepare_cached(&r.sql).map_err(map_err)?;
            let id: i64 = stmt
                .query_row(rusqlite::params_from_iter(r.params.iter().map(|v| Param(v))), |row| row.get(0))
                .map_err(map_err)?;
            Ok(ExecResult { rows_affected: 1, last_insert_id: Some(Value::I64(id)) })
        }
        _ => {
            let r = sql::render(query, Dialect::Sqlite)?;
            let mut stmt = conn.prepare_cached(&r.sql).map_err(map_err)?;
            let n = stmt.execute(rusqlite::params_from_iter(r.params.iter().map(|v| Param(v)))).map_err(map_err)?;
            Ok(ExecResult { rows_affected: n as u64, last_insert_id: None })
        }
    }
}

/// A SQLite database.
///
/// The handle is cheap to clone (it shares one connection). Statements are
/// serialised on that connection, which is how SQLite performs best for writes;
/// open several `SqliteDb` on the same file for parallel readers (WAL mode is
/// enabled for files).
///
/// `LIKE` is made case-sensitive (like PostgreSQL and the memory engine) so that
/// `like`, `contains`, `starts_with`... behave the same on every backend; use
/// `ilike` / `icontains` for case-insensitive matching.
#[derive(Clone)]
pub struct SqliteDb {
    conn: Arc<Mutex<Connection>>,
}

impl fmt::Debug for SqliteDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteDb").finish_non_exhaustive()
    }
}

impl SqliteDb {
    /// Opens (or creates) a database file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(map_err)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;")
            .map_err(map_err)?;
        Self::from_connection(conn)
    }

    /// Opens a private in-memory database.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(map_err)?)
    }

    /// Wraps an existing connection (foreign keys and case-sensitive `LIKE` are
    /// enabled on it).
    pub fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA case_sensitive_like = ON;").map_err(map_err)?;
        conn.set_prepared_statement_cache_capacity(256);
        Ok(SqliteDb { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Runs a closure with direct access to the underlying `rusqlite`
    /// connection (escape hatch for features Memoire does not cover).
    pub async fn with_connection<R>(&self, f: impl FnOnce(&mut Connection) -> R) -> R {
        let mut conn = self.conn.lock().await;
        f(&mut conn)
    }
}

impl Executor for SqliteDb {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::Sqlite)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let conn = self.conn.lock().await;
        run_fetch(&conn, &query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let conn = self.conn.lock().await;
        run_execute(&conn, &query)
    }
}

impl Database for SqliteDb {
    type Tx<'c> = SqliteTransaction;

    async fn begin(&self) -> Result<SqliteTransaction> {
        let guard = self.conn.lock_arc().await;
        guard.execute_batch("BEGIN").map_err(map_err)?;
        Ok(SqliteTransaction { guard: StdMutex::new(Some(guard)) })
    }
}

/// A transaction on a [`SqliteDb`]. It holds the connection until it is
/// committed, rolled back or dropped (dropping rolls back).
pub struct SqliteTransaction {
    guard: StdMutex<Option<MutexGuardArc<Connection>>>,
}

impl fmt::Debug for SqliteTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqliteTransaction").finish_non_exhaustive()
    }
}

impl SqliteTransaction {
    fn with<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let guard = self.guard.lock().unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(conn) => f(conn),
            None => Err(Error::TransactionFinished),
        }
    }

    fn finish(&self, sql: &str) -> Result<()> {
        let mut guard = self.guard.lock().unwrap_or_else(|p| p.into_inner());
        let conn = guard.take().ok_or(Error::TransactionFinished)?;
        let res = conn.execute_batch(sql).map_err(map_err);
        if res.is_err() && !conn.is_autocommit() {
            // A failed COMMIT leaves the transaction open: never hand the
            // connection back in that state.
            let _ = conn.execute_batch("ROLLBACK");
        }
        res
    }
}

impl Drop for SqliteTransaction {
    fn drop(&mut self) {
        let slot = self.guard.get_mut().unwrap_or_else(|p| p.into_inner());
        if let Some(conn) = slot.take()
            && !conn.is_autocommit()
        {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }
}

impl Executor for SqliteTransaction {
    fn dialect(&self) -> Option<Dialect> {
        Some(Dialect::Sqlite)
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        self.with(|c| run_fetch(c, &query))
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        self.with(|c| run_execute(c, &query))
    }
}

impl Transaction for SqliteTransaction {
    async fn commit(self) -> Result<()> {
        self.finish("COMMIT")
    }

    async fn rollback(self) -> Result<()> {
        self.finish("ROLLBACK")
    }
}
