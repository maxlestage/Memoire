//! Browser storage (feature `web`): application state and databases kept in
//! `localStorage`.
//!
//! * [`Store`] — a state store in the spirit of [Pinia](https://pinia.vuejs.org/):
//!   one store per id, getters, actions, `patch`, `reset`, `subscribe`, saved
//!   on every change, restored on the next visit, in step across tabs.
//! * [`LocalStorageDb`] — a complete Memoire database (models, queries, joins,
//!   transactions, indexes, full-text search) whose rows live in
//!   `localStorage`, one readable JSON value per row.
//!
//! Both work on any [`KeyValueStorage`]: the browser's [`LocalStorage`] and
//! [`SessionStorage`] on wasm32, or [`MemoryStorage`] for tests, server-side
//! rendering and native programs.

mod db;
mod local;
mod storage;
mod store;

pub use db::LocalStorageDb;
pub use local::LocalSubscription;
pub use storage::{KeyValueStorage, LocalStorage, MemoryStorage, OnChange, SessionStorage, Watch};
pub use store::{Store, StoreBuilder, StoreState, StoreSubscription};
