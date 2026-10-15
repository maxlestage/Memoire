//! Query log: every statement run through a handle, with its SQL, parameters,
//! duration and row count, on any backend.
//!
//! Wrap a handle in [`Logged`] (like `DB::enableQueryLog()` in Laravel or the
//! SQL logger of Doctrine), run your code, then read
//! [`queries`](Logged::queries), for instance to find N+1 queries in a test,
//! or [`listen`](Logged::listen) to print each statement as it runs.
//!
//! ```
//! use memoire_orm::log::Logged;
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug, Clone)]
//! #[memoire(table = "notes")]
//! struct Note {
//!     #[memoire(primary_key, auto_increment)]
//!     id: i64,
//!     text: String,
//! }
//!
//! # memoire_orm::block_on(async {
//! let db = Logged::new(MemoryDb::new()); // or SqliteDb, PgPool, MySqlPool, RedisDb, MongoDb
//! db.create_table::<Note>().await?;
//! Note { id: 0, text: "hello".into() }.insert(&db).await?;
//! let found = Note::query().filter(Note::TEXT.eq("hello")).all(&db).await?;
//!
//! let log = db.queries();
//! assert_eq!(log.len(), 3);
//! assert_eq!(log[2].sql, r#"SELECT "notes"."id", "notes"."text" FROM "notes" WHERE "notes"."text" = ?"#);
//! assert_eq!(log[2].params, vec![Value::Text("hello".into())]);
//! assert_eq!(log[2].rows, Some(1));
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```
//!
//! The SQL is the statement the backend runs, in its dialect. The in-memory
//! engine, Redis and MongoDB run no SQL: their log shows the equivalent
//! statement in the SQLite dialect. Durations are measured on the writing
//! task, from the call to the result (`None` on `wasm32`, which has no clock
//! for it).

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::FromRow;
use crate::schema::{Dialect, TableSchema};
use crate::value::Value;

/// One statement of the log of a [`Logged`] handle.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct LoggedQuery {
    /// The SQL text (`BEGIN`, `COMMIT` and `ROLLBACK` for transactions).
    pub sql: String,
    /// The bound parameters, in order.
    pub params: Vec<Value>,
    /// Time from the call to the result (`None` on `wasm32`).
    pub duration: Option<Duration>,
    /// Rows returned (queries) or affected (writes); `None` when it failed.
    pub rows: Option<u64>,
    /// The error, when the statement failed.
    pub error: Option<String>,
    /// Run inside a transaction.
    pub in_transaction: bool,
}

impl fmt::Display for LoggedQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sql)?;
        if !self.params.is_empty() {
            f.write_str(" -- [")?;
            for (i, p) in self.params.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{p}")?;
            }
            f.write_str("]")?;
        }
        if let Some(d) = self.duration {
            write!(f, " ({:.3} ms)", d.as_secs_f64() * 1000.0)?;
        }
        if let Some(e) = &self.error {
            write!(f, " failed: {e}")?;
        }
        Ok(())
    }
}

type Listener = Arc<dyn Fn(&LoggedQuery) + Send + Sync>;

struct Log {
    entries: Mutex<VecDeque<LoggedQuery>>,
    /// Keep at most this many entries (the oldest go first).
    limit: Option<usize>,
    recording: AtomicBool,
    listeners: RwLock<Vec<Listener>>,
}

/// When a statement started (nothing on `wasm32`, which has no clock for it).
#[derive(Clone, Copy)]
struct Start {
    #[cfg(not(target_arch = "wasm32"))]
    at: std::time::Instant,
}

fn start() -> Start {
    Start {
        #[cfg(not(target_arch = "wasm32"))]
        at: std::time::Instant::now(),
    }
}

fn elapsed(start: Start) -> Option<Duration> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(start.at.elapsed())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let Start {} = start;
        None
    }
}

impl Log {
    fn new(limit: Option<usize>) -> Self {
        Log {
            entries: Mutex::new(VecDeque::new()),
            limit,
            recording: AtomicBool::new(true),
            listeners: RwLock::new(Vec::new()),
        }
    }

    fn listeners(&self) -> Vec<Listener> {
        self.listeners.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn wanted(&self) -> bool {
        self.recording.load(Ordering::Relaxed) || !self.listeners.read().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    fn push(&self, entry: LoggedQuery) {
        for l in self.listeners() {
            l(&entry);
        }
        if self.recording.load(Ordering::Relaxed) {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(limit) = self.limit {
                if limit == 0 {
                    return;
                }
                while entries.len() >= limit {
                    entries.pop_front();
                }
            }
            entries.push_back(entry);
        }
    }

    fn record(
        &self,
        query: &Query<'_>,
        dialect: Option<Dialect>,
        start: Start,
        outcome: std::result::Result<u64, &Error>,
        in_transaction: bool,
    ) {
        if !self.wanted() {
            return;
        }
        let duration = elapsed(start);
        let (sql, params) = match crate::sql::render(query, dialect.unwrap_or(Dialect::Sqlite)) {
            Ok(r) => {
                let params = r.owned_params();
                (r.sql.into_owned(), params)
            }
            Err(_) => (format!("{query:?}"), Vec::new()),
        };
        let (rows, error) = match outcome {
            Ok(n) => (Some(n), None),
            Err(e) => (None, Some(e.to_string())),
        };
        self.push(LoggedQuery { sql, params, duration, rows, error, in_transaction });
    }

    fn mark(&self, sql: &str, start: Start, outcome: std::result::Result<(), &Error>) {
        if !self.wanted() {
            return;
        }
        let (rows, error) = match outcome {
            Ok(()) => (Some(0), None),
            Err(e) => (None, Some(e.to_string())),
        };
        self.push(LoggedQuery {
            sql: sql.to_owned(),
            params: Vec::new(),
            duration: elapsed(start),
            rows,
            error,
            in_transaction: true,
        });
    }
}

/// A database handle (any backend) that logs every statement run through it,
/// its clones and its transactions. It is itself a [`Database`]: use it
/// wherever you used the wrapped handle. Cloning shares the log.
///
/// See the [module documentation](self).
pub struct Logged<D> {
    inner: D,
    log: Arc<Log>,
}

impl<D: Clone> Clone for Logged<D> {
    fn clone(&self) -> Self {
        Logged { inner: self.inner.clone(), log: Arc::clone(&self.log) }
    }
}

impl<D: fmt::Debug> fmt::Debug for Logged<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logged").field("inner", &self.inner).field("queries", &self.query_count()).finish()
    }
}

impl<D> Logged<D> {
    /// Wraps a database handle; every statement is kept until
    /// [`clear`](Self::clear) or [`take_queries`](Self::take_queries).
    pub fn new(inner: D) -> Self {
        Logged { inner, log: Arc::new(Log::new(None)) }
    }

    /// Wraps a database handle, keeping only the last `limit` statements (for
    /// a long-running program; `0` keeps none, for [`listen`](Self::listen) only).
    pub fn with_limit(inner: D, limit: usize) -> Self {
        Logged { inner, log: Arc::new(Log::new(Some(limit))) }
    }

    /// The wrapped handle (statements run through it directly are not logged).
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// The statements logged so far, oldest first.
    pub fn queries(&self) -> Vec<LoggedQuery> {
        self.log.entries.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    /// Returns the statements logged so far and empties the log.
    pub fn take_queries(&self) -> Vec<LoggedQuery> {
        self.log.entries.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
    }

    /// Number of statements in the log.
    pub fn query_count(&self) -> usize {
        self.log.entries.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Empties the log.
    pub fn clear(&self) {
        self.log.entries.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Stops (`false`) or resumes (`true`) keeping statements in the log;
    /// listeners are called either way.
    pub fn set_recording(&self, on: bool) {
        self.log.recording.store(on, Ordering::Relaxed);
    }

    /// Calls `listener` with every statement, right after it ran (on the
    /// running task: keep it short), for as long as the handle lives.
    ///
    /// ```
    /// # use memoire_orm::{log::Logged, MemoryDb};
    /// let db = Logged::new(MemoryDb::new());
    /// db.listen(|q| eprintln!("[sql] {q}"));
    /// ```
    pub fn listen(&self, listener: impl Fn(&LoggedQuery) + Send + Sync + 'static) {
        self.log.listeners.write().unwrap_or_else(|e| e.into_inner()).push(Arc::new(listener));
    }
}

