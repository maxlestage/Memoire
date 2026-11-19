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

/// [`fetch`], consuming the tables: when the statement returns whole rows of
/// one table (`SELECT *` from a single table, with any filter, order, limit and
/// offset, or a lookup by primary key), the rows are moved out of the tables
/// instead of copied. The same rows, in the same order, as [`fetch`] (which
/// is what runs for any other statement). Meant for rows loaded for one
/// statement, then dropped: what an engine filtering chunks of its own rows
/// does. It saves the copy of each row returned, so it pays when many rows
/// come back (a filter); for a few (`ORDER BY ... LIMIT 20` of a chunk) it
/// is no faster than [`fetch`]. Tables shared with a clone keep their rows:
/// those are copied.
///
/// ```
/// use memoire_orm::evaluator::{self, Tables};
/// use memoire_orm::prelude::*;
/// use memoire_orm::Query;
///
/// #[derive(Model, Debug, PartialEq)]
/// struct Note { #[memoire(primary_key)] id: i64, text: String }
///
/// let rows = (1..=4).map(|i| vec![Value::I64(i), Value::Text(format!("n{i}"))]).collect();
/// let mut tables = Tables::new();
/// tables.insert(Note::SCHEMA, rows, 0);
/// let ast = Note::query().filter(Note::ID.gt(2_i64)).into_ast();
/// let expected = evaluator::fetch(&tables, Query::Select(&ast)).unwrap();
/// let rows = evaluator::fetch_owned(tables, Query::Select(&ast)).unwrap();
/// assert_eq!(rows, expected);
/// assert_eq!(rows[0][1], Value::Text("n3".into()));
/// ```
pub fn fetch_owned(tables: Tables, query: Query<'_>) -> Result<Vec<Vec<Value>>> {
    mem::fetch_owned(tables.state, &query)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::{JoinKind, Order};

    #[derive(Model, Debug, Clone)]
    #[memoire(table = "owned_items")]
    struct Item {
        id: i64,
        #[memoire(unique)]
        code: Option<String>,
        group: Option<i32>,
        weight: Option<f64>,
    }

    #[derive(Model, Debug, Clone)]
    #[memoire(table = "owned_tags")]
    struct Tag {
        id: i64,
        item_id: i64,
        label: String,
    }

    /// Rows or error, as text (`NaN` is not equal to itself).
    fn show(r: Result<Vec<Vec<Value>>>) -> String {
        format!("{:?}", r.map_err(|e| e.to_string()))
    }

    /// `fetch_owned` answers exactly what `fetch` answers (rows and order, or
    /// the same error), whether the rows can be moved out or not: filters,
    /// orders, limits, offsets, `DISTINCT`, projections, joins, aggregates,
    /// lookups by key, and tables shared with a clone.
    #[test]
    fn owned_rows_are_the_rows_fetch_returns() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for round in 0..300u64 {
            let n = rnd(60) as i64;
            let items: Vec<Vec<Value>> = (1..=n)
                .map(|id| {
                    vec![
                        Value::I64(id),
                        if rnd(4) == 0 { Value::Null } else { Value::Text(format!("c{}", rnd(1000))) },
                        if rnd(5) == 0 { Value::Null } else { Value::I32(rnd(6) as i32) },
                        match rnd(8) {
                            0 => Value::Null,
                            1 => Value::I64(3), // another type than the column's
                            2 => Value::F64(f64::NAN),
                            _ => Value::F64(rnd(40) as f64 / 4.0),
                        },
                    ]
                })
                .collect();
            let tags: Vec<Vec<Value>> = (1..=rnd(30) as i64)
                .map(|id| vec![Value::I64(id), Value::I64(rnd(n as u64 + 3) as i64), Value::Text(format!("t{id}"))])
                .collect();
            let with_tags = rnd(2) == 0;
            let load = || {
                let mut t = Tables::new();
                t.insert(Item::SCHEMA, items.clone(), 0);
                if with_tags {
                    t.insert(Tag::SCHEMA, tags.clone(), 0);
                }
                t
            };
            let tables = load();

            let mut q = Item::query();
            q = match rnd(6) {
                0 => q,
                1 => q.filter(Item::GROUP.ge(rnd(6) as i32)),
                2 => q.filter(Item::WEIGHT.lt(rnd(10) as f64) | Item::CODE.is_null()),
                3 => q.filter(Item::CODE.contains(format!("{}", rnd(10)))),
                4 => q.filter(Item::ID.eq(rnd(n as u64 + 2) as i64)),
                _ => q.filter(Item::GROUP.between(1, 3) & Item::WEIGHT.gt(1.0)),
            };
            let order: Option<Order> = match rnd(6) {
                0 => Some(Item::WEIGHT.desc()),
                1 => Some(Item::CODE.asc()),
                2 => Some(Item::GROUP.asc()),
                3 => Some(Item::ID.desc()),
                _ => None,
            };
            if let Some(o) = order {
                q = q.order_by(o);
                if rnd(3) == 0 {
                    q = q.order_by(Item::ID.asc());
                }
            }
            if rnd(2) == 0 {
                q = q.limit(rnd(12));
            }
            if rnd(3) == 0 {
                q = q.offset(rnd(8));
            }
            if rnd(6) == 0 {
                q = q.distinct();
            }
            if rnd(6) == 0 {
                q = q.join::<Tag>(JoinKind::Inner, Tag::ITEM_ID.eq_col(Item::ID));
            }
            let mut ast = q.into_ast();
            match rnd(8) {
                0 => ast.projection = crate::ast::Projection::Exprs(vec![Item::CODE.into(), Item::ID.into()]),
                1 => ast.projection = crate::ast::Projection::Tables(vec![Item::SCHEMA, Item::SCHEMA]),
                _ => {}
            }
            let key = Value::I64(rnd(n as u64 + 2) as i64);
            let queries = [Query::Select(&ast), Query::FindByKey { table: Item::SCHEMA, key: &key }];
            for query in queries {
                let expected = show(fetch(&tables, query));
                // Moved out of tables nothing else holds.
                assert_eq!(show(fetch_owned(load(), query)), expected, "round {round}: {query:?}");
                // Copied out of tables shared with a clone, which keeps its rows.
                assert_eq!(show(fetch_owned(tables.clone(), query)), expected, "round {round}, shared: {query:?}");
                assert_eq!(show(Ok(tables.rows(Item::SCHEMA.name).unwrap().to_vec())), show(Ok(items.clone())));
            }
        }
    }

    /// A table that is not loaded is the same error.
    #[test]
    fn owned_rows_of_a_missing_table() {
        let ast = Item::query().into_ast();
        let a = fetch(&Tables::new(), Query::Select(&ast));
        assert!(a.is_err());
        assert_eq!(show(a), show(fetch_owned(Tables::new(), Query::Select(&ast))));
    }
}
