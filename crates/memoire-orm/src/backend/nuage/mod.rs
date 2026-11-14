//! Nuage DB: its protocol (feature `nuage-wire`) and its client (feature `nuage`).
//!
//! Nuage DB is Memoire's own database server, written in Rust (the `nuage`
//! crate): data on disk with a write-ahead log, copy-on-write branches,
//! backups and a management console. Its client, `NuageDb` (feature `nuage`), talks to it
//! over HTTP(S): it sends Memoire statements (their AST, as JSON, see [`wire`]) and
//! receives rows, so models, queries, relations, transactions and migrations work
//! exactly as with the other backends. It runs natively (Tokio) and in
//! WebAssembly, through the browser's `fetch`.
//!
//! The protocol alone ([`wire`], [`Branch`], [`NuageError`]: feature `nuage-wire`) is pure
//! Rust, with no network dependency: the `nuage` crate's embedded engine uses it without
//! the client's HTTP and TLS stack.
//!
//! The same database can run inside the application, without a server:
//! `nuage::Embedded::open("data/")` in the `nuage` crate, which also offers
//! `nuage::Db::from_env()` to pick the embedded engine or this client from
//! configuration.

use serde_json::{Value as Json, json};

use crate::error::{Error, Result};

mod sha256;
pub mod wire;

#[cfg(feature = "nuage")]
mod client;
#[cfg(feature = "nuage")]
pub use client::{BatchResult, NuageBatch, NuageBuilder, NuageDb, NuageTransaction};

pub use wire::NuageError;

/// Header naming the branch a request runs on.
pub const BRANCH_HEADER: &str = "Nuage-Branch";
/// Header naming the interactive transaction a request belongs to.
pub const TRANSACTION_HEADER: &str = "Nuage-Transaction";

/// A branch: a copy-on-write copy of the database, for a preview or a test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Name of the branch.
    pub name: String,
    /// The branch it was made from, `None` for the main database.
    pub parent: Option<String>,
    /// Creation time, Unix time in milliseconds.
    pub created_at: i64,
    /// Bytes written by the branch itself (what it does not share).
    pub own_bytes: u64,
}

impl Branch {
    /// The branch as JSON (the protocol's form).
    pub fn to_json(&self) -> Json {
        json!({ "name": self.name, "parent": self.parent, "createdAt": self.created_at, "ownBytes": self.own_bytes })
    }

    /// Reads a branch from JSON.
    pub fn from_json(j: &Json) -> Result<Branch> {
        Ok(Branch {
            name: j
                .get("name")
                .and_then(Json::as_str)
                .ok_or_else(|| Error::Conversion("invalid branch".into()))?
                .to_owned(),
            parent: j.get("parent").and_then(Json::as_str).map(str::to_owned),
            created_at: j.get("createdAt").and_then(Json::as_i64).unwrap_or(0),
            own_bytes: j.get("ownBytes").and_then(Json::as_u64).unwrap_or(0),
        })
    }
}
