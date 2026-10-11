//! [`LocalStorageDb`]: a Memoire database whose rows live in browser storage.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};

use super::local::{LocalOnChange, LocalSubscription};
use super::storage::{KeyValueStorage, LocalStorage, Watch};
use crate::ast::Query;
use crate::backend::json_row::{encode_row_named, field};
use crate::backend::memory::persist::{Journal, Pending, Record};
use crate::backend::memory::{MemoryDb, MemoryTransaction, schemas_of};
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor};
use crate::row::FromRow;
use crate::schema::{Dialect, TableSchema};
use crate::value::Value;

/// What `<prefix>:<table>` holds.
#[derive(Clone, Debug)]
struct Meta {
    columns: Vec<String>,
    pk: usize,
    /// The next generated id: ids are not reused after a delete.
    seq: i64,
    /// Bumped by every write to the table, so a tab sees that another one
    /// changed it.
    ver: u64,
}

impl Meta {
    fn to_json(&self) -> String {
        serde_json::json!({ "columns": self.columns, "pk": self.pk, "seq": self.seq, "ver": self.ver }).to_string()
    }

    fn from_json(text: &str) -> Option<Meta> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let columns =
            v.get("columns")?.as_array()?.iter().map(|c| c.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>()?;
        let pk = v.get("pk")?.as_u64()? as usize;
        let seq = v.get("seq").and_then(serde_json::Value::as_i64).unwrap_or(1);
        let ver = v.get("ver").and_then(serde_json::Value::as_u64).unwrap_or(0);
        (pk < columns.len()).then_some(Meta { columns, pk, seq, ver })
    }
}

/// Key layout under `prefix`.
#[derive(Clone)]
struct Keys {
    prefix: String,
}

impl Keys {
    fn meta(&self, table: &str) -> String {
        format!("{}:{table}", self.prefix)
    }
    fn row(&self, table: &str, key: &Value) -> String {
        format!("{}:{table}:{}", self.prefix, field(key))
    }
    fn rows_of(&self, table: &str) -> String {
        format!("{}:{table}:", self.prefix)
    }
    /// `(table, Some(row key))` for a key of this database.
    fn parse<'k>(&self, key: &'k str) -> Option<(&'k str, Option<&'k str>)> {
        let rest = key.strip_prefix(&self.prefix)?.strip_prefix(':')?;
        Some(match rest.split_once(':') {
            Some((table, row)) => (table, Some(row)),
            None => (rest, None),
        })
    }
}

fn stored_meta(storage: &dyn KeyValueStorage, keys: &Keys, table: &str) -> Result<Option<Meta>> {
    Ok(storage.get(&keys.meta(table))?.and_then(|t| Meta::from_json(&t)))
}

/// Reads the stored tables (all, or `only` those), with their version.
fn load(
    storage: &dyn KeyValueStorage,
    keys: &Keys,
    only: Option<&[String]>,
) -> Result<HashMap<String, (u64, Pending)>> {
    let mut metas: HashMap<String, Meta> = HashMap::new();
    let mut rows: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for key in storage.keys()? {
        let Some((table, row)) = keys.parse(&key) else { continue };
        if only.is_some_and(|o| !o.iter().any(|t| t == table)) {
            continue;
        }
        let Some(value) = storage.get(&key)? else { continue };
        match row {
            None => {
                if let Some(meta) = Meta::from_json(&value) {
                    metas.insert(table.to_owned(), meta);
                }
            }
            Some(row) => rows.entry(table.to_owned()).or_default().push((row.to_owned(), value)),
        }
    }
    Ok(metas
        .into_iter()
        .map(|(table, meta)| {
            let mut found = rows.remove(&table).unwrap_or_default();
            // Stable order: by key, numbers numerically.
            found.sort_by(|(a, _), (b, _)| match (a.parse::<i64>(), b.parse::<i64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                _ => a.cmp(b),
            });
            let max_id = found.iter().filter_map(|(k, _)| k.parse::<i64>().ok()).max().unwrap_or(0);
            let next_id = meta.seq.max(max_id.saturating_add(1));
            let rows = found.into_iter().map(|(_, v)| v).collect();
            (table, (meta.ver, Pending::from_json(meta.columns, meta.pk, next_id, rows)))
        })
        .collect())
}

