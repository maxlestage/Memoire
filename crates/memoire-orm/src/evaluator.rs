//! Memoire's in-memory query evaluator, over rows you supply (feature
//! `memory`).
//!
//! Storage engines that keep their data elsewhere (on disk, in another
//! service) load the rows a statement needs into [`Tables`] and let Memoire
//! evaluate it there, with exactly the semantics of
//! [`MemoryDb`](crate::MemoryDb): filters, joins, grouping, ordering,
//! full-text search, unique checks, generated keys. The Redis and MongoDB
//! backends work this way, and so does the Nuage DB engine.
//!
//! ```
//! use memoire_orm::evaluator::{self, Tables};
//! use memoire_orm::prelude::*;
//! use memoire_orm::Query;
//!
//! #[derive(Model, Debug, PartialEq)]
//! struct Note { #[memoire(primary_key, auto_increment)] id: i64, text: String }
//!
//! let mut tables = Tables::new();
//! tables.insert(Note::SCHEMA, vec![vec![Value::I64(1), Value::Text("a".into())]], 1);
//! let n = Note { id: 0, text: "b".into() };
//! let values = n.to_values();
//! let res = evaluator::execute(&mut tables, Query::InsertRow { table: Note::SCHEMA, values: &values }).unwrap();
//! assert_eq!(res.last_insert_id, Some(Value::I64(2)));
//! assert_eq!(tables.rows(Note::SCHEMA.name).unwrap().len(), 2);
//! ```

use crate::ast::Query;
use crate::backend::memory::persist::{Dec, Enc};
use crate::backend::memory::{self as mem, State, Table};
use crate::error::Result;
use crate::executor::ExecResult;
use crate::row::FromRow;
use crate::schema::TableSchema;
use crate::value::Value;

/// Tables loaded for one statement.
#[derive(Clone, Default)]
pub struct Tables {
    state: State,
}

impl std::fmt::Debug for Tables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tables").finish_non_exhaustive()
    }
}

impl Tables {
    /// No table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) a table holding `rows`, in schema order. Generated
    /// keys continue after `last_id` (and after the largest integer key).
    pub fn insert(&mut self, schema: &'static TableSchema, rows: Vec<Vec<Value>>, last_id: i64) {
        self.state.put(Table::from_rows(schema, rows, last_id));
    }

    /// `true` when the table is loaded.
    pub fn contains(&self, table: &str) -> bool {
        self.state.get(table).is_some()
    }

    /// The rows of a table, in table order.
    pub fn rows(&self, table: &str) -> Option<&[Vec<Value>]> {
        self.state.get(table).map(|t| t.rows.as_slice())
    }

    /// The last key generated (or inserted) in a table.
    pub fn last_id(&self, table: &str) -> Option<i64> {
        self.state.get(table).map(|t| t.next_id - 1)
    }
}

/// Runs a statement returning rows.
pub fn fetch(tables: &Tables, query: Query<'_>) -> Result<Vec<Vec<Value>>> {
    mem::fetch(&tables.state, &query)
}

/// [`fetch`], decoded into `O`.
pub fn fetch_as<O: FromRow>(tables: &Tables, query: Query<'_>) -> Result<Vec<O>> {
    mem::fetch_decode(&tables.state, &query)
}

/// Runs a write statement on the loaded tables.
pub fn execute(tables: &mut Tables, query: Query<'_>) -> Result<ExecResult> {
    Ok(mem::execute(&mut tables.state, &query)?.0)
}

/// Checks the width of a row and its `NOT NULL` constraints.
pub fn validate_row(schema: &TableSchema, row: &[Value]) -> Result<()> {
    mem::validate_row(schema, row)
}

/// Appends the compact binary form of a value (the format of `MemoryDb`
/// files).
pub fn encode_value(out: &mut Vec<u8>, value: &Value) {
    let mut e = Enc(std::mem::take(out));
    e.value(value);
    *out = e.0;
}

/// Reads a value written by [`encode_value`] and advances `input` past it.
pub fn decode_value(input: &mut &[u8]) -> Result<Value> {
    let mut d = Dec(input);
    let v = d.value()?;
    *input = d.0;
    Ok(v)
}

/// CRC-32 (IEEE) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    crate::backend::memory::persist::crc32(data)
}
