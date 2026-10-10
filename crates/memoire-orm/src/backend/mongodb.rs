//! MongoDB backend (feature `mongodb`, requires a Tokio runtime).
//!
//! # Mapping
//!
//! | Memoire | MongoDB |
//! |---|---|
//! | table | collection |
//! | row | document, the primary key is stored in `_id` |
//! | `NULL` | the field is absent (so unique indexes allow many `NULL`s, like SQL) |
//! | `#[memoire(unique)]` / `#[memoire(index)]` | unique (partial) / regular index named after the column |
//! | `auto_increment` | counter in the `__memoire_counters` collection |
//! | `DateTime<Utc>` / `NaiveDateTime` | BSON date (millisecond precision) |
//! | `NaiveDate` / `NaiveTime` | ISO 8601 strings (they sort chronologically) |
//! | `Uuid` | BSON binary, subtype 4 |
//! | JSON columns | embedded documents and arrays |
//!
//! # Queries
//!
//! `WHERE` clauses are translated to MongoDB filters that respect SQL's
//! three-valued logic (`NOT (age > 18)` does not match documents without an
//! age), and `ORDER BY`, `LIMIT` and `OFFSET` run on the server. Joins,
//! `GROUP BY`, aggregates and `DISTINCT` push the translatable part of the filter
//! to MongoDB and finish in the in-memory engine. `NULL`s sort first in
//! ascending order (MongoDB's rule); use `nulls_first()` / `nulls_last()` to
//! choose explicitly.
//!
//! # Transactions
//!
//! On a replica set or a sharded cluster (MongoDB Atlas, `--replSet`...),
//! [`Database::begin`] opens a real multi-document transaction, and Memoire
//! also wraps its own multi-document writes (`insert_many`, `update_many`) in a
//! transaction so they are all-or-nothing. A standalone server has no
//! transactions: `begin()` returns [`Error::Unsupported`] and multi-document
//! writes are best effort.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use async_lock::Mutex;
use mongodb::bson::{self, Bson, Document, doc, spec::BinarySubtype};
use mongodb::error::{ErrorKind, WriteFailure};
use mongodb::options::{IndexOptions, ReturnDocument};
use mongodb::{Client, ClientSession, Collection, IndexModel};
use parking_lot::RwLock;

use crate::ast::{JoinKind, Projection, Query, SelectAst};
use crate::backend::memory::{self as mem, State, Table, validate_row};
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::expr::{AggFunc, BinOp, ColumnRef, Expr, Nulls};
use crate::row::FromRow;
use crate::schema::{Dialect, TableSchema};
use crate::value::{ColumnType, Value};

const COUNTERS: &str = "__memoire_counters";

/// A session borrowed for one operation (`None` outside transactions).
type Session<'a> = Option<&'a mut ClientSession>;

/// Runs a driver action with or without the session.
macro_rules! run {
    ($action:expr, $session:expr) => {
        match $session.as_deref_mut() {
            Some(s) => $action.session(s).await,
            None => $action.await,
        }
    };
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn map_err(err: mongodb::error::Error, table: Option<&TableSchema>) -> Error {
    if err.contains_label("TransientTransactionError") || err.contains_label("UnknownTransactionCommitResult") {
        return Error::Conflict(table.map(|t| t.name.to_owned()).unwrap_or_default());
    }
    let duplicate = |code: i32, message: &str| -> Option<Error> {
        if code != 11000 {
            return None;
        }
        // "E11000 duplicate key error collection: db.users index: email dup key: { ... }"
        let index = message.split("index: ").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or_default();
        let column = match (index, table) {
            ("_id_", Some(t)) => t.pk().name.to_owned(),
            (i, _) => i.to_owned(),
        };
        Some(Error::UniqueViolation { table: table.map(|t| t.name.to_owned()).unwrap_or_default(), column })
    };
    match &*err.kind {
        ErrorKind::Write(WriteFailure::WriteError(w)) => {
            if let Some(e) = duplicate(w.code, &w.message) {
                return e;
            }
            if w.code == 112 {
                return Error::Conflict(table.map(|t| t.name.to_owned()).unwrap_or_default());
            }
        }
        ErrorKind::InsertMany(e) => {
            if let Some(w) = e.write_errors.as_ref().and_then(|w| w.first())
                && let Some(e) = duplicate(w.code, &w.message)
            {
                return e;
            }
        }
        ErrorKind::Command(c) => {
            if let Some(e) = duplicate(c.code, &c.message) {
                return e;
            }
            match c.code {
                48 => return Error::TableExists(table.map(|t| t.name.to_owned()).unwrap_or_default()),
                112 | 251 => return Error::Conflict(table.map(|t| t.name.to_owned()).unwrap_or_default()),
                _ => {}
            }
        }
        _ => {}
    }
    Error::Database(Box::new(err))
}

// ---------------------------------------------------------------------------
// Value <-> BSON
// ---------------------------------------------------------------------------

fn to_bson(v: &Value) -> Bson {
    match v {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Boolean(*b),
        Value::I32(i) => Bson::Int32(*i),
        Value::I64(i) => Bson::Int64(*i),
        Value::F64(f) => Bson::Double(*f),
        Value::Text(s) => Bson::String(s.clone()),
        Value::Bytes(b) => Bson::Binary(bson::Binary { subtype: BinarySubtype::Generic, bytes: b.clone() }),
        #[cfg(feature = "uuid")]
        Value::Uuid(u) => Bson::Binary(bson::Binary { subtype: BinarySubtype::Uuid, bytes: u.as_bytes().to_vec() }),
        #[cfg(feature = "chrono")]
        Value::Timestamp(t) => Bson::DateTime(bson::DateTime::from_millis(t.and_utc().timestamp_millis())),
        #[cfg(feature = "chrono")]
        Value::TimestampTz(t) => Bson::DateTime(bson::DateTime::from_millis(t.timestamp_millis())),
        #[cfg(feature = "chrono")]
        Value::Date(d) => Bson::String(d.format("%Y-%m-%d").to_string()),
        #[cfg(feature = "chrono")]
        Value::Time(t) => Bson::String(t.format("%H:%M:%S%.9f").to_string()),
        #[cfg(feature = "json")]
        Value::Json(j) => bson::to_bson(j).unwrap_or(Bson::Null),
    }
}

#[cfg(feature = "chrono")]
fn utc(d: bson::DateTime) -> Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::from_timestamp_millis(d.timestamp_millis())
        .ok_or_else(|| Error::Conversion(format!("date out of range: {d}")))
}

