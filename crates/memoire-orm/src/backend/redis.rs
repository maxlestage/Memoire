//! Redis backend (feature `redis`, requires a Tokio runtime).
//!
//! # Data layout
//!
//! Every table uses a handful of keys sharing the `{table}` hash tag:
//!
//! | Key | Type | Content |
//! |---|---|---|
//! | `<prefix>:tables` | set | names of the existing tables |
//! | `<prefix>:{<table>}:rows` | hash | primary key → row as a JSON object |
//! | `<prefix>:{<table>}:u:<column>` | hash | unique value → primary key |
//! | `<prefix>:{<table>}:u:<index>` | hash | JSON array of the values of a unique multi-column index → primary key |
//! | `<prefix>:{<table>}:seq` | string | last generated id |
//! | `<prefix>:{<table>}:ver` | string | version, bumped by every write |
//! | `<prefix>:{<table>}:meta` | hash | column list and key names |
//!
//! Rows are readable JSON (`HGET memoire:{users}:rows 1`), so other programs can
//! read them too.
//!
//! # Consistency
//!
//! Every write is a single Lua script: it first checks its preconditions (table
//! exists, unique values free, row or table version unchanged), then applies all
//! its changes, so writes are atomic. Writes that depend on data already read
//! (updates, deletes, transactions) are optimistic: if another client changed the
//! data in between, single statements are retried automatically and
//! transactions fail with [`Error::Conflict`].
//!
//! # Queries
//!
//! Primary-key lookups (`find`, `filter(pk = ..)`, `filter(pk IN ..)`) read only
//! the requested rows and `COUNT(*)` without filter uses `HLEN`. Other queries
//! load the tables they use and are evaluated by the in-memory engine, with
//! exactly the same semantics as [`MemoryDb`](crate::MemoryDb). Redis is a great
//! fit for caches, sessions, queues of records and small-to-medium datasets.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, LazyLock};

use async_lock::Mutex;
use redis::aio::ConnectionManager;
use redis::{FromRedisValue, Script, Value as RValue};

use crate::ast::{InsertAst, Projection, Query, SelectAst};
use crate::backend::json_row::{decode_row, encode_row, field};
use crate::backend::memory::{self as mem, State, Table, validate_row};
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::expr::{AggFunc, BinOp, Expr};
use crate::row::FromRow;
use crate::schema::{Dialect, TableSchema};
use crate::value::Value;

/// Checks every precondition first, then applies every write.
const APPLY_LUA: &str = r#"
local arity = {V=2, A=3, E=3, M=2, N=2, HSET=3, HDEL=2, INCR=1, SETMAX=2, DEL=1, SADD=2, SREM=2}
local i = 1
while i <= #ARGV do
  local op = ARGV[i]
  local k = KEYS[tonumber(ARGV[i + 1])]
  if op == 'V' then
    local cur = redis.call('GET', k)
    if cur == false then cur = '' end
    if cur ~= ARGV[i + 2] then return {'conflict', ARGV[i + 1]} end
  elseif op == 'A' then
    if redis.call('HEXISTS', k, ARGV[i + 2]) == 1 then return {'dup', ARGV[i + 3]} end
  elseif op == 'E' then
    local cur = redis.call('HGET', k, ARGV[i + 2])
    if cur == false then cur = '' end
    if cur ~= ARGV[i + 3] then return {'conflict', ARGV[i + 1]} end
  elseif op == 'M' then
    if redis.call('SISMEMBER', k, ARGV[i + 2]) == 0 then return {'missing', ARGV[i + 2]} end
  elseif op == 'N' then
    if redis.call('SISMEMBER', k, ARGV[i + 2]) == 1 then return {'exists', ARGV[i + 2]} end
  end
  i = i + 1 + arity[op]
end
i = 1
while i <= #ARGV do
  local op = ARGV[i]
  local k = KEYS[tonumber(ARGV[i + 1])]
  if op == 'HSET' then redis.call('HSET', k, ARGV[i + 2], ARGV[i + 3])
  elseif op == 'HDEL' then redis.call('HDEL', k, ARGV[i + 2])
  elseif op == 'INCR' then redis.call('INCR', k)
  elseif op == 'SETMAX' then
    local cur = tonumber(redis.call('GET', k) or '0') or 0
    if tonumber(ARGV[i + 2]) > cur then redis.call('SET', k, ARGV[i + 2]) end
  elseif op == 'DEL' then redis.call('DEL', k)
  elseif op == 'SADD' then redis.call('SADD', k, ARGV[i + 2])
  elseif op == 'SREM' then redis.call('SREM', k, ARGV[i + 2])
  end
  i = i + 1 + arity[op]
end
return {'ok'}
"#;

/// Reserves `ARGV[2]` ids for table `ARGV[1]` if it exists; returns the last id.
const NEXT_IDS_LUA: &str = r#"
if redis.call('SISMEMBER', KEYS[1], ARGV[1]) == 0 then return false end
return redis.call('INCRBY', KEYS[2], ARGV[2])
"#;

static APPLY: LazyLock<Script> = LazyLock::new(|| Script::new(APPLY_LUA));
static NEXT_IDS: LazyLock<Script> = LazyLock::new(|| Script::new(NEXT_IDS_LUA));

/// Retries of a single statement that lost an optimistic race.
const MAX_RETRIES: usize = 64;

fn redis_err(e: redis::RedisError) -> Error {
    Error::Database(Box::new(e))
}

