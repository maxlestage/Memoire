//! The Nuage DB protocol: Memoire statements as JSON.
//!
//! A client sends the statement itself (its AST, with the schemas of the
//! tables it uses), never SQL; the server evaluates it with Memoire's own
//! engine and answers rows. Shared by the client (`NuageDb`)
//! and the server (the `nuage` crate).
//!
//! | Request | Body | Answer |
//! |---|---|---|
//! | `POST /v1/fetch` | `{"query": <statement>}` | `{"rows": [[<value>, ...], ...]}` |
//! | `POST /v1/execute` | `{"query": <statement>}` | `{"rowsAffected": n, "lastInsertId": <value>}` |
//! | `POST /v1/batch` | `{"statements": [{"fetch" \| "execute": <statement>}, ...]}` | `{"results": [...]}`, all or nothing |
//! | `POST /v1/transactions` | `{}` | `{"id": "..."}` |
//! | `POST /v1/transactions/<id>/commit` \| `rollback` | | `{"ok": true}` |
//! | `GET` \| `POST /v1/branches`, `DELETE /v1/branches/<name>`, `POST /v1/branches/<name>/reset` | | branches |
//!
//! Requests carry `Authorization: Bearer <token>`, and optionally
//! `Nuage-Transaction: <id>` and `Nuage-Branch: <name>`. Errors are
//! `{"error": {"kind": "...", "message": "...", ...}}` with an HTTP status;
//! the kinds are Memoire's [`Error`] variants, so a unique violation on the
//! server is an [`Error::UniqueViolation`] in the client.
//!
//! Values are JSON where JSON is exact (`null`, booleans, 64-bit integers,
//! text) and tagged objects otherwise: `{"i32": 1}`, `{"f64": 1.5}`,
//! `{"bytes": "00ff"}`, `{"uuid": "..."}`, `{"ts": "2026-01-01T12:00:00"}`,
//! `{"tstz": "2026-01-01T12:00:00Z"}`, `{"date": "2026-01-01"}`,
//! `{"time": "12:00:00"}`, `{"json": ...}`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::{Map, Value as Json, json};

use crate::ast::{DeleteAst, InsertAst, Join, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::executor::ExecResult;
use crate::expr::{AggFunc, BinOp, ColumnRef, Expr, Nulls, Order, TextSearch};
use crate::schema::{ColumnDef, ForeignKey, IndexDef, StaticSql, TableSchema};
use crate::value::{ColumnType, Value};

fn bad(what: impl std::fmt::Display) -> Error {
    Error::Conversion(format!("invalid Nuage DB message: {what}"))
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(bad("odd hex string"));
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| bad("hex"))).collect()
}

/// A value as JSON.
pub fn value_to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::I32(i) => json!({ "i32": i }),
        Value::I64(i) => json!(i),
        Value::F64(f) if f.is_finite() => json!({ "f64": f }),
        Value::F64(f) => json!({ "f64": if f.is_nan() { "NaN" } else if *f > 0.0 { "Infinity" } else { "-Infinity" } }),
        Value::Text(s) => Json::String(s.clone()),
        Value::Bytes(b) => json!({ "bytes": hex(b) }),
        #[cfg(feature = "uuid")]
        Value::Uuid(u) => json!({ "uuid": u.to_string() }),
        #[cfg(feature = "chrono")]
        Value::Timestamp(t) => json!({ "ts": t.format("%Y-%m-%dT%H:%M:%S%.f").to_string() }),
        #[cfg(feature = "chrono")]
        Value::TimestampTz(t) => json!({ "tstz": t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true) }),
        #[cfg(feature = "chrono")]
        Value::Date(d) => json!({ "date": d.to_string() }),
        #[cfg(feature = "chrono")]
        Value::Time(t) => json!({ "time": t.format("%H:%M:%S%.f").to_string() }),
        #[cfg(feature = "json")]
        Value::Json(j) => json!({ "json": j }),
    }
}