fn from_bson(kind: ColumnType, b: Bson) -> Result<Value> {
    #[cfg(feature = "chrono")]
    fn parse<T: crate::value::FromValue>(s: String) -> Result<T> {
        T::from_value(Value::Text(s))
    }
    Ok(match (kind, b) {
        (_, Bson::Null | Bson::Undefined) => Value::Null,
        (_, Bson::Boolean(b)) => Value::Bool(b),
        (ColumnType::Bool, Bson::Int32(i)) => Value::Bool(i != 0),
        (_, Bson::Int32(i)) => Value::I32(i),
        (_, Bson::Int64(i)) => Value::I64(i),
        (_, Bson::Double(d)) => Value::F64(d),
        #[cfg(feature = "uuid")]
        (ColumnType::Uuid, Bson::String(s)) => Value::Uuid(uuid::Uuid::parse_str(&s).map_err(Error::conversion)?),
        #[cfg(feature = "chrono")]
        (ColumnType::Timestamp, Bson::String(s)) => Value::Timestamp(parse(s)?),
        #[cfg(feature = "chrono")]
        (ColumnType::TimestampTz, Bson::String(s)) => Value::TimestampTz(parse(s)?),
        #[cfg(feature = "chrono")]
        (ColumnType::Date, Bson::String(s)) => Value::Date(parse(s)?),
        #[cfg(feature = "chrono")]
        (ColumnType::Time, Bson::String(s)) => Value::Time(parse(s)?),
        #[cfg(feature = "json")]
        (ColumnType::Json, Bson::String(s)) => Value::Json(serde_json::Value::String(s)),
        (_, Bson::String(s)) => Value::Text(s),
        #[cfg(feature = "uuid")]
        (ColumnType::Uuid, Bson::Binary(bin)) => {
            Value::Uuid(uuid::Uuid::from_slice(&bin.bytes).map_err(Error::conversion)?)
        }
        (_, Bson::Binary(bin)) => Value::Bytes(bin.bytes),
        #[cfg(feature = "chrono")]
        (ColumnType::Timestamp, Bson::DateTime(d)) => Value::Timestamp(utc(d)?.naive_utc()),
        #[cfg(feature = "chrono")]
        (ColumnType::Date, Bson::DateTime(d)) => Value::Date(utc(d)?.date_naive()),
        #[cfg(feature = "chrono")]
        (_, Bson::DateTime(d)) => Value::TimestampTz(utc(d)?),
        #[cfg(not(feature = "chrono"))]
        (_, Bson::DateTime(d)) => Value::Text(d.try_to_rfc3339_string().unwrap_or_default()),
        #[cfg(feature = "json")]
        (ColumnType::Json, other) => Value::Json(other.into_relaxed_extjson()),
        (_, Bson::ObjectId(o)) => Value::Text(o.to_hex()),
        (_, Bson::Decimal128(d)) => Value::Text(d.to_string()),
        (_, other) => Value::Text(other.into_relaxed_extjson().to_string()),
    })
}

fn path(schema: &TableSchema, column: usize) -> &'static str {
    if column == schema.primary_key { "_id" } else { schema.columns[column].name }
}

fn to_document(schema: &TableSchema, row: &[Value]) -> Document {
    let mut d = Document::new();
    for (i, v) in row.iter().enumerate() {
        if !v.is_null() {
            d.insert(path(schema, i), to_bson(v));
        }
    }
    d
}

fn from_document(schema: &TableSchema, mut d: Document) -> Result<Vec<Value>> {
    schema
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| match d.remove(path(schema, i)) {
            Some(b) => from_bson(c.kind, b),
            None => Ok(Value::Null),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Filter translation (SQL three-valued logic)
// ---------------------------------------------------------------------------

fn never() -> Document {
    doc! { "_id": { "$exists": false } }
}

fn always() -> Document {
    Document::new()
}

fn field_of<'a>(e: &Expr, table: &'a TableSchema) -> Option<&'a str> {
    match e {
        Expr::Column(c) if c.table == table.name => Some(path(table, c.index)),
        _ => None,
    }
}

fn op_name(op: BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Eq => "$eq",
        BinOp::Ne => "$ne",
        BinOp::Lt => "$lt",
        BinOp::Le => "$lte",
        BinOp::Gt => "$gt",
        BinOp::Ge => "$gte",
        _ => return None,
    })
}

