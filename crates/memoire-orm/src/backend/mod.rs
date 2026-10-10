//! Database backends.
//!
//! | Backend | Feature | Pure Rust | Notes |
//! |---|---|---|---|
//! | [`memory::MemoryDb`] | `memory` (default) | yes | also runs on `wasm32` |
//! | `sqlite::SqliteDb` | `sqlite` | driver in Rust, SQLite compiled in | embedded, zero config |
//! | `postgres::PgPool` | `postgres` | yes | `tokio-postgres` + pooling |
//! | `mysql::MySqlPool` | `mysql` | yes | `mysql_async`, MySQL 5.7+/8 and MariaDB 10.3+ |
//! | `redis::RedisDb` | `redis` | yes | NoSQL key-value store, rows as JSON |

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "mysql")]
pub mod mysql;

#[cfg(feature = "redis")]
pub mod redis;