/// A value read from JSON. A typed value whose feature is off (`uuid`,
/// `chrono`, `json`) becomes text.
pub fn value_from_json(j: &Json) -> Result<Value> {
    Ok(match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::I64(i),
            None => Value::F64(n.as_f64().ok_or_else(|| bad("number"))?),
        },
        Json::String(s) => Value::Text(s.clone()),
        Json::Object(map) if map.len() == 1 => {
            let (tag, v) = map.iter().next().ok_or_else(|| bad("empty value"))?;
            let text = || v.as_str().ok_or_else(|| bad(format!("`{tag}` value")));
            match tag.as_str() {
                "i32" => Value::I32(v.as_i64().and_then(|i| i32::try_from(i).ok()).ok_or_else(|| bad("i32"))?),
                "f64" => Value::F64(match v {
                    Json::String(s) if s == "NaN" => f64::NAN,
                    Json::String(s) if s == "Infinity" => f64::INFINITY,
                    Json::String(s) if s == "-Infinity" => f64::NEG_INFINITY,
                    _ => v.as_f64().ok_or_else(|| bad("f64"))?,
                }),
                "bytes" => Value::Bytes(unhex(text()?)?),
                #[cfg(feature = "uuid")]
                "uuid" => Value::Uuid(uuid::Uuid::parse_str(text()?).map_err(|_| bad("uuid"))?),
                #[cfg(feature = "chrono")]
                "ts" => Value::Timestamp(
                    chrono::NaiveDateTime::parse_from_str(text()?, "%Y-%m-%dT%H:%M:%S%.f").map_err(|_| bad("ts"))?,
                ),
                #[cfg(feature = "chrono")]
                "tstz" => Value::TimestampTz(
                    chrono::DateTime::parse_from_rfc3339(text()?).map_err(|_| bad("tstz"))?.with_timezone(&chrono::Utc),
                ),
                #[cfg(feature = "chrono")]
                "date" => Value::Date(text()?.parse().map_err(|_| bad("date"))?),
                #[cfg(feature = "chrono")]
                "time" => {
                    Value::Time(chrono::NaiveTime::parse_from_str(text()?, "%H:%M:%S%.f").map_err(|_| bad("time"))?)
                }
                #[cfg(feature = "json")]
                "json" => Value::Json(v.clone()),
                // Without the feature, the value stays readable as text.
                #[allow(unreachable_patterns)]
                "json" => Value::Text(v.to_string()),
                #[allow(unreachable_patterns)]
                "uuid" | "ts" | "tstz" | "date" | "time" => Value::Text(text()?.to_owned()),
                other => return Err(bad(format!("unknown value type `{other}`"))),
            }
        }
        other => return Err(bad(format!("value `{other}`"))),
    })
}

/// Rows as JSON arrays.
pub fn rows_to_json(rows: &[Vec<Value>]) -> Json {
    Json::Array(rows.iter().map(|r| Json::Array(r.iter().map(value_to_json).collect())).collect())
}

/// Rows read from JSON arrays.
pub fn rows_from_json(j: &Json) -> Result<Vec<Vec<Value>>> {
    j.as_array()
        .ok_or_else(|| bad("rows"))?
        .iter()
        .map(|r| r.as_array().ok_or_else(|| bad("row"))?.iter().map(value_from_json).collect())
        .collect()
}

/// The answer to an `execute`.
pub fn exec_result_to_json(r: &ExecResult) -> Json {
    json!({ "rowsAffected": r.rows_affected, "lastInsertId": r.last_insert_id.as_ref().map(value_to_json) })
}

/// Reads the answer to an `execute`.
pub fn exec_result_from_json(j: &Json) -> Result<ExecResult> {
    Ok(ExecResult {
        rows_affected: j.get("rowsAffected").and_then(Json::as_u64).unwrap_or(0),
        last_insert_id: match j.get("lastInsertId") {
            None | Some(Json::Null) => None,
            Some(v) => Some(value_from_json(v)?),
        },
    })
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error as JSON, with the HTTP status it goes with.
pub fn error_to_json(e: &Error) -> (u16, Json) {
    let (status, kind, extra) = match e {
        Error::NotFound => (404, "not_found", json!({})),
        Error::UniqueViolation { table, column } => {
            (409, "unique_violation", json!({ "table": table, "column": column }))
        }
        Error::Constraint(_) => (409, "constraint", json!({})),
        Error::NoSuchTable(t) => (404, "no_such_table", json!({ "table": t })),
        Error::TableExists(t) => (409, "table_exists", json!({ "table": t })),
        Error::Conflict(t) => (409, "conflict", json!({ "table": t })),
        Error::Validation(_) => (400, "validation", json!({})),
        Error::Unsupported(_) => (400, "unsupported", json!({})),
        Error::Conversion(_) => (400, "conversion", json!({})),
        Error::TransactionFinished => (409, "transaction_finished", json!({})),
        Error::UnexpectedNull { column } => (400, "unexpected_null", json!({ "column": column })),
        Error::Decode { column, .. } => (400, "decode", json!({ "column": column })),
        Error::StaleRow { table, key } => (409, "stale_row", json!({ "table": table, "key": key })),
        _ => (500, "database", json!({})),
    };
    let message = match e {
        Error::Constraint(m) | Error::Validation(m) | Error::Unsupported(m) | Error::Conversion(m) => m.clone(),
        Error::Decode { message, .. } => message.clone(),
        other => other.to_string(),
    };
    let mut body = Map::new();
    body.insert("kind".into(), json!(kind));
    body.insert("message".into(), json!(message));
    if let Json::Object(extra) = extra {
        body.extend(extra);
    }
    (status, json!({ "error": body }))
}

/// An error answered by a Nuage DB server that Memoire has no variant for
/// (authentication, server failure...), inside [`Error::Database`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NuageError {
    /// HTTP status.
    pub status: u16,
    /// Error kind (`unauthorized`, `database`...).
    pub kind: String,
    /// Message.
    pub message: String,
}

impl NuageError {
    /// `true` when the server refused the credentials.
    pub fn is_unauthorized(&self) -> bool {
        self.status == 401 || self.status == 403
    }
}

impl std::fmt::Display for NuageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Nuage DB ({}, HTTP {}): {}", self.kind, self.status, self.message)
    }
}

impl std::error::Error for NuageError {}

