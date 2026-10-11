//! JSON form of rows, shared by the Redis backend and browser storage: one JSON
//! object per row (`{"id": 1, "name": "Ada"}`), readable by other programs and
//! in the browser's developer tools. Values are decoded with the column types
//! of the model, so dates, UUIDs and bytes come back typed.

use crate::error::{Error, Result};
use crate::schema::TableSchema;
use crate::value::{ColumnType, Value};

/// Canonical string of a key value (integers are normalised).
pub(crate) fn field(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::I32(i) => i.to_string(),
        Value::I64(i) => i.to_string(),
        Value::F64(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => (*f as i64).to_string(),
        Value::F64(f) => f.to_string(),
        Value::Text(s) => s.clone(),
        Value::Bytes(b) => hex(b),
        #[allow(unreachable_patterns)]
        other => match encode_value(other) {
            serde_json::Value::String(s) => s,
            j => j.to_string(),
        },
    }
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::Conversion("invalid hex string".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| Error::Conversion(format!("invalid hex: {e}"))))
        .collect()
}

pub(crate) fn encode_value(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::I32(i) => J::from(*i),
        Value::I64(i) => J::from(*i),
        Value::F64(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or_else(|| J::String(f.to_string())),
        Value::Text(s) => J::String(s.clone()),
        Value::Bytes(b) => J::String(hex(b)),
        #[cfg(feature = "uuid")]
        Value::Uuid(u) => J::String(u.hyphenated().to_string()),
        #[cfg(feature = "chrono")]
        Value::Timestamp(t) => J::String(t.format("%Y-%m-%d %H:%M:%S%.9f").to_string()),
        #[cfg(feature = "chrono")]
        Value::TimestampTz(t) => J::String(t.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)),
        #[cfg(feature = "chrono")]
        Value::Date(d) => J::String(d.format("%Y-%m-%d").to_string()),
        #[cfg(feature = "chrono")]
        Value::Time(t) => J::String(t.format("%H:%M:%S%.9f").to_string()),
        #[cfg(feature = "json")]
        Value::Json(j) => j.clone(),
    }
}

pub(crate) fn decode_value(kind: ColumnType, j: serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    let bad = |j: &J| Error::Conversion(format!("unexpected JSON value {j} for a {kind:?} column"));
    Ok(match (kind, j) {
        (_, J::Null) => Value::Null,
        (ColumnType::Bool, J::Bool(b)) => Value::Bool(b),
        (ColumnType::Bool, J::Number(n)) => Value::Bool(n.as_i64().unwrap_or(0) != 0),
        (ColumnType::SmallInt | ColumnType::Int | ColumnType::BigInt, J::Number(n)) => match n.as_i64() {
            Some(i) if kind != ColumnType::BigInt => i32::try_from(i).map(Value::I32).unwrap_or(Value::I64(i)),
            Some(i) => Value::I64(i),
            None => Value::F64(n.as_f64().unwrap_or(0.0)),
        },
        (ColumnType::Real | ColumnType::Double, J::Number(n)) => Value::F64(n.as_f64().unwrap_or(0.0)),
        (ColumnType::Real | ColumnType::Double, J::String(s)) => {
            Value::F64(s.parse().map_err(|_| Error::Conversion(format!("invalid float `{s}`")))?)
        }
        (ColumnType::Bytes, J::String(s)) => Value::Bytes(unhex(&s)?),
        #[cfg(feature = "uuid")]
        (ColumnType::Uuid, J::String(s)) => Value::Uuid(uuid::Uuid::parse_str(&s).map_err(Error::conversion)?),
        #[cfg(feature = "chrono")]
        (ColumnType::Timestamp, J::String(s)) => Value::Timestamp(crate::value::FromValue::from_value(Value::Text(s))?),
        #[cfg(feature = "chrono")]
        (ColumnType::TimestampTz, J::String(s)) => {
            Value::TimestampTz(crate::value::FromValue::from_value(Value::Text(s))?)
        }
        #[cfg(feature = "chrono")]
        (ColumnType::Date, J::String(s)) => Value::Date(crate::value::FromValue::from_value(Value::Text(s))?),
        #[cfg(feature = "chrono")]
        (ColumnType::Time, J::String(s)) => Value::Time(crate::value::FromValue::from_value(Value::Text(s))?),
        #[cfg(feature = "json")]
        (ColumnType::Json, j) => Value::Json(j),
        #[cfg(not(feature = "json"))]
        (ColumnType::Json, j) => Value::Text(j.to_string()),
        (_, J::String(s)) => Value::Text(s),
        (_, J::Bool(b)) => Value::Bool(b),
        (_, J::Number(n)) => n.as_i64().map(Value::I64).unwrap_or_else(|| Value::F64(n.as_f64().unwrap_or(0.0))),
        (_, other) => return Err(bad(&other)),
    })
}

#[cfg_attr(not(feature = "redis"), allow(dead_code))]
pub(crate) fn encode_row(schema: &TableSchema, row: &[Value]) -> String {
    let mut map = serde_json::Map::with_capacity(row.len());
    for (c, v) in schema.columns.iter().zip(row) {
        map.insert(c.name.to_owned(), encode_value(v));
    }
    serde_json::Value::Object(map).to_string()
}

pub(crate) fn decode_row(schema: &TableSchema, json: &str) -> Result<Vec<Value>> {
    let mut obj = match serde_json::from_str(json).map_err(Error::conversion)? {
        serde_json::Value::Object(o) => o,
        _ => return Err(Error::Conversion(format!("row of `{}` is not a JSON object", schema.name))),
    };
    schema.columns.iter().map(|c| decode_value(c.kind, obj.remove(c.name).unwrap_or(serde_json::Value::Null))).collect()
}

/// [`encode_row`] with the column names given (rows read back from storage
/// before their model is known).
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub(crate) fn encode_row_named(columns: &[String], row: &[Value]) -> String {
    let mut map = serde_json::Map::with_capacity(row.len());
    for (c, v) in columns.iter().zip(row) {
        map.insert(c.clone(), encode_value(v));
    }
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 1, 254, 255])).unwrap(), vec![0, 1, 254, 255]);
        assert!(unhex("abc").is_err());
    }
}