fn invert(op: BinOp) -> BinOp {
    match op {
        BinOp::Eq => BinOp::Ne,
        BinOp::Ne => BinOp::Eq,
        BinOp::Lt => BinOp::Ge,
        BinOp::Le => BinOp::Gt,
        BinOp::Gt => BinOp::Le,
        BinOp::Ge => BinOp::Lt,
        other => other,
    }
}

fn flip(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

/// `field op value`, true only for non-null fields.
fn compare(field: &str, op: BinOp, v: &Value) -> Option<Document> {
    if v.is_null() {
        return Some(never());
    }
    let b = to_bson(v);
    Some(match op {
        // `$ne` alone would also match missing fields.
        BinOp::Ne => doc! { field: { "$nin": [Bson::Null, b] } },
        op => doc! { field: { op_name(op)?: b } },
    })
}

/// Regex equivalent of a `LIKE` pattern.
fn like_regex(pattern: &str, escaped: bool) -> String {
    let mut re = String::from("^");
    for tok in mem::like_tokens(pattern, escaped) {
        match tok {
            mem::LikeTok::Any => re.push_str(".*"),
            mem::LikeTok::One => re.push('.'),
            mem::LikeTok::Lit(c) => {
                if "\\^$.|?*+()[]{}/-".contains(c) {
                    re.push('\\');
                }
                re.push(c);
            }
        }
    }
    re.push('$');
    re
}

/// Translates `e` into a filter matching the documents where `e` is TRUE
/// (`want = true`) or FALSE (`want = false`). `None` when not translatable.
fn translate(e: &Expr, table: &TableSchema, want: bool) -> Option<Document> {
    match e {
        Expr::Value(Value::Bool(b)) => Some(if *b == want { always() } else { never() }),
        Expr::Value(Value::Null) => Some(never()),
        Expr::Binary { op: BinOp::And, left, right } | Expr::Binary { op: BinOp::Or, left, right } => {
            let is_and = matches!(e, Expr::Binary { op: BinOp::And, .. });
            let (l, r) = (translate(left, table, want)?, translate(right, table, want)?);
            // De Morgan: FALSE(a AND b) = FALSE(a) OR FALSE(b).
            let key = if is_and == want { "$and" } else { "$or" };
            Some(doc! { key: [l, r] })
        }
        Expr::Not(inner) => translate(inner, table, !want),
        Expr::Binary { op, left, right } if op_name(*op).is_some() => {
            let op = if want { *op } else { invert(*op) };
            match (field_of(left, table), field_of(right, table), &**left, &**right) {
                (Some(f), None, _, Expr::Value(v)) => compare(f, op, v),
                (None, Some(f), Expr::Value(v), _) => compare(f, flip(op), v),
                (Some(a), Some(b), _, _) => {
                    let (fa, fb) = (format!("${a}"), format!("${b}"));
                    Some(doc! { "$expr": { "$and": [
                        { "$gt": [&fa, Bson::Null] },
                        { "$gt": [&fb, Bson::Null] },
                        { op_name(op)?: [&fa, &fb] },
                    ] } })
                }
                _ => None,
            }
        }
        Expr::IsNull { expr, negated } => {
            let f = field_of(expr, table)?;
            let is_null = want != *negated;
            Some(if is_null {
                doc! { f: Bson::Null }
            } else {
                doc! { f: { "$ne": Bson::Null } }
            })
        }
        Expr::InList { expr, list, negated } => {
            let f = field_of(expr, table)?;
            let values: Vec<Bson> = list.iter().filter(|v| !v.is_null()).map(to_bson).collect();
            let has_null = values.len() != list.len();
            // TRUE(x IN l) / FALSE(x IN l); NOT IN swaps them.
            let true_side = want != *negated;
            if true_side {
                Some(if values.is_empty() {
                    never()
                } else {
                    doc! { f: { "$in": values } }
                })
            } else if list.is_empty() {
                Some(always())
            } else if has_null {
                Some(never())
            } else {
                let mut nin = values;
                nin.push(Bson::Null);
                Some(doc! { f: { "$nin": nin } })
            }
        }
        Expr::Between { expr, low, high, negated } => {
            let f = field_of(expr, table)?;
            let (Expr::Value(lo), Expr::Value(hi)) = (&**low, &**high) else { return None };
            if lo.is_null() || hi.is_null() {
                return Some(never());
            }
            let inside = want != *negated;
            Some(if inside {
                doc! { f: { "$gte": to_bson(lo), "$lte": to_bson(hi) } }
            } else {
                doc! { "$or": [ { f: { "$lt": to_bson(lo) } }, { f: { "$gt": to_bson(hi) } } ] }
            })
        }
        Expr::Like { expr, pattern, case_insensitive, negated, escaped } => {
            let f = field_of(expr, table)?;
            let Expr::Value(Value::Text(p)) = &**pattern else { return None };
            let regex =
                doc! { "$regex": like_regex(p, *escaped), "$options": if *case_insensitive { "si" } else { "s" } };
            Some(if want != *negated {
                doc! { f: regex }
            } else {
                doc! { f: { "$type": "string", "$not": regex } }
            })
        }
        _ => None,
    }
}

/// Top-level `AND` terms.
fn conjuncts(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Binary { op: BinOp::And, left, right } => {
            let mut v = conjuncts(left);
            v.extend(conjuncts(right));
            v
        }
        other => vec![other],
    }
}

/// Filter selecting a superset of the rows of `table` that can satisfy `e`
/// (the translatable conjuncts that only use `table`).
fn pushdown(e: Option<&Expr>, table: &TableSchema) -> Document {
    let Some(e) = e else { return always() };
    let parts: Vec<Document> = conjuncts(e).into_iter().filter_map(|c| translate(c, table, true)).collect();
    match parts.len() {
        0 => always(),
        1 => parts.into_iter().next().unwrap_or_default(),
        _ => doc! { "$and": parts },
    }
}