/// The [`Error`] an error answer stands for.
pub fn error_from_json(status: u16, body: &[u8]) -> Error {
    let parsed: Option<Json> = serde_json::from_slice(body).ok();
    let Some(err) = parsed.as_ref().and_then(|j| j.get("error")) else {
        let text = String::from_utf8_lossy(body);
        let message =
            if text.trim().is_empty() { format!("HTTP status {status}") } else { text.chars().take(500).collect() };
        return Error::Database(Box::new(NuageError { status, kind: "http".into(), message }));
    };
    let s = |k: &str| err.get(k).and_then(Json::as_str).unwrap_or_default().to_owned();
    let message = s("message");
    match s("kind").as_str() {
        "not_found" => Error::NotFound,
        "unique_violation" => Error::UniqueViolation { table: s("table"), column: s("column") },
        "constraint" => Error::Constraint(message),
        "no_such_table" => Error::NoSuchTable(s("table")),
        "table_exists" => Error::TableExists(s("table")),
        "conflict" => Error::Conflict(s("table")),
        "validation" => Error::Validation(message),
        "unsupported" => Error::Unsupported(message),
        "conversion" => Error::Conversion(message),
        "transaction_finished" => Error::TransactionFinished,
        "unexpected_null" => Error::UnexpectedNull { column: s("column") },
        "decode" => Error::Decode { column: s("column"), message },
        "stale_row" => Error::StaleRow { table: s("table"), key: s("key") },
        kind => Error::Database(Box::new(NuageError { status, kind: kind.to_owned(), message })),
    }
}

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

const COLUMN_TYPES: [(ColumnType, &str); 14] = [
    (ColumnType::Bool, "bool"),
    (ColumnType::SmallInt, "smallint"),
    (ColumnType::Int, "int"),
    (ColumnType::BigInt, "bigint"),
    (ColumnType::Real, "real"),
    (ColumnType::Double, "double"),
    (ColumnType::Text, "text"),
    (ColumnType::Bytes, "bytes"),
    (ColumnType::Uuid, "uuid"),
    (ColumnType::Timestamp, "timestamp"),
    (ColumnType::TimestampTz, "timestamptz"),
    (ColumnType::Date, "date"),
    (ColumnType::Time, "time"),
    (ColumnType::Json, "json"),
];

fn type_name(t: ColumnType) -> &'static str {
    COLUMN_TYPES.iter().find(|(c, _)| *c == t).map_or("text", |(_, n)| n)
}

fn type_of(name: &str) -> Result<ColumnType> {
    COLUMN_TYPES.iter().find(|(_, n)| *n == name).map(|(c, _)| *c).ok_or_else(|| bad(format!("column type `{name}`")))
}

/// A table schema as JSON.
pub fn schema_to_json(t: &TableSchema) -> Json {
    json!({
        "name": t.name,
        "pk": t.primary_key,
        "columns": t.columns.iter().map(|c| json!({
            "name": c.name,
            "field": c.field,
            "kind": type_name(c.kind),
            "nullable": c.nullable,
            "primaryKey": c.primary_key,
            "autoIncrement": c.auto_increment,
            "unique": c.unique,
            "indexed": c.indexed,
            "fulltext": c.fulltext,
            "sqlType": c.sql_type,
            "default": c.default,
            "references": c.references.map(|r| json!({
                "table": r.table, "column": r.column, "onDelete": r.on_delete, "onUpdate": r.on_update,
            })),
        })).collect::<Vec<_>>(),
        "indexes": t.indexes.iter().map(|ix| json!({
            "name": ix.name, "columns": ix.columns, "unique": ix.unique,
        })).collect::<Vec<_>>(),
    })
}

/// Schemas and names received from clients, made `'static` once and shared
/// by every later statement. Memoire's statements point to `'static`
/// schemas: a server builds each distinct schema once and keeps it. The
/// number of distinct schemas and names is capped, so a client cannot grow
/// the server's memory without bound.
pub struct SchemaCache {
    schemas: Mutex<HashMap<String, &'static TableSchema>>,
    names: Mutex<HashSet<&'static str>>,
    max_schemas: usize,
}

impl std::fmt::Debug for SchemaCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaCache").field("schemas", &self.len()).finish()
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new(10_000)
    }
}

impl SchemaCache {
    /// A cache holding at most `max_schemas` distinct schemas.
    pub fn new(max_schemas: usize) -> Self {
        SchemaCache { schemas: Mutex::new(HashMap::new()), names: Mutex::new(HashSet::new()), max_schemas }
    }

    /// Number of schemas kept.
    pub fn len(&self) -> usize {
        self.schemas.lock().map_or(0, |m| m.len())
    }

