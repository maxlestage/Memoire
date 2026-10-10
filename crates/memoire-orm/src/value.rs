//! Dynamically typed SQL values and the conversion traits between Rust types and
//! [`Value`].

use std::fmt;

use crate::error::{Error, Result};

/// A single SQL value.
///
/// Every backend reads rows as `Value`s and every Rust field is converted to and
/// from a `Value` through [`ToValue`], [`IntoValue`] and [`FromValue`]. Integer
/// and floating point widths are normalised (`I32`, `I64`, `F64`) and converted
/// to the exact database type when binding.
#[derive(Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub enum Value {
    /// SQL `NULL`.
    #[default]
    Null,
    /// Boolean.
    Bool(bool),
    /// 32-bit (or smaller) integer.
    I32(i32),
    /// 64-bit integer.
    I64(i64),
    /// Floating point number.
    F64(f64),
    /// UTF-8 text.
    Text(String),
    /// Binary data.
    Bytes(Vec<u8>),
    /// UUID.
    #[cfg(feature = "uuid")]
    Uuid(uuid::Uuid),
    /// Timestamp without time zone.
    #[cfg(feature = "chrono")]
    Timestamp(chrono::NaiveDateTime),
    /// Timestamp with time zone (normalised to UTC).
    #[cfg(feature = "chrono")]
    TimestampTz(chrono::DateTime<chrono::Utc>),
    /// Calendar date.
    #[cfg(feature = "chrono")]
    Date(chrono::NaiveDate),
    /// Time of day.
    #[cfg(feature = "chrono")]
    Time(chrono::NaiveTime),
    /// JSON document.
    #[cfg(feature = "json")]
    Json(serde_json::Value),
}

impl Value {
    /// `true` if the value is `NULL`.
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Short name of the variant, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Bool(_) => "bool",
            Value::I32(_) => "i32",
            Value::I64(_) => "i64",
            Value::F64(_) => "f64",
            Value::Text(_) => "text",
            Value::Bytes(_) => "bytes",
            #[cfg(feature = "uuid")]
            Value::Uuid(_) => "uuid",
            #[cfg(feature = "chrono")]
            Value::Timestamp(_) => "timestamp",
            #[cfg(feature = "chrono")]
            Value::TimestampTz(_) => "timestamptz",
            #[cfg(feature = "chrono")]
            Value::Date(_) => "date",
            #[cfg(feature = "chrono")]
            Value::Time(_) => "time",
            #[cfg(feature = "json")]
            Value::Json(_) => "json",
        }
    }

    /// The value as an `i64` if it is an integer.
    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::I32(v) => Some(v as i64),
            Value::I64(v) => Some(v),
            _ => None,
        }
    }

    /// The value as an `f64` if it is numeric.
    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::I32(v) => Some(v as f64),
            Value::I64(v) => Some(v as f64),
            Value::F64(v) => Some(v),
            _ => None,
        }
    }

    /// The value as a `&str` if it is text.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// The value as a `bool` if it is a boolean.
    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    /// Formats the value as a SQL literal (for logs and debugging only, never
    /// for building queries: Memoire always binds parameters).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("NULL"),
            Value::Bool(b) => f.write_str(if *b { "TRUE" } else { "FALSE" }),
            Value::I32(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::F64(v) => write!(f, "{v}"),
            Value::Text(s) => write!(f, "'{}'", s.replace('\'', "''")),
            Value::Bytes(b) => {
                f.write_str("X'")?;
                for byte in b {
                    write!(f, "{byte:02X}")?;
                }
                f.write_str("'")
            }
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => write!(f, "'{u}'"),
            #[cfg(feature = "chrono")]
            Value::Timestamp(t) => write!(f, "'{}'", t.format("%Y-%m-%d %H:%M:%S%.f")),
            #[cfg(feature = "chrono")]
            Value::TimestampTz(t) => write!(f, "'{}'", t.to_rfc3339()),
            #[cfg(feature = "chrono")]
            Value::Date(d) => write!(f, "'{d}'"),
            #[cfg(feature = "chrono")]
            Value::Time(t) => write!(f, "'{t}'"),
            #[cfg(feature = "json")]
            Value::Json(j) => write!(f, "'{}'", j.to_string().replace('\'', "''")),
        }
    }
}