fn parse<T: FromRedisValue>(v: RValue) -> Result<T> {
    redis::from_redis_value(v).map_err(|e| Error::Conversion(format!("unexpected Redis reply: {e}")))
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Keys {
    prefix: Arc<str>,
}

impl Keys {
    fn tables(&self) -> String {
        format!("{}:tables", self.prefix)
    }
    fn rows(&self, t: &str) -> String {
        format!("{}:{{{t}}}:rows", self.prefix)
    }
    fn seq(&self, t: &str) -> String {
        format!("{}:{{{t}}}:seq", self.prefix)
    }
    fn ver(&self, t: &str) -> String {
        format!("{}:{{{t}}}:ver", self.prefix)
    }
    fn meta(&self, t: &str) -> String {
        format!("{}:{{{t}}}:meta", self.prefix)
    }
    fn unique(&self, t: &str, column: &str) -> String {
        format!("{}:{{{t}}}:u:{column}", self.prefix)
    }
    /// Keys removed by `DROP TABLE` (the version key is kept so that versions
    /// never repeat).
    fn data_keys(&self, schema: &TableSchema) -> Vec<String> {
        let mut v = vec![self.rows(schema.name), self.seq(schema.name)];
        for u in unique_keys(schema) {
            v.push(self.unique(schema.name, u.name));
        }
        v
    }
}

/// A unique constraint besides the primary key, kept in a `u:<name>` hash: a
/// `unique` column, or a unique index over several columns.
struct UniqueKey {
    cols: Vec<usize>,
    /// Suffix of the hash key: the column or index name.
    name: &'static str,
    /// Reported in [`Error::UniqueViolation`]: `email`, or `user_id, team_id`.
    label: String,
}

impl UniqueKey {
    /// The hash field of `row`'s key; `None` when one of the columns is NULL
    /// (SQL lets such rows repeat).
    fn field(&self, row: &[Value]) -> Option<String> {
        if self.cols.iter().any(|&c| row[c].is_null()) {
            return None;
        }
        match *self.cols {
            [c] => Some(field(&row[c])),
            _ => {
                let parts: Vec<String> = self.cols.iter().map(|&c| field(&row[c])).collect();
                serde_json::to_string(&parts).ok()
            }
        }
    }
}

fn unique_keys(schema: &TableSchema) -> Vec<UniqueKey> {
    let columns = schema
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.unique && !c.primary_key)
        .map(|(i, c)| UniqueKey { cols: vec![i], name: c.name, label: c.name.to_owned() });
    let indexes = schema.indexes.iter().filter(|ix| ix.unique).map(|ix| UniqueKey {
        cols: ix.columns.to_vec(),
        name: ix.name,
        label: ix.label(schema),
    });
    columns.chain(indexes).collect()
}

// ---------------------------------------------------------------------------
// Atomic scripts
// ---------------------------------------------------------------------------

enum Outcome {
    Ok,
    Conflict(String),
    Duplicate(String),
    Missing(String),
    Exists(String),
}

#[derive(Default)]
struct Ops {
    keys: Vec<String>,
    index: HashMap<String, usize>,
    checks: Vec<String>,
    deletes: Vec<String>,
    writes: Vec<String>,
}

impl Ops {
    fn key(&mut self, key: String) -> String {
        let next = self.keys.len() + 1;
        let i = *self.index.entry(key.clone()).or_insert_with(|| next);
        if i == next {
            self.keys.push(key);
        }
        i.to_string()
    }

    fn check(&mut self, op: &str, key: String, args: &[&str]) {
        let k = self.key(key);
        self.checks.push(op.to_owned());
        self.checks.push(k);
        self.checks.extend(args.iter().map(|a| (*a).to_owned()));
    }

    fn version(&mut self, ver_key: String, expected: &str) {
        self.check("V", ver_key, &[expected]);
    }
    fn absent(&mut self, hash: String, field: &str, label: &str) {
        self.check("A", hash, &[field, label]);
    }
    fn unchanged(&mut self, hash: String, field: &str, expected: &str) {
        self.check("E", hash, &[field, expected]);
    }
    fn member(&mut self, set: String, member: &str) {
        self.check("M", set, &[member]);
    }
    fn not_member(&mut self, set: String, member: &str) {
        self.check("N", set, &[member]);
    }

    fn write(&mut self, delete_phase: bool, op: &str, key: String, args: &[&str]) {
        let k = self.key(key);
        let target = if delete_phase { &mut self.deletes } else { &mut self.writes };
        target.push(op.to_owned());
        target.push(k);
        target.extend(args.iter().map(|a| (*a).to_owned()));
    }
    fn hset(&mut self, hash: String, field: &str, value: &str) {
        self.write(false, "HSET", hash, &[field, value]);
    }
    /// Deletions run before every `HSET` of the same script.
    fn hdel(&mut self, hash: String, field: &str) {
        self.write(true, "HDEL", hash, &[field]);
    }
    fn incr(&mut self, key: String) {
        self.write(false, "INCR", key, &[]);
    }
    fn setmax(&mut self, key: String, n: i64) {
        self.write(false, "SETMAX", key, &[&n.to_string()]);
    }
    fn del(&mut self, key: String) {
        self.write(true, "DEL", key, &[]);
    }
    fn sadd(&mut self, set: String, member: &str) {
        self.write(false, "SADD", set, &[member]);
    }
    fn srem(&mut self, set: String, member: &str) {
        self.write(true, "SREM", set, &[member]);
    }