    /// `true` when no schema is kept.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A `'static` copy of a name.
    pub fn name(&self, s: &str) -> Result<&'static str> {
        let mut names = self.names.lock().map_err(|_| bad("poisoned cache"))?;
        if let Some(n) = names.get(s) {
            return Ok(n);
        }
        if names.len() >= self.max_schemas * 64 {
            return Err(Error::Unsupported("too many distinct table and column names".into()));
        }
        let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
        names.insert(leaked);
        Ok(leaked)
    }

    fn opt_name(&self, j: Option<&Json>) -> Result<Option<&'static str>> {
        match j.and_then(Json::as_str) {
            Some(s) => Ok(Some(self.name(s)?)),
            None => Ok(None),
        }
    }

    /// The `'static` schema described by `j`.
    pub fn schema(&self, j: &Json) -> Result<&'static TableSchema> {
        let key = j.to_string();
        if let Some(s) = self.schemas.lock().map_err(|_| bad("poisoned cache"))?.get(&key) {
            return Ok(s);
        }
        let s = |k: &str| j.get(k).and_then(Json::as_str).ok_or_else(|| bad(format!("schema `{k}`")));
        let name = self.name(s("name")?)?;
        let mut columns = Vec::new();
        for c in j.get("columns").and_then(Json::as_array).ok_or_else(|| bad("schema columns"))? {
            let cs = |k: &str| c.get(k).and_then(Json::as_str).ok_or_else(|| bad(format!("column `{k}`")));
            let flag = |k: &str| c.get(k).and_then(Json::as_bool).unwrap_or(false);
            let references = match c.get("references") {
                Some(r) if r.is_object() => Some(ForeignKey {
                    table: self.name(r.get("table").and_then(Json::as_str).ok_or_else(|| bad("reference table"))?)?,
                    column: self
                        .name(r.get("column").and_then(Json::as_str).ok_or_else(|| bad("reference column"))?)?,
                    on_delete: self.opt_name(r.get("onDelete"))?,
                    on_update: self.opt_name(r.get("onUpdate"))?,
                }),
                _ => None,
            };
            columns.push(ColumnDef {
                name: self.name(cs("name")?)?,
                field: self.name(c.get("field").and_then(Json::as_str).unwrap_or_else(|| cs("name").unwrap_or("")))?,
                kind: type_of(cs("kind")?)?,
                nullable: flag("nullable"),
                primary_key: flag("primaryKey"),
                auto_increment: flag("autoIncrement"),
                unique: flag("unique"),
                indexed: flag("indexed"),
                fulltext: flag("fulltext"),
                sql_type: self.opt_name(c.get("sqlType"))?,
                default: self.opt_name(c.get("default"))?,
                references,
            });
        }
        let pk = j.get("pk").and_then(Json::as_u64).ok_or_else(|| bad("schema pk"))? as usize;
        if pk >= columns.len() {
            return Err(bad("primary key out of range"));
        }
        let mut indexes = Vec::new();
        for ix in j.get("indexes").and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default() {
            let cols: Vec<usize> = ix
                .get("columns")
                .and_then(Json::as_array)
                .ok_or_else(|| bad("index columns"))?
                .iter()
                .map(|c| {
                    c.as_u64().map(|c| c as usize).filter(|&c| c < columns.len()).ok_or_else(|| bad("index column"))
                })
                .collect::<Result<_>>()?;
            indexes.push(IndexDef {
                name: self.name(ix.get("name").and_then(Json::as_str).unwrap_or_default())?,
                columns: Box::leak(cols.into_boxed_slice()),
                unique: ix.get("unique").and_then(Json::as_bool).unwrap_or(false),
            });
        }
        let mut schemas = self.schemas.lock().map_err(|_| bad("poisoned cache"))?;
        if let Some(s) = schemas.get(&key) {
            return Ok(s);
        }
        if schemas.len() >= self.max_schemas {
            return Err(Error::Unsupported("too many distinct table schemas".into()));
        }
        // SQL is never rendered on a Nuage DB server: the pre-rendered
        // statements stay empty.
        let empty = StaticSql { columns: "", select: "", find: "", insert: "", update: "", delete: "", upsert: "" };
        let schema: &'static TableSchema = Box::leak(Box::new(TableSchema {
            name,
            columns: Box::leak(columns.into_boxed_slice()),
            primary_key: pk,
            indexes: Box::leak(indexes.into_boxed_slice()),
            sql: [empty; 3],
        }));
        schemas.insert(key, schema);
        Ok(schema)
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

const BIN_OPS: [(BinOp, &str); 12] = [
    (BinOp::Eq, "="),
    (BinOp::Ne, "<>"),
    (BinOp::Lt, "<"),
    (BinOp::Le, "<="),
    (BinOp::Gt, ">"),
    (BinOp::Ge, ">="),
    (BinOp::And, "and"),
    (BinOp::Or, "or"),
    (BinOp::Add, "+"),
    (BinOp::Sub, "-"),
    (BinOp::Mul, "*"),
    (BinOp::Div, "/"),
];

const AGGS: [(AggFunc, &str); 5] = [
    (AggFunc::Count, "count"),
    (AggFunc::Sum, "sum"),
    (AggFunc::Avg, "avg"),
    (AggFunc::Min, "min"),
    (AggFunc::Max, "max"),
];