/// Logical column type, used to generate DDL and to pick casts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColumnType {
    /// `BOOLEAN`
    Bool,
    /// `SMALLINT`
    SmallInt,
    /// `INTEGER`
    Int,
    /// `BIGINT`
    BigInt,
    /// `REAL` (32-bit float)
    Real,
    /// `DOUBLE PRECISION`
    Double,
    /// `TEXT`
    Text,
    /// `BYTEA` / `BLOB`
    Bytes,
    /// `UUID`
    Uuid,
    /// `TIMESTAMP`
    Timestamp,
    /// `TIMESTAMPTZ`
    TimestampTz,
    /// `DATE`
    Date,
    /// `TIME`
    Time,
    /// `JSONB` / `JSON`
    Json,
}

impl ColumnType {
    /// `true` for integer types.
    pub const fn is_integer(self) -> bool {
        matches!(self, ColumnType::SmallInt | ColumnType::Int | ColumnType::BigInt)
    }

    /// `true` for floating point types.
    pub const fn is_float(self) -> bool {
        matches!(self, ColumnType::Real | ColumnType::Double)
    }
}

/// Maps a Rust type to a SQL column type.
///
/// `Base` is the type used for typed comparisons: `Option<i32>` has `Base = i32`
/// so `User::AGE.gt(18)` compiles while `User::AGE.gt("18")` does not.
pub trait SqlType {
    /// The non-nullable type used for comparisons.
    type Base;
    /// SQL type of the column.
    const KIND: ColumnType;
    /// Whether the column accepts `NULL`.
    const NULLABLE: bool = false;
}

/// Converts a borrowed Rust value into a [`Value`].
pub trait ToValue {
    /// Performs the conversion.
    fn to_value(&self) -> Value;
}

/// Converts an owned Rust value into a [`Value`] (moving strings and buffers).
pub trait IntoValue {
    /// Performs the conversion.
    fn into_value(self) -> Value;
}

/// Converts a [`Value`] back into a Rust value.
pub trait FromValue: Sized {
    /// Performs the conversion.
    fn from_value(value: Value) -> Result<Self>;
}

/// "Can be compared with / assigned to a column of Rust type `T`".
///
/// This is what makes the query builder type safe: `Column<User, String>::eq`
/// accepts `String`, `&str` and `&String`, but not an integer.
pub trait Bind<T> {
    /// Converts `self` into the value that will be bound as a parameter.
    fn into_bind(self) -> Value;
}

impl<T: IntoValue> Bind<T> for T {
    #[inline]
    fn into_bind(self) -> Value {
        self.into_value()
    }
}

impl<T: IntoValue> Bind<Option<T>> for T {
    #[inline]
    fn into_bind(self) -> Value {
        self.into_value()
    }
}