    async fn run(self, conn: &mut ConnectionManager) -> Result<Outcome> {
        let mut inv = APPLY.prepare_invoke();
        for k in &self.keys {
            inv.key(k);
        }
        for a in self.checks.iter().chain(&self.deletes).chain(&self.writes) {
            inv.arg(a);
        }
        let reply: Vec<String> = inv.invoke_async(conn).await.map_err(redis_err)?;
        let arg = |i: usize| reply.get(i).cloned().unwrap_or_default();
        Ok(match reply.first().map(String::as_str) {
            Some("ok") => Outcome::Ok,
            Some("conflict") => {
                let key = arg(1).parse::<usize>().ok().and_then(|i| self.keys.get(i - 1).cloned()).unwrap_or_default();
                Outcome::Conflict(table_of_key(&key))
            }
            Some("dup") => Outcome::Duplicate(arg(1)),
            Some("missing") => Outcome::Missing(arg(1)),
            Some("exists") => Outcome::Exists(arg(1)),
            _ => return Err(Error::Conversion(format!("unexpected script reply {reply:?}"))),
        })
    }
}

/// `prefix:{users}:ver` → `users`.
fn table_of_key(key: &str) -> String {
    key.split_once('{').and_then(|(_, r)| r.split_once('}')).map(|(t, _)| t.to_owned()).unwrap_or_default()
}