fn expr_to_json(e: &Expr) -> Json {
    match e {
        Expr::Column(c) => {
            json!({ "col": [c.table, c.name, c.index, type_name(c.kind)] })
        }
        Expr::Value(v) => json!({ "val": value_to_json(v) }),
        Expr::Binary { op, left, right } => json!({
            "bin": [BIN_OPS.iter().find(|(o, _)| o == op).map_or("=", |(_, s)| s), expr_to_json(left), expr_to_json(right)]
        }),
        Expr::Not(inner) => json!({ "not": expr_to_json(inner) }),
        Expr::IsNull { expr, negated } => json!({ "isNull": expr_to_json(expr), "negated": negated }),
        Expr::InList { expr, list, negated } => json!({
            "in": expr_to_json(expr), "list": list.iter().map(value_to_json).collect::<Vec<_>>(), "negated": negated
        }),
        Expr::Between { expr, low, high, negated } => json!({
            "between": expr_to_json(expr), "low": expr_to_json(low), "high": expr_to_json(high), "negated": negated
        }),
        Expr::Like { expr, pattern, case_insensitive, negated, escaped } => json!({
            "like": expr_to_json(expr), "pattern": expr_to_json(pattern),
            "ci": case_insensitive, "negated": negated, "escaped": escaped
        }),
        Expr::Match(m) => json!({
            "match": expr_to_json(&m.expr), "words": m.words, "query": value_to_json(&m.query),
            "patterns": m.patterns.iter().map(value_to_json).collect::<Vec<_>>()
        }),
        Expr::Aggregate { func, arg, distinct } => json!({
            "agg": AGGS.iter().find(|(f, _)| f == func).map_or("count", |(_, s)| s),
            "arg": arg.as_deref().map(expr_to_json), "distinct": distinct
        }),
        Expr::InSubquery { expr, query, negated } => json!({
            "inQuery": expr_to_json(expr), "query": select_to_json(query), "negated": negated
        }),
    }
}

fn expr_from_json(j: &Json, cache: &SchemaCache) -> Result<Expr> {
    let sub = |k: &str| -> Result<Box<Expr>> { Ok(Box::new(expr_from_json(j.get(k).ok_or_else(|| bad(k))?, cache)?)) };
    let flag = |k: &str| j.get(k).and_then(Json::as_bool).unwrap_or(false);
    let obj = j.as_object().ok_or_else(|| bad("expression"))?;
    if let Some(c) = obj.get("col") {
        let a = c.as_array().filter(|a| a.len() == 4).ok_or_else(|| bad("column"))?;
        return Ok(Expr::Column(ColumnRef {
            table: cache.name(a[0].as_str().ok_or_else(|| bad("column table"))?)?,
            name: cache.name(a[1].as_str().ok_or_else(|| bad("column name"))?)?,
            index: a[2].as_u64().ok_or_else(|| bad("column index"))? as usize,
            kind: type_of(a[3].as_str().ok_or_else(|| bad("column kind"))?)?,
        }));
    }
    if let Some(v) = obj.get("val") {
        return Ok(Expr::Value(value_from_json(v)?));
    }
    if let Some(b) = obj.get("bin") {
        let a = b.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad("binary"))?;
        let op = a[0].as_str().and_then(|s| BIN_OPS.iter().find(|(_, n)| *n == s)).ok_or_else(|| bad("operator"))?.0;
        return Ok(Expr::binary(op, expr_from_json(&a[1], cache)?, expr_from_json(&a[2], cache)?));
    }
    if obj.contains_key("not") {
        return Ok(Expr::Not(sub("not")?));
    }
    if obj.contains_key("isNull") {
        return Ok(Expr::IsNull { expr: sub("isNull")?, negated: flag("negated") });
    }
    if obj.contains_key("in") {
        let list = j.get("list").and_then(Json::as_array).ok_or_else(|| bad("list"))?;
        return Ok(Expr::InList {
            expr: sub("in")?,
            list: list.iter().map(value_from_json).collect::<Result<_>>()?,
            negated: flag("negated"),
        });
    }
    if obj.contains_key("between") {
        return Ok(Expr::Between {
            expr: sub("between")?,
            low: sub("low")?,
            high: sub("high")?,
            negated: flag("negated"),
        });
    }
    if obj.contains_key("like") {
        return Ok(Expr::Like {
            expr: sub("like")?,
            pattern: sub("pattern")?,
            case_insensitive: flag("ci"),
            negated: flag("negated"),
            escaped: flag("escaped"),
        });
    }
    if obj.contains_key("match") {
        let words = j
            .get("words")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("words"))?
            .iter()
            .map(|w| w.as_str().map(str::to_owned).ok_or_else(|| bad("word")))
            .collect::<Result<_>>()?;
        let patterns = j
            .get("patterns")
            .and_then(Json::as_array)
            .map(|p| p.iter().map(value_from_json).collect::<Result<Vec<_>>>())
            .transpose()?
            .unwrap_or_default();
        return Ok(Expr::Match(Box::new(TextSearch {
            expr: sub("match")?,
            words,
            query: value_from_json(j.get("query").unwrap_or(&Json::Null))?,
            patterns,
        })));
    }
    if let Some(f) = obj.get("agg") {
        let func = f.as_str().and_then(|s| AGGS.iter().find(|(_, n)| *n == s)).ok_or_else(|| bad("aggregate"))?.0;
        let arg = match j.get("arg") {
            None | Some(Json::Null) => None,
            Some(a) => Some(Box::new(expr_from_json(a, cache)?)),
        };
        return Ok(Expr::Aggregate { func, arg, distinct: flag("distinct") });
    }
    if obj.contains_key("inQuery") {
        let query = select_from_json(j.get("query").ok_or_else(|| bad("subquery"))?, cache)?;
        return Ok(Expr::InSubquery { expr: sub("inQuery")?, query: Box::new(query), negated: flag("negated") });
    }
    Err(bad(format!("expression `{j}`")))
}

