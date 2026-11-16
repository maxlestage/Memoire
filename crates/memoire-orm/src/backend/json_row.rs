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

use crate::backend::hex::encode as hex;

fn unhex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(Error::Conversion("invalid hex string".into()));
    }
    // A pair that is not hexadecimal is what `u8::from_str_radix` called an invalid digit.
    crate::backend::hex::decode(s).ok_or_else(|| Error::Conversion("invalid hex: invalid digit found in string".into()))
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

/// The JSON text of `j`, the same as `j.to_string()` writes, two to three
/// times faster (`Display` goes through `fmt` for every piece).
fn text(j: &serde_json::Value) -> String {
    serde_json::to_string(j).unwrap_or_else(|_| j.to_string())
}

#[cfg_attr(not(feature = "redis"), allow(dead_code))]
pub(crate) fn encode_row(schema: &TableSchema, row: &[Value]) -> String {
    let mut map = serde_json::Map::with_capacity(row.len());
    for (c, v) in schema.columns.iter().zip(row) {
        map.insert(c.name.to_owned(), encode_value(v));
    }
    text(&serde_json::Value::Object(map))
}

/// A row read from its JSON object: each column from the field of its name
/// (the last one, when a name is repeated), `NULL` when it has none.
///
/// The fields are read straight into their columns, without building the
/// object first (a map, with a copy of every name).
pub(crate) fn decode_row(schema: &TableSchema, json: &str) -> Result<Vec<Value>> {
    use serde::de::DeserializeSeed as _;
    let mut de = serde_json::Deserializer::from_str(json);
    let fields = row::Fields(schema).deserialize(&mut de).and_then(|f| de.end().map(|()| f));
    let Some(fields) = fields.map_err(Error::conversion)? else {
        return Err(Error::Conversion(format!("row of `{}` is not a JSON object", schema.name)));
    };
    schema.columns.iter().zip(fields).map(|(c, j)| decode_value(c.kind, j.unwrap_or(serde_json::Value::Null))).collect()
}

/// [`decode_row`]'s reader: the fields of a row's object, by column. Every
/// value is read as JSON, those of fields that are no column too, so that the
/// same texts are accepted and refused as when the object was built whole.
mod row {
    use std::borrow::Cow;
    use std::fmt;

    use serde::Deserialize;
    use serde::de::value::SeqAccessDeserializer;
    use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
    use serde_json::Value as Json;

    use crate::schema::TableSchema;

    /// The value of each column, `None` for anything else than an object.
    pub(super) struct Fields<'s>(pub(super) &'s TableSchema);

    type Found = Option<Vec<Option<Json>>>;

    impl<'de> DeserializeSeed<'de> for Fields<'_> {
        type Value = Found;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Found, D::Error> {
            d.deserialize_any(self)
        }
    }

    /// A field name, borrowed from the text when it holds no escape.
    struct Name<'de>(Cow<'de, str>);

    impl<'de> Deserialize<'de> for Name<'de> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(NameVisitor)
        }
    }

    struct NameVisitor;

    impl<'de> Visitor<'de> for NameVisitor {
        type Value = Name<'de>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a field name")
        }
        fn visit_borrowed_str<E>(self, s: &'de str) -> Result<Name<'de>, E> {
            Ok(Name(Cow::Borrowed(s)))
        }
        fn visit_str<E>(self, s: &str) -> Result<Name<'de>, E> {
            Ok(Name(Cow::Owned(s.to_owned())))
        }
        fn visit_string<E>(self, s: String) -> Result<Name<'de>, E> {
            Ok(Name(Cow::Owned(s)))
        }
    }

    impl<'de> Visitor<'de> for Fields<'_> {
        type Value = Found;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a row")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Found, A::Error> {
            let columns = self.0.columns;
            let mut values: Vec<Option<Json>> = vec![None; columns.len()];
            while let Some(Name(name)) = map.next_key::<Name<'de>>()? {
                let value: Json = map.next_value()?;
                // The first column of that name, as `Map::remove` found it.
                if let Some(i) = columns.iter().position(|c| c.name == name) {
                    values[i] = Some(value);
                }
            }
            Ok(Some(values))
        }
        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Found, A::Error> {
            Json::deserialize(SeqAccessDeserializer::new(seq))?;
            Ok(None)
        }
        fn visit_bool<E>(self, _: bool) -> Result<Found, E> {
            Ok(None)
        }
        fn visit_i64<E>(self, _: i64) -> Result<Found, E> {
            Ok(None)
        }
        fn visit_u64<E>(self, _: u64) -> Result<Found, E> {
            Ok(None)
        }
        fn visit_f64<E>(self, _: f64) -> Result<Found, E> {
            Ok(None)
        }
        fn visit_str<E>(self, _: &str) -> Result<Found, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Found, E> {
            Ok(None)
        }
    }
}

