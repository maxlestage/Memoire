//! Row decoding.

use crate::error::{Error, Result};
use crate::model::Model;
use crate::value::{FromValue, Value};

/// A row returned by a backend.
///
/// Values are *taken* (moved out) by position, so decoding a row into a struct
/// never clones strings or buffers.
pub trait RowSource {
    /// Number of columns.
    fn len(&self) -> usize;

    /// `true` when the row has no column.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Moves the value at `index` out of the row.
    fn take(&mut self, index: usize) -> Result<Value>;

    /// `true` if the value at `index` is `NULL` (does not consume it).
    fn is_null(&self, index: usize) -> bool;
}

/// A row stored as a vector of values (used by the memory engine and tests).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValueRow(pub Vec<Value>);

impl RowSource for ValueRow {
    #[inline]
    fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn take(&mut self, index: usize) -> Result<Value> {
        self.0
            .get_mut(index)
            .map(std::mem::take)
            .ok_or_else(|| Error::Conversion(format!("column index {index} out of range ({} columns)", self.0.len())))
    }

    #[inline]
    fn is_null(&self, index: usize) -> bool {
        self.0.get(index).is_none_or(Value::is_null)
    }
}

/// Types that can be built from (a slice of) a row.
///
/// Implemented by every [`Model`] (through the derive), by `Option<M>` (for
/// `LEFT JOIN`s), by scalar types (`i64`, `String`, ...) and by tuples of
/// `FromRow` types, which is how joins and projections are decoded.
pub trait FromRow: Sized {
    /// Number of columns consumed.
    const WIDTH: usize;

    /// Decodes the value starting at column `offset`.
    fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self>;
}

impl<M: Model> FromRow for Option<M> {
    const WIDTH: usize = M::WIDTH;

    #[inline]
    fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self> {
        if row.is_null(offset + M::SCHEMA.primary_key) { Ok(None) } else { M::from_row(row, offset).map(Some) }
    }
}

macro_rules! scalar_from_row {
    ($($t:ty),* $(,)?) => {$(
        impl FromRow for $t {
            const WIDTH: usize = 1;
            #[inline]
            fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self> {
                <$t as FromValue>::from_value(row.take(offset)?).map_err(|e| e.in_column(&format!("#{offset}")))
            }
        }
        impl FromRow for Option<$t> {
            const WIDTH: usize = 1;
            #[inline]
            fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self> {
                <Option<$t> as FromValue>::from_value(row.take(offset)?).map_err(|e| e.in_column(&format!("#{offset}")))
            }
        }
    )*};
}

scalar_from_row!(bool, i8, i16, i32, i64, u8, u16, u32, f32, f64, String, Vec<u8>, Value);

#[cfg(feature = "uuid")]
scalar_from_row!(uuid::Uuid);
#[cfg(feature = "chrono")]
scalar_from_row!(chrono::NaiveDateTime, chrono::DateTime<chrono::Utc>, chrono::NaiveDate, chrono::NaiveTime);
#[cfg(feature = "json")]
scalar_from_row!(serde_json::Value);

#[cfg(feature = "json")]
impl<T: serde::de::DeserializeOwned> FromRow for crate::value::Json<T> {
    const WIDTH: usize = 1;
    fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self> {
        FromValue::from_value(row.take(offset)?)
    }
}

macro_rules! tuple_from_row {
    ($($name:ident)+) => {
        impl<$($name: FromRow),+> FromRow for ($($name,)+) {
            const WIDTH: usize = 0 $(+ $name::WIDTH)+;

            #[inline]
            #[allow(unused_assignments)]
            fn from_row<R: RowSource + ?Sized>(row: &mut R, offset: usize) -> Result<Self> {
                let mut at = offset;
                Ok(($({
                    let v = $name::from_row(row, at)?;
                    at += $name::WIDTH;
                    v
                },)+))
            }
        }
    };
}

tuple_from_row!(A);
tuple_from_row!(A B);
tuple_from_row!(A B C);
tuple_from_row!(A B C D);
tuple_from_row!(A B C D E);
tuple_from_row!(A B C D E F);
tuple_from_row!(A B C D E F G);
tuple_from_row!(A B C D E F G H);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tuples_consume_columns_in_order() {
        let mut row = ValueRow(vec![Value::I64(1), Value::Text("a".into()), Value::Null]);
        let (a, b, c): (i64, String, Option<i32>) = FromRow::from_row(&mut row, 0).unwrap();
        assert_eq!((a, b.as_str(), c), (1, "a", None));
    }

    #[test]
    fn errors_mention_the_column() {
        let mut row = ValueRow(vec![Value::Null]);
        let err = <i64 as FromRow>::from_row(&mut row, 0).unwrap_err();
        assert!(matches!(err, Error::UnexpectedNull { .. }));
    }
}
