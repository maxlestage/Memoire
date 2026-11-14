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

/// A JSON object with these fields, in this order. The values are moved in:
/// `json!` would serialize each value that is already JSON into a new copy,
/// so a statement built with it was copied once per level of nesting.
fn object<const N: usize>(fields: [(&str, Json); N]) -> Json {
    let mut map = Map::with_capacity(N);
    for (key, value) in fields {
        map.insert(key.to_owned(), value);
    }
    Json::Object(map)
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

use crate::backend::hex::encode as hex;

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(bad("odd hex string"));
    }
    crate::backend::hex::decode(s).ok_or_else(|| bad("hex"))
}

/// A value as JSON.
pub fn value_to_json(v: &Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::I32(i) => object([("i32", (*i).into())]),
        Value::I64(i) => (*i).into(),
        Value::F64(f) if f.is_finite() => object([("f64", (*f).into())]),
        Value::F64(f) => {
            let name = if f.is_nan() {
                "NaN"
            } else if *f > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            };
            object([("f64", name.into())])
        }
        Value::Text(s) => Json::String(s.clone()),
        Value::Bytes(b) => object([("bytes", hex(b).into())]),
        #[cfg(feature = "uuid")]
        Value::Uuid(u) => object([("uuid", u.to_string().into())]),
        #[cfg(feature = "chrono")]
        Value::Timestamp(t) => object([("ts", t.format("%Y-%m-%dT%H:%M:%S%.f").to_string().into())]),
        #[cfg(feature = "chrono")]
        Value::TimestampTz(t) => object([("tstz", t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true).into())]),
        #[cfg(feature = "chrono")]
        Value::Date(d) => object([("date", d.to_string().into())]),
        #[cfg(feature = "chrono")]
        Value::Time(t) => object([("time", t.format("%H:%M:%S%.f").to_string().into())]),
        #[cfg(feature = "json")]
        Value::Json(j) => object([("json", j.clone())]),
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

/// The rows of a `fetch` answer (`{"rows": [[<value>, ...], ...]}`), read
/// straight from its text: the same rows and the same errors as parsing the
/// answer, then [`rows_from_json`] on its `rows`, without building the JSON
/// tree in between (one string per text value instead of two).
///
/// An answer that is not JSON is an [`Error::Conversion`] saying so, as when
/// the client parses any other answer.
pub fn rows_from_slice(body: &[u8]) -> Result<Vec<Vec<Value>>> {
    read_rows(body)?
}

/// [`rows_from_slice`], its two kinds of errors apart: `Err` for an answer
/// that is not JSON, `Ok(Err)` for JSON that does not hold rows.
pub(crate) fn read_rows(body: &[u8]) -> Result<Result<Vec<Vec<Value>>>> {
    if body.is_empty() {
        return Ok(Err(bad("rows")));
    }
    let top: rows::Top =
        serde_json::from_slice(body).map_err(|e| Error::Conversion(format!("invalid answer from Nuage DB: {e}")))?;
    Ok(top.0.rows.unwrap_or_else(|| Err(bad("rows"))))
}

/// [`rows_from_slice`]'s reader. An error in the rows does not stop it: the
/// text is read to its end, so that an answer that is not JSON is reported as
/// such first, as when it was parsed whole before its rows were read.
mod rows {
    use serde::Deserialize;
    use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
    use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

    use super::{Json, bad, value_from_json};
    use crate::error::Error;
    use crate::value::Value;

    type Rows = Result<Vec<Vec<Value>>, Error>;

    #[derive(Default)]
    pub(super) struct Answer {
        /// The last `rows` field (a JSON object keeps the last of a repeated key).
        pub(super) rows: Option<Rows>,
    }

    pub(super) struct Top(pub(super) Answer);