/// [`encode_row`] with the column names given (rows read back from storage
/// before their model is known).
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub(crate) fn encode_row_named(columns: &[String], row: &[Value]) -> String {
    let mut map = serde_json::Map::with_capacity(row.len());
    for (c, v) in columns.iter().zip(row) {
        map.insert(c.clone(), encode_value(v));
    }
    text(&serde_json::Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `decode_row` answers what parsing the object whole, then taking each
    /// column's field out of it, answered: row or error, message included.
    #[test]
    fn rows_read_straight_into_their_columns() {
        use crate::model::Model;
        fn old(schema: &TableSchema, json: &str) -> Result<Vec<Value>> {
            let mut obj = match serde_json::from_str(json).map_err(Error::conversion)? {
                serde_json::Value::Object(o) => o,
                _ => return Err(Error::Conversion(format!("row of `{}` is not a JSON object", schema.name))),
            };
            schema
                .columns
                .iter()
                .map(|c| decode_value(c.kind, obj.remove(c.name).unwrap_or(serde_json::Value::Null)))
                .collect()
        }
        #[derive(memoire_orm_derive::Model)]
        #[memoire(table = "t")]
        #[allow(dead_code)]
        struct Row {
            id: i64,
            name: Option<String>,
            age: Option<i32>,
            score: f64,
            data: Vec<u8>,
            on: bool,
        }
        let rows = [
            r#"{"id": 1, "name": "Ada", "age": 36, "score": 1.5, "data": "00ff", "on": true}"#,
            r#"{"data": "0a", "score": "2.5", "age": 99999999999, "name": null, "id": -1, "on": 0}"#,
            r#"{}"#,
            r#" {"id": 1} "#,
            r#"{"id": 1, "id": 2, "name": "a", "name": 3}"#,
            r#"{"other": [1, {"x": "\ud800"}], "id": 7}"#,
            r#"{"other": "\ud800", "id": 7}"#,
            r#"{"name": "\ud800"}"#,
            r#"{"id": 5, "na\"me": "x", "name": "aé\n"}"#,
            r#"{"id": 1.0, "age": true, "score": "x"}"#,
            r#"{"id": [1], "name": {"a": 1}}"#,
            r#"{"data": "abc"}"#,
            r#"{"data": "zz"}"#,
            r#"{"data": 1}"#,
            r#"{"id": 1e400}"#,
            r#"{"id": 18446744073709551615}"#,
            "[1, 2]",
            "[1, }",
            "7",
            "\"row\"",
            "null",
            "",
            "{",
            r#"{"id": 1} trailing"#,
            r#"{"id": 1, "x": }"#,
            r#"{"id": 1 "x": 2}"#,
            r#"{1: 1}"#,
        ];
        for json in rows {
            match (decode_row(Row::SCHEMA, json), old(Row::SCHEMA, json)) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{json}"),
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{json}"),
                (a, b) => panic!("{json}: {a:?} / {b:?}"),
            }
        }
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 1, 254, 255])).unwrap(), vec![0, 1, 254, 255]);
        assert!(unhex("abc").is_err());
    }
}
