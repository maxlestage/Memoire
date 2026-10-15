//! Model lifecycle: automatic timestamps, soft deletes and hooks.
//!
//! Three opt-in behaviours, declared on the model with `#[derive(Model)]`
//! attributes, in the spirit of Laravel's Eloquent and Doctrine's lifecycle
//! callbacks:
//!
//! * **Timestamps** — `#[memoire(timestamps)]` on the struct (fields named
//!   `created_at` and `updated_at`), or `#[memoire(created_at)]` /
//!   `#[memoire(updated_at)]` on any field. [`Model::insert`] fills both,
//!   [`Model::update`], [`Model::update_only`], [`Model::save`],
//!   [`Model::upsert`] and [`Update`](crate::Update) refresh `updated_at`.
//! * **Soft deletes** — `#[memoire(soft_deletes)]` on the struct (a field named
//!   `deleted_at`), or `#[memoire(soft_delete)]` on a nullable field.
//!   [`Model::delete`] then stamps the column instead of removing the row, and
//!   every query built with [`Model::query`] leaves those rows out
//!   ([`Select::with_trashed`](crate::Select::with_trashed),
//!   [`Select::only_trashed`](crate::Select::only_trashed),
//!   [`Model::restore`], [`Model::force_delete`]).
//! * **Hooks** — `#[memoire(hooks)]` on the struct and an `impl Hooks for ...`:
//!   code that runs before and after each insert, update and delete of a row,
//!   and can reject it.
//!
//! ```
//! use memoire_orm::lifecycle::Hooks;
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug, Clone, PartialEq)]
//! #[memoire(table = "articles", timestamps, soft_deletes, hooks)]
//! struct Article {
//!     #[memoire(primary_key, auto_increment)]
//!     id: i64,
//!     title: String,
//!     slug: String,
//!     created_at: Option<i64>, // milliseconds since the Unix epoch
//!     updated_at: Option<i64>,
//!     deleted_at: Option<i64>,
//! }
//!
//! impl Hooks for Article {
//!     fn before_save(&mut self) -> Result<()> {
//!         if self.title.trim().is_empty() {
//!             return Err(Error::Validation("an article needs a title".into()));
//!         }
//!         self.slug = self.title.to_lowercase().replace(' ', "-");
//!         Ok(())
//!     }
//! }
//!
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new();
//! db.create_table::<Article>().await?;
//!
//! let mut a = Article { id: 0, title: "Hello World".into(), slug: String::new(),
//!                       created_at: None, updated_at: None, deleted_at: None };
//! a.insert(&db).await?;
//! assert_eq!(a.slug, "hello-world");
//! assert!(a.created_at.is_some() && a.updated_at == a.created_at);
//!
//! a.delete(&db).await?;                                   // soft delete
//! assert_eq!(Article::count(&db).await?, 0);              // hidden from queries
//! assert_eq!(Article::with_trashed().count(&db).await?, 1); // but still there
//! a.restore(&db).await?;
//! assert_eq!(Article::count(&db).await?, 1);
//!
//! let mut empty = Article { title: " ".into(), ..a.clone() };
//! assert!(matches!(empty.save(&db).await, Err(Error::Validation(_))));
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```
//!
//! ## Timestamp column types
//!
//! The value written depends on the Rust type of the field:
//!
//! | Field type | Value |
//! |---|---|
//! | `i64` | milliseconds since the Unix epoch |
//! | `i32` | seconds since the Unix epoch |
//! | `f64` | seconds since the Unix epoch, with a fraction |
//! | `String` | RFC 3339 in UTC, `2026-10-15T08:30:00.123Z` (sorts like the time) |
//! | `chrono::DateTime<Utc>`, `chrono::NaiveDateTime` (UTC), `chrono::NaiveDate` | now (feature `chrono`) |
//!
//! Any other type is rejected at compile time. Every field may be an
//! `Option<...>`; a soft delete column must be one.

use crate::error::Result;
use crate::model::Model;
use crate::value::{ColumnType, Value};

/// Lifecycle hooks of a model, run around the writes of one row, like
/// Doctrine's lifecycle callbacks (`prePersist`, `postUpdate`...) or
/// Eloquent's model events (`saving`, `created`...).
///
/// Implement the methods you need (all do nothing by default) and add
/// `#[memoire(hooks)]` to the model. A `before_*` hook returning an error
/// cancels the write, which is not run; the error is returned to the caller
/// ([`Error::Validation`](crate::Error::Validation) is meant for that).
///
/// | Method | Runs in |
/// |---|---|
/// | [`before_save`](Self::before_save) | [`Model::insert`], [`Model::save`] (can modify the row) |
/// | [`before_insert`](Self::before_insert) / [`after_insert`](Self::after_insert) | [`Model::insert`], [`Model::save`] of a new row |
/// | [`before_update`](Self::before_update) / [`after_update`](Self::after_update) | [`Model::update`], [`Model::update_only`], [`Model::save`] of a stored row |
/// | [`before_delete`](Self::before_delete) / [`after_delete`](Self::after_delete) | [`Model::delete`] (soft too), [`Model::force_delete`] |
///
/// Writes of many rows at once ([`Model::insert_many`], [`Model::upsert`],
/// `update_many`, `delete_many`) run no hook, like Eloquent's mass updates. Hooks
/// are synchronous and see only the row: to react to writes with database
/// access, subscribe to [`Observed`](crate::Observed) instead.
pub trait Hooks: Model {
    /// Before an insert or a [`save`](Model::save), before the more specific
    /// hooks. It may modify the row: normalise an email, derive a slug...
    fn before_save(&mut self) -> Result<()> {
        Ok(())
    }