fn order_to_json(o: &Order) -> Json {
    json!({
        "expr": expr_to_json(&o.expr),
        "desc": o.desc,
        "nulls": o.nulls.map(|n| if n == Nulls::First { "first" } else { "last" }),
    })
}

fn order_from_json(j: &Json, cache: &SchemaCache) -> Result<Order> {
    Ok(Order {
        expr: expr_from_json(j.get("expr").ok_or_else(|| bad("order"))?, cache)?,
        desc: j.get("desc").and_then(Json::as_bool).unwrap_or(false),
        nulls: match j.get("nulls").and_then(Json::as_str) {
            Some("first") => Some(Nulls::First),
            Some("last") => Some(Nulls::Last),
            _ => None,
        },
    })
}

fn select_to_json(s: &SelectAst) -> Json {
    json!({
        "from": schema_to_json(s.from),
        "projection": match &s.projection {
            Projection::Tables(t) => json!({ "tables": t.iter().map(|t| t.name).collect::<Vec<_>>() }),
            Projection::Exprs(e) => json!({ "exprs": e.iter().map(expr_to_json).collect::<Vec<_>>() }),
        },
        "joins": s.joins.iter().map(|j| json!({
            "kind": if j.kind == JoinKind::Inner { "inner" } else { "left" },
            "table": schema_to_json(j.table),
            "on": expr_to_json(&j.on),
        })).collect::<Vec<_>>(),
        "filter": s.filter.as_ref().map(expr_to_json),
        "groupBy": s.group_by.iter().map(expr_to_json).collect::<Vec<_>>(),
        "having": s.having.as_ref().map(expr_to_json),
        "order": s.order.iter().map(order_to_json).collect::<Vec<_>>(),
        "limit": s.limit,
        "offset": s.offset,
        "distinct": s.distinct,
    })
}

fn opt_expr(j: Option<&Json>, cache: &SchemaCache) -> Result<Option<Expr>> {
    match j {
        None | Some(Json::Null) => Ok(None),
        Some(e) => Ok(Some(expr_from_json(e, cache)?)),
    }
}

fn exprs(j: Option<&Json>, cache: &SchemaCache) -> Result<Vec<Expr>> {
    j.and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default().iter().map(|e| expr_from_json(e, cache)).collect()
}

fn select_from_json(j: &Json, cache: &SchemaCache) -> Result<SelectAst> {
    let from = cache.schema(j.get("from").ok_or_else(|| bad("from"))?)?;
    let mut joins = Vec::new();
    for jj in j.get("joins").and_then(Json::as_array).map(Vec::as_slice).unwrap_or_default() {
        joins.push(Join {
            kind: if jj.get("kind").and_then(Json::as_str) == Some("left") { JoinKind::Left } else { JoinKind::Inner },
            table: cache.schema(jj.get("table").ok_or_else(|| bad("join table"))?)?,
            on: expr_from_json(jj.get("on").ok_or_else(|| bad("join condition"))?, cache)?,
        });
    }
    let p = j.get("projection").ok_or_else(|| bad("projection"))?;
    let projection = if let Some(names) = p.get("tables").and_then(Json::as_array) {
        let visible: Vec<&'static TableSchema> = std::iter::once(from).chain(joins.iter().map(|j| j.table)).collect();
        Projection::Tables(
            names
                .iter()
                .map(|n| {
                    let n = n.as_str().ok_or_else(|| bad("projected table"))?;
                    visible.iter().copied().find(|t| t.name == n).ok_or_else(|| bad(format!("projected table `{n}`")))
                })
                .collect::<Result<_>>()?,
        )
    } else {
        Projection::Exprs(exprs(p.get("exprs"), cache)?)
    };
    Ok(SelectAst {
        from,
        projection,
        joins,
        filter: opt_expr(j.get("filter"), cache)?,
        group_by: exprs(j.get("groupBy"), cache)?,
        having: opt_expr(j.get("having"), cache)?,
        order: j
            .get("order")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|o| order_from_json(o, cache))
            .collect::<Result<_>>()?,
        limit: j.get("limit").and_then(Json::as_u64),
        offset: j.get("offset").and_then(Json::as_u64),
        distinct: j.get("distinct").and_then(Json::as_bool).unwrap_or(false),
    })
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

fn values_json(values: &[Value]) -> Json {
    Json::Array(values.iter().map(value_to_json).collect())
}

fn values_from(j: Option<&Json>) -> Result<Vec<Value>> {
    j.and_then(Json::as_array).ok_or_else(|| bad("values"))?.iter().map(value_from_json).collect()
}