/// The version of each table as this page last read or wrote it.
type Versions = Arc<Mutex<HashMap<String, u64>>>;

/// Writes committed row changes as storage keys.
struct KvJournal {
    storage: Arc<dyn KeyValueStorage>,
    keys: Keys,
    known: Versions,
}

impl KvJournal {
    /// Applies the records; every key it touches is recorded in `undo` first.
    fn write(&self, records: &[Record], undo: &mut Vec<(String, Option<String>)>) -> Result<HashMap<String, u64>> {
        let storage = &*self.storage;
        let set = |key: String, value: Option<&str>, undo: &mut Vec<(String, Option<String>)>| -> Result<()> {
            undo.push((key.clone(), storage.get(&key)?));
            match value {
                Some(v) => storage.set(&key, v),
                None => storage.remove(&key),
            }
        };
        // The metadata of every table written, read once and checked: if
        // another tab wrote the table since this page read it, nothing is
        // written (the statement then fails with a conflict).
        let mut metas: HashMap<String, Option<Meta>> = HashMap::new();
        {
            let known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for r in records {
                let table = r.table();
                if metas.contains_key(table) {
                    continue;
                }
                let meta = stored_meta(storage, &self.keys, table)?;
                if meta.as_ref().map(|m| m.ver) != known.get(table).copied() {
                    return Err(Error::Conflict(table.to_owned()));
                }
                metas.insert(table.to_owned(), meta);
            }
        }
        for r in records {
            match r {
                Record::Create { table, columns, pk } => {
                    if table.contains(':') {
                        return Err(Error::Unsupported(format!(
                            "browser storage: the table name `{table}` cannot contain `:`"
                        )));
                    }
                    let slot = metas.entry(table.clone()).or_default();
                    let (seq, ver) = slot.as_ref().map_or((1, 0), |m| (m.seq, m.ver));
                    *slot = Some(Meta { columns: columns.clone(), pk: *pk, seq, ver });
                }
                Record::Drop { table } => {
                    let rows = self.keys.rows_of(table);
                    for key in storage.keys()?.into_iter().filter(|k| k.starts_with(&rows)) {
                        set(key, None, undo)?;
                    }
                    metas.insert(table.clone(), None);
                }
                Record::Put { table, row } => {
                    let meta = metas.get_mut(table).and_then(Option::as_mut).ok_or_else(|| {
                        Error::Storage(format!("browser storage: row for the unknown table `{table}`"))
                    })?;
                    let key = row.get(meta.pk).cloned().unwrap_or(Value::Null);
                    if let Some(id) = key.as_i64().filter(|id| *id >= meta.seq) {
                        meta.seq = id.saturating_add(1);
                    }
                    set(self.keys.row(table, &key), Some(&encode_row_named(&meta.columns, row)), undo)?;
                }
                Record::Del { table, key } => set(self.keys.row(table, key), None, undo)?,
                Record::Seq { table, next_id } => {
                    if let Some(meta) = metas.get_mut(table).and_then(Option::as_mut) {
                        meta.seq = meta.seq.max(*next_id);
                    }
                }
            }
        }
        // One metadata write per table, with its new version.
        let mut versions = HashMap::new();
        for (table, meta) in metas {
            match meta {
                Some(mut m) => {
                    m.ver = m.ver.wrapping_add(1).max(1);
                    set(self.keys.meta(&table), Some(&m.to_json()), undo)?;
                    versions.insert(table, m.ver);
                }
                None => set(self.keys.meta(&table), None, undo)?,
            }
        }
        Ok(versions)
    }
}