    /// Before an insert (after [`before_save`](Self::before_save)). The
    /// timestamps are not filled yet.
    fn before_insert(&mut self) -> Result<()> {
        Ok(())
    }

    /// After a successful insert; the generated key and the timestamps are set.
    fn after_insert(&self) {}

    /// Before an update of this row.
    fn before_update(&self) -> Result<()> {
        Ok(())
    }

    /// After a successful update of this row.
    fn after_update(&self) {}

    /// Before this row is deleted (or soft deleted).
    fn before_delete(&self) -> Result<()> {
        Ok(())
    }

    /// After this row was deleted (or soft deleted).
    fn after_delete(&self) {}
}

/// The [`Hooks`] of a model as function pointers, returned by
/// [`Model::hooks`] (`#[memoire(hooks)]` generates it).
pub struct HookFns<M> {
    /// [`Hooks::before_save`].
    pub before_save: fn(&mut M) -> Result<()>,
    /// [`Hooks::before_insert`].
    pub before_insert: fn(&mut M) -> Result<()>,
    /// [`Hooks::after_insert`].
    pub after_insert: fn(&M),
    /// [`Hooks::before_update`].
    pub before_update: fn(&M) -> Result<()>,
    /// [`Hooks::after_update`].
    pub after_update: fn(&M),
    /// [`Hooks::before_delete`].
    pub before_delete: fn(&M) -> Result<()>,
    /// [`Hooks::after_delete`].
    pub after_delete: fn(&M),
}

impl<M> Clone for HookFns<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for HookFns<M> {}

impl<M> std::fmt::Debug for HookFns<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookFns").finish_non_exhaustive()
    }
}

impl<M: Hooks> HookFns<M> {
    /// The hooks implemented by `M`.
    pub fn of() -> Self {
        HookFns {
            before_save: M::before_save,
            before_insert: M::before_insert,
            after_insert: M::after_insert,
            before_update: M::before_update,
            after_update: M::after_update,
            before_delete: M::before_delete,
            after_delete: M::after_delete,
        }
    }
}

/// `true` for the column types that can hold a timestamp (see the
/// [module documentation](self)).
pub const fn is_timestamp_kind(kind: ColumnType) -> bool {
    matches!(
        kind,
        ColumnType::Int
            | ColumnType::BigInt
            | ColumnType::Double
            | ColumnType::Text
            | ColumnType::Timestamp
            | ColumnType::TimestampTz
            | ColumnType::Date
    )
}

/// Milliseconds since the Unix epoch, from the system clock (the browser's
/// clock on `wasm32` with the `web` feature).
pub fn now_millis() -> i64 {
    #[cfg(all(target_arch = "wasm32", feature = "web"))]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "web")))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
            Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
        }
    }
}

/// The current time as a value of a timestamp column of type `kind` (see the
/// [module documentation](self) for the encoding of each type). `NULL` for a
/// type that cannot hold a timestamp.
pub fn now_value(kind: ColumnType) -> Value {
    timestamp_value(kind, now_millis())
}

/// `millis` (milliseconds since the Unix epoch) as a value of a column of type `kind`.
pub fn timestamp_value(kind: ColumnType, millis: i64) -> Value {
    match kind {
        ColumnType::BigInt => Value::I64(millis),
        ColumnType::Int => Value::I32(i32::try_from(millis.div_euclid(1000)).unwrap_or(i32::MAX)),
        ColumnType::Double => Value::F64(millis as f64 / 1000.0),
        ColumnType::Text => Value::Text(rfc3339(millis)),
        #[cfg(feature = "chrono")]
        ColumnType::TimestampTz => {
            chrono::DateTime::from_timestamp_millis(millis).map_or(Value::Null, Value::TimestampTz)
        }
        #[cfg(feature = "chrono")]
        ColumnType::Timestamp => {
            chrono::DateTime::from_timestamp_millis(millis).map_or(Value::Null, |d| Value::Timestamp(d.naive_utc()))
        }
        #[cfg(feature = "chrono")]
        ColumnType::Date => {
            chrono::DateTime::from_timestamp_millis(millis).map_or(Value::Null, |d| Value::Date(d.date_naive()))
        }
        _ => Value::Null,
    }
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for milliseconds since the Unix epoch.
fn rfc3339(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let ms = millis.rem_euclid(86_400_000);
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// A timestamp value that was never set: `NULL`, `0` or an empty string.
pub(crate) fn is_unset(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::I32(0) | Value::I64(0) => true,
        Value::F64(f) => *f == 0.0,
        Value::Text(s) => s.is_empty(),
        _ => false,
    }
}

/// The timestamp of the column at `index` of `M`.
pub(crate) fn now_for<M: Model>(index: usize) -> Value {
    now_value(M::SCHEMA.columns[index].kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_dates() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(1_791_965_096_789), "2026-10-14T08:04:56.789Z");
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn values_per_type() {
        assert_eq!(timestamp_value(ColumnType::BigInt, 1_500), Value::I64(1_500));
        assert_eq!(timestamp_value(ColumnType::Int, 1_500), Value::I32(1));
        assert_eq!(timestamp_value(ColumnType::Double, 1_500), Value::F64(1.5));
        assert_eq!(timestamp_value(ColumnType::Bool, 1_500), Value::Null);
        assert!(now_millis() > 1_700_000_000_000);
        assert!(is_unset(&Value::I64(0)) && !is_unset(&Value::I64(1)));
    }
}
