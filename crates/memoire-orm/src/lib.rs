//! # Memoire ORM
//!
//! A blazing-fast, type-safe, async ORM written in pure Rust.
//!
//! * **Compile-time SQL** — `#[derive(Model)]` renders every primary-key
//!   statement (`find`, `insert`, `update`, `delete`, `upsert`) for PostgreSQL,
//!   SQLite and MySQL while your code compiles.
//! * **Type-safe query builder** — `User::AGE.gt(18)` compiles, `User::AGE.gt("18")`
//!   does not. Combine conditions with `&`, `|` and `!`.
//! * **Zero-cost row mapping** — rows are decoded by position in one pass, values
//!   are moved (never cloned) into your structs, parameters are borrowed.
//! * **SQL and NoSQL, one API** — [`MemoryDb`] (pure Rust, runs in WebAssembly),
//!   `SqliteDb` (`sqlite`), `PgPool` (`postgres`), `MySqlPool` (`mysql`),
//!   `RedisDb` (`redis`) and `MongoDb` (`mongodb`). The same test suite passes on
//!   all of them.
//! * **Relations without N+1** — `users.load_children::<Post>(&db)`, `load_parent`,
//!   many-to-many `load_via`, all from the foreign keys you already declare
//!   ([`relation`]).
//! * **Full-text search and multi-column indexes on every backend** —
//!   `Post::BODY.matches("rust orm")`, `#[memoire(unique(user_id, team_id))]`.
//! * **Change notifications** — [`Observed`] announces every write, on any backend.
//! * **Model lifecycle, Eloquent-style** — `#[memoire(timestamps, soft_deletes, hooks)]`:
//!   automatic `created_at`/`updated_at`, soft deletes (`with_trashed`, `restore`) and
//!   [`Hooks`] that can reject a write ([`lifecycle`]); `save`, `first_or_create`,
//!   `update_or_create`, `chunks`, `cursor_paginate`, and a query log ([`Logged`]).
//! * **Browser storage** — `web::Store`, a state store in the spirit of Pinia, and
//!   `web::LocalStorageDb`, a database in `localStorage` (feature `web`).
//! * **Embedded and durable** — `MemoryDb::open(path)` stores the in-memory engine
//!   in a crash-safe file, optionally encrypted (feature `encryption`).
//! * **Transactions, migrations, joins, aggregates, pagination, upserts, partial
//!   updates.**
//! * `#![forbid(unsafe_code)]`.
//!
//! ## Quick start
//!
//! ```
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug, Clone, PartialEq)]
//! #[memoire(table = "users")]
//! pub struct User {
//!     #[memoire(primary_key, auto_increment)]
//!     pub id: i64,
//!     #[memoire(unique)]
//!     pub email: String,
//!     pub name: String,
//!     pub age: Option<i32>,
//! }
//!
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new(); // or SqliteDb, PgPool, MySqlPool, RedisDb, MongoDb
//! db.create_table::<User>().await?;
//!
//! let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
//! alice.insert(&db).await?; // alice.id is now set
//!
//! let adults = User::query()
//!     .filter(User::AGE.ge(18) & User::NAME.contains("li"))
//!     .order_by(User::NAME.asc())
//!     .limit(20)
//!     .all(&db)
//!     .await?;
//! assert_eq!(adults, vec![alice]);
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```
//!
//! ## Feature flags
//!
//! | Feature | Default | Description |
//! |---|---|---|
//! | `memory` | yes | In-memory engine ([`MemoryDb`]), optionally stored in a file |
//! | `encryption` | | Encrypted [`MemoryDb`] files (AES-256-GCM) |
//! | `web` | | Browser storage (module `web`): a Pinia-like `Store` and `LocalStorageDb` |
//! | `sqlite` | | SQLite backend (`SqliteDb`) |
//! | `postgres` | | PostgreSQL backend (`PgPool`, requires Tokio) |
//! | `postgres-tls` | | TLS for PostgreSQL (rustls, honours `sslmode`) |
//! | `mysql` | | MySQL / MariaDB backend (`MySqlPool`, requires Tokio) |
//! | `mysql-tls` | | TLS for MySQL / MariaDB (rustls) |
//! | `redis` | | Redis backend (`RedisDb`, requires Tokio) |
//! | `redis-tls` | | TLS for Redis (`rediss://`) |
//! | `mongodb` | | MongoDB backend (`MongoDb`, requires Tokio) |
//! | `chrono` | | `chrono` date and time types |
//! | `uuid` | | `uuid::Uuid` |
//! | `json` | | `serde_json::Value` and [`Json<T>`](crate::Json) columns |
//! | `full` | | everything |

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]
#![allow(clippy::should_implement_trait)]