    impl<'de> Deserialize<'de> for Top {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_any(TopVisitor).map(Top)
        }
    }

    /// Anything else than an object has no `rows`.
    struct TopVisitor;

    impl<'de> Visitor<'de> for TopVisitor {
        type Value = Answer;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an answer")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Answer, A::Error> {
            let mut answer = Answer::default();
            while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
                if key == "rows" {
                    answer.rows = Some(map.next_value_seed(RowsSeed)?);
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            Ok(answer)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Answer, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(Answer::default())
        }
        fn visit_bool<E>(self, _: bool) -> Result<Answer, E> {
            Ok(Answer::default())
        }
        fn visit_i64<E>(self, _: i64) -> Result<Answer, E> {
            Ok(Answer::default())
        }
        fn visit_u64<E>(self, _: u64) -> Result<Answer, E> {
            Ok(Answer::default())
        }
        fn visit_f64<E>(self, _: f64) -> Result<Answer, E> {
            Ok(Answer::default())
        }
        fn visit_str<E>(self, _: &str) -> Result<Answer, E> {
            Ok(Answer::default())
        }
        fn visit_unit<E>(self) -> Result<Answer, E> {
            Ok(Answer::default())
        }
    }

    /// A value that is not an array: read through, `Err(bad(what))`.
    macro_rules! not_an_array {
        ($what:literal, $ty:ty) => {
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<$ty, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Err(bad($what)))
            }
            fn visit_bool<E>(self, _: bool) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
            fn visit_i64<E>(self, _: i64) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
            fn visit_u64<E>(self, _: u64) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
            fn visit_f64<E>(self, _: f64) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
            fn visit_str<E>(self, _: &str) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
            fn visit_unit<E>(self) -> Result<$ty, E> {
                Ok(Err(bad($what)))
            }
        };
    }

    struct RowsSeed;

    impl<'de> DeserializeSeed<'de> for RowsSeed {
        type Value = Rows;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Rows, D::Error> {
            d.deserialize_any(self)
        }
    }

    impl<'de> Visitor<'de> for RowsSeed {
        type Value = Rows;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("rows")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Rows, A::Error> {
            let mut rows = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            let mut failed = None;
            while let Some(row) = seq.next_element_seed(RowSeed { failed: failed.is_some() })? {
                match row {
                    Ok(row) if failed.is_none() => rows.push(row),
                    Ok(_) => {}
                    Err(e) => failed = failed.or(Some(e)),
                }
            }
            Ok(failed.map_or(Ok(rows), Err))
        }
        not_an_array!("rows", Rows);
    }

    /// One row; once a row before it failed, it is only read through.
    struct RowSeed {
        failed: bool,
    }

    impl<'de> DeserializeSeed<'de> for RowSeed {
        type Value = Result<Vec<Value>, Error>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_any(self)
        }
    }

    impl<'de> Visitor<'de> for RowSeed {
        type Value = Result<Vec<Value>, Error>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a row")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            if self.failed {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                return Ok(Ok(Vec::new()));
            }
            let mut row = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            let mut failed = None;
            while let Some(value) = seq.next_element_seed(ValueSeed)? {
                match value {
                    Ok(v) if failed.is_none() => row.push(v),
                    Ok(_) => {}
                    Err(e) => failed = failed.or(Some(e)),
                }
            }
            Ok(failed.map_or(Ok(row), Err))
        }
        not_an_array!("row", Result<Vec<Value>, Error>);
    }

    /// One value, as [`value_from_json`] reads it; objects (tagged values)
    /// and arrays are built as JSON and handed to it.
    struct ValueSeed;

    impl<'de> DeserializeSeed<'de> for ValueSeed {
        type Value = Result<Value, Error>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_any(self)
        }
    }

    impl<'de> Visitor<'de> for ValueSeed {
        type Value = Result<Value, Error>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a value")
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(Ok(Value::Null))
        }
        fn visit_bool<E>(self, b: bool) -> Result<Self::Value, E> {
            Ok(Ok(Value::Bool(b)))
        }
        fn visit_i64<E>(self, i: i64) -> Result<Self::Value, E> {
            Ok(Ok(Value::I64(i)))
        }
        fn visit_u64<E>(self, u: u64) -> Result<Self::Value, E> {
            // As `serde_json::Number::as_i64`, then `as_f64`.
            Ok(Ok(i64::try_from(u).map_or(Value::F64(u as f64), Value::I64)))
        }
        fn visit_f64<E>(self, f: f64) -> Result<Self::Value, E> {
            Ok(Ok(Value::F64(f)))
        }
        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E> {
            Ok(Ok(Value::Text(s.to_owned())))
        }
        fn visit_string<E>(self, s: String) -> Result<Self::Value, E> {
            Ok(Ok(Value::Text(s)))
        }
        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            let j = Json::deserialize(MapAccessDeserializer::new(map))?;
            Ok(value_from_json(&j))
        }
        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
            let j = Json::deserialize(SeqAccessDeserializer::new(seq))?;
            Ok(value_from_json(&j))
        }
    }
}

