//! Database backends.
//!
//! | Backend | Feature | Pure Rust | Notes |
//! |---|---|---|---|
//! | [`memory::MemoryDb`] | `memory` (default) | yes | also runs on `wasm32` |
//! | `sqlite::SqliteDb` | `sqlite` | driver in Rust, SQLite compiled in | embedded, zero config |
//! | `postgres::PgPool` | `postgres` | yes | `tokio-postgres` + pooling |

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;