/// Adds the operations turning `old` rows into `new` rows (rows and unique
/// indexes).
fn diff(ops: &mut Ops, keys: &Keys, schema: &TableSchema, old: &HashMap<String, String>, new_rows: &[Vec<Value>]) {
    let pk = schema.primary_key;
    let mut seen: HashSet<String> = HashSet::with_capacity(new_rows.len());
    let uniques = unique_keys(schema);
    for row in new_rows {
        let f = field(&row[pk]);
        let json = encode_row(schema, row);
        let previous = old.get(&f);
        seen.insert(f.clone());
        if previous == Some(&json) {
            continue;
        }
        let old_row = previous.and_then(|j| decode_row(schema, j).ok());
        for u in &uniques {
            let before = old_row.as_deref().and_then(|r| u.field(r));
            let after = u.field(row);
            if before != after {
                if let Some(b) = before {
                    ops.hdel(keys.unique(schema.name, u.name), &b);
                }
                if let Some(a) = after {
                    ops.hset(keys.unique(schema.name, u.name), &a, &f);
                }
            }
        }
        ops.hset(keys.rows(schema.name), &f, &json);
    }
    for (f, json) in old {
        if seen.contains(f) {
            continue;
        }
        ops.hdel(keys.rows(schema.name), f);
        if let Ok(row) = decode_row(schema, json) {
            for u in &uniques {
                if let Some(b) = u.field(&row) {
                    ops.hdel(keys.unique(schema.name, u.name), &b);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

struct Snapshot {
    exists: bool,
    ver: String,
    seq: i64,
    raw: HashMap<String, String>,
}

impl Snapshot {
    fn table(&self, schema: &'static TableSchema) -> Result<Table> {
        let rows = self.raw.values().map(|j| decode_row(schema, j)).collect::<Result<Vec<_>>>()?;
        Ok(Table::from_rows(schema, rows, self.seq))
    }
}

/// Tables a statement reads or writes.
fn tables_of(query: &Query<'_>) -> Vec<&'static TableSchema> {
    match *query {
        Query::Select(s) => s.tables().collect(),
        Query::Insert(i) => vec![i.table],
        Query::Update(u) => vec![u.table],
        Query::Delete(d) => vec![d.table],
        Query::FindByKey { table, .. }
        | Query::InsertRow { table, .. }
        | Query::UpsertRow { table, .. }
        | Query::UpdateRow { table, .. }
        | Query::DeleteByKey { table, .. }
        | Query::CreateTable { table, .. } => vec![table],
        _ => Vec::new(),
    }
}

/// Primary keys pinned by a filter on the root table (`pk = v`, `pk IN (..)`).
fn pinned_keys<'a>(filter: &'a Expr, table: &TableSchema) -> Option<Vec<&'a Value>> {
    let is_pk = |e: &Expr| matches!(e, Expr::Column(c) if c.table == table.name && c.index == table.primary_key);
    match filter {
        Expr::Binary { op: BinOp::Eq, left, right } => match (&**left, &**right) {
            (l, Expr::Value(v)) if is_pk(l) => Some(vec![v]),
            (Expr::Value(v), r) if is_pk(r) => Some(vec![v]),
            _ => None,
        },
        Expr::InList { expr, list, negated: false } if is_pk(expr) => Some(list.iter().collect()),
        Expr::Binary { op: BinOp::And, left, right } => pinned_keys(left, table).or_else(|| pinned_keys(right, table)),
        _ => None,
    }
}

fn is_plain_count(s: &SelectAst) -> bool {
    s.joins.is_empty()
        && s.filter.is_none()
        && s.group_by.is_empty()
        && s.having.is_none()
        && !s.distinct
        && matches!(&s.projection, Projection::Exprs(e)
            if matches!(e.as_slice(), [Expr::Aggregate { func: AggFunc::Count, arg: None, distinct: false }]))
}

// ---------------------------------------------------------------------------
// RedisDb
// ---------------------------------------------------------------------------

/// A Redis database used through the Memoire API.
///
/// ```no_run
/// # async fn demo() -> memoire_orm::Result<()> {
/// use memoire_orm::prelude::*;
/// let db = RedisDb::connect("redis://127.0.0.1:6379").await?;
/// # Ok(()) }
/// ```
///
/// The handle multiplexes commands on one auto-reconnecting connection; cloning
/// it is cheap. See the [module documentation](self) for the data layout.
#[derive(Clone)]
pub struct RedisDb {
    conn: ConnectionManager,
    keys: Keys,
}

impl fmt::Debug for RedisDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisDb").field("prefix", &self.keys.prefix).finish_non_exhaustive()
    }
}

impl RedisDb {
    /// Connects with a `redis://` (or `rediss://` for TLS) URL, using the key
    /// prefix `memoire`.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with_prefix(url, "memoire").await
    }

    /// Connects and stores every key under `prefix:` (several applications
    /// can share one Redis server with different prefixes).
    pub async fn connect_with_prefix(url: &str, prefix: &str) -> Result<Self> {
        let client = redis::Client::open(url).map_err(redis_err)?;
        let conn = ConnectionManager::new(client).await.map_err(redis_err)?;
        Ok(RedisDb { conn, keys: Keys { prefix: Arc::from(prefix) } })
    }

    /// The underlying `redis` connection (escape hatch).
    pub fn connection(&self) -> ConnectionManager {
        self.conn.clone()
    }

    async fn snapshot(&self, tables: &[&str]) -> Result<Vec<Snapshot>> {
        let mut conn = self.conn.clone();
        let mut pipe = redis::pipe();
        pipe.atomic();
        for t in tables {
            pipe.cmd("SISMEMBER").arg(self.keys.tables()).arg(*t);
            pipe.cmd("GET").arg(self.keys.ver(t));
            pipe.cmd("GET").arg(self.keys.seq(t));
            pipe.cmd("HGETALL").arg(self.keys.rows(t));
        }
        let replies: Vec<RValue> = pipe.query_async(&mut conn).await.map_err(redis_err)?;
        let mut it = replies.into_iter();
        let mut out = Vec::with_capacity(tables.len());
        for _ in tables {
            let exists: bool = parse(it.next().unwrap_or_default())?;
            let ver: Option<String> = parse(it.next().unwrap_or_default())?;
            let seq: Option<i64> = parse(it.next().unwrap_or_default())?;
            let raw: HashMap<String, String> = parse(it.next().unwrap_or_default())?;
            out.push(Snapshot { exists, ver: ver.unwrap_or_default(), seq: seq.unwrap_or(0), raw });
        }
        Ok(out)
    }

    async fn fetch_rows(&self, query: &Query<'_>) -> Result<Vec<Vec<Value>>> {
        let mut conn = self.conn.clone();
        match *query {
            Query::FindByKey { table, key } => {
                let (exists, json): (bool, Option<String>) = redis::pipe()
                    .atomic()
                    .cmd("SISMEMBER")
                    .arg(self.keys.tables())
                    .arg(table.name)
                    .cmd("HGET")
                    .arg(self.keys.rows(table.name))
                    .arg(field(key))
                    .query_async(&mut conn)
                    .await
                    .map_err(redis_err)?;
                if !exists {
                    return Err(Error::NoSuchTable(table.name.to_owned()));
                }
                json.map(|j| decode_row(table, &j)).into_iter().collect()
            }
            Query::Select(s) => {
                if is_plain_count(s) {
                    let (exists, n): (bool, i64) = redis::pipe()
                        .atomic()
                        .cmd("SISMEMBER")
                        .arg(self.keys.tables())
                        .arg(s.from.name)
                        .cmd("HLEN")
                        .arg(self.keys.rows(s.from.name))
                        .query_async(&mut conn)
                        .await
                        .map_err(redis_err)?;
                    if !exists {
                        return Err(Error::NoSuchTable(s.from.name.to_owned()));
                    }
                    return Ok(vec![vec![Value::I64(n)]]);
                }
                let mut state = State::default();
                if s.joins.is_empty()
                    && let Some(pinned) = s.filter.as_ref().and_then(|f| pinned_keys(f, s.from))
                {
                    // Read only the pinned rows (each once: `pk IN (1, 1)` is one row).
                    let mut fields: Vec<String> = pinned.iter().map(|v| field(v)).collect();
                    let mut seen = HashSet::with_capacity(fields.len());
                    fields.retain(|f| seen.insert(f.clone()));
                    let mut pipe = redis::pipe();
                    pipe.atomic().cmd("SISMEMBER").arg(self.keys.tables()).arg(s.from.name);
                    if !fields.is_empty() {
                        pipe.cmd("HMGET").arg(self.keys.rows(s.from.name)).arg(&fields);
                    }
                    let mut replies: Vec<RValue> = pipe.query_async(&mut conn).await.map_err(redis_err)?;
                    let exists: bool = parse(replies.remove(0))?;
                    if !exists {
                        return Err(Error::NoSuchTable(s.from.name.to_owned()));
                    }
                    let found: Vec<Option<String>> =
                        if fields.is_empty() { Vec::new() } else { parse(replies.remove(0))? };
                    let rows = found.iter().flatten().map(|j| decode_row(s.from, j)).collect::<Result<Vec<_>>>()?;
                    state.put(Table::from_rows(s.from, rows, 0));
                } else {
                    let tables: Vec<&'static TableSchema> = s.tables().collect();
                    let names: Vec<&str> = tables.iter().map(|t| t.name).collect();
                    for (schema, snap) in tables.iter().zip(self.snapshot(&names).await?) {
                        if !snap.exists {
                            return Err(Error::NoSuchTable(schema.name.to_owned()));
                        }
                        state.put(snap.table(schema)?);
                    }
                }
                mem::fetch(&state, query)
            }
            Query::Raw { .. } | Query::Script { .. } => {
                Err(Error::Unsupported("raw SQL is not supported by the Redis backend".into()))
            }
            _ => Err(Error::Unsupported("statement does not return rows".into())),
        }
    }

    /// Reserves `n` ids; returns the first one.
    async fn reserve_ids(&self, table: &str, n: usize) -> Result<i64> {
        let mut conn = self.conn.clone();
        let last: Option<i64> = NEXT_IDS
            .key(self.keys.tables())
            .key(self.keys.seq(table))
            .arg(table)
            .arg(n)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;
        let last = last.ok_or_else(|| Error::NoSuchTable(table.to_owned()))?;
        Ok(last - n as i64 + 1)
    }

    async fn apply(&self, ops: Ops, table: &TableSchema) -> Result<Outcome> {
        let mut conn = self.conn.clone();
        let outcome = ops.run(&mut conn).await?;
        Ok(match outcome {
            Outcome::Missing(t) => return Err(Error::NoSuchTable(t)),
            Outcome::Duplicate(column) => {
                return Err(Error::UniqueViolation { table: table.name.to_owned(), column });
            }
            other => other,
        })
    }

    fn insert_ops(&self, ops: &mut Ops, table: &TableSchema, row: &[Value]) -> Result<()> {
        validate_row(table, row)?;
        let pk = table.primary_key;
        let f = field(&row[pk]);
        ops.absent(self.keys.rows(table.name), &f, table.pk().name);
        for u in unique_keys(table) {
            if let Some(v) = u.field(row) {
                ops.absent(self.keys.unique(table.name, u.name), &v, &u.label);
                ops.hset(self.keys.unique(table.name, u.name), &v, &f);
            }
        }
        ops.hset(self.keys.rows(table.name), &f, &encode_row(table, row));
        if !table.auto_increment()
            && let Some(k) = row[pk].as_i64()
        {
            ops.setmax(self.keys.seq(table.name), k);
        }
        Ok(())
    }

    async fn insert_row(&self, table: &'static TableSchema, values: &[Value]) -> Result<ExecResult> {
        let mut row = values.to_vec();
        let generated = if table.auto_increment() {
            let id = self.reserve_ids(table.name, 1).await?;
            row[table.primary_key] = Value::I64(id);
            Some(Value::I64(id))
        } else {
            None
        };
        let mut ops = Ops::default();
        ops.member(self.keys.tables(), table.name);
        self.insert_ops(&mut ops, table, &row)?;
        ops.incr(self.keys.ver(table.name));
        self.apply(ops, table).await?;
        Ok(ExecResult { rows_affected: 1, last_insert_id: generated })
    }

    async fn insert_batch(&self, ins: &InsertAst) -> Result<u64> {
        let table = ins.table;
        if ins.rows.is_empty() {
            return Ok(0);
        }
        let mut rows = ins.rows.clone();
        if table.auto_increment() {
            let first = self.reserve_ids(table.name, rows.len()).await?;
            for (i, r) in rows.iter_mut().enumerate() {
                r[table.primary_key] = Value::I64(first + i as i64);
            }
        }
        // Duplicates inside the batch itself.
        let mut seen: HashSet<(&str, String)> = HashSet::new();
        let pk = UniqueKey { cols: vec![table.primary_key], name: table.pk().name, label: table.pk().name.to_owned() };
        let indexed: Vec<UniqueKey> = std::iter::once(pk).chain(unique_keys(table)).collect();
        for r in &rows {
            for u in &indexed {
                if let Some(v) = u.field(r)
                    && !seen.insert((u.name, v))
                {
                    return Err(Error::UniqueViolation { table: table.name.to_owned(), column: u.label.clone() });
                }
            }
        }
        let mut ops = Ops::default();
        ops.member(self.keys.tables(), table.name);
        for r in &rows {
            self.insert_ops(&mut ops, table, r)?;
        }
        ops.incr(self.keys.ver(table.name));
        self.apply(ops, table).await?;
        Ok(rows.len() as u64)
    }

    async fn read_row(&self, table: &TableSchema, key: &str) -> Result<Option<String>> {
        let mut conn = self.conn.clone();
        let (exists, json): (bool, Option<String>) = redis::pipe()
            .atomic()
            .cmd("SISMEMBER")
            .arg(self.keys.tables())
            .arg(table.name)
            .cmd("HGET")
            .arg(self.keys.rows(table.name))
            .arg(key)
            .query_async(&mut conn)
            .await
            .map_err(redis_err)?;
        if !exists {
            return Err(Error::NoSuchTable(table.name.to_owned()));
        }
        Ok(json)
    }

    /// Replaces (or deletes when `new` is `None`) one row, if it still holds
    /// `old_json`.
    fn replace_ops(
        &self,
        ops: &mut Ops,
        table: &TableSchema,
        f: &str,
        old_json: &str,
        new: Option<&[Value]>,
    ) -> Result<()> {
        let old = decode_row(table, old_json)?;
        ops.unchanged(self.keys.rows(table.name), f, old_json);
        for u in unique_keys(table) {
            let before = u.field(&old);
            let after = new.and_then(|r| u.field(r));
            if before != after {
                if let Some(b) = &before {
                    ops.hdel(self.keys.unique(table.name, u.name), b);
                }
                if let Some(a) = &after {
                    ops.absent(self.keys.unique(table.name, u.name), a, &u.label);
                    ops.hset(self.keys.unique(table.name, u.name), a, f);
                }
            }
        }
        match new {
            Some(row) => {
                validate_row(table, row)?;
                ops.hset(self.keys.rows(table.name), f, &encode_row(table, row));
            }
            None => ops.hdel(self.keys.rows(table.name), f),
        }
        ops.incr(self.keys.ver(table.name));
        Ok(())
    }

    async fn update_row(&self, table: &'static TableSchema, values: &[Value]) -> Result<u64> {
        let f = field(&values[table.primary_key]);
        for _ in 0..MAX_RETRIES {
            let Some(old) = self.read_row(table, &f).await? else { return Ok(0) };
            let mut ops = Ops::default();
            self.replace_ops(&mut ops, table, &f, &old, Some(values))?;
            if let Outcome::Ok = self.apply(ops, table).await? {
                return Ok(1);
            }
        }
        Err(Error::Conflict(table.name.to_owned()))
    }

    async fn delete_row(&self, table: &'static TableSchema, key: &Value) -> Result<u64> {
        let f = field(key);
        for _ in 0..MAX_RETRIES {
            let Some(old) = self.read_row(table, &f).await? else { return Ok(0) };
            let mut ops = Ops::default();
            self.replace_ops(&mut ops, table, &f, &old, None)?;
            if let Outcome::Ok = self.apply(ops, table).await? {
                return Ok(1);
            }
        }
        Err(Error::Conflict(table.name.to_owned()))
    }

    async fn upsert_row(&self, table: &'static TableSchema, values: &[Value]) -> Result<()> {
        let f = field(&values[table.primary_key]);
        for _ in 0..MAX_RETRIES {
            let mut ops = Ops::default();
            match self.read_row(table, &f).await? {
                Some(old) => self.replace_ops(&mut ops, table, &f, &old, Some(values))?,
                None => {
                    ops.member(self.keys.tables(), table.name);
                    self.insert_ops(&mut ops, table, values)?;
                    ops.incr(self.keys.ver(table.name));
                }
            }
            if let Some(k) = values[table.primary_key].as_i64() {
                ops.setmax(self.keys.seq(table.name), k);
            }
            match self.apply(ops, table).await {
                Ok(Outcome::Ok) => return Ok(()),
                Ok(_) => continue,
                // Somebody inserted the same key concurrently: retry as an update.
                Err(Error::UniqueViolation { column, .. }) if column == table.pk().name => continue,
                Err(e) => return Err(e),
            }
        }
        Err(Error::Conflict(table.name.to_owned()))
    }

    /// `UPDATE` / `DELETE` with a filter: evaluated by the memory engine on a
    /// snapshot, then applied if the table did not change meanwhile.
    async fn write_with_snapshot(&self, query: &Query<'_>, table: &'static TableSchema) -> Result<ExecResult> {
        for _ in 0..MAX_RETRIES {
            let snap = self.snapshot(&[table.name]).await?.pop().unwrap_or(Snapshot {
                exists: false,
                ver: String::new(),
                seq: 0,
                raw: HashMap::new(),
            });
            if !snap.exists {
                return Err(Error::NoSuchTable(table.name.to_owned()));
            }
            let mut state = State::default();
            state.put(snap.table(table)?);
            let (result, _) = mem::execute(&mut state, query)?;
            if result.rows_affected == 0 {
                return Ok(result);
            }
            let mut ops = Ops::default();
            ops.version(self.keys.ver(table.name), &snap.ver);
            let new_rows = state.get(table.name).map(|t| t.rows.as_slice()).unwrap_or_default();
            diff(&mut ops, &self.keys, table, &snap.raw, new_rows);
            ops.incr(self.keys.ver(table.name));
            if let Outcome::Ok = self.apply(ops, table).await? {
                return Ok(result);
            }
        }
        Err(Error::Conflict(table.name.to_owned()))
    }

    async fn create_table(&self, table: &'static TableSchema, if_not_exists: bool) -> Result<()> {
        let mut ops = Ops::default();
        ops.not_member(self.keys.tables(), table.name);
        self.create_ops(&mut ops, table);
        match self.apply(ops, table).await? {
            Outcome::Exists(_) if if_not_exists => Ok(()),
            Outcome::Exists(t) => Err(Error::TableExists(t)),
            _ => Ok(()),
        }
    }

    fn create_ops(&self, ops: &mut Ops, table: &'static TableSchema) {
        ops.sadd(self.keys.tables(), table.name);
        let columns: Vec<&str> = table.columns.iter().map(|c| c.name).collect();
        let meta = self.keys.meta(table.name);
        ops.hset(meta.clone(), "columns", &serde_json::to_string(&columns).unwrap_or_default());
        ops.hset(meta, "keys", &serde_json::to_string(&self.keys.data_keys(table)).unwrap_or_default());
        ops.incr(self.keys.ver(table.name));
    }

    /// Operations dropping a table whose data keys are listed in its metadata.
    async fn drop_ops(&self, ops: &mut Ops, name: &str) -> Result<()> {
        let mut conn = self.conn.clone();
        let keys: Option<String> =
            redis::cmd("HGET").arg(self.keys.meta(name)).arg("keys").query_async(&mut conn).await.map_err(redis_err)?;
        let keys: Vec<String> = keys.and_then(|k| serde_json::from_str(&k).ok()).unwrap_or_default();
        for k in keys {
            ops.del(k);
        }
        ops.del(self.keys.meta(name));
        ops.srem(self.keys.tables(), name);
        ops.incr(self.keys.ver(name));
        Ok(())
    }

    async fn drop_table(&self, name: &str, if_exists: bool) -> Result<()> {
        let mut ops = Ops::default();
        ops.member(self.keys.tables(), name);
        self.drop_ops(&mut ops, name).await?;
        let mut conn = self.conn.clone();
        match ops.run(&mut conn).await? {
            Outcome::Missing(_) if if_exists => Ok(()),
            Outcome::Missing(t) => Err(Error::NoSuchTable(t)),
            _ => Ok(()),
        }
    }
}

impl Executor for RedisDb {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        mem::decode(self.fetch_rows(&query).await?)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let done = |n: u64| Ok(ExecResult { rows_affected: n, last_insert_id: None });
        match query {
            Query::InsertRow { table, values } => {
                check_width(table, values)?;
                self.insert_row(table, values).await
            }
            Query::UpdateRow { table, values } => {
                check_width(table, values)?;
                done(self.update_row(table, values).await?)
            }
            Query::UpsertRow { table, values } => {
                check_width(table, values)?;
                self.upsert_row(table, values).await?;
                done(1)
            }
            Query::DeleteByKey { table, key } => done(self.delete_row(table, key).await?),
            Query::Insert(ins) => done(self.insert_batch(ins).await?),
            Query::Update(u) => self.write_with_snapshot(&query, u.table).await,
            Query::Delete(d) => self.write_with_snapshot(&query, d.table).await,
            Query::CreateTable { table, if_not_exists } => {
                self.create_table(table, if_not_exists).await?;
                done(0)
            }
            Query::DropTable { name, if_exists } => {
                self.drop_table(name, if_exists).await?;
                done(0)
            }
            Query::Raw { .. } | Query::Script { .. } => {
                Err(Error::Unsupported("raw SQL is not supported by the Redis backend".into()))
            }
            _ => Err(Error::Unsupported("use `fetch_all` for statements returning rows".into())),
        }
    }
}