impl Journal for KvJournal {
    /// All or nothing: browser storage has no transactions, so a failed write
    /// (storage full, conflict) puts back every key this append touched.
    fn append(&mut self, records: &[Record]) -> Result<()> {
        let mut undo = Vec::new();
        match self.write(records, &mut undo) {
            Ok(versions) => {
                let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
                for r in records {
                    known.remove(r.table());
                }
                known.extend(versions);
                Ok(())
            }
            Err(e) => {
                for (key, old) in undo.into_iter().rev() {
                    let _ = match old {
                        Some(v) => self.storage.set(&key, &v),
                        None => self.storage.remove(&key),
                    };
                }
                Err(e)
            }
        }
    }

    fn reload(&self, tables: &[String]) -> Option<Vec<(String, Option<Pending>)>> {
        let mut stored = load(&*self.storage, &self.keys, Some(tables)).ok()?;
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        Some(
            tables
                .iter()
                .map(|t| match stored.remove(t) {
                    Some((ver, p)) => {
                        known.insert(t.clone(), ver);
                        (t.clone(), Some(p))
                    }
                    None => {
                        known.remove(t);
                        (t.clone(), None)
                    }
                })
                .collect(),
        )
    }
}

struct Inner {
    db: MemoryDb,
    storage: Arc<dyn KeyValueStorage>,
    keys: Keys,
    known: Versions,
}

/// Open databases, by storage scope and prefix: opening the same one again
/// gives the same database, until [`LocalStorageDb::close`].
static OPEN: LazyLock<Mutex<HashMap<String, Arc<Inner>>>> = LazyLock::new(Default::default);

/// A complete Memoire database kept in the browser's `localStorage`.
///
/// It is the in-memory engine (the same queries, joins, transactions,
/// indexes and full-text search, at the same speed) with every committed row
/// change written to the storage as it happens:
///
/// | Key | Value |
/// |---|---|
/// | `<prefix>:<table>` | `{"columns": [...], "pk": 0, "seq": 3, "ver": 7}` |
/// | `<prefix>:<table>:<primary key>` | the row, as a JSON object |
///
/// Rows are readable (and editable) in the browser's developer tools. Tables
/// come back the first time their model is used, columns matched by name, so
/// adding an `Option` field to a model keeps the saved rows.
///
/// Several tabs can use the same database: before each statement, the tables
/// it uses are read again if another tab changed them (their version moved),
/// and a write that would overwrite another tab's change fails with
/// [`Error::Conflict`] instead. [`on_external_change`](Self::on_external_change)
/// tells when to refresh the page's view.
///
/// ```
/// use memoire_orm::prelude::*;
/// use memoire_orm::web::{KeyValueStorage, LocalStorageDb, MemoryStorage};
///
/// #[derive(Model, Debug, Clone, PartialEq)]
/// #[memoire(table = "todos")]
/// struct Todo {
///     #[memoire(primary_key, auto_increment)]
///     id: i64,
///     text: String,
///     done: bool,
/// }
///
/// # memoire_orm::block_on(async {
/// // `LocalStorageDb::open("app")` in a browser; any `KeyValueStorage` elsewhere.
/// let storage = MemoryStorage::new();
/// let db = LocalStorageDb::open_with("app", storage.clone())?;
/// db.create_table_if_not_exists::<Todo>().await?;
/// Todo { id: 0, text: "Write docs".into(), done: false }.insert(&db).await?;
/// let row: serde_json::Value = serde_json::from_str(&storage.get("app:todos:1")?.unwrap()).unwrap();
/// assert_eq!(row, serde_json::json!({ "id": 1, "text": "Write docs", "done": false }));
///
/// let todo = Todo::query().filter(Todo::DONE.eq(false)).one(&db).await?;
/// assert_eq!(todo.text, "Write docs");
/// # Ok::<(), memoire_orm::Error>(())
/// # }).unwrap();
/// ```
///
/// `localStorage` holds about 5 MB per site: a write that does not fit fails
/// with [`Error::Storage`] and leaves both the storage and the database as they
/// were.
#[derive(Clone)]
pub struct LocalStorageDb {
    inner: Arc<Inner>,
}