/// Aggregation expression for `SET column = <expr>`.
fn set_expression(e: &Expr, table: &TableSchema) -> Option<Bson> {
    Some(match e {
        Expr::Value(Value::Null) => Bson::String("$$REMOVE".into()),
        Expr::Value(v) => Bson::Document(doc! { "$literal": to_bson(v) }),
        Expr::Column(c) if c.table == table.name => Bson::String(format!("${}", path(table, c.index))),
        Expr::Binary { op, left, right } => {
            let (l, r) = (set_expression(left, table)?, set_expression(right, table)?);
            match op {
                BinOp::Add => Bson::Document(doc! { "$add": [l, r] }),
                BinOp::Sub => Bson::Document(doc! { "$subtract": [l, r] }),
                BinOp::Mul => Bson::Document(doc! { "$multiply": [l, r] }),
                BinOp::Div => {
                    let integer = matches!(&**left, Expr::Column(ColumnRef { kind, .. }) if kind.is_integer());
                    let div = Bson::Document(doc! { "$divide": [l, r] });
                    if integer { Bson::Document(doc! { "$trunc": [div] }) } else { div }
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// MongoDb
// ---------------------------------------------------------------------------

/// A MongoDB database used through the Memoire API.
///
/// ```no_run
/// # async fn demo() -> memoire_orm::Result<()> {
/// use memoire_orm::prelude::*;
/// let db = MongoDb::connect("mongodb://localhost:27017/app").await?;
/// # Ok(()) }
/// ```
///
/// See the [module documentation](self) for the mapping and the semantics.
/// Cloning the handle is cheap (it shares the driver's connection pool).
#[derive(Clone)]
pub struct MongoDb {
    client: Client,
    db: mongodb::Database,
    known: Arc<RwLock<HashSet<String>>>,
    transactions: bool,
}

impl fmt::Debug for MongoDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MongoDb")
            .field("database", &self.db.name())
            .field("transactions", &self.transactions)
            .finish_non_exhaustive()
    }
}

impl MongoDb {
    /// Connects with a `mongodb://` or `mongodb+srv://` URI that names the
    /// database (`mongodb://host/app`).
    pub async fn connect(uri: &str) -> Result<Self> {
        let client = Client::with_uri_str(uri).await.map_err(|e| map_err(e, None))?;
        let db = client.default_database().ok_or_else(|| {
            Error::Pool("the MongoDB URI must name a database, e.g. mongodb://localhost:27017/app".into())
        })?;
        Self::from_parts(client, db).await
    }

    /// Connects to `database` on the server of `uri`.
    pub async fn connect_with_database(uri: &str, database: &str) -> Result<Self> {
        let client = Client::with_uri_str(uri).await.map_err(|e| map_err(e, None))?;
        let db = client.database(database);
        Self::from_parts(client, db).await
    }

    async fn from_parts(client: Client, db: mongodb::Database) -> Result<Self> {
        let hello = db.run_command(doc! { "hello": 1 }).await.map_err(|e| map_err(e, None))?;
        let transactions = hello.contains_key("setName") || hello.get_str("msg").is_ok_and(|m| m == "isdbgrid");
        Ok(MongoDb { client, db, known: Arc::default(), transactions })
    }

    /// The underlying driver database (escape hatch for native MongoDB features).
    pub fn database(&self) -> &mongodb::Database {
        &self.db
    }

    /// `true` when the server supports multi-document transactions.
    pub fn supports_transactions(&self) -> bool {
        self.transactions
    }

    fn collection(&self, table: &TableSchema) -> Collection<Document> {
        self.db.collection(table.name)
    }

    /// Fails with [`Error::NoSuchTable`] unless every table exists.
    async fn ensure_exists(&self, names: &[&str]) -> Result<()> {
        let unknown: Vec<&str> = {
            let known = self.known.read();
            names.iter().copied().filter(|n| !known.contains(*n)).collect()
        };
        if unknown.is_empty() {
            return Ok(());
        }
        let found = self
            .db
            .list_collection_names()
            .filter(doc! { "name": { "$in": &unknown } })
            .await
            .map_err(|e| map_err(e, None))?;
        let mut known = self.known.write();
        for n in &found {
            known.insert(n.clone());
        }
        match unknown.iter().find(|n| !found.iter().any(|f| f == *n)) {
            Some(missing) => Err(Error::NoSuchTable((*missing).to_owned())),
            None => Ok(()),
        }
    }

    async fn find_docs(
        &self,
        table: &TableSchema,
        filter: Document,
        sort: Option<Document>,
        skip: Option<u64>,
        limit: Option<u64>,
        session: &mut Session<'_>,
    ) -> Result<Vec<Vec<Value>>> {
        let coll = self.collection(table);
        let mut find = coll.find(filter);
        if let Some(s) = sort {
            find = find.sort(s);
        }
        if let Some(s) = skip {
            find = find.skip(s);
        }
        if let Some(l) = limit {
            find = find.limit(l.min(i64::MAX as u64) as i64);
        }
        let mut out = Vec::new();
        match session.as_deref_mut() {
            Some(s) => {
                let mut cursor = find.session(&mut *s).await.map_err(|e| map_err(e, Some(table)))?;
                while cursor.advance(s).await.map_err(|e| map_err(e, Some(table)))? {
                    let d = cursor.deserialize_current().map_err(|e| map_err(e, Some(table)))?;
                    out.push(from_document(table, d)?);
                }
            }
            None => {
                let mut cursor = find.await.map_err(|e| map_err(e, Some(table)))?;
                while cursor.advance().await.map_err(|e| map_err(e, Some(table)))? {
                    let d = cursor.deserialize_current().map_err(|e| map_err(e, Some(table)))?;
                    out.push(from_document(table, d)?);
                }
            }
        }
        Ok(out)
    }

    /// Next `n` ids of an auto-increment table; returns the first one.
    async fn reserve_ids(&self, table: &TableSchema, n: usize) -> Result<i64> {
        let counters: Collection<Document> = self.db.collection(COUNTERS);
        let updated = counters
            .find_one_and_update(doc! { "_id": table.name }, doc! { "$inc": { "seq": n as i64 } })
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await
            .map_err(|e| map_err(e, Some(table)))?
            .ok_or_else(|| Error::Conversion("counter update returned nothing".into()))?;
        let last = match updated.get("seq") {
            Some(Bson::Int64(i)) => *i,
            Some(Bson::Int32(i)) => *i as i64,
            Some(Bson::Double(d)) => *d as i64,
            _ => return Err(Error::Conversion("invalid counter value".into())),
        };
        Ok(last - n as i64 + 1)
    }

    /// Keeps the counter above explicitly inserted integer keys.
    async fn bump_counter(&self, table: &TableSchema, key: &Value) -> Result<()> {
        if let Some(k) = key.as_i64() {
            let counters: Collection<Document> = self.db.collection(COUNTERS);
            counters
                .update_one(doc! { "_id": table.name }, doc! { "$max": { "seq": k } })
                .upsert(true)
                .await
                .map_err(|e| map_err(e, Some(table)))?;
        }
        Ok(())
    }

    // -- reads ---------------------------------------------------------------

    async fn fetch_rows(&self, query: &Query<'_>, session: &mut Session<'_>) -> Result<Vec<Vec<Value>>> {
        match *query {
            Query::FindByKey { table, key } => {
                self.ensure_exists(&[table.name]).await?;
                self.find_docs(table, doc! { "_id": to_bson(key) }, None, None, Some(1), session).await
            }
            Query::Select(s) => self.select(s, session).await,
            Query::Raw { .. } | Query::Script { .. } => Err(Error::Unsupported(
                "raw SQL is not supported by MongoDB: use `MongoDb::database()` for native commands".into(),
            )),
            _ => Err(Error::Unsupported("statement does not return rows".into())),
        }
    }

    async fn select(&self, s: &SelectAst, session: &mut Session<'_>) -> Result<Vec<Vec<Value>>> {
        let names: Vec<&str> = s.tables().map(|t| t.name).collect();
        self.ensure_exists(&names).await?;
        let table = s.from;
        let simple = s.joins.is_empty() && s.group_by.is_empty() && s.having.is_none() && !s.distinct;
        let filter = match &s.filter {
            Some(f) => translate(f, table, true),
            None => Some(always()),
        };

        // SELECT COUNT(*) WHERE ...
        if simple
            && let Some(filter) = &filter
            && let Projection::Exprs(e) = &s.projection
            && matches!(e.as_slice(), [Expr::Aggregate { func: AggFunc::Count, arg: None, distinct: false }])
        {
            let coll = self.collection(table);
            let n = run!(coll.count_documents(filter.clone()), session).map_err(|e| map_err(e, Some(table)))?;
            return Ok(vec![vec![Value::I64(n as i64)]]);
        }

        // Everything on the server: filter, sort, skip, limit.
        let columns: Option<Vec<usize>> = match &s.projection {
            Projection::Tables(t) if t.len() == 1 => Some((0..table.columns.len()).collect()),
            Projection::Exprs(exprs) => exprs
                .iter()
                .map(|e| match e {
                    Expr::Column(c) if c.table == table.name => Some(c.index),
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        let sort: Option<Document> = s
            .order
            .iter()
            .map(|o| match (&o.expr, o.nulls) {
                (Expr::Column(c), nulls)
                    if c.table == table.name
                        && (nulls.is_none() || nulls == Some(if o.desc { Nulls::Last } else { Nulls::First })) =>
                {
                    Some((path(table, c.index).to_owned(), Bson::Int32(if o.desc { -1 } else { 1 })))
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|terms| terms.into_iter().collect());
        if simple && let (Some(filter), Some(columns), Some(sort)) = (filter, columns, sort) {
            let sort = (!sort.is_empty()).then_some(sort);
            let rows = self.find_docs(table, filter, sort, s.offset, s.limit, session).await?;
            return Ok(rows
                .into_iter()
                .map(|mut r| columns.iter().map(|&i| std::mem::take(&mut r[i])).collect())
                .collect());
        }

        // Otherwise: load the candidate rows and finish in the memory engine.
        let mut state = State::default();
        let root_rows = self.find_docs(table, pushdown(s.filter.as_ref(), table), None, None, None, session).await?;
        let mut loaded: Vec<(&'static TableSchema, Vec<Vec<Value>>)> = vec![(table, root_rows)];
        for join in &s.joins {
            let jt = join.table;
            let mut filter = pushdown(s.filter.as_ref(), jt);
            if join.kind == JoinKind::Left {
                filter = always();
            }
            // Equi-join: only fetch the rows whose key appears on the other side.
            if let Expr::Binary { op: BinOp::Eq, left, right } = &join.on
                && let (Expr::Column(a), Expr::Column(b)) = (&**left, &**right)
            {
                let (mine, other) = if a.table == jt.name { (a, b) } else { (b, a) };
                if mine.table == jt.name
                    && let Some((_, rows)) = loaded.iter().find(|(t, _)| t.name == other.table)
                {
                    let keys: Vec<Bson> =
                        rows.iter().map(|r| &r[other.index]).filter(|v| !v.is_null()).map(to_bson).collect();
                    let f = path(jt, mine.index);
                    filter = doc! { "$and": [filter, { f: { "$in": keys } }] };
                }
            }
            let rows = self.find_docs(jt, filter, None, None, None, session).await?;
            loaded.push((jt, rows));
        }
        for (schema, rows) in loaded {
            state.put(Table::from_rows(schema, rows, 0));
        }
        // NULL placement follows MongoDB everywhere for this backend.
        let mut ast = s.clone();
        for o in &mut ast.order {
            if o.nulls.is_none() {
                o.nulls = Some(if o.desc { Nulls::Last } else { Nulls::First });
            }
        }
        mem::fetch(&state, &Query::Select(&ast))
    }

    // -- writes --------------------------------------------------------------

    async fn create_table(&self, table: &'static TableSchema, if_not_exists: bool) -> Result<()> {
        let existing = self
            .db
            .list_collection_names()
            .filter(doc! { "name": table.name })
            .await
            .map_err(|e| map_err(e, Some(table)))?;
        if !existing.is_empty() {
            self.known.write().insert(table.name.to_owned());
            return if if_not_exists { Ok(()) } else { Err(Error::TableExists(table.name.to_owned())) };
        }
        match self.db.create_collection(table.name).await {
            Ok(()) => {}
            Err(e) => match map_err(e, Some(table)) {
                Error::TableExists(_) if if_not_exists => return Ok(()),
                other => return Err(other),
            },
        }
        let coll = self.collection(table);
        for (i, c) in table.columns.iter().enumerate() {
            if i == table.primary_key || !(c.unique || c.indexed) {
                continue;
            }
            let mut options = IndexOptions::default();
            options.name = Some(c.name.to_owned());
            if c.unique {
                options.unique = Some(true);
                // Absent fields (NULL) are not indexed, so many NULLs are allowed.
                options.partial_filter_expression = Some(doc! { c.name: { "$exists": true } });
            }
            let mut model = IndexModel::default();
            model.keys = doc! { c.name: 1 };
            model.options = Some(options);
            coll.create_index(model).await.map_err(|e| map_err(e, Some(table)))?;
        }
        self.known.write().insert(table.name.to_owned());
        Ok(())
    }

    async fn drop_table(&self, name: &str, if_exists: bool) -> Result<()> {
        let existing =
            self.db.list_collection_names().filter(doc! { "name": name }).await.map_err(|e| map_err(e, None))?;
        self.known.write().remove(name);
        if existing.is_empty() {
            return if if_exists { Ok(()) } else { Err(Error::NoSuchTable(name.to_owned())) };
        }
        self.db.collection::<Document>(name).drop().await.map_err(|e| map_err(e, None))?;
        let counters: Collection<Document> = self.db.collection(COUNTERS);
        counters.delete_one(doc! { "_id": name }).await.map_err(|e| map_err(e, None))?;
        Ok(())
    }

    /// Starts a transaction of our own for a multi-document write, when the
    /// server supports it and the caller is not already in one. If the write
    /// fails, dropping the session aborts that transaction.
    async fn own_transaction(&self, session: &Session<'_>, table: &TableSchema) -> Result<Option<ClientSession>> {
        if session.is_some() || !self.transactions {
            return Ok(None);
        }
        let mut own = self.client.start_session().await.map_err(|e| map_err(e, Some(table)))?;
        own.start_transaction().await.map_err(|e| map_err(e, Some(table)))?;
        Ok(Some(own))
    }

    async fn commit_own(own: Option<ClientSession>, table: &TableSchema) -> Result<()> {
        if let Some(mut own) = own {
            own.commit_transaction().await.map_err(|e| map_err(e, Some(table)))?;
        }
        Ok(())
    }

    async fn execute_query(&self, query: &Query<'_>, session: &mut Session<'_>) -> Result<ExecResult> {
        let done = |n: u64| Ok(ExecResult { rows_affected: n, last_insert_id: None });
        match *query {
            Query::CreateTable { table, if_not_exists } => {
                self.create_table(table, if_not_exists).await?;
                done(0)
            }
            Query::DropTable { name, if_exists } => {
                self.drop_table(name, if_exists).await?;
                done(0)
            }
            Query::InsertRow { table, values } => {
                self.ensure_exists(&[table.name]).await?;
                let mut row = values.to_vec();
                validate_row(table, &row)?;
                let generated = if table.auto_increment() {
                    let id = self.reserve_ids(table, 1).await?;
                    row[table.primary_key] = Value::I64(id);
                    Some(Value::I64(id))
                } else {
                    self.bump_counter(table, &row[table.primary_key]).await?;
                    None
                };
                let coll = self.collection(table);
                run!(coll.insert_one(to_document(table, &row)), session).map_err(|e| map_err(e, Some(table)))?;
                Ok(ExecResult { rows_affected: 1, last_insert_id: generated })
            }
            Query::UpsertRow { table, values } => {
                self.ensure_exists(&[table.name]).await?;
                validate_row(table, values)?;
                self.bump_counter(table, &values[table.primary_key]).await?;
                let coll = self.collection(table);
                let id = to_bson(&values[table.primary_key]);
                run!(coll.replace_one(doc! { "_id": id }, to_document(table, values)).upsert(true), session)
                    .map_err(|e| map_err(e, Some(table)))?;
                done(1)
            }
            Query::UpdateRow { table, values } => {
                self.ensure_exists(&[table.name]).await?;
                validate_row(table, values)?;
                let coll = self.collection(table);
                let id = to_bson(&values[table.primary_key]);
                let r = run!(coll.replace_one(doc! { "_id": id }, to_document(table, values)), session)
                    .map_err(|e| map_err(e, Some(table)))?;
                done(r.matched_count)
            }
            Query::DeleteByKey { table, key } => {
                self.ensure_exists(&[table.name]).await?;
                let coll = self.collection(table);
                let r = run!(coll.delete_one(doc! { "_id": to_bson(key) }), session)
                    .map_err(|e| map_err(e, Some(table)))?;
                done(r.deleted_count)
            }
            Query::Insert(ins) => {
                let table = ins.table;
                self.ensure_exists(&[table.name]).await?;
                if ins.rows.is_empty() {
                    return done(0);
                }
                let mut rows = ins.rows.clone();
                if table.auto_increment() {
                    let first = self.reserve_ids(table, rows.len()).await?;
                    for (i, r) in rows.iter_mut().enumerate() {
                        r[table.primary_key] = Value::I64(first + i as i64);
                    }
                }
                // Duplicates inside the batch itself.
                let indexed: Vec<usize> = table
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| *i == table.primary_key || c.unique)
                    .map(|(i, _)| i)
                    .collect();
                let mut seen = HashSet::new();
                for r in &rows {
                    validate_row(table, r)?;
                    for &c in &indexed {
                        if !r[c].is_null() && !seen.insert((c, r[c].to_string())) {
                            return Err(Error::UniqueViolation {
                                table: table.name.to_owned(),
                                column: table.columns[c].name.to_owned(),
                            });
                        }
                    }
                }
                if !table.auto_increment() {
                    for r in &rows {
                        self.bump_counter(table, &r[table.primary_key]).await?;
                    }
                }
                let docs: Vec<Document> = rows.iter().map(|r| to_document(table, r)).collect();
                let coll = self.collection(table);
                if session.is_some() || self.transactions {
                    let mut own = self.own_transaction(session, table).await?;
                    {
                        let s: &mut Session<'_> = &mut match own.as_mut() {
                            Some(o) => Some(o),
                            None => session.as_deref_mut(),
                        };
                        run!(coll.insert_many(&docs).ordered(true), s).map_err(|e| map_err(e, Some(table)))?;
                    }
                    Self::commit_own(own, table).await?;
                } else if let Err(e) = coll.insert_many(&docs).ordered(true).await {
                    // Standalone server: best effort, remove what went in before the failure.
                    let inserted = inserted_before_failure(&e, docs.len());
                    let ids: Vec<Bson> = rows.iter().take(inserted).map(|r| to_bson(&r[table.primary_key])).collect();
                    if !ids.is_empty() {
                        let _ = coll.delete_many(doc! { "_id": { "$in": ids } }).await;
                    }
                    return Err(map_err(e, Some(table)));
                }
                done(rows.len() as u64)
            }
            Query::Update(u) => {
                let table = u.table;
                self.ensure_exists(&[table.name]).await?;
                let filter = match &u.filter {
                    Some(f) => translate(f, table, true),
                    None => Some(always()),
                };
                let mut set = Document::new();
                let mut translatable = filter.is_some();
                for (i, e) in &u.sets {
                    if *i == table.primary_key {
                        translatable = false;
                        break;
                    }
                    match set_expression(e, table) {
                        Some(expr) => {
                            // A NULL result removes the field.
                            set.insert(path(table, *i), doc! { "$ifNull": [expr, "$$REMOVE"] });
                        }
                        None => translatable = false,
                    }
                }
                let coll = self.collection(table);
                if let (true, Some(filter)) = (translatable, filter) {
                    let pipeline = vec![doc! { "$set": set }];
                    let mut own = self.own_transaction(session, table).await?;
                    let matched = {
                        let s: &mut Session<'_> = &mut match own.as_mut() {
                            Some(o) => Some(o),
                            None => session.as_deref_mut(),
                        };
                        run!(coll.update_many(filter, pipeline), s).map_err(|e| map_err(e, Some(table)))?.matched_count
                    };
                    Self::commit_own(own, table).await?;
                    return done(matched);
                }
                // Read, compute with the memory engine, write back.
                let mut own = self.own_transaction(session, table).await?;
                let result = {
                    let s: &mut Session<'_> = &mut match own.as_mut() {
                        Some(o) => Some(o),
                        None => session.as_deref_mut(),
                    };
                    let rows = self.find_docs(table, pushdown(u.filter.as_ref(), table), None, None, None, s).await?;
                    let mut state = State::default();
                    state.put(Table::from_rows(table, rows.clone(), 0));
                    let (result, _) = mem::execute(&mut state, query)?;
                    let new_rows = state.get(table.name).map(|t| t.rows.clone()).unwrap_or_default();
                    // Primary keys are never assigned here, so rows keep their position.
                    for new in rows.iter().zip(&new_rows).filter(|(o, n)| o != n).map(|(_, n)| n) {
                        let id = to_bson(&new[table.primary_key]);
                        run!(coll.replace_one(doc! { "_id": id }, to_document(table, new)), s)
                            .map_err(|e| map_err(e, Some(table)))?;
                    }
                    result
                };
                Self::commit_own(own, table).await?;
                Ok(result)
            }
            Query::Delete(d) => {
                let table = d.table;
                self.ensure_exists(&[table.name]).await?;
                let coll = self.collection(table);
                let filter = match &d.filter {
                    Some(f) => translate(f, table, true),
                    None => Some(always()),
                };
                let filter = match filter {
                    Some(f) => f,
                    None => {
                        let rows = self
                            .find_docs(table, pushdown(d.filter.as_ref(), table), None, None, None, session)
                            .await?;
                        let mut state = State::default();
                        state.put(Table::from_rows(table, rows, 0));
                        let select = SelectAst {
                            projection: Projection::Exprs(vec![Expr::Column(ColumnRef {
                                table: table.name,
                                name: table.pk().name,
                                index: table.primary_key,
                                kind: table.pk().kind,
                            })]),
                            filter: d.filter.clone(),
                            ..SelectAst::new(table)
                        };
                        let ids: Vec<Bson> =
                            mem::fetch(&state, &Query::Select(&select))?.into_iter().map(|r| to_bson(&r[0])).collect();
                        doc! { "_id": { "$in": ids } }
                    }
                };
                let r = run!(coll.delete_many(filter), session).map_err(|e| map_err(e, Some(table)))?;
                done(r.deleted_count)
            }
            Query::Raw { .. } | Query::Script { .. } => Err(Error::Unsupported(
                "raw SQL is not supported by MongoDB: use `MongoDb::database()` for native commands".into(),
            )),
            Query::Select(_) | Query::FindByKey { .. } => {
                Err(Error::Unsupported("use `fetch_all` for statements returning rows".into()))
            }
        }
    }
}

/// Number of documents an ordered `insert_many` wrote before failing.
fn inserted_before_failure(e: &mongodb::error::Error, total: usize) -> usize {
    match &*e.kind {
        ErrorKind::InsertMany(m) => m.write_errors.as_ref().and_then(|w| w.first()).map_or(total, |w| w.index),
        _ => 0,
    }
}

impl Executor for MongoDb {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        mem::decode(self.fetch_rows(&query, &mut None).await?)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        self.execute_query(&query, &mut None).await
    }
}

impl Database for MongoDb {
    type Tx<'c> = MongoTransaction;

    async fn begin(&self) -> Result<MongoTransaction> {
        if !self.transactions {
            return Err(Error::Unsupported(
                "MongoDB transactions need a replica set or a sharded cluster (this server is standalone)".into(),
            ));
        }
        let mut session = self.client.start_session().await.map_err(|e| map_err(e, None))?;
        session.start_transaction().await.map_err(|e| map_err(e, None))?;
        Ok(MongoTransaction { db: self.clone(), session: Mutex::new(session) })
    }
}

/// A MongoDB multi-document transaction (replica sets and sharded clusters).
///
/// Reads see the transaction's own writes; [`commit`](Transaction::commit)
/// fails with [`Error::Conflict`] when MongoDB reports a write conflict
/// (retrying the transaction is safe). Dropping it aborts it. `CREATE TABLE`
/// and `DROP TABLE` run outside the transaction, like DDL on MySQL.
pub struct MongoTransaction {
    db: MongoDb,
    session: Mutex<ClientSession>,
}

impl fmt::Debug for MongoTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MongoTransaction").finish_non_exhaustive()
    }
}

impl Executor for MongoTransaction {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        let mut guard = self.session.lock().await;
        let mut session: Session<'_> = Some(&mut *guard);
        mem::decode(self.db.fetch_rows(&query, &mut session).await?)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let mut guard = self.session.lock().await;
        let mut session: Session<'_> = Some(&mut *guard);
        self.db.execute_query(&query, &mut session).await
    }
}

impl Transaction for MongoTransaction {
    async fn commit(self) -> Result<()> {
        let mut session = self.session.into_inner();
        session.commit_transaction().await.map_err(|e| map_err(e, None))
    }

    async fn rollback(self) -> Result<()> {
        let mut session = self.session.into_inner();
        session.abort_transaction().await.map_err(|e| map_err(e, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_to_regex() {
        assert_eq!(like_regex("%li%", false), "^.*li.*$");
        assert_eq!(like_regex("_ob@%", false), "^.ob@.*$");
        assert_eq!(like_regex("a.b\\%", true), "^a\\.b%$");
    }

    #[derive(memoire_orm_derive::Model)]
    #[memoire(table = "t")]
    struct T {
        id: i64,
        age: Option<i32>,
    }

    #[test]
    fn three_valued_logic() {
        use crate::Model;
        let schema = <T as Model>::SCHEMA;
        // NOT (age > 18) only matches documents whose age is present and <= 18.
        let not_gt = T::AGE.gt(18).negate();
        assert_eq!(translate(&not_gt, schema, true), Some(doc! { "age": { "$lte": 18 } }));
        // age <> 3 must not match documents without an age.
        assert_eq!(translate(&T::AGE.ne(3), schema, true), Some(doc! { "age": { "$nin": [Bson::Null, 3] } }));
        // The primary key is stored in `_id`.
        assert_eq!(translate(&T::ID.eq(1), schema, true), Some(doc! { "_id": { "$eq": 1_i64 } }));
        // NOT (age IN (1, NULL)) is never true.
        let in_null =
            Expr::InList { expr: Box::new(T::AGE.expr()), list: vec![Value::I32(1), Value::Null], negated: true };
        assert_eq!(translate(&in_null, schema, true), Some(never()));
    }
}