/// A statement as JSON.
pub fn query_to_json(q: &Query<'_>) -> Result<Json> {
    Ok(match *q {
        Query::Select(s) => json!({ "select": select_to_json(s) }),
        Query::Insert(i) => json!({
            "insert": schema_to_json(i.table),
            "rows": i.rows.iter().map(|r| values_json(r)).collect::<Vec<_>>(),
        }),
        Query::Update(u) => json!({
            "update": schema_to_json(u.table),
            "sets": u.sets.iter().map(|(c, e)| json!([c, expr_to_json(e)])).collect::<Vec<_>>(),
            "filter": u.filter.as_ref().map(expr_to_json),
        }),
        Query::Delete(d) => json!({ "delete": schema_to_json(d.table), "filter": d.filter.as_ref().map(expr_to_json) }),
        Query::FindByKey { table, key } => json!({ "findByKey": schema_to_json(table), "key": value_to_json(key) }),
        Query::InsertRow { table, values } => {
            json!({ "insertRow": schema_to_json(table), "values": values_json(values) })
        }
        Query::UpsertRow { table, values } => {
            json!({ "upsertRow": schema_to_json(table), "values": values_json(values) })
        }
        Query::UpdateRow { table, values } => {
            json!({ "updateRow": schema_to_json(table), "values": values_json(values) })
        }
        Query::DeleteByKey { table, key } => json!({ "deleteByKey": schema_to_json(table), "key": value_to_json(key) }),
        Query::Raw { sql, params } => json!({ "raw": sql, "params": values_json(params) }),
        Query::Script { sql } => json!({ "script": sql }),
        Query::CreateTable { table, if_not_exists } => {
            json!({ "createTable": schema_to_json(table), "ifNotExists": if_not_exists })
        }
        Query::DropTable { name, if_exists } => json!({ "dropTable": name, "ifExists": if_exists }),
    })
}