/// The answer to an `execute`.
pub fn exec_result_to_json(r: &ExecResult) -> Json {
    object([
        ("rowsAffected", r.rows_affected.into()),
        ("lastInsertId", r.last_insert_id.as_ref().map(value_to_json).into()),
    ])
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
    let column = |c: &ColumnDef| {
        object([
            ("name", c.name.into()),
            ("field", c.field.into()),
            ("kind", type_name(c.kind).into()),
            ("nullable", c.nullable.into()),
            ("primaryKey", c.primary_key.into()),
            ("autoIncrement", c.auto_increment.into()),
            ("unique", c.unique.into()),
            ("indexed", c.indexed.into()),
            ("fulltext", c.fulltext.into()),
            ("sqlType", c.sql_type.into()),
            ("default", c.default.into()),
            (
                "references",
                c.references
                    .map(|r| {
                        object([
                            ("table", r.table.into()),
                            ("column", r.column.into()),
                            ("onDelete", r.on_delete.into()),
                            ("onUpdate", r.on_update.into()),
                        ])
                    })
                    .into(),
            ),
        ])
    };
    let index = |ix: &IndexDef| {
        object([("name", ix.name.into()), ("columns", ix.columns.into()), ("unique", ix.unique.into())])
    };
    object([
        ("name", t.name.into()),
        ("pk", t.primary_key.into()),
        ("columns", t.columns.iter().map(column).collect::<Vec<_>>().into()),
        ("indexes", t.indexes.iter().map(index).collect::<Vec<_>>().into()),
    ])
}