fn check_width(table: &TableSchema, values: &[Value]) -> Result<()> {
    if values.len() == table.columns.len() {
        Ok(())
    } else {
        Err(Error::Conversion(format!("`{}` expects {} values, got {}", table.name, table.columns.len(), values.len())))
    }
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

struct Loaded {
    schema: Option<&'static TableSchema>,
    existed: bool,
    ver: String,
    raw: HashMap<String, String>,
    dirty: bool,
    dropped: bool,
}

#[derive(Default)]
struct TxState {
    state: State,
    loaded: HashMap<String, Loaded>,
}

impl Database for RedisDb {
    type Tx<'c> = RedisTransaction;

    async fn begin(&self) -> Result<RedisTransaction> {
        Ok(RedisTransaction { db: self.clone(), inner: Mutex::new(TxState::default()) })
    }
}

/// A Redis transaction.
///
/// Each table is read once (a consistent snapshot) the first time the
/// transaction uses it; every statement then runs on that private copy.
/// [`commit`](Transaction::commit) writes all changes in one atomic Lua script,
/// or fails with [`Error::Conflict`] if another client modified one of the
/// written tables since it was read. Dropping the transaction discards it.
pub struct RedisTransaction {
    db: RedisDb,
    inner: Mutex<TxState>,
}

impl fmt::Debug for RedisTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTransaction").finish_non_exhaustive()
    }
}