impl fmt::Debug for LocalStorageDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalStorageDb")
            .field("prefix", &self.inner.keys.prefix)
            .field("tables", &self.inner.db.table_names())
            .finish()
    }
}

impl LocalStorageDb {
    /// The database `prefix` in the browser's `localStorage`. Opening it again
    /// returns the same database, so there is no need to keep the handle.
    pub fn open(prefix: impl Into<String>) -> Result<Self> {
        Self::open_with(prefix, LocalStorage)
    }

    /// The database `prefix` in any storage: `SessionStorage`, or
    /// `MemoryStorage` for tests and server-side rendering.
    pub fn open_with(prefix: impl Into<String>, storage: impl KeyValueStorage) -> Result<Self> {
        let prefix = prefix.into();
        if prefix.is_empty() || prefix.contains(':') {
            return Err(Error::Unsupported("the prefix must be non-empty and contain no `:`".into()));
        }
        let storage: Arc<dyn KeyValueStorage> = Arc::new(storage);
        let slot = format!("{}\u{0}{prefix}", storage.scope());
        let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(inner) = open.get(&slot).cloned() {
            return Ok(LocalStorageDb { inner });
        }
        let keys = Keys { prefix };
        let stored = load(&*storage, &keys, None)?;
        let known: Versions = Arc::new(Mutex::new(stored.iter().map(|(t, (v, _))| (t.clone(), *v)).collect()));
        let journal = KvJournal { storage: Arc::clone(&storage), keys: keys.clone(), known: Arc::clone(&known) };
        let pending = stored.into_iter().map(|(t, (_, p))| (t, Arc::new(p))).collect();
        let db = MemoryDb::with_journal(Box::new(journal), pending);
        let inner = Arc::new(Inner { db, storage, keys, known });
        open.insert(slot, Arc::clone(&inner));
        Ok(LocalStorageDb { inner })
    }

    /// Forgets the open database (its rows stay in the storage): the next
    /// `open` reads the storage again. Handles still held keep working.
    pub fn close(&self) {
        let slot = format!("{}\u{0}{}", self.inner.storage.scope(), self.inner.keys.prefix);
        let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
        if open.get(&slot).is_some_and(|alive| Arc::ptr_eq(alive, &self.inner)) {
            open.remove(&slot);
        }
    }

    /// The key prefix of this database.
    pub fn prefix(&self) -> &str {
        &self.inner.keys.prefix
    }

    /// The tables, live or not yet used (sorted).
    pub fn table_names(&self) -> Vec<String> {
        self.inner.db.table_names()
    }

    /// Number of rows in a table.
    pub fn row_count(&self, table: &str) -> Result<usize> {
        self.sync([table])?;
        self.inner.db.row_count(table)
    }

    /// Drops every table of this database and removes its keys from the
    /// storage (the other keys of the site are left alone).
    pub async fn clear(&self) -> Result<()> {
        for name in self.stored_tables()? {
            self.execute(Query::DropTable { name: &name, if_exists: true }).await?;
        }
        Ok(())
    }

    /// Calls `on_change` with the table name when another tab changes a table
    /// of this database (`None` when it cleared the whole storage), until the
    /// returned [`Watch`] is dropped: the moment to refresh what the page
    /// shows. Queries read the new rows on their own.
    #[must_use = "the listener is removed when the watch is dropped"]
    pub fn on_external_change(&self, on_change: impl Fn(Option<&str>) + Send + Sync + 'static) -> Watch {
        let keys = self.inner.keys.clone();
        self.inner.storage.watch(Arc::new(move |key: Option<&str>| match key {
            None => on_change(None),
            Some(k) => {
                if let Some((table, _)) = keys.parse(k) {
                    on_change(Some(table));
                }
            }
        }))
    }