impl<D: Executor> Executor for Logged<D> {
    fn dialect(&self) -> Option<Dialect> {
        self.inner.dialect()
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let t = start();
        let res = self.inner.fetch_all::<O>(query).await;
        self.log.record(&query, self.inner.dialect(), t, res.as_ref().map(|r| r.len() as u64), false);
        res
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let t = start();
        let res = self.inner.execute(query).await;
        self.log.record(&query, self.inner.dialect(), t, res.as_ref().map(|r| r.rows_affected), false);
        res
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        if !self.log.wanted() {
            return self.inner.insert_rows(table, rows).await;
        }
        let ast = crate::ast::InsertAst { table, rows };
        let t = start();
        let res = self.inner.insert_rows(table, ast.rows.clone()).await;
        self.log.record(&Query::Insert(&ast), self.inner.dialect(), t, res.as_ref().copied(), false);
        res
    }
}

impl<D: Database> Database for Logged<D> {
    type Tx<'c>
        = LoggedTransaction<D::Tx<'c>>
    where
        Self: 'c;

    async fn begin(&self) -> Result<Self::Tx<'_>> {
        let t = start();
        let res = self.inner.begin().await;
        self.log.mark("BEGIN", t, res.as_ref().map(|_| ()));
        Ok(LoggedTransaction { inner: res?, log: Arc::clone(&self.log) })
    }
}

/// A transaction of a [`Logged`] handle: its statements go to the same log,
/// marked [`in_transaction`](LoggedQuery::in_transaction).
pub struct LoggedTransaction<T> {
    inner: T,
    log: Arc<Log>,
}

impl<T: fmt::Debug> fmt::Debug for LoggedTransaction<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoggedTransaction").field("inner", &self.inner).finish_non_exhaustive()
    }
}

impl<T: Transaction> Executor for LoggedTransaction<T> {
    fn dialect(&self) -> Option<Dialect> {
        self.inner.dialect()
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let t = start();
        let res = self.inner.fetch_all::<O>(query).await;
        self.log.record(&query, self.inner.dialect(), t, res.as_ref().map(|r| r.len() as u64), true);
        res
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let t = start();
        let res = self.inner.execute(query).await;
        self.log.record(&query, self.inner.dialect(), t, res.as_ref().map(|r| r.rows_affected), true);
        res
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        if !self.log.wanted() {
            return self.inner.insert_rows(table, rows).await;
        }
        let ast = crate::ast::InsertAst { table, rows };
        let t = start();
        let res = self.inner.insert_rows(table, ast.rows.clone()).await;
        self.log.record(&Query::Insert(&ast), self.inner.dialect(), t, res.as_ref().copied(), true);
        res
    }
}

impl<T: Transaction> Transaction for LoggedTransaction<T> {
    async fn commit(self) -> Result<()> {
        let t = start();
        let res = self.inner.commit().await;
        self.log.mark("COMMIT", t, res.as_ref().copied());
        res
    }

    async fn rollback(self) -> Result<()> {
        let t = start();
        let res = self.inner.rollback().await;
        self.log.mark("ROLLBACK", t, res.as_ref().copied());
        res
    }
}