/// A statement read from JSON, owning what [`Query`] borrows.
#[derive(Clone, Debug)]
pub enum OwnedQuery {
    /// `SELECT`.
    Select(SelectAst),
    /// Multi-row `INSERT`.
    Insert(InsertAst),
    /// `UPDATE`.
    Update(UpdateAst),
    /// `DELETE`.
    Delete(DeleteAst),
    /// [`Query::FindByKey`].
    FindByKey(&'static TableSchema, Value),
    /// [`Query::InsertRow`].
    InsertRow(&'static TableSchema, Vec<Value>),
    /// [`Query::UpsertRow`].
    UpsertRow(&'static TableSchema, Vec<Value>),
    /// [`Query::UpdateRow`].
    UpdateRow(&'static TableSchema, Vec<Value>),
    /// [`Query::DeleteByKey`].
    DeleteByKey(&'static TableSchema, Value),
    /// [`Query::Raw`].
    Raw(String, Vec<Value>),
    /// [`Query::Script`].
    Script(String),
    /// [`Query::CreateTable`].
    CreateTable(&'static TableSchema, bool),
    /// [`Query::DropTable`].
    DropTable(String, bool),
}

impl OwnedQuery {
    /// The statement, as executors take it.
    pub fn as_query(&self) -> Query<'_> {
        match self {
            OwnedQuery::Select(s) => Query::Select(s),
            OwnedQuery::Insert(i) => Query::Insert(i),
            OwnedQuery::Update(u) => Query::Update(u),
            OwnedQuery::Delete(d) => Query::Delete(d),
            OwnedQuery::FindByKey(table, key) => Query::FindByKey { table, key },
            OwnedQuery::InsertRow(table, values) => Query::InsertRow { table, values },
            OwnedQuery::UpsertRow(table, values) => Query::UpsertRow { table, values },
            OwnedQuery::UpdateRow(table, values) => Query::UpdateRow { table, values },
            OwnedQuery::DeleteByKey(table, key) => Query::DeleteByKey { table, key },
            OwnedQuery::Raw(sql, params) => Query::Raw { sql, params },
            OwnedQuery::Script(sql) => Query::Script { sql },
            OwnedQuery::CreateTable(table, if_not_exists) => {
                Query::CreateTable { table, if_not_exists: *if_not_exists }
            }
            OwnedQuery::DropTable(name, if_exists) => Query::DropTable { name, if_exists: *if_exists },
        }
    }
}

/// Reads a statement, with the schemas it names made `'static` by `cache`.
pub fn query_from_json(j: &Json, cache: &SchemaCache) -> Result<OwnedQuery> {
    let schema = |k: &str| cache.schema(j.get(k).ok_or_else(|| bad(k))?);
    let flag = |k: &str| j.get(k).and_then(Json::as_bool).unwrap_or(false);
    let obj = j.as_object().ok_or_else(|| bad("statement"))?;
    Ok(if let Some(s) = obj.get("select") {
        OwnedQuery::Select(select_from_json(s, cache)?)
    } else if obj.contains_key("insert") {
        let rows = j
            .get("rows")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("rows"))?
            .iter()
            .map(|r| values_from(Some(r)))
            .collect::<Result<_>>()?;
        OwnedQuery::Insert(InsertAst { table: schema("insert")?, rows })
    } else if obj.contains_key("update") {
        let sets = j
            .get("sets")
            .and_then(Json::as_array)
            .ok_or_else(|| bad("sets"))?
            .iter()
            .map(|s| {
                let a = s.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad("set"))?;
                Ok((a[0].as_u64().ok_or_else(|| bad("set column"))? as usize, expr_from_json(&a[1], cache)?))
            })
            .collect::<Result<_>>()?;
        OwnedQuery::Update(UpdateAst { table: schema("update")?, sets, filter: opt_expr(j.get("filter"), cache)? })
    } else if obj.contains_key("delete") {
        OwnedQuery::Delete(DeleteAst { table: schema("delete")?, filter: opt_expr(j.get("filter"), cache)? })
    } else if obj.contains_key("findByKey") {
        OwnedQuery::FindByKey(schema("findByKey")?, value_from_json(j.get("key").ok_or_else(|| bad("key"))?)?)
    } else if obj.contains_key("insertRow") {
        OwnedQuery::InsertRow(schema("insertRow")?, values_from(j.get("values"))?)
    } else if obj.contains_key("upsertRow") {
        OwnedQuery::UpsertRow(schema("upsertRow")?, values_from(j.get("values"))?)
    } else if obj.contains_key("updateRow") {
        OwnedQuery::UpdateRow(schema("updateRow")?, values_from(j.get("values"))?)
    } else if obj.contains_key("deleteByKey") {
        OwnedQuery::DeleteByKey(schema("deleteByKey")?, value_from_json(j.get("key").ok_or_else(|| bad("key"))?)?)
    } else if let Some(sql) = obj.get("raw").and_then(Json::as_str) {
        OwnedQuery::Raw(sql.to_owned(), values_from(j.get("params"))?)
    } else if let Some(sql) = obj.get("script").and_then(Json::as_str) {
        OwnedQuery::Script(sql.to_owned())
    } else if obj.contains_key("createTable") {
        OwnedQuery::CreateTable(schema("createTable")?, flag("ifNotExists"))
    } else if let Some(name) = obj.get("dropTable").and_then(Json::as_str) {
        OwnedQuery::DropTable(name.to_owned(), flag("ifExists"))
    } else {
        return Err(bad("unknown statement"));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "wire_users", index(name, age))]
    struct U {
        #[memoire(primary_key, auto_increment)]
        id: i64,
        #[memoire(unique)]
        email: String,
        name: String,
        age: Option<i32>,
    }

    #[test]
    #[cfg_attr(not(any(feature = "chrono", feature = "uuid", feature = "json")), allow(unused_mut))]
    fn values_round_trip() {
        let mut values = vec![
            Value::Null,
            Value::Bool(true),
            Value::I32(-3),
            Value::I64(i64::MAX),
            Value::F64(0.1),
            Value::F64(f64::NEG_INFINITY),
            Value::Text("é".into()),
            Value::Bytes(vec![0, 255]),
        ];
        #[cfg(feature = "chrono")]
        {
            use chrono::TimeZone;
            let t = chrono::Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap() + chrono::Duration::microseconds(7);
            values.extend([
                Value::TimestampTz(t),
                Value::Timestamp(t.naive_utc()),
                Value::Date(t.date_naive()),
                Value::Time(t.time()),
            ]);
        }
        #[cfg(feature = "uuid")]
        values.push(Value::Uuid(uuid::Uuid::from_u128(42)));
        #[cfg(feature = "json")]
        values.push(Value::Json(json!({"a": [1, null]})));
        for v in values {
            assert_eq!(value_from_json(&value_to_json(&v)).unwrap(), v);
        }
        assert!(matches!(value_from_json(&json!({"f64": "NaN"})).unwrap(), Value::F64(f) if f.is_nan()));
    }

    #[test]
    fn statements_round_trip() {
        let cache = SchemaCache::default();
        let select = U::query()
            .filter((U::AGE.gt(18) | U::NAME.like("A%")) & U::EMAIL.is_in(["a", "b"]))
            .order_by(U::NAME.desc())
            .limit(5);
        let ast = select.ast().clone();
        let j = query_to_json(&Query::Select(&ast)).unwrap();
        let back = query_from_json(&j, &cache).unwrap();
        let OwnedQuery::Select(back) = back else { panic!() };
        assert_eq!(back.filter, ast.filter);
        assert_eq!(back.order, ast.order);
        assert_eq!(back.limit, Some(5));
        assert_eq!(back.from.name, "wire_users");
        assert_eq!(back.from.columns, U::SCHEMA.columns);
        assert_eq!(back.from.indexes, U::SCHEMA.indexes);
        // The same schema is built once.
        let again = query_from_json(&j, &cache).unwrap();
        let OwnedQuery::Select(again) = again else { panic!() };
        assert!(std::ptr::eq(again.from, back.from));

        let values = vec![Value::I64(0), Value::Text("x".into()), Value::Text("n".into()), Value::Null];
        let j = query_to_json(&Query::InsertRow { table: U::SCHEMA, values: &values }).unwrap();
        let OwnedQuery::InsertRow(t, v) = query_from_json(&j, &cache).unwrap() else { panic!() };
        assert_eq!((t.name, v), ("wire_users", values));
    }

    #[test]
    fn errors_round_trip() {
        for e in [
            Error::UniqueViolation { table: "t".into(), column: "c".into() },
            Error::NoSuchTable("t".into()),
            Error::Conflict("t".into()),
            Error::Unsupported("no".into()),
            Error::NotFound,
        ] {
            let (status, body) = error_to_json(&e);
            let back = error_from_json(status, body.to_string().as_bytes());
            assert_eq!(back.to_string(), e.to_string());
        }
        let e = error_from_json(401, br#"{"error": {"kind": "unauthorized", "message": "token"}}"#);
        assert!(
            matches!(&e, Error::Database(d) if d.downcast_ref::<NuageError>().is_some_and(NuageError::is_unauthorized))
        );
    }
}