    /// [`on_external_change`](Self::on_external_change) for a listener that is
    /// not `Send`, such as one updating a signal of a UI framework: it runs on
    /// the thread that registered it, the only one in a browser.
    #[must_use = "the listener is removed when the subscription is dropped"]
    pub fn on_external_change_local(&self, on_change: impl Fn(Option<&str>) + 'static) -> LocalSubscription {
        let owner = super::local::next_id();
        let listener: LocalOnChange = Rc::new(on_change);
        let local = super::local::add(owner, listener);
        let keys = self.inner.keys.clone();
        let watch = self.inner.storage.watch(Arc::new(move |key: Option<&str>| {
            let table = match key {
                None => None,
                Some(k) => match keys.parse(k) {
                    Some((table, _)) => Some(table),
                    None => return,
                },
            };
            for f in super::local::listeners::<LocalOnChange>(owner) {
                f(table);
            }
        }));
        local.with_watch(watch)
    }

    /// The underlying in-memory engine. Statements run on it directly skip
    /// the check for changes made by other tabs.
    pub fn memory(&self) -> &MemoryDb {
        &self.inner.db
    }

    /// The tables saved in the storage.
    fn stored_tables(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = self
            .inner
            .storage
            .keys()?
            .iter()
            .filter_map(|k| match self.inner.keys.parse(k) {
                Some((table, None)) => Some(table.to_owned()),
                _ => None,
            })
            .collect();
        names.extend(self.table_names());
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Reads again the tables another tab changed since this page last saw
    /// them (one storage read per table).
    fn sync<'t>(&self, tables: impl IntoIterator<Item = &'t str>) -> Result<()> {
        let mut stale = Vec::new();
        {
            let known = self.inner.known.lock().unwrap_or_else(|e| e.into_inner());
            for table in tables {
                let stored = stored_meta(&*self.inner.storage, &self.inner.keys, table)?.map(|m| m.ver);
                if stored != known.get(table).copied() {
                    stale.push(table.to_owned());
                }
            }
        }
        if stale.is_empty() {
            return Ok(());
        }
        let mut stored = load(&*self.inner.storage, &self.inner.keys, Some(&stale))?;
        for table in stale {
            // The engine's lock is never taken while holding `known` (the
            // journal takes them in the other order).
            let found = stored.remove(&table);
            let ver = found.as_ref().map(|(v, _)| *v);
            self.inner.db.replace_stored(&table, found.map(|(_, p)| p));
            let mut known = self.inner.known.lock().unwrap_or_else(|e| e.into_inner());
            match ver {
                Some(v) => known.insert(table, v),
                None => known.remove(&table),
            };
        }
        Ok(())
    }

    fn sync_query(&self, query: &Query<'_>) -> Result<()> {
        match *query {
            Query::DropTable { name, .. } => self.sync([name]),
            _ => self.sync(schemas_of(query).iter().map(|s| s.name)),
        }
    }
}

impl Executor for LocalStorageDb {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        self.sync_query(&query)?;
        self.inner.db.fetch_all(query).await
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        self.sync_query(&query)?;
        self.inner.db.execute(query).await
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        self.sync([table.name])?;
        self.inner.db.insert_rows(table, rows).await
    }
}

impl Database for LocalStorageDb {
    type Tx<'c> = MemoryTransaction;

    /// Starts a transaction on the current rows of every table (read again
    /// first if another tab changed them). Its commit fails with
    /// [`Error::Conflict`] if another tab wrote one of its tables meanwhile.
    async fn begin(&self) -> Result<MemoryTransaction> {
        let tables = self.stored_tables()?;
        self.sync(tables.iter().map(String::as_str))?;
        self.inner.db.begin().await
    }
}
