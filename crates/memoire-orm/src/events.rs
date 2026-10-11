//! Change notifications: subscribe to every write made through a database
//! handle, on any backend.
//!
//! Wrap a handle in [`Observed`] and [`subscribe`](Observed::subscribe) a
//! listener: it receives a [`Change`] after each successful write made through
//! that handle (or its clones and transactions). Writes made inside a
//! transaction are announced when it commits, in order, and never if it rolls
//! back.
//!
//! ```
//! use std::sync::{Arc, Mutex};
//! use memoire_orm::events::{ChangeKind, Observed};
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
//! let db = Observed::new(MemoryDb::new()); // or SqliteDb, PgPool, MySqlPool, RedisDb, MongoDb
//! let seen = Arc::new(Mutex::new(Vec::new()));
//! let log = Arc::clone(&seen);
//! let _subscription = db.subscribe(move |change| log.lock().unwrap().push((change.table.clone(), change.kind)));
//!
//! db.create_table::<Note>().await?;
//! let mut note = Note { id: 0, text: "hello".into() };
//! note.insert(&db).await?;
//! note.delete(&db).await?;
//!
//! let seen = seen.lock().unwrap();
//! assert_eq!(seen[1], ("notes".to_owned(), ChangeKind::Insert));
//! assert_eq!(seen[2], ("notes".to_owned(), ChangeKind::Delete));
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```
//!
//! Only writes made through the observed handle are seen: another program
//! writing to the same database is not. Listeners run on the writing task,
//! right after the write; keep them short, or forward the change to a channel
//! ([`Observed::channel`]).

use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak, mpsc};

use crate::ast::Query;
use crate::error::Result;
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::row::FromRow;
use crate::schema::{Dialect, TableSchema};
use crate::value::Value;

/// What a [`Change`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChangeKind {
    /// Rows were inserted.
    Insert,
    /// Rows were updated.
    Update,
    /// A row was inserted or updated by primary key ([`Model::upsert`](crate::Model::upsert)).
    Upsert,
    /// Rows were deleted.
    Delete,
    /// A table was created.
    CreateTable,
    /// A table was dropped.
    DropTable,
    /// Raw SQL or a migration script ran (`table` is empty).
    Sql,
}

/// One write, as announced to the listeners of an [`Observed`] handle.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Change {
    /// The table written (empty for [`ChangeKind::Sql`]).
    pub table: String,
    /// What happened.
    pub kind: ChangeKind,
    /// Number of rows affected.
    pub rows: u64,
    /// The primary key of the row, for writes of one row by key (`insert`,
    /// `update`, `upsert`, `delete`); `None` for statements over many rows.
    pub key: Option<Value>,
    /// Every column of the row as written, in schema order, for `insert`,
    /// `update` and `upsert` of one row (with the generated key filled in).
    pub row: Option<Vec<Value>>,
}

impl Change {
    fn table(table: &TableSchema, kind: ChangeKind, rows: u64) -> Self {
        Change { table: table.name.to_owned(), kind, rows, key: None, row: None }
    }

    /// The change a successful statement made, if it changed anything.
    fn of(query: &Query<'_>, res: &ExecResult) -> Option<Change> {
        let n = res.rows_affected;
        let one_row = |table: &TableSchema, kind, values: &[Value], key: Value| {
            let mut row = values.to_vec();
            if let Some(slot) = row.get_mut(table.primary_key) {
                *slot = key.clone();
            }
            Change { table: table.name.to_owned(), kind, rows: n.max(1), key: Some(key), row: Some(row) }
        };
        Some(match *query {
            Query::Select(_) | Query::FindByKey { .. } => return None,
            Query::Insert(ins) => Change::table(ins.table, ChangeKind::Insert, n),
            Query::Update(u) if n > 0 => Change::table(u.table, ChangeKind::Update, n),
            Query::Delete(d) if n > 0 => Change::table(d.table, ChangeKind::Delete, n),
            Query::Update(_) | Query::Delete(_) => return None,
            Query::InsertRow { table, values } => {
                let key = res.last_insert_id.clone().unwrap_or_else(|| values[table.primary_key].clone());
                one_row(table, ChangeKind::Insert, values, key)
            }
            Query::UpsertRow { table, values } => {
                one_row(table, ChangeKind::Upsert, values, values[table.primary_key].clone())
            }
            Query::UpdateRow { table, values } if n > 0 => {
                one_row(table, ChangeKind::Update, values, values[table.primary_key].clone())
            }
            Query::UpdateRow { .. } => return None,
            Query::DeleteByKey { table, key } if n > 0 => {
                Change { key: Some(key.clone()), ..Change::table(table, ChangeKind::Delete, n) }
            }
            Query::DeleteByKey { .. } => return None,
            Query::CreateTable { table, .. } => Change::table(table, ChangeKind::CreateTable, 0),
            Query::DropTable { name, .. } => {
                Change { table: name.to_owned(), kind: ChangeKind::DropTable, rows: 0, key: None, row: None }
            }
            Query::Raw { .. } | Query::Script { .. } => {
                Change { table: String::new(), kind: ChangeKind::Sql, rows: n, key: None, row: None }
            }
        })
    }
}

type Listener = Arc<dyn Fn(&Change) + Send + Sync>;

#[derive(Default)]
struct Hub {
    listeners: RwLock<Vec<(u64, Listener)>>,
    /// Number of listeners, read without locking on every write.
    count: AtomicUsize,
    next_id: AtomicU64,
}

impl Hub {
    fn active(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    fn publish(&self, change: &Change) {
        // Listeners are called without holding the lock, so they may subscribe
        // or unsubscribe themselves.
        let listeners: Vec<Listener> =
            self.listeners.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, l)| Arc::clone(l)).collect();
        for l in listeners {
            l(change);
        }
    }
}

/// A database handle (any backend, or a transaction's database) whose writes
/// are announced to listeners. It is itself a [`Database`]: use it wherever
/// you used the wrapped handle. Cloning shares the listeners.
pub struct Observed<D> {
    inner: D,
    hub: Arc<Hub>,
}

impl<D: Clone> Clone for Observed<D> {
    fn clone(&self) -> Self {
        Observed { inner: self.inner.clone(), hub: Arc::clone(&self.hub) }
    }
}

impl<D: fmt::Debug> fmt::Debug for Observed<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observed")
            .field("inner", &self.inner)
            .field("listeners", &self.hub.count.load(Ordering::Relaxed))
            .finish()
    }
}

impl<D> Observed<D> {
    /// Wraps a database handle.
    pub fn new(inner: D) -> Self {
        Observed { inner, hub: Arc::default() }
    }

    /// The wrapped handle (writes made through it directly are not announced).
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Calls `listener` after every write made through this handle, until the
    /// returned [`Subscription`] is dropped.
    #[must_use = "the listener is removed when the subscription is dropped"]
    pub fn subscribe(&self, listener: impl Fn(&Change) + Send + Sync + 'static) -> Subscription {
        let id = self.hub.next_id.fetch_add(1, Ordering::Relaxed);
        self.hub.listeners.write().unwrap_or_else(|e| e.into_inner()).push((id, Arc::new(listener)));
        self.hub.count.fetch_add(1, Ordering::Release);
        Subscription { hub: Arc::downgrade(&self.hub), id }
    }

    /// Sends every change to a channel, until the returned [`Subscription`] is
    /// dropped (or the receiver is).
    #[must_use = "the channel is closed when the subscription is dropped"]
    pub fn channel(&self) -> (Subscription, mpsc::Receiver<Change>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sub = self.subscribe(move |c| {
            let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(c.clone());
        });
        (sub, rx)
    }

    /// Number of active listeners.
    pub fn listener_count(&self) -> usize {
        self.hub.count.load(Ordering::Acquire)
    }
}

/// Keeps a listener registered; dropping it unsubscribes.
pub struct Subscription {
    hub: Weak<Hub>,
    id: u64,
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription").field("id", &self.id).finish()
    }
}

impl Subscription {
    /// Removes the listener now (same as dropping the subscription).
    pub fn unsubscribe(self) {}
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.upgrade() {
            let mut listeners = hub.listeners.write().unwrap_or_else(|e| e.into_inner());
            let before = listeners.len();
            listeners.retain(|(id, _)| *id != self.id);
            if listeners.len() != before {
                hub.count.fetch_sub(1, Ordering::Release);
            }
        }
    }
}

impl<D: Executor> Executor for Observed<D> {
    fn dialect(&self) -> Option<Dialect> {
        self.inner.dialect()
    }

    fn fetch_all<O>(&self, query: Query<'_>) -> impl Future<Output = Result<Vec<O>>> + Send
    where
        O: FromRow + Send,
    {
        self.inner.fetch_all(query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let res = self.inner.execute(query).await?;
        if self.hub.active()
            && let Some(change) = Change::of(&query, &res)
        {
            self.hub.publish(&change);
        }
        Ok(res)
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        let n = self.inner.insert_rows(table, rows).await?;
        if self.hub.active() {
            self.hub.publish(&Change::table(table, ChangeKind::Insert, n));
        }
        Ok(n)
    }
}

impl<D: Database> Database for Observed<D> {
    type Tx<'c>
        = ObservedTransaction<D::Tx<'c>>
    where
        Self: 'c;

    async fn begin(&self) -> Result<Self::Tx<'_>> {
        let inner = self.inner.begin().await?;
        Ok(ObservedTransaction { inner, hub: Arc::clone(&self.hub), pending: Mutex::new(Vec::new()) })
    }
}

/// A transaction of an [`Observed`] handle: its changes are announced when it
/// commits.
pub struct ObservedTransaction<T> {
    inner: T,
    hub: Arc<Hub>,
    pending: Mutex<Vec<Change>>,
}

impl<T: fmt::Debug> fmt::Debug for ObservedTransaction<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservedTransaction").field("inner", &self.inner).finish_non_exhaustive()
    }
}

impl<T> ObservedTransaction<T> {
    fn record(&self, change: impl FnOnce() -> Option<Change>) {
        if self.hub.active()
            && let Some(c) = change()
        {
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).push(c);
        }
    }
}

impl<T: Transaction> Executor for ObservedTransaction<T> {
    fn dialect(&self) -> Option<Dialect> {
        self.inner.dialect()
    }

    fn fetch_all<O>(&self, query: Query<'_>) -> impl Future<Output = Result<Vec<O>>> + Send
    where
        O: FromRow + Send,
    {
        self.inner.fetch_all(query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let res = self.inner.execute(query).await?;
        self.record(|| Change::of(&query, &res));
        Ok(res)
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        let n = self.inner.insert_rows(table, rows).await?;
        self.record(|| Some(Change::table(table, ChangeKind::Insert, n)));
        Ok(n)
    }
}

impl<T: Transaction> Transaction for ObservedTransaction<T> {
    async fn commit(self) -> Result<()> {
        let ObservedTransaction { inner, hub, pending } = self;
        inner.commit().await?;
        for change in pending.into_inner().unwrap_or_else(|e| e.into_inner()) {
            hub.publish(&change);
        }
        Ok(())
    }

    fn rollback(self) -> impl Future<Output = Result<()>> + Send {
        self.inner.rollback()
    }
}