impl RedisTransaction {
    async fn ensure_loaded(&self, tx: &mut TxState, tables: &[&'static TableSchema]) -> Result<()> {
        let missing: Vec<&'static TableSchema> =
            tables.iter().copied().filter(|t| !tx.loaded.contains_key(t.name)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let names: Vec<&str> = missing.iter().map(|t| t.name).collect();
        for (schema, snap) in missing.iter().zip(self.db.snapshot(&names).await?) {
            if snap.exists {
                tx.state.put(snap.table(schema)?);
            }
            tx.loaded.insert(
                schema.name.to_owned(),
                Loaded {
                    schema: Some(schema),
                    existed: snap.exists,
                    ver: snap.ver,
                    raw: snap.raw,
                    dirty: false,
                    dropped: false,
                },
            );
        }
        Ok(())
    }

    async fn ensure_known(&self, tx: &mut TxState, name: &str) -> Result<()> {
        if tx.loaded.contains_key(name) {
            return Ok(());
        }
        let mut conn = self.db.conn.clone();
        let (exists, ver): (bool, Option<String>) = redis::pipe()
            .atomic()
            .cmd("SISMEMBER")
            .arg(self.db.keys.tables())
            .arg(name)
            .cmd("GET")
            .arg(self.db.keys.ver(name))
            .query_async(&mut conn)
            .await
            .map_err(redis_err)?;
        tx.loaded.insert(
            name.to_owned(),
            Loaded {
                schema: None,
                existed: exists,
                ver: ver.unwrap_or_default(),
                raw: HashMap::new(),
                dirty: false,
                dropped: false,
            },
        );
        Ok(())
    }
}

impl Executor for RedisTransaction {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        if matches!(query, Query::Raw { .. } | Query::Script { .. }) {
            return Err(Error::Unsupported("raw SQL is not supported by the Redis backend".into()));
        }
        let mut tx = self.inner.lock().await;
        self.ensure_loaded(&mut tx, &tables_of(&query)).await?;
        mem::fetch_decode(&tx.state, &query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let mut tx = self.inner.lock().await;
        match query {
            Query::Raw { .. } | Query::Script { .. } => {
                return Err(Error::Unsupported("raw SQL is not supported by the Redis backend".into()));
            }
            Query::DropTable { name, if_exists } => {
                self.ensure_known(&mut tx, name).await?;
                let tx = &mut *tx;
                let in_state = tx.state.remove(name);
                let loaded = tx.loaded.get_mut(name).ok_or_else(|| Error::NoSuchTable(name.to_owned()))?;
                // Present: loaded in this transaction, or existing in Redis and
                // not dropped yet.
                let present = in_state || (loaded.existed && !loaded.dropped && loaded.schema.is_none());
                if !present {
                    return if if_exists {
                        Ok(ExecResult::default())
                    } else {
                        Err(Error::NoSuchTable(name.to_owned()))
                    };
                }
                loaded.dirty = true;
                loaded.dropped = loaded.existed;
                loaded.schema = None;
                return Ok(ExecResult::default());
            }
            _ => {}
        }
        let tables = tables_of(&query);
        self.ensure_loaded(&mut tx, &tables).await?;
        let tx = &mut *tx;

        // Generated keys come from Redis so they never collide with concurrent
        // inserts; tables created inside this transaction use local ids.
        let reserve = |table: &'static TableSchema, tx: &TxState| {
            table.auto_increment() && tx.loaded.get(table.name).is_some_and(|l| l.existed)
        };
        let result = match query {
            Query::InsertRow { table, values } if reserve(table, tx) => {
                check_width(table, values)?;
                let id = self.db.reserve_ids(table.name, 1).await?;
                let mut row = values.to_vec();
                row[table.primary_key] = Value::I64(id);
                let t = tx.state.get_mut(table.name).ok_or_else(|| Error::NoSuchTable(table.name.to_owned()))?;
                t.insert(row, false)?;
                ExecResult { rows_affected: 1, last_insert_id: Some(Value::I64(id)) }
            }
            Query::Insert(ins) if reserve(ins.table, tx) && !ins.rows.is_empty() => {
                let first = self.db.reserve_ids(ins.table.name, ins.rows.len()).await?;
                let t =
                    tx.state.get_mut(ins.table.name).ok_or_else(|| Error::NoSuchTable(ins.table.name.to_owned()))?;
                let before = t.rows.len();
                for (i, row) in ins.rows.iter().enumerate() {
                    let mut row = row.clone();
                    row[ins.table.primary_key] = Value::I64(first + i as i64);
                    if let Err(e) = t.insert(row, false) {
                        let added: Vec<usize> = (before..t.rows.len()).collect();
                        t.remove_rows(added);
                        return Err(e);
                    }
                }
                ExecResult { rows_affected: ins.rows.len() as u64, last_insert_id: None }
            }
            Query::CreateTable { table, if_not_exists } => {
                if tx.state.get(table.name).is_some() {
                    if if_not_exists {
                        return Ok(ExecResult::default());
                    }
                    return Err(Error::TableExists(table.name.to_owned()));
                }
                mem::execute(&mut tx.state, &query)?.0
            }
            _ => mem::execute(&mut tx.state, &query)?.0,
        };
        for t in tables {
            if let Some(l) = tx.loaded.get_mut(t.name) {
                if !matches!(query, Query::Select(_) | Query::FindByKey { .. }) {
                    l.dirty = true;
                }
                if l.schema.is_none() && tx.state.get(t.name).is_some() {
                    l.schema = Some(t);
                }
            }
        }
        Ok(result)
    }
}

impl Transaction for RedisTransaction {
    async fn commit(self) -> Result<()> {
        let tx = self.inner.into_inner();
        let keys = &self.db.keys;
        let mut ops = Ops::default();
        let mut first_table: Option<&'static TableSchema> = None;
        let empty = HashMap::new();
        for (name, loaded) in &tx.loaded {
            if !loaded.dirty {
                continue;
            }
            if loaded.existed {
                ops.version(keys.ver(name), &loaded.ver);
            } else {
                ops.not_member(keys.tables(), name);
            }
            if loaded.dropped {
                self.db.drop_ops(&mut ops, name).await?;
            }
            if let (Some(schema), Some(table)) = (loaded.schema, tx.state.get(name)) {
                first_table.get_or_insert(schema);
                let recreated = loaded.dropped || !loaded.existed;
                if recreated {
                    self.db.create_ops(&mut ops, schema);
                }
                let old = if loaded.dropped { &empty } else { &loaded.raw };
                diff(&mut ops, keys, schema, old, &table.rows);
                ops.setmax(keys.seq(name), table.next_id - 1);
                ops.incr(keys.ver(name));
            }
        }
        if ops.checks.is_empty() && ops.writes.is_empty() && ops.deletes.is_empty() {
            return Ok(());
        }
        let mut conn = self.db.conn.clone();
        match ops.run(&mut conn).await? {
            Outcome::Ok => Ok(()),
            Outcome::Conflict(t) => Err(Error::Conflict(t)),
            Outcome::Exists(t) => Err(Error::Conflict(t)),
            Outcome::Missing(t) => Err(Error::NoSuchTable(t)),
            Outcome::Duplicate(column) => Err(Error::UniqueViolation {
                table: first_table.map(|t| t.name.to_owned()).unwrap_or_default(),
                column,
            }),
        }
    }

    async fn rollback(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_fields() {
        assert_eq!(field(&Value::I32(5)), field(&Value::I64(5)));
        assert_eq!(field(&Value::F64(5.0)), "5");
        assert_eq!(field(&Value::Bytes(vec![1, 255])), "01ff");
        assert_eq!(table_of_key("memoire:{users}:ver"), "users");
    }
}
