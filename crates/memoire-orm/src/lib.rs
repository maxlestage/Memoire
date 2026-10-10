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
//! * **Several backends, one API** — [`MemoryDb`] (pure Rust, runs in WebAssembly),
//!   `SqliteDb` (feature `sqlite`) and `PgPool` (feature `postgres`).
//! * **Transactions, migrations, joins, aggregates, pagination, upserts.**
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
//! let db = MemoryDb::new(); // or SqliteDb::open("app.db")?, PgPool::connect(url).await?
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
//! | `memory` | yes | In-memory engine ([`MemoryDb`]) |
//! | `sqlite` | | SQLite backend (`SqliteDb`) |
//! | `postgres` | | PostgreSQL backend (`PgPool`, requires Tokio) |
//! | `postgres-tls` | | TLS for PostgreSQL (rustls, honours `sslmode`) |
//! | `mysql` | | MySQL / MariaDB backend (`MySqlPool`, requires Tokio) |
//! | `redis` | | Redis backend (`RedisDb`, requires Tokio) |
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

pub mod ast;
pub mod backend;
mod error;
mod executor;
mod expr;
pub mod migrate;
mod model;
mod query;
mod row;
mod schema;
pub mod sql;
mod value;

pub use ast::{JoinKind, Query};
pub use error::{Error, Result};
pub use executor::{Database, ExecResult, Executor, Transaction};
pub use expr::{AggFunc, BinOp, Column, ColumnRef, Expr, Nulls, Numeric, Order, count_all, escape_like};
pub use migrate::{Migration, Migrator};
pub use model::Model;
pub use query::{ColumnList, Delete, Joined, Page, Select, SumType, Update};
pub use row::{FromRow, RowSource, ValueRow};
pub use schema::{ColumnDef, Dialect, ForeignKey, StaticSql, TableSchema};
#[cfg(feature = "json")]
pub use value::Json;
pub use value::{Bind, ColumnType, FromValue, IntoValue, SqlType, ToValue, Value};

/// Derive macros. `#[derive(Model)]` implements [`Model`](trait@Model) and
/// [`FromRow`](trait@FromRow); `#[derive(FromRow)]` only the latter.
pub use memoire_orm_derive::{FromRow, Model};

#[cfg(feature = "memory")]
pub use backend::memory::{MemoryDb, MemoryTransaction};
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
        Database, Dialect, Error, Executor, Expr, FromRow, JoinKind, Migration, Migrator, Model, Page, Result,
        Transaction, Value, count_all, params,
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

    /// Converts a value (used by the derive macros).
    #[inline]
    pub fn convert<T: FromValue>(value: Value, label: &str) -> Result<T> {
        T::from_value(value).map_err(|e: Error| e.in_column(label))
    }
}