/// How a statement names a schema when it is written as JSON: the schema
/// whole ([`schema_to_json`]), or its [reference](schema_ref) once the server
/// has it.
pub type SchemaEncoder<'a> = &'a dyn Fn(&'static TableSchema) -> Json;

/// What a server answers (in an `unsupported` error) to a schema reference it
/// does not know: the client sends the statement again with its schemas whole.
pub const UNKNOWN_SCHEMA: &str = "unknown schema reference";

/// `true` for the answer to a schema reference the server does not know.
pub fn is_unknown_schema(e: &Error) -> bool {
    matches!(e, Error::Unsupported(m) if m.starts_with(UNKNOWN_SCHEMA))
}

/// `true` for what a server from before references answers to one: it took
/// the reference for a description without a name, and ran nothing. Only a
/// server set back to such a version says this to a client that was sending
/// references: the client goes back to whole schemas.
pub fn is_reference_unread(e: &Error) -> bool {
    matches!(e, Error::Conversion(m) if m.contains("schema `name`"))
}

/// A short reference to a schema, the same for the client that describes it
/// and the server that read its description: the table's name and a
/// fingerprint of everything [`schema_to_json`] says about it. A client sends
/// a schema whole once, then this in its place (see [`query_to_json_with`]):
/// a statement no longer carries the description of its tables each time.
///
/// The fingerprint is half a SHA-256: a reference names one schema, and
/// nobody can write another schema that answers to it.
pub fn schema_ref(t: &TableSchema) -> String {
    // Every field, each one with its length or of a fixed size: two schemas
    // that differ never give the same bytes.
    struct Fields(Vec<u8>);
    impl Fields {
        fn number(&mut self, n: usize) {
            self.0.extend_from_slice(&(n as u64).to_le_bytes());
        }
        fn text(&mut self, s: &str) {
            self.number(s.len());
            self.0.extend_from_slice(s.as_bytes());
        }
        fn opt(&mut self, s: Option<&str>) {
            match s {
                Some(s) => {
                    self.0.push(1);
                    self.text(s);
                }
                None => self.0.push(0),
            }
        }
    }
    let mut f = Fields(Vec::with_capacity(64 + 96 * t.columns.len()));
    f.text(t.name);
    f.number(t.primary_key);
    f.number(t.columns.len());
    for c in t.columns {
        f.text(c.name);
        f.text(c.field);
        f.text(type_name(c.kind));
        let flags = [c.nullable, c.primary_key, c.auto_increment, c.unique, c.indexed, c.fulltext];
        f.0.extend_from_slice(&flags.map(u8::from));
        f.opt(c.sql_type);
        f.opt(c.default);
        match &c.references {
            Some(r) => {
                f.0.push(1);
                f.text(r.table);
                f.text(r.column);
                f.opt(r.on_delete);
                f.opt(r.on_update);
            }
            None => f.0.push(0),
        }
    }
    f.number(t.indexes.len());
    for ix in t.indexes {
        f.text(ix.name);
        f.0.push(u8::from(ix.unique));
        f.number(ix.columns.len());
        for c in ix.columns {
            f.number(*c);
        }
    }
    let digest = super::sha256::sha256(&f.0);
    let mut reference = String::with_capacity(t.name.len() + 33);
    reference.push_str(t.name);
    reference.push('#');
    reference.push_str(&hex(&digest[..16]));
    reference
}

/// Schemas and names received from clients, made `'static` once and shared
/// by every later statement. Memoire's statements point to `'static`
/// schemas: a server builds each distinct schema once and keeps it. The
/// number of distinct schemas and names is capped, so a client cannot grow
/// the server's memory without bound.
pub struct SchemaCache {
    schemas: Mutex<HashMap<String, &'static TableSchema>>,
    /// The schemas kept, by their [reference](schema_ref). `None`: two
    /// different schemas were described under this reference, and it is
    /// answered as unknown from then on (their clients send them whole).
    refs: Mutex<HashMap<String, Option<&'static TableSchema>>>,
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
        SchemaCache {
            schemas: Mutex::new(HashMap::new()),
            refs: Mutex::new(HashMap::new()),
            names: Mutex::new(HashSet::new()),
            max_schemas,
        }
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

    /// The `'static` schema described by `j`: its description, or the
    /// [reference](schema_ref) of one described before (an error that
    /// [`is_unknown_schema`] recognizes when it was not).
    pub fn schema(&self, j: &Json) -> Result<&'static TableSchema> {
        if let Some(reference) = j.as_str() {
            return self
                .refs
                .lock()
                .map_err(|_| bad("poisoned cache"))?
                .get(reference)
                .copied()
                .flatten()
                .ok_or_else(|| Error::Unsupported(format!("{UNKNOWN_SCHEMA} `{reference}`: send the schema")));
        }
        // The same text as `j.to_string()`, written without going through `fmt`.
        let key = serde_json::to_string(j).map_err(bad)?;
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
        drop(schemas);
        let mut refs = self.refs.lock().map_err(|_| bad("poisoned cache"))?;
        let held = refs.entry(schema_ref(schema)).or_insert(Some(schema));
        // The same schema written another way keeps its reference. Another
        // schema under it (which a reference should never allow) and the
        // reference names none: a statement never runs on a schema its client
        // did not describe.
        if held.is_some_and(|h| !std::ptr::eq(h, schema) && schema_to_json(h) != schema_to_json(schema)) {
            *held = None;
        }
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

fn expr_to_json(e: &Expr, enc: SchemaEncoder<'_>) -> Json {
    match e {
        Expr::Column(c) => object([(
            "col",
            Json::Array(vec![c.table.into(), c.name.into(), c.index.into(), type_name(c.kind).into()]),
        )]),
        Expr::Value(v) => object([("val", value_to_json(v))]),
        Expr::Binary { op, left, right } => object([(
            "bin",
            Json::Array(vec![
                BIN_OPS.iter().find(|(o, _)| o == op).map_or("=", |(_, s)| s).into(),
                expr_to_json(left, enc),
                expr_to_json(right, enc),
            ]),
        )]),
        Expr::Not(inner) => object([("not", expr_to_json(inner, enc))]),
        Expr::IsNull { expr, negated } => object([("isNull", expr_to_json(expr, enc)), ("negated", (*negated).into())]),
        Expr::InList { expr, list, negated } => object([
            ("in", expr_to_json(expr, enc)),
            ("list", list.iter().map(value_to_json).collect::<Vec<_>>().into()),
            ("negated", (*negated).into()),
        ]),
        Expr::Between { expr, low, high, negated } => object([
            ("between", expr_to_json(expr, enc)),
            ("low", expr_to_json(low, enc)),
            ("high", expr_to_json(high, enc)),
            ("negated", (*negated).into()),
        ]),
        Expr::Like { expr, pattern, case_insensitive, negated, escaped } => object([
            ("like", expr_to_json(expr, enc)),
            ("pattern", expr_to_json(pattern, enc)),
            ("ci", (*case_insensitive).into()),
            ("negated", (*negated).into()),
            ("escaped", (*escaped).into()),
        ]),
        Expr::Match(m) => object([
            ("match", expr_to_json(&m.expr, enc)),
            ("words", m.words.as_slice().into()),
            ("query", value_to_json(&m.query)),
            ("patterns", m.patterns.iter().map(value_to_json).collect::<Vec<_>>().into()),
        ]),
        Expr::Aggregate { func, arg, distinct } => object([
            ("agg", AGGS.iter().find(|(f, _)| f == func).map_or("count", |(_, s)| s).into()),
            ("arg", arg.as_deref().map(|a| expr_to_json(a, enc)).into()),
            ("distinct", (*distinct).into()),
        ]),
        Expr::InSubquery { expr, query, negated } => object([
            ("inQuery", expr_to_json(expr, enc)),
            ("query", select_to_json(query, enc)),
            ("negated", (*negated).into()),
        ]),
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

fn order_to_json(o: &Order, enc: SchemaEncoder<'_>) -> Json {
    object([
        ("expr", expr_to_json(&o.expr, enc)),
        ("desc", o.desc.into()),
        ("nulls", o.nulls.map(|n| if n == Nulls::First { "first" } else { "last" }).into()),
    ])
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

fn select_to_json(s: &SelectAst, enc: SchemaEncoder<'_>) -> Json {
    let expr = |e: &Expr| expr_to_json(e, enc);
    let join = |j: &Join| {
        object([
            ("kind", (if j.kind == JoinKind::Inner { "inner" } else { "left" }).into()),
            ("table", enc(j.table)),
            ("on", expr(&j.on)),
        ])
    };
    object([
        ("from", enc(s.from)),
        (
            "projection",
            match &s.projection {
                Projection::Tables(t) => object([("tables", t.iter().map(|t| t.name).collect::<Vec<_>>().into())]),
                Projection::Exprs(e) => object([("exprs", e.iter().map(expr).collect::<Vec<_>>().into())]),
            },
        ),
        ("joins", s.joins.iter().map(join).collect::<Vec<_>>().into()),
        ("filter", s.filter.as_ref().map(expr).into()),
        ("groupBy", s.group_by.iter().map(expr).collect::<Vec<_>>().into()),
        ("having", s.having.as_ref().map(expr).into()),
        ("order", s.order.iter().map(|o| order_to_json(o, enc)).collect::<Vec<_>>().into()),
        ("limit", s.limit.into()),
        ("offset", s.offset.into()),
        ("distinct", s.distinct.into()),
    ])
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
    query_to_json_with(q, &|t| schema_to_json(t))
}

/// [`query_to_json`] with the schemas written by `enc`: whole, or as their
/// [reference](schema_ref) for a server that already has them.
pub fn query_to_json_with(q: &Query<'_>, enc: SchemaEncoder<'_>) -> Result<Json> {
    let expr = |e: &Expr| expr_to_json(e, enc);
    Ok(match *q {
        Query::Select(s) => object([("select", select_to_json(s, enc))]),
        Query::Insert(i) => object([
            ("insert", enc(i.table)),
            ("rows", i.rows.iter().map(|r| values_json(r)).collect::<Vec<_>>().into()),
        ]),
        Query::Update(u) => object([
            ("update", enc(u.table)),
            ("sets", u.sets.iter().map(|(c, e)| Json::Array(vec![(*c).into(), expr(e)])).collect::<Vec<_>>().into()),
            ("filter", u.filter.as_ref().map(expr).into()),
        ]),
        Query::Delete(d) => object([("delete", enc(d.table)), ("filter", d.filter.as_ref().map(expr).into())]),
        Query::FindByKey { table, key } => object([("findByKey", enc(table)), ("key", value_to_json(key))]),
        Query::InsertRow { table, values } => object([("insertRow", enc(table)), ("values", values_json(values))]),
        Query::UpsertRow { table, values } => object([("upsertRow", enc(table)), ("values", values_json(values))]),
        Query::UpdateRow { table, values } => object([("updateRow", enc(table)), ("values", values_json(values))]),
        Query::DeleteByKey { table, key } => object([("deleteByKey", enc(table)), ("key", value_to_json(key))]),
        Query::Raw { sql, params } => object([("raw", sql.into()), ("params", values_json(params))]),
        Query::Script { sql } => object([("script", sql.into())]),
        Query::CreateTable { table, if_not_exists } => {
            object([("createTable", enc(table)), ("ifNotExists", if_not_exists.into())])
        }
        Query::DropTable { name, if_exists } => object([("dropTable", name.into()), ("ifExists", if_exists.into())]),
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
    use serde_json::Value as Json;

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

    /// A table with everything a schema can say: defaults, a foreign key, SQL types.
    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "wire_posts", index(author, title), unique(slug, author))]
    struct P {
        #[memoire(primary_key, auto_increment)]
        id: i64,
        #[memoire(references = "wire_users(id)", on_delete = "cascade", index)]
        author: i64,
        #[memoire(unique)]
        slug: String,
        #[memoire(default = "'untitled'")]
        title: String,
        body: Option<String>,
    }

    #[test]
    fn schemas_by_reference() {
        let cache = SchemaCache::default();
        // The reference is the same for the client's schema and the one the server built from its description.
        for schema in [U::SCHEMA, P::SCHEMA] {
            let reference = schema_ref(schema);
            assert!(reference.starts_with(schema.name), "{reference}");
            // Not known before it was described.
            let unknown = cache.schema(&json!(reference)).unwrap_err();
            assert!(is_unknown_schema(&unknown), "{unknown}");
            let built = cache.schema(&schema_to_json(schema)).unwrap();
            assert_eq!(schema_ref(built), reference);
            assert!(std::ptr::eq(cache.schema(&json!(reference)).unwrap(), built));
        }
        assert_ne!(schema_ref(U::SCHEMA), schema_ref(P::SCHEMA));

        // A statement whose schemas go by reference reads back as the one that describes them.
        let select = P::query().filter(P::AUTHOR.eq(3) & P::TITLE.like("a%")).order_by(P::ID.desc()).limit(2);
        let ast = select.ast().clone();
        let whole = query_to_json(&Query::Select(&ast)).unwrap();
        let short = query_to_json_with(&Query::Select(&ast), &|t| json!(schema_ref(t))).unwrap();
        assert!(short.to_string().len() * 3 < whole.to_string().len(), "{short}");
        let (OwnedQuery::Select(a), OwnedQuery::Select(b)) =
            (query_from_json(&whole, &cache).unwrap(), query_from_json(&short, &cache).unwrap())
        else {
            panic!()
        };
        assert!(std::ptr::eq(a.from, b.from));
        assert_eq!((&a.filter, &a.order, a.limit), (&b.filter, &b.order, b.limit));
        let values = vec![Value::I64(1), Value::I64(2), Value::Text("s".into()), Value::Text("t".into()), Value::Null];
        let short =
            query_to_json_with(&Query::InsertRow { table: P::SCHEMA, values: &values }, &|t| json!(schema_ref(t)));
        let OwnedQuery::InsertRow(t, v) = query_from_json(&short.unwrap(), &cache).unwrap() else { panic!() };
        assert_eq!((t.name, v), ("wire_posts", values));
        // 128 bits of SHA-256 after the name: nobody writes another schema with the same reference.
        let reference = schema_ref(P::SCHEMA);
        let (name, digest) = reference.split_once('#').unwrap();
        assert_eq!((name, digest.len()), ("wire_posts", 32));
        assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
        // The same schema described in other words (a field the server does not read) keeps its reference.
        let mut reworded = schema_to_json(P::SCHEMA);
        reworded["note"] = json!("from another client");
        let again = cache.schema(&reworded).unwrap();
        assert_eq!(schema_ref(again), reference);
        assert!(cache.schema(&json!(reference)).is_ok());
        // Were two different schemas ever described under one reference, it would name
        // neither: each client is told to send its own, whole.
        let other = SchemaCache::default();
        let users = other.schema(&schema_to_json(U::SCHEMA)).unwrap();
        other.refs.lock().unwrap().insert(reference.clone(), Some(users));
        other.schema(&schema_to_json(P::SCHEMA)).unwrap();
        assert!(is_unknown_schema(&other.schema(&json!(reference)).unwrap_err()));
        other.schema(&reworded).unwrap();
        assert!(is_unknown_schema(&other.schema(&json!(reference)).unwrap_err()), "for good");
        assert!(is_reference_unread(&Error::Conversion("invalid Nuage DB message: schema `name`".into())));
        assert!(!is_reference_unread(&Error::Conversion("invalid Nuage DB message: schema columns".into())));
        // A server that never saw the schema says so, in a way the client recognizes.
        let fresh = SchemaCache::default();
        let err =
            query_from_json(&query_to_json_with(&Query::Select(&ast), &|t| json!(schema_ref(t))).unwrap(), &fresh);
        assert!(is_unknown_schema(&err.unwrap_err()));
        // The error survives the trip to the client.
        let (status, body) = error_to_json(&Error::Unsupported(format!("{UNKNOWN_SCHEMA} `x#1`: send the schema")));
        assert!(is_unknown_schema(&error_from_json(status, body.to_string().as_bytes())));
        assert!(!is_unknown_schema(&Error::Unsupported("raw SQL".into())));
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

    /// The encoding written before statements were built without `json!`
    /// (which copied every value that was already JSON): the same text, field
    /// order included.
    #[test]
    fn encoding_is_unchanged() {
        let old_schema = |t: &TableSchema| {
            json!({
                "name": t.name,
                "pk": t.primary_key,
                "columns": t.columns.iter().map(|c| json!({
                    "name": c.name, "field": c.field, "kind": type_name(c.kind), "nullable": c.nullable,
                    "primaryKey": c.primary_key, "autoIncrement": c.auto_increment, "unique": c.unique,
                    "indexed": c.indexed, "fulltext": c.fulltext, "sqlType": c.sql_type, "default": c.default,
                    "references": c.references.map(|r| json!({
                        "table": r.table, "column": r.column, "onDelete": r.on_delete, "onUpdate": r.on_update,
                    })),
                })).collect::<Vec<_>>(),
                "indexes": t.indexes.iter().map(|ix| json!({
                    "name": ix.name, "columns": ix.columns, "unique": ix.unique,
                })).collect::<Vec<_>>(),
            })
        };
        for t in [U::SCHEMA, P::SCHEMA] {
            assert_eq!(schema_to_json(t).to_string(), old_schema(t).to_string());
        }
        let values = [
            (Value::I32(-3), json!({ "i32": -3 })),
            (Value::I64(7), json!(7)),
            (Value::F64(0.5), json!({ "f64": 0.5 })),
            (Value::F64(f64::NAN), json!({ "f64": "NaN" })),
            (Value::F64(f64::NEG_INFINITY), json!({ "f64": "-Infinity" })),
            (Value::Bytes(vec![0, 15, 16, 255]), json!({ "bytes": "000f10ff" })),
        ];
        for (v, j) in values {
            assert_eq!(value_to_json(&v).to_string(), j.to_string());
        }
        let col = |c: &str, i: usize, k: &str| json!({ "col": ["wire_users", c, i, k] });
        let select = U::query()
            .filter((U::AGE.between(1, 9) | U::NAME.like("A%")) & !U::EMAIL.is_in(["a"]) & U::AGE.is_null())
            .order_by(U::NAME.desc())
            .limit(5);
        let filter = select.ast().filter.clone().unwrap();
        let old = json!({ "select": {
            "from": "ref",
            "projection": { "tables": ["wire_users"] },
            "joins": [],
            "filter": expr_to_json(&filter, &|_| json!("ref")),
            "groupBy": [],
            "having": null,
            "order": [{ "expr": col("name", 2, "text"), "desc": true, "nulls": null }],
            "limit": 5,
            "offset": null,
            "distinct": false,
        } });
        let new = query_to_json_with(&Query::Select(select.ast()), &|_| json!("ref")).unwrap();
        assert_eq!(new.to_string(), old.to_string());
        let r = ExecResult { rows_affected: 2, last_insert_id: Some(Value::I32(4)) };
        assert_eq!(
            exec_result_to_json(&r).to_string(),
            json!({ "rowsAffected": 2, "lastInsertId": { "i32": 4 } }).to_string()
        );
        // Expressions, one of each kind, as `json!` wrote them.
        let between = json!({ "between": col("age", 3, "int"), "low": { "val": { "i32": 1 } },
            "high": { "val": { "i32": 9 } }, "negated": false });
        assert_eq!(expr_to_json(&U::AGE.between(1, 9), &|_| json!("ref")).to_string(), between.to_string());
        let like = json!({ "like": col("name", 2, "text"), "pattern": { "val": "A%" }, "ci": false,
            "negated": false, "escaped": true });
        assert_eq!(expr_to_json(&U::NAME.like("A%"), &|_| json!("ref")).to_string(), like.to_string());
        let not_in = json!({ "not": { "in": col("email", 1, "text"), "list": ["a"], "negated": false } });
        assert_eq!(expr_to_json(&!U::EMAIL.is_in(["a"]), &|_| json!("ref")).to_string(), not_in.to_string());
        let is_null = json!({ "isNull": col("age", 3, "int"), "negated": false });
        assert_eq!(expr_to_json(&U::AGE.is_null(), &|_| json!("ref")).to_string(), is_null.to_string());
        let gt = json!({ "bin": [">", col("age", 3, "int"), { "val": { "i32": 18 } }] });
        assert_eq!(expr_to_json(&U::AGE.gt(18), &|_| json!("ref")).to_string(), gt.to_string());
    }

    /// `rows_from_slice` answers what parsing the answer, then
    /// `rows_from_json` on its `rows`, answers: rows or error, message included.
    #[test]
    fn rows_read_straight_from_the_text() {
        let old = |body: &[u8]| -> Result<Vec<Vec<Value>>> {
            let j: Json = if body.is_empty() {
                Json::Null
            } else {
                serde_json::from_slice(body)
                    .map_err(|e| Error::Conversion(format!("invalid answer from Nuage DB: {e}")))?
            };
            rows_from_json(j.get("rows").unwrap_or(&Json::Null))
        };
        #[cfg_attr(not(feature = "json"), allow(unused_mut))]
        let mut row = vec![
            Value::Null,
            Value::Bool(false),
            Value::I32(i32::MIN),
            Value::I64(i64::MIN),
            Value::F64(-0.0),
            Value::F64(f64::INFINITY),
            Value::Text("é \" \\n \u{1F600}".into()),
            Value::Bytes(vec![1, 2, 254]),
        ];
        #[cfg(feature = "json")]
        row.push(Value::Json(json!({"a": [1, {"b": null}]})));
        let good = json!({ "rows": rows_to_json(&[row.clone(), row]) }).to_string();
        let bodies: Vec<&[u8]> = vec![
            good.as_bytes(),
            b"",
            b" ",
            b"null",
            b"[]",
            b"[[1]]",
            b"7",
            br#""rows""#,
            b"{}",
            br#"{"rows": []}"#,
            br#"{"other": [1, {"x": 2}], "rows": [[1, "a", 1.5, 18446744073709551615, -1, 1e300]]}"#,
            br#"{"rows": [[1e400]]}"#,
            br#"{"rows": [[1]], "rows": [[2], [3]]}"#,
            br#"{"rows": [[{"bad": 1}]], "rows": [[4]]}"#,
            br#"{"rows": [[4]], "rows": 5}"#,
            br#"{"rows": {"a": 1}}"#,
            br#"{"rows": [1, [2]]}"#,
            br#"{"rows": [[1], 2]}"#,
            br#"{"rows": [[[1]]]}"#,
            br#"{"rows": [[{"i32": 1, "f64": 2}]]}"#,
            br#"{"rows": [[{"i32": 99999999999}]]}"#,
            br#"{"rows": [[{"bytes": "abc"}], [{"bytes": "zz"}]]}"#,
            br#"{"rows": [[{"nope": 1}]]}"#,
            br#"{"rows": [[{"f64": "NaN"}, {"f64": "Infinity"}, {"f64": 2}]]}"#,
            br#"{"rows": [[{"uuid": "00000000-0000-0000-0000-00000000002a"}, {"date": "2026-01-02"}]]}"#,
            br#"{"rows": [[{"json": [1, 2]}], [{"bad": 1}]], "x": }"#,
            br#"{"rows": [[1]]} trailing"#,
            br#"{"rows": [[1]"#,
            br#"{"rows": [["\ud800"]]}"#,
            "{\"rows\": [[\"a\\u00e9\\n\", \"é\"]]}".as_bytes(),
        ];
        for body in bodies {
            let (a, b) = (rows_from_slice(body), old(body));
            let text = String::from_utf8_lossy(body);
            match (a, b) {
                // `NaN` is not equal to itself: compare what the values say.
                (Ok(a), Ok(b)) => assert_eq!(format!("{a:?}"), format!("{b:?}"), "{text}"),
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{text}"),
                (a, b) => panic!("{text}: {a:?} / {b:?}"),
            }
        }
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