extern crate self as memoire_orm;

/// The version of this crate, e.g. `"0.1.0"`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod ast;
pub mod backend;
mod error;
pub mod events;
mod executor;
mod expr;
pub mod lifecycle;
pub mod log;
pub mod migrate;
mod model;
mod query;
pub mod relation;
mod row;
mod schema;
pub mod sql;
mod value;
#[cfg(feature = "web")]
#[cfg_attr(docsrs, doc(cfg(feature = "web")))]
pub mod web;

pub use ast::{JoinKind, Query};
pub use error::{Error, Result};
pub use events::{Change, ChangeKind, Observed};
pub use executor::{Database, ExecResult, Executor, Transaction};
pub use expr::{
    AggFunc, BinOp, Column, ColumnRef, Expr, Nulls, Numeric, Order, TextSearch, count_all, escape_like, text_words,
};
pub use lifecycle::Hooks;
pub use log::{Logged, LoggedQuery};
pub use migrate::{Migration, Migrator};
pub use model::Model;
pub use query::{Chunks, ColumnList, CursorPage, Delete, Joined, ModelColumns, Page, Select, SumType, Update};
pub use relation::LoadRelated;
pub use row::{FromRow, RowSource, ValueRow};
pub use schema::{ColumnDef, Dialect, ForeignKey, IndexDef, StaticSql, TableSchema};
#[cfg(feature = "json")]
pub use value::Json;
pub use value::{Bind, ColumnType, FromValue, IntoValue, SqlType, ToValue, Value};

/// Derive macros. `#[derive(Model)]` implements [`Model`](trait@Model) and
/// [`FromRow`](trait@FromRow); `#[derive(FromRow)]` only the latter.
pub use memoire_orm_derive::{FromRow, Model};

#[cfg(feature = "memory")]
pub use backend::memory::{MemoryDb, MemoryTransaction, StorageOptions};
#[cfg(feature = "mongodb")]
pub use backend::mongodb::{MongoDb, MongoTransaction};
#[cfg(feature = "mysql")]
pub use backend::mysql::{MySqlPool, MySqlTransaction};
#[cfg(feature = "postgres")]
pub use backend::postgres::{PgPool, PgTransaction};
#[cfg(feature = "redis")]
pub use backend::redis::{RedisDb, RedisTransaction};
#[cfg(feature = "sqlite")]
pub use backend::sqlite::{SqliteDb, SqliteTransaction};

/// Everything you need in one import: `use memoire_orm::prelude::*;`.
pub mod prelude {
    #[cfg(feature = "json")]
    pub use crate::Json;
    #[cfg(feature = "memory")]
    pub use crate::MemoryDb;
    #[cfg(feature = "mongodb")]
    pub use crate::MongoDb;
    #[cfg(feature = "mysql")]
    pub use crate::MySqlPool;
    #[cfg(feature = "postgres")]
    pub use crate::PgPool;
    #[cfg(feature = "redis")]
    pub use crate::RedisDb;
    #[cfg(feature = "sqlite")]
    pub use crate::SqliteDb;
    pub use crate::{
        Database, Dialect, Error, Executor, Expr, FromRow, JoinKind, LoadRelated, Migration, Migrator, Model, Observed,
        Page, Result, Transaction, Value, count_all, params,
    };
}

/// Runs a future to completion on the current thread.
///
/// Handy for the memory and SQLite backends, whose futures never wait on I/O,
/// in synchronous code, tests, CLIs or WebAssembly. Use a real runtime (Tokio)
/// for PostgreSQL.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    thread_local! {
        static WAKER: Waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    }

    let mut future = std::pin::pin!(future);
    WAKER.with(|waker| {
        let mut cx = Context::from_waker(waker);
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::park(),
            }
        }
    })
}

/// Compiles the code examples of the crate README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[doc(hidden)]
pub mod __private {
    use crate::{Error, FromValue, Result, RowSource, Value};

    /// Decodes one column (used by the derive macros).
    #[inline]
    pub fn column<T: FromValue, R: RowSource + ?Sized>(row: &mut R, index: usize, label: &str) -> Result<T> {
        T::from_value(row.take(index)?).map_err(|e: Error| e.in_column(label))
    }

    pub use crate::lifecycle::is_timestamp_kind;

    /// Converts a value (used by the derive macros).
    #[inline]
    pub fn convert<T: FromValue>(value: Value, label: &str) -> Result<T> {
        T::from_value(value).map_err(|e: Error| e.in_column(label))
    }
}