impl Bind<String> for &str {
    #[inline]
    fn into_bind(self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl Bind<String> for &String {
    #[inline]
    fn into_bind(self) -> Value {
        Value::Text(self.clone())
    }
}

impl Bind<Option<String>> for &str {
    #[inline]
    fn into_bind(self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl Bind<Vec<u8>> for &[u8] {
    #[inline]
    fn into_bind(self) -> Value {
        Value::Bytes(self.to_vec())
    }
}

fn mismatch(expected: &str, got: &Value) -> Error {
    if got.is_null() {
        Error::UnexpectedNull { column: String::new() }
    } else {
        Error::Conversion(format!("expected {expected}, found {}", got.type_name()))
    }
}

// ---------------------------------------------------------------------------
// Value itself
// ---------------------------------------------------------------------------

impl ToValue for Value {
    #[inline]
    fn to_value(&self) -> Value {
        self.clone()
    }
}

impl IntoValue for Value {
    #[inline]
    fn into_value(self) -> Value {
        self
    }
}

impl FromValue for Value {
    #[inline]
    fn from_value(value: Value) -> Result<Self> {
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// Integers
// ---------------------------------------------------------------------------

macro_rules! int_impls {
    ($($t:ty => $variant:ident as $repr:ty, $kind:ident;)*) => {$(
        impl SqlType for $t {
            type Base = $t;
            const KIND: ColumnType = ColumnType::$kind;
        }
        impl ToValue for $t {
            #[inline]
            fn to_value(&self) -> Value {
                Value::$variant(*self as $repr)
            }
        }
        impl IntoValue for $t {
            #[inline]
            fn into_value(self) -> Value {
                Value::$variant(self as $repr)
            }
        }
        impl FromValue for $t {
            #[inline]
            fn from_value(value: Value) -> Result<Self> {
                match value {
                    Value::I32(v) => <$t>::try_from(v)
                        .map_err(|_| Error::Conversion(format!("{v} is out of range for {}", stringify!($t)))),
                    Value::I64(v) => <$t>::try_from(v)
                        .map_err(|_| Error::Conversion(format!("{v} is out of range for {}", stringify!($t)))),
                    Value::Bool(b) => Ok(b as $t),
                    Value::Text(ref s) => s.trim().parse::<$t>().map_err(|_| mismatch(stringify!($t), &value)),
                    ref other => Err(mismatch(stringify!($t), other)),
                }
            }
        }
    )*};
}

int_impls! {
    i8 => I32 as i32, SmallInt;
    i16 => I32 as i32, SmallInt;
    i32 => I32 as i32, Int;
    i64 => I64 as i64, BigInt;
    u8 => I32 as i32, SmallInt;
    u16 => I32 as i32, Int;
    u32 => I64 as i64, BigInt;
}

// ---------------------------------------------------------------------------
// Floats
// ---------------------------------------------------------------------------

macro_rules! float_impls {
    ($($t:ty, $kind:ident;)*) => {$(
        impl SqlType for $t {
            type Base = $t;
            const KIND: ColumnType = ColumnType::$kind;
        }
        impl ToValue for $t {
            #[inline]
            fn to_value(&self) -> Value {
                Value::F64(*self as f64)
            }
        }
        impl IntoValue for $t {
            #[inline]
            fn into_value(self) -> Value {
                Value::F64(self as f64)
            }
        }
        impl FromValue for $t {
            #[inline]
            fn from_value(value: Value) -> Result<Self> {
                match value {
                    Value::F64(v) => Ok(v as $t),
                    Value::I32(v) => Ok(v as $t),
                    Value::I64(v) => Ok(v as $t),
                    Value::Text(ref s) => s.trim().parse::<$t>().map_err(|_| mismatch(stringify!($t), &value)),
                    ref other => Err(mismatch(stringify!($t), other)),
                }
            }
        }
    )*};
}

float_impls! {
    f32, Real;
    f64, Double;
}

// ---------------------------------------------------------------------------
// bool
// ---------------------------------------------------------------------------

impl SqlType for bool {
    type Base = bool;
    const KIND: ColumnType = ColumnType::Bool;
}

impl ToValue for bool {
    #[inline]
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl IntoValue for bool {
    #[inline]
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl FromValue for bool {
    #[inline]
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Bool(b) => Ok(b),
            Value::I32(v) => Ok(v != 0),
            Value::I64(v) => Ok(v != 0),
            Value::Text(ref s) => match s.as_str() {
                "t" | "true" | "TRUE" | "1" => Ok(true),
                "f" | "false" | "FALSE" | "0" => Ok(false),
                _ => Err(mismatch("bool", &value)),
            },
            ref other => Err(mismatch("bool", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

impl SqlType for String {
    type Base = String;
    const KIND: ColumnType = ColumnType::Text;
}

impl ToValue for String {
    #[inline]
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl IntoValue for String {
    #[inline]
    fn into_value(self) -> Value {
        Value::Text(self)
    }
}

impl IntoValue for &str {
    #[inline]
    fn into_value(self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl IntoValue for &String {
    #[inline]
    fn into_value(self) -> Value {
        Value::Text(self.clone())
    }
}

impl ToValue for str {
    #[inline]
    fn to_value(&self) -> Value {
        Value::Text(self.to_owned())
    }
}

impl FromValue for String {
    #[inline]
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Text(s) => Ok(s),
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => Ok(u.to_string()),
            #[cfg(feature = "json")]
            Value::Json(j) => Ok(j.to_string()),
            other => Err(mismatch("text", &other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Bytes
// ---------------------------------------------------------------------------

impl SqlType for Vec<u8> {
    type Base = Vec<u8>;
    const KIND: ColumnType = ColumnType::Bytes;
}

impl ToValue for Vec<u8> {
    #[inline]
    fn to_value(&self) -> Value {
        Value::Bytes(self.clone())
    }
}

impl IntoValue for Vec<u8> {
    #[inline]
    fn into_value(self) -> Value {
        Value::Bytes(self)
    }
}

impl IntoValue for &[u8] {
    #[inline]
    fn into_value(self) -> Value {
        Value::Bytes(self.to_vec())
    }
}

impl FromValue for Vec<u8> {
    #[inline]
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Bytes(b) => Ok(b),
            Value::Text(s) => Ok(s.into_bytes()),
            other => Err(mismatch("bytes", &other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Option<T>
// ---------------------------------------------------------------------------

impl<T: SqlType> SqlType for Option<T> {
    type Base = T::Base;
    const KIND: ColumnType = T::KIND;
    const NULLABLE: bool = true;
}

impl<T: ToValue> ToValue for Option<T> {
    #[inline]
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
}

impl<T: IntoValue> IntoValue for Option<T> {
    #[inline]
    fn into_value(self) -> Value {
        match self {
            Some(v) => v.into_value(),
            None => Value::Null,
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    #[inline]
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            v => T::from_value(v).map(Some),
        }
    }
}

impl<T: ToValue + ?Sized> ToValue for &T {
    #[inline]
    fn to_value(&self) -> Value {
        (**self).to_value()
    }
}

// ---------------------------------------------------------------------------
// uuid
// ---------------------------------------------------------------------------

#[cfg(feature = "uuid")]
mod uuid_impls {
    use super::*;

    impl SqlType for uuid::Uuid {
        type Base = uuid::Uuid;
        const KIND: ColumnType = ColumnType::Uuid;
    }

    impl ToValue for uuid::Uuid {
        #[inline]
        fn to_value(&self) -> Value {
            Value::Uuid(*self)
        }
    }

    impl IntoValue for uuid::Uuid {
        #[inline]
        fn into_value(self) -> Value {
            Value::Uuid(self)
        }
    }

    impl FromValue for uuid::Uuid {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Uuid(u) => Ok(u),
                Value::Text(ref s) => uuid::Uuid::parse_str(s).map_err(Error::conversion),
                Value::Bytes(ref b) => uuid::Uuid::from_slice(b).map_err(Error::conversion),
                ref other => Err(mismatch("uuid", other)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// chrono
// ---------------------------------------------------------------------------

#[cfg(feature = "chrono")]
mod chrono_impls {
    use super::*;
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    const DATETIME_FORMATS: &[&str] =
        &["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"];

    fn parse_naive(s: &str) -> Option<NaiveDateTime> {
        let s = s.trim();
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(dt.naive_utc());
        }
        for f in DATETIME_FORMATS {
            if let Ok(dt) = NaiveDateTime::parse_from_str(s, f) {
                return Some(dt);
            }
        }
        // `YYYY-MM-DD HH:MM:SS+00` / `+00:00` as produced by some drivers.
        DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z").ok().map(|d| d.naive_utc())
    }

    impl SqlType for NaiveDateTime {
        type Base = NaiveDateTime;
        const KIND: ColumnType = ColumnType::Timestamp;
    }
    impl ToValue for NaiveDateTime {
        #[inline]
        fn to_value(&self) -> Value {
            Value::Timestamp(*self)
        }
    }
    impl IntoValue for NaiveDateTime {
        #[inline]
        fn into_value(self) -> Value {
            Value::Timestamp(self)
        }
    }
    impl FromValue for NaiveDateTime {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Timestamp(t) => Ok(t),
                Value::TimestampTz(t) => Ok(t.naive_utc()),
                Value::Text(ref s) => parse_naive(s).ok_or_else(|| mismatch("timestamp", &value)),
                ref other => Err(mismatch("timestamp", other)),
            }
        }
    }

    impl SqlType for DateTime<Utc> {
        type Base = DateTime<Utc>;
        const KIND: ColumnType = ColumnType::TimestampTz;
    }
    impl ToValue for DateTime<Utc> {
        #[inline]
        fn to_value(&self) -> Value {
            Value::TimestampTz(*self)
        }
    }
    impl IntoValue for DateTime<Utc> {
        #[inline]
        fn into_value(self) -> Value {
            Value::TimestampTz(self)
        }
    }
    impl FromValue for DateTime<Utc> {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::TimestampTz(t) => Ok(t),
                Value::Timestamp(t) => Ok(t.and_utc()),
                Value::Text(ref s) => {
                    parse_naive(s).map(|t| t.and_utc()).ok_or_else(|| mismatch("timestamptz", &value))
                }
                ref other => Err(mismatch("timestamptz", other)),
            }
        }
    }

    impl SqlType for NaiveDate {
        type Base = NaiveDate;
        const KIND: ColumnType = ColumnType::Date;
    }
    impl ToValue for NaiveDate {
        #[inline]
        fn to_value(&self) -> Value {
            Value::Date(*self)
        }
    }
    impl IntoValue for NaiveDate {
        #[inline]
        fn into_value(self) -> Value {
            Value::Date(self)
        }
    }
    impl FromValue for NaiveDate {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Date(d) => Ok(d),
                Value::Timestamp(t) => Ok(t.date()),
                Value::TimestampTz(t) => Ok(t.date_naive()),
                Value::Text(ref s) => {
                    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| mismatch("date", &value))
                }
                ref other => Err(mismatch("date", other)),
            }
        }
    }

    impl SqlType for NaiveTime {
        type Base = NaiveTime;
        const KIND: ColumnType = ColumnType::Time;
    }
    impl ToValue for NaiveTime {
        #[inline]
        fn to_value(&self) -> Value {
            Value::Time(*self)
        }
    }
    impl IntoValue for NaiveTime {
        #[inline]
        fn into_value(self) -> Value {
            Value::Time(self)
        }
    }
    impl FromValue for NaiveTime {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Time(t) => Ok(t),
                Value::Text(ref s) => {
                    NaiveTime::parse_from_str(s.trim(), "%H:%M:%S%.f").map_err(|_| mismatch("time", &value))
                }
                ref other => Err(mismatch("time", other)),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Stores any `serde` type as a JSON column (`JSONB` on PostgreSQL, `TEXT` on
/// SQLite).
///
/// ```ignore
/// #[derive(Model)]
/// struct Event {
///     id: i64,
///     payload: Json<MyPayload>,
/// }
/// ```
#[cfg(feature = "json")]
#[derive(Clone, Debug, PartialEq, Eq, Default, Hash, PartialOrd, Ord)]
pub struct Json<T>(pub T);

#[cfg(feature = "json")]
mod json_impls {
    use super::*;

    impl<T> std::ops::Deref for Json<T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.0
        }
    }

    impl<T> std::ops::DerefMut for Json<T> {
        fn deref_mut(&mut self) -> &mut T {
            &mut self.0
        }
    }

    impl SqlType for serde_json::Value {
        type Base = serde_json::Value;
        const KIND: ColumnType = ColumnType::Json;
    }
    impl ToValue for serde_json::Value {
        #[inline]
        fn to_value(&self) -> Value {
            Value::Json(self.clone())
        }
    }
    impl IntoValue for serde_json::Value {
        #[inline]
        fn into_value(self) -> Value {
            Value::Json(self)
        }
    }
    impl FromValue for serde_json::Value {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Json(j) => Ok(j),
                Value::Text(ref s) => serde_json::from_str(s).map_err(Error::conversion),
                Value::Bytes(ref b) => serde_json::from_slice(b).map_err(Error::conversion),
                ref other => Err(mismatch("json", other)),
            }
        }
    }

    impl<T> SqlType for Json<T> {
        type Base = Json<T>;
        const KIND: ColumnType = ColumnType::Json;
    }
    impl<T: serde::Serialize> ToValue for Json<T> {
        fn to_value(&self) -> Value {
            Value::Json(serde_json::to_value(&self.0).expect("Json<T>: value cannot be represented as JSON"))
        }
    }
    impl<T: serde::Serialize> IntoValue for Json<T> {
        fn into_value(self) -> Value {
            self.to_value()
        }
    }
    impl<T: serde::de::DeserializeOwned> FromValue for Json<T> {
        fn from_value(value: Value) -> Result<Self> {
            match value {
                Value::Json(j) => serde_json::from_value(j).map(Json).map_err(Error::conversion),
                Value::Text(ref s) => serde_json::from_str(s).map(Json).map_err(Error::conversion),
                Value::Bytes(ref b) => serde_json::from_slice(b).map(Json).map_err(Error::conversion),
                ref other => Err(mismatch("json", other)),
            }
        }
    }
}

/// Builds a `Vec<Value>` of query parameters from heterogeneous Rust values.
///
/// ```
/// use memoire_orm::{params, Value};
/// let p = params![1, "alice", Some(3.5_f64), None::<i32>];
/// assert_eq!(p[1], Value::Text("alice".into()));
/// assert!(p[3].is_null());
/// ```
#[macro_export]
macro_rules! params {
    () => { ::std::vec::Vec::<$crate::Value>::new() };
    ($($value:expr),+ $(,)?) => {
        ::std::vec![$($crate::IntoValue::into_value($value)),+]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_round_trips_and_range_checks() {
        assert_eq!(i64::from_value(Value::I32(5)).unwrap(), 5);
        assert_eq!(i16::from_value(Value::I64(300)).unwrap(), 300);
        assert!(i8::from_value(Value::I64(300)).is_err());
        assert!(matches!(i32::from_value(Value::Null), Err(Error::UnexpectedNull { .. })));
        assert_eq!(Option::<i32>::from_value(Value::Null).unwrap(), None);
        assert_eq!(u32::MAX.to_value(), Value::I64(u32::MAX as i64));
    }

    #[test]
    fn text_and_bool() {
        assert_eq!(String::from_value(Value::Text("x".into())).unwrap(), "x");
        assert!(String::from_value(Value::I32(1)).is_err());
        assert!(bool::from_value(Value::I64(1)).unwrap());
        assert!(!bool::from_value(Value::Text("f".into())).unwrap());
    }

    #[test]
    fn display_is_sql_literal() {
        assert_eq!(Value::Text("it's".into()).to_string(), "'it''s'");
        assert_eq!(Value::Bytes(vec![0xde, 0xad]).to_string(), "X'DEAD'");
        assert_eq!(Value::Null.to_string(), "NULL");
    }

    #[test]
    fn params_macro() {
        let p = params![1_i64, "a", None::<i32>];
        assert_eq!(p, vec![Value::I64(1), Value::Text("a".into()), Value::Null]);
    }
}
