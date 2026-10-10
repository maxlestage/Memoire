//! Pure-Rust in-memory storage engine.
//!
//! [`MemoryDb`] interprets Memoire statements directly, without SQL parsing:
//! primary-key and unique lookups use hash indexes, equi-joins are executed as
//! hash joins, `ORDER BY ... LIMIT` uses partial selection, and transactions use
//! copy-on-write snapshots with optimistic conflict detection.
//!
//! It is perfect for tests, prototypes, caches and WebAssembly (it compiles to
//! `wasm32-unknown-unknown`, no runtime required).

mod eval;
mod index;
mod scan;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::ast::{DeleteAst, InsertAst, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::expr::{AggFunc, BinOp, ColumnRef, Expr};
use crate::row::{FromRow, RowSource, ValueRow};
use crate::schema::{Dialect, TableSchema};
use crate::value::{ColumnType, Value};

use eval::{Ctx, Group, Key, Parts, Pred, Rows, Slot, eval, order_cmp, slot_of, truthy};
#[cfg(feature = "mongodb")]
pub(crate) use eval::{Tok as LikeTok, tokenize as like_tokens};
use index::Index;

type Map<K, V> = HashMap<K, V, foldhash::fast::RandomState>;
type Set<K> = HashSet<K, foldhash::fast::RandomState>;

/// Checks the width of a row and its `NOT NULL` constraints.
pub(crate) fn validate_row(schema: &TableSchema, row: &[Value]) -> Result<()> {
    if row.len() != schema.columns.len() {
        return Err(Error::Conversion(format!(
            "`{}` expects {} values, got {}",
            schema.name,
            schema.columns.len(),
            row.len()
        )));
    }
    for (c, v) in schema.columns.iter().zip(row) {
        if v.is_null() && !c.nullable && !c.auto_increment {
            return Err(Error::Constraint(format!(
                "NULL value in column `{}.{}` violates the NOT NULL constraint",
                schema.name, c.name
            )));
        }
    }
    Ok(())
}

/// Per-row state of a [`Mirror`].
pub(crate) const MIRROR_NULL: u8 = 0;
pub(crate) const MIRROR_VALUE: u8 = 1;
/// A value of another type than the column's: scans read it from the row.
pub(crate) const MIRROR_OTHER: u8 = 2;

/// A numeric column copied into a contiguous typed array, kept in step with
/// the rows: filters and sorts on it read 9 bytes per row instead of
/// following a pointer to each row (see `scan.rs`).
#[derive(Clone)]
pub(crate) struct Mirror {
    pub(crate) col: usize,
    pub(crate) data: MirrorData,
    /// One of `MIRROR_NULL`, `MIRROR_VALUE`, `MIRROR_OTHER` per row.
    pub(crate) state: Vec<u8>,
    /// Number of `MIRROR_OTHER` rows (scans take a branch-free loop when 0).
    pub(crate) others: usize,
}

#[derive(Clone)]
pub(crate) enum MirrorData {
    Int(Vec<i64>),
    Float(Vec<f64>),
}

impl Mirror {
    fn for_column(col: usize, kind: ColumnType) -> Option<Mirror> {
        let data = match kind {
            ColumnType::SmallInt | ColumnType::Int | ColumnType::BigInt => MirrorData::Int(Vec::new()),
            ColumnType::Real | ColumnType::Double => MirrorData::Float(Vec::new()),
            _ => return None,
        };
        Some(Mirror { col, data, state: Vec::new(), others: 0 })
    }

    fn push(&mut self, row: &[Value]) {
        let v = row.get(self.col).unwrap_or(&Value::Null);
        match &mut self.data {
            MirrorData::Int(d) => {
                let (x, st) = match *v {
                    Value::I32(x) => (i64::from(x), MIRROR_VALUE),
                    Value::I64(x) => (x, MIRROR_VALUE),
                    Value::Null => (0, MIRROR_NULL),
                    _ => (0, MIRROR_OTHER),
                };
                d.push(x);
                self.state.push(st);
                self.others += usize::from(st == MIRROR_OTHER);
            }
            MirrorData::Float(d) => {
                let (x, st) = match *v {
                    Value::F64(x) if !x.is_nan() => (x, MIRROR_VALUE),
                    Value::Null => (0.0, MIRROR_NULL),
                    _ => (0.0, MIRROR_OTHER),
                };
                d.push(x);
                self.state.push(st);
                self.others += usize::from(st == MIRROR_OTHER);
            }
        }
    }

    fn set(&mut self, idx: usize, row: &[Value]) {
        self.push(row);
        self.swap_remove(idx);
    }

    fn swap_remove(&mut self, idx: usize) {
        match &mut self.data {
            MirrorData::Int(d) => {
                d.swap_remove(idx);
            }
            MirrorData::Float(d) => {
                d.swap_remove(idx);
            }
        }
        self.others -= usize::from(self.state.swap_remove(idx) == MIRROR_OTHER);
    }

    fn rebuild(&mut self, rows: &[Vec<Value>]) {
        match &mut self.data {
            MirrorData::Int(d) => d.clear(),
            MirrorData::Float(d) => d.clear(),
        }
        self.state.clear();
        self.others = 0;
        for row in rows {
            self.push(row);
        }
    }
}

#[derive(Clone)]
pub(crate) struct Table {
    pub(crate) schema: &'static TableSchema,
    pub(crate) rows: Vec<Vec<Value>>,
    /// The primary key first, then `unique` columns, then `index` columns.
    indexes: Vec<Index>,
    /// Typed copies of the numeric columns.
    mirrors: Vec<Mirror>,
    pub(crate) next_id: i64,
    version: u64,
}

impl Table {
    /// Builds a table from existing rows (used by the Redis and MongoDB
    /// backends to evaluate queries on loaded data).
    #[cfg_attr(not(any(feature = "redis", feature = "mongodb")), allow(dead_code))]
    pub(crate) fn from_rows(schema: &'static TableSchema, rows: Vec<Vec<Value>>, last_id: i64) -> Self {
        let mut t = Table::new(schema, 0);
        // Built for one statement: the `index` columns' indexes would rarely pay
        // for themselves (scans are fast); keep those that enforce uniqueness.
        t.indexes.retain(|i| i.unique);
        t.rows = rows;
        t.rebuild_indexes();
        t.next_id = last_id.saturating_add(1).max(1);
        for i in 0..t.rows.len() {
            let key = t.rows[i][schema.primary_key].clone();
            t.bump_next_id(&key);
        }
        t
    }

    pub(crate) fn new(schema: &'static TableSchema, version: u64) -> Self {
        let mut indexes = vec![Index::new(schema.primary_key, true)];
        let others = |unique: bool| {
            schema
                .columns
                .iter()
                .enumerate()
                .filter(move |(i, c)| *i != schema.primary_key && c.unique == unique && (unique || c.indexed))
                .map(move |(i, _)| Index::new(i, unique))
        };
        indexes.extend(others(true));
        indexes.extend(others(false));
        Table {
            schema,
            rows: Vec::new(),
            indexes,
            mirrors: schema.columns.iter().enumerate().filter_map(|(i, c)| Mirror::for_column(i, c.kind)).collect(),
            next_id: 1,
            version,
        }
    }

    /// The typed copy of column `col`, if it is numeric.
    pub(crate) fn mirror(&self, col: usize) -> Option<&Mirror> {
        self.mirrors.iter().find(|m| m.col == col)
    }

    fn name(&self) -> &'static str {
        self.schema.name
    }

    fn violation(&self, col: usize) -> Error {
        Error::UniqueViolation { table: self.name().to_owned(), column: self.schema.columns[col].name.to_owned() }
    }

    fn index(&self, col: usize) -> Option<&Index> {
        self.indexes.iter().find(|i| i.col == col)
    }

    /// The row holding `value` in the primary key or a `unique` column.
    fn lookup(&self, col: usize, value: &Value) -> Option<usize> {
        self.index(col).filter(|i| i.unique).and_then(|i| i.find(&self.rows, value))
    }

    /// Every row holding `value` in an indexed column, in table order.
    fn lookup_all(&self, col: usize, value: &Value) -> Option<Vec<usize>> {
        self.index(col).map(|i| i.find_all(&self.rows, value))
    }

    fn has_index(&self, col: usize) -> bool {
        self.index(col).is_some()
    }

    fn check_row(&self, row: &[Value]) -> Result<()> {
        validate_row(self.schema, row)
    }

    fn index_row(&mut self, idx: usize) {
        for index in &mut self.indexes {
            index.insert(&self.rows, idx);
        }
    }

    fn unindex_row(&mut self, idx: usize) {
        for index in &mut self.indexes {
            index.remove(&self.rows, idx);
        }
    }

    fn rebuild_indexes(&mut self) {
        for m in &mut self.mirrors {
            m.rebuild(&self.rows);
        }
        for index in &mut self.indexes {
            index.clear();
        }
        for i in 0..self.rows.len() {
            self.index_row(i);
        }
    }

    fn bump_next_id(&mut self, key: &Value) {
        if let Some(k) = key.as_i64()
            && k >= self.next_id
        {
            self.next_id = k.saturating_add(1);
        }
    }

    /// Inserts a row; generates the key when `generate` is set.
    pub(crate) fn insert(&mut self, mut row: Vec<Value>, generate: bool) -> Result<Value> {
        let pk = self.schema.primary_key;
        if generate && self.schema.auto_increment() {
            row.get_mut(pk)
                .map(|v| *v = Value::I64(self.next_id))
                .ok_or_else(|| Error::Conversion(format!("`{}`: row has no primary key column", self.name())))?;
        }
        self.check_row(&row)?;
        // Each indexed value is hashed once: for the unique check, then to file it.
        const HASHED: usize = 8;
        let mut hashes = [None::<u64>; HASHED];
        for (k, index) in self.indexes.iter().enumerate() {
            let v = &row[index.col];
            if v.is_null() {
                continue;
            }
            let h = index.hash(v);
            if index.unique && index.find_hashed(&self.rows, v, h).is_some() {
                return Err(self.violation(index.col));
            }
            if k < HASHED {
                hashes[k] = Some(h);
            }
        }
        let key = row[pk].clone();
        self.bump_next_id(&key);
        let idx = self.rows.len();
        for m in &mut self.mirrors {
            m.push(&row);
        }
        self.rows.push(row);
        for (k, index) in self.indexes.iter_mut().enumerate() {
            match hashes.get(k) {
                Some(Some(h)) => index.insert_hashed(idx, *h),
                Some(None) => {} // NULL: not indexed
                None => index.insert(&self.rows, idx),
            }
        }
        Ok(key)
    }

    fn upsert(&mut self, row: Vec<Value>) -> Result<()> {
        self.check_row(&row)?;
        let pk = self.schema.primary_key;
        match self.lookup(pk, &row[pk]) {
            Some(idx) => self.replace_rows(vec![(idx, row)]).map(|_| ()),
            None => self.insert(row, false).map(|_| ()),
        }
    }

    /// Atomically replaces rows, validating unique constraints first. Only the
    /// indexes whose column changes are touched.
    fn replace_rows(&mut self, changes: Vec<(usize, Vec<Value>)>) -> Result<u64> {
        if changes.is_empty() {
            return Ok(0);
        }
        for (_, row) in &changes {
            self.check_row(row)?;
        }
        // Indexes whose column changes in at least one row.
        let touched: Vec<usize> = (0..self.indexes.len())
            .filter(|&k| {
                let col = self.indexes[k].col;
                changes.iter().any(|(i, row)| !eval::key_eq(&self.rows[*i][col], &row[col]))
            })
            .collect();
        if touched.iter().any(|&k| self.indexes[k].unique) {
            let changed: Set<usize> = changes.iter().map(|(i, _)| *i).collect();
            for &k in touched.iter().filter(|&&k| self.indexes[k].unique) {
                let col = self.indexes[k].col;
                let mut seen: Set<Key> = Set::default();
                for (_, row) in &changes {
                    let v = &row[col];
                    if v.is_null() {
                        continue;
                    }
                    if !seen.insert(Key(v.clone())) {
                        return Err(self.violation(col));
                    }
                    if let Some(other) = self.lookup(col, v)
                        && !changed.contains(&other)
                    {
                        return Err(self.violation(col));
                    }
                }
            }
        }
        for &k in &touched {
            for (idx, _) in &changes {
                self.indexes[k].remove(&self.rows, *idx);
            }
        }
        let pk = self.schema.primary_key;
        let n = changes.len() as u64;
        let mut written = Vec::with_capacity(if touched.is_empty() { 0 } else { changes.len() });
        for (idx, row) in changes {
            self.bump_next_id(&row[pk]);
            for m in &mut self.mirrors {
                m.set(idx, &row);
            }
            self.rows[idx] = row;
            if !touched.is_empty() {
                written.push(idx);
            }
        }
        for &k in &touched {
            for &idx in &written {
                self.indexes[k].insert(&self.rows, idx);
            }
        }
        Ok(n)
    }

    pub(crate) fn remove_rows(&mut self, mut idxs: Vec<usize>) -> u64 {
        if idxs.is_empty() {
            return 0;
        }
        idxs.sort_unstable();
        idxs.dedup();
        let n = idxs.len() as u64;
        if idxs.len() > 16 && idxs.len() * 4 > self.rows.len() {
            let mut kill = vec![false; self.rows.len()];
            for i in idxs {
                kill[i] = true;
            }
            let mut i = 0;
            self.rows.retain(|_| {
                let keep = !kill[i];
                i += 1;
                keep
            });
            self.rebuild_indexes();
        } else {
            for &idx in idxs.iter().rev() {
                self.unindex_row(idx);
                let last = self.rows.len() - 1;
                self.rows.swap_remove(idx);
                for m in &mut self.mirrors {
                    m.swap_remove(idx);
                }
                if idx != last {
                    // The former last row moved to `idx`.
                    for index in &mut self.indexes {
                        index.moved(&self.rows, last, idx);
                    }
                }
            }
        }
        n
    }
}

#[derive(Clone, Default)]
pub(crate) struct State {
    tables: Map<String, Arc<Table>>,
    clock: u64,
}

impl State {
    #[cfg_attr(not(any(feature = "redis", feature = "mongodb")), allow(dead_code))]
    pub(crate) fn put(&mut self, table: Table) {
        self.tables.insert(table.schema.name.to_owned(), Arc::new(table));
    }

    #[cfg_attr(not(any(feature = "redis", feature = "mongodb")), allow(dead_code))]
    pub(crate) fn get(&self, name: &str) -> Option<&Table> {
        self.tables.get(name).map(|t| &**t)
    }

    #[cfg_attr(not(feature = "redis"), allow(dead_code))]
    pub(crate) fn get_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(name).map(Arc::make_mut)
    }

    #[cfg_attr(not(feature = "redis"), allow(dead_code))]
    pub(crate) fn remove(&mut self, name: &str) -> bool {
        self.tables.remove(name).is_some()
    }

    fn table(&self, name: &str) -> Result<&Table> {
        self.tables.get(name).map(|t| &**t).ok_or_else(|| Error::NoSuchTable(name.to_owned()))
    }

    fn table_mut(&mut self, name: &str) -> Result<&mut Table> {
        self.clock += 1;
        let clock = self.clock;
        let t = self.tables.get_mut(name).ok_or_else(|| Error::NoSuchTable(name.to_owned()))?;
        let t = Arc::make_mut(t);
        t.version = clock;
        Ok(t)
    }
}

// ---------------------------------------------------------------------------
// SELECT
// ---------------------------------------------------------------------------

/// If `filter` pins an indexed column of the root table (primary key,
/// `unique` or `index`) to constants, returns the candidate row indexes.
fn index_candidates(filter: &Expr, table: &Table) -> Option<Vec<usize>> {
    let indexed_col = |e: &Expr| match e {
        Expr::Column(c) if c.table == table.name() && table.has_index(c.index) => Some(c.index),
        _ => None,
    };
    match filter {
        Expr::Binary { op: BinOp::Eq, left, right } => {
            let (col, v) = match (&**left, &**right) {
                (l, Expr::Value(v)) => (indexed_col(l)?, v),
                (Expr::Value(v), r) => (indexed_col(r)?, v),
                _ => return None,
            };
            table.lookup_all(col, v)
        }
        Expr::InList { expr, list, negated: false } => {
            let col = indexed_col(expr)?;
            let mut out: Vec<usize> = list.iter().flat_map(|v| table.lookup_all(col, v).unwrap_or_default()).collect();
            out.sort_unstable();
            out.dedup();
            Some(out)
        }
        Expr::Binary { op: BinOp::And, left, right } => {
            index_candidates(left, table).or_else(|| index_candidates(right, table))
        }
        _ => None,
    }
}

/// Recognises `joined.col = other.col` join conditions.
fn equi_join(on: &Expr, joined: &'static str, known: &[&'static str]) -> Option<(usize, ColumnRef)> {
    if let Expr::Binary { op: BinOp::Eq, left, right } = on
        && let (Expr::Column(a), Expr::Column(b)) = (&**left, &**right)
    {
        if a.table == joined && b.table != joined && known.contains(&b.table) {
            return Some((a.index, *b));
        }
        if b.table == joined && a.table != joined && known.contains(&a.table) {
            return Some((b.index, *a));
        }
    }
    None
}

/// Result of a `SELECT`: whole table rows borrowed from the store (decoded
/// straight into the caller's types, without an intermediate copy), or
/// computed values.
pub(crate) enum Selected<'a> {
    Tables {
        rows: Rows<'a>,
        /// Result rows, as positions in `rows`.
        order: Vec<usize>,
        /// `(slot in the row, width)` of each projected table.
        columns: Vec<(usize, usize)>,
    },
    Values(Vec<Vec<Value>>),
}

impl Selected<'_> {
    pub(crate) fn into_values(self) -> Vec<Vec<Value>> {
        match self {
            Selected::Values(v) => v,
            Selected::Tables { rows, order, columns } => {
                let width = columns.iter().map(|c| c.1).sum();
                order
                    .into_iter()
                    .map(|i| {
                        let parts = rows.get(i);
                        let mut out = Vec::with_capacity(width);
                        for &(slot, width) in &columns {
                            match parts[slot] {
                                Some(r) => out.extend_from_slice(r),
                                None => out.extend(std::iter::repeat_n(Value::Null, width)),
                            }
                        }
                        out
                    })
                    .collect()
            }
        }
    }

    pub(crate) fn decode<O: FromRow>(self) -> Result<Vec<O>> {
        match self {
            Selected::Values(v) => decode(v),
            Selected::Tables { rows, order, columns } => {
                order.into_iter().map(|i| O::from_row(&mut PartsRow::new(rows.get(i), &columns), 0)).collect()
            }
        }
    }
}

/// One result row made of borrowed table rows (`None`: the NULL side of a
/// `LEFT JOIN`); values are cloned only when the caller takes them.
struct PartsRow<'a, 'b> {
    parts: &'b Parts<'a>,
    columns: &'b [(usize, usize)],
    len: usize,
}

impl<'a, 'b> PartsRow<'a, 'b> {
    fn new(parts: &'b Parts<'a>, columns: &'b [(usize, usize)]) -> Self {
        PartsRow { parts, columns, len: columns.iter().map(|c| c.1).sum() }
    }

    /// The value at `index`; `None` for a missing (NULL) table row.
    #[inline]
    fn get(&self, mut index: usize) -> Option<&'a Value> {
        for &(slot, width) in self.columns {
            if index < width {
                return self.parts[slot].and_then(|r| r.get(index));
            }
            index -= width;
        }
        None
    }
}

impl RowSource for PartsRow<'_, '_> {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn take(&mut self, index: usize) -> Result<Value> {
        if index >= self.len {
            return Err(Error::Conversion(format!("column index {index} out of range ({} columns)", self.len)));
        }
        Ok(self.get(index).cloned().unwrap_or(Value::Null))
    }

    #[inline]
    fn is_null(&self, index: usize) -> bool {
        self.get(index).is_none_or(Value::is_null)
    }
}

/// `SELECT COUNT(*) FROM t [WHERE ...]`: counted without building rows.
fn is_count_all(ast: &SelectAst) -> bool {
    ast.joins.is_empty()
        && ast.group_by.is_empty()
        && ast.having.is_none()
        && !ast.distinct
        && matches!(&ast.projection, Projection::Exprs(e)
            if matches!(e.as_slice(), [Expr::Aggregate { func: AggFunc::Count, arg: None, distinct: false }]))
}

fn select<'a>(state: &'a State, ast: &SelectAst) -> Result<Selected<'a>> {
    let layout: Vec<&'static str> = ast.tables().map(|t| t.name).collect();
    for (i, name) in layout.iter().enumerate() {
        if layout[..i].contains(name) {
            return Err(Error::Unsupported(format!(
                "table `{name}` appears twice in the query (self-joins need table aliases)"
            )));
        }
    }
    let tables: Vec<&'a Table> = layout.iter().map(|n| state.table(n)).collect::<Result<_>>()?;
    let stride = layout.len();
    let root = tables[0];

    // Single table: the matching rows' indexes, for the sort fast path.
    let mut single: Option<Vec<u32>> = None;
    let rows = if ast.joins.is_empty() {
        // Single table: vectorised scan (scan.rs).
        if is_count_all(ast) {
            let n = scan::count(root, ast.filter.as_ref())?;
            let shown = ast.offset.unwrap_or(0) == 0 && ast.limit != Some(0);
            return Ok(Selected::Values(if shown { vec![vec![Value::I64(n as i64)]] } else { Vec::new() }));
        }
        let sel = scan::matching(root, ast.filter.as_ref())?;
        let mut rows = Rows::with_capacity(1, sel.len());
        rows.flat.extend(sel.iter().map(|&i| Some(root.rows[i as usize].as_slice())));
        single = Some(sel);
        rows
    } else {
        let pred = ast.filter.as_ref().map(|f| Pred::compile(f, &layout)).transpose()?;

        // 1. Root rows (index lookup when possible).
        let candidates = ast.filter.as_ref().and_then(|f| index_candidates(f, root));
        let root_rows: Box<dyn Iterator<Item = &[Value]>> = match &candidates {
            Some(idx) => Box::new(idx.iter().map(|&i| root.rows[i].as_slice())),
            None => Box::new(root.rows.iter().map(Vec::as_slice)),
        };
        let mut current = Rows::with_capacity(stride, root.rows.len());
        for r in root_rows {
            current.flat.push(Some(r));
            current.flat.extend(std::iter::repeat_n(None, stride - 1));
        }
        // 2. Joins.
        for (ji, join) in ast.joins.iter().enumerate() {
            let slot = ji + 1;
            let jt = tables[slot];
            let mut next = Rows::with_capacity(stride, current.len());
            fn emit<'a>(next: &mut Rows<'a>, base: &[Option<&'a [Value]>], slot: usize, row: Option<&'a [Value]>) {
                let start = next.flat.len();
                next.flat.extend_from_slice(base);
                next.flat[start + slot] = row;
            }
            if let Some((jcol, other)) = equi_join(&join.on, jt.name(), &layout[..slot]) {
                let other_slot = slot_of(&layout, other.table)?;
                // Hash join: build on the joined table (or reuse its index).
                let index = jt.index(jcol);
                let mut built: Map<Key, Vec<usize>> = Map::default();
                if index.is_none() {
                    for (i, r) in jt.rows.iter().enumerate() {
                        if !r[jcol].is_null() {
                            built.entry(Key(r[jcol].clone())).or_default().push(i);
                        }
                    }
                }
                for i in 0..current.len() {
                    let base = current.get(i);
                    let probe = base[other_slot].and_then(|r| r.get(other.index)).filter(|v| !v.is_null());
                    let mut matched = false;
                    if let Some(v) = probe {
                        if let Some(index) = index.filter(|i| i.unique) {
                            if let Some(m) = index.find(&jt.rows, v) {
                                emit(&mut next, base, slot, Some(&jt.rows[m]));
                                matched = true;
                            }
                        } else if let Some(index) = index {
                            let ms = index.find_all(&jt.rows, v);
                            for &m in &ms {
                                emit(&mut next, base, slot, Some(&jt.rows[m]));
                            }
                            matched = !ms.is_empty();
                        } else if let Some(ms) = built.get(&Key(v.clone())) {
                            for &m in ms {
                                emit(&mut next, base, slot, Some(&jt.rows[m]));
                            }
                            matched = !ms.is_empty();
                        }
                    }
                    if !matched && join.kind == JoinKind::Left {
                        emit(&mut next, base, slot, None);
                    }
                }
            } else {
                // Nested loop join.
                let on = Pred::compile(&join.on, &layout)?;
                let mut scratch: Vec<Option<&[Value]>> = vec![None; stride];
                for i in 0..current.len() {
                    let base = current.get(i);
                    let mut matched = false;
                    for r in &jt.rows {
                        scratch.copy_from_slice(base);
                        scratch[slot] = Some(r);
                        if on.passes(&scratch, &layout)? {
                            emit(&mut next, base, slot, Some(r));
                            matched = true;
                        }
                    }
                    if !matched && join.kind == JoinKind::Left {
                        emit(&mut next, base, slot, None);
                    }
                }
            }
            current = next;
        }

        // 3. WHERE.
        match &pred {
            Some(p) => {
                let mut rows = Rows::with_capacity(stride, 0);
                for i in 0..current.len() {
                    let parts = current.get(i);
                    if p.passes(parts, &layout)? {
                        rows.flat.extend_from_slice(parts);
                    }
                }
                rows
            }
            None => current,
        }
    };

    let aggregated = !ast.group_by.is_empty()
        || ast.having.is_some()
        || matches!(&ast.projection, Projection::Exprs(e) if e.iter().any(Expr::has_aggregate));

    if aggregated {
        return select_grouped(ast, &layout, &rows).map(Selected::Values);
    }

    // 4. ORDER BY / OFFSET / LIMIT / projection.
    let n = rows.len();
    let offset = ast.offset.unwrap_or(0).min(n as u64) as usize;
    let take = ast.limit.map_or(n, |l| l.min(n as u64) as usize);

    let mut order: Vec<usize> = (0..n).collect();
    if !ast.order.is_empty() {
        let need = offset.saturating_add(take);
        let partial = !ast.distinct && need < n && need > 0;
        let column_slots: Option<Vec<Slot>> = ast
            .order
            .iter()
            .map(|o| match &o.expr {
                Expr::Column(c) => slot_of(&layout, c.table).ok().map(|slot| Slot { slot, index: c.index }),
                _ => None,
            })
            .collect();
        let need_some = partial.then_some(need);
        let fast = match (column_slots.as_deref(), &single) {
            (Some(&[slot]), Some(sel)) => scan::sort_selection(root, sel, slot.index, &ast.order[0], need_some),
            (Some(&[slot]), None) => scan::sort_rows(&rows, slot, &ast.order[0], need_some),
            _ => None,
        };
        if let Some(sorted) = fast {
            // One plain column: keys extracted once, compared without dispatch.
            order = sorted;
        } else if let Some(slots) = column_slots {
            // Plain columns: compare in place, no copies.
            let cmp = |a: &usize, b: &usize| {
                let (pa, pb) = (rows.get(*a), rows.get(*b));
                for (s, o) in slots.iter().zip(&ast.order) {
                    let c = order_cmp(s.get(pa), s.get(pb), o);
                    if c != std::cmp::Ordering::Equal {
                        return c;
                    }
                }
                a.cmp(b)
            };
            if partial {
                order.select_nth_unstable_by(need - 1, cmp);
                order.truncate(need);
            }
            order.sort_unstable_by(cmp);
        } else {
            let mut keys: Vec<Vec<Value>> = Vec::with_capacity(n);
            for i in 0..n {
                let ctx = Ctx { layout: &layout, parts: rows.get(i), group: None };
                keys.push(ast.order.iter().map(|o| eval(&o.expr, &ctx).map(Cow::into_owned)).collect::<Result<_>>()?);
            }
            let cmp = |a: &usize, b: &usize| {
                for (k, o) in ast.order.iter().enumerate() {
                    let c = order_cmp(&keys[*a][k], &keys[*b][k], o);
                    if c != std::cmp::Ordering::Equal {
                        return c;
                    }
                }
                a.cmp(b)
            };
            if partial {
                order.select_nth_unstable_by(need - 1, cmp);
                order.truncate(need);
            }
            order.sort_unstable_by(cmp);
        }
    }

    let project = |parts: &[Option<&[Value]>]| -> Result<Vec<Value>> {
        match &ast.projection {
            Projection::Tables(ts) => {
                let mut out = Vec::with_capacity(ts.iter().map(|t| t.columns.len()).sum());
                for t in ts {
                    let slot = layout
                        .iter()
                        .position(|n| *n == t.name)
                        .ok_or_else(|| Error::Conversion(format!("table `{}` is not part of the query", t.name)))?;
                    match parts[slot] {
                        Some(r) => out.extend_from_slice(r),
                        None => out.extend(std::iter::repeat_n(Value::Null, t.columns.len())),
                    }
                }
                Ok(out)
            }
            Projection::Exprs(es) => {
                let ctx = Ctx { layout: &layout, parts, group: None };
                es.iter().map(|e| eval(e, &ctx).map(Cow::into_owned)).collect()
            }
        }
    };

    if let (Projection::Tables(ts), false) = (&ast.projection, ast.distinct) {
        let columns =
            ts.iter().map(|t| Ok((slot_of(&layout, t.name)?, t.columns.len()))).collect::<Result<Vec<_>>>()?;
        let order = order.into_iter().skip(offset).take(take).collect();
        return Ok(Selected::Tables { rows, order, columns });
    }

    if ast.distinct {
        let mut seen: Set<Vec<Key>> = Set::default();
        let mut out = Vec::new();
        for &i in &order {
            let row = project(rows.get(i))?;
            if seen.insert(row.iter().cloned().map(Key).collect()) {
                out.push(row);
            }
        }
        return Ok(Selected::Values(out.into_iter().skip(offset).take(take).collect()));
    }

    order.iter().skip(offset).take(take).map(|&i| project(rows.get(i))).collect::<Result<_>>().map(Selected::Values)
}

fn select_grouped(ast: &SelectAst, layout: &[&'static str], rows: &Rows<'_>) -> Result<Vec<Vec<Value>>> {
    // Group rows, keeping groups in first-seen order.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    if ast.group_by.is_empty() {
        groups.push((0..rows.len()).collect());
    } else {
        let mut by_key: Map<Vec<Key>, usize> = Map::default();
        for i in 0..rows.len() {
            let ctx = Ctx { layout, parts: rows.get(i), group: None };
            let key: Vec<Key> =
                ast.group_by.iter().map(|e| eval(e, &ctx).map(|v| Key(v.into_owned()))).collect::<Result<_>>()?;
            match by_key.get(&key) {
                Some(&g) => groups[g].push(i),
                None => {
                    by_key.insert(key, groups.len());
                    groups.push(vec![i]);
                }
            }
        }
    }

    let empty: Vec<Option<&[Value]>> = vec![None; rows.stride];
    let mut out: Vec<(Vec<Value>, Vec<Value>)> = Vec::with_capacity(groups.len());
    for members in &groups {
        let parts = members.first().map_or(empty.as_slice(), |&i| rows.get(i));
        let ctx = Ctx { layout, parts, group: Some(Group { rows, members }) };
        if let Some(h) = &ast.having
            && !truthy(&*eval(h, &ctx)?)
        {
            continue;
        }
        let values: Vec<Value> = match &ast.projection {
            Projection::Exprs(es) => es.iter().map(|e| eval(e, &ctx).map(Cow::into_owned)).collect::<Result<_>>()?,
            Projection::Tables(ts) => {
                let mut v = Vec::new();
                for t in ts {
                    let slot = layout.iter().position(|n| *n == t.name).unwrap_or(0);
                    match parts[slot] {
                        Some(r) => v.extend_from_slice(r),
                        None => v.extend(std::iter::repeat_n(Value::Null, t.columns.len())),
                    }
                }
                v
            }
        };
        let keys: Vec<Value> =
            ast.order.iter().map(|o| eval(&o.expr, &ctx).map(Cow::into_owned)).collect::<Result<_>>()?;
        out.push((keys, values));
    }

    if !ast.order.is_empty() {
        out.sort_by(|a, b| {
            for (k, o) in ast.order.iter().enumerate() {
                let c = order_cmp(&a.0[k], &b.0[k], o);
                if c != std::cmp::Ordering::Equal {
                    return c;
                }
            }
            std::cmp::Ordering::Equal
        });
    }
    let mut values: Vec<Vec<Value>> = out.into_iter().map(|(_, v)| v).collect();
    if ast.distinct {
        let mut seen: Set<Vec<Key>> = Set::default();
        values.retain(|row| seen.insert(row.iter().cloned().map(Key).collect()));
    }
    let offset = ast.offset.unwrap_or(0) as usize;
    let take = ast.limit.map_or(usize::MAX, |l| l as usize);
    Ok(values.into_iter().skip(offset).take(take).collect())
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

fn matching_rows(table: &Table, filter: Option<&Expr>) -> Result<Vec<usize>> {
    Ok(scan::matching(table, filter)?.into_iter().map(|i| i as usize).collect())
}

fn update(state: &mut State, u: &UpdateAst) -> Result<u64> {
    if u.sets.is_empty() {
        return Err(Error::Unsupported("UPDATE without any SET clause".into()));
    }
    let table = state.table(u.table.name)?;
    let idxs = matching_rows(table, u.filter.as_ref())?;
    if idxs.is_empty() {
        return Ok(0);
    }
    let layout = [table.name()];
    let mut changes = Vec::with_capacity(idxs.len());
    for i in idxs {
        let old = &table.rows[i];
        let parts = [Some(old.as_slice())];
        let ctx = Ctx { layout: &layout, parts: &parts, group: None };
        let mut new = old.clone();
        for (col, e) in &u.sets {
            let slot = new
                .get_mut(*col)
                .ok_or_else(|| Error::Conversion(format!("column index {col} out of range for `{}`", u.table.name)))?;
            *slot = eval(e, &ctx)?.into_owned();
        }
        changes.push((i, new));
    }
    state.table_mut(u.table.name)?.replace_rows(changes)
}

fn delete(state: &mut State, d: &DeleteAst) -> Result<u64> {
    let idxs = matching_rows(state.table(d.table.name)?, d.filter.as_ref())?;
    if idxs.is_empty() {
        return Ok(0);
    }
    Ok(state.table_mut(d.table.name)?.remove_rows(idxs))
}

pub(crate) fn insert_many(state: &mut State, ins: &InsertAst) -> Result<u64> {
    insert_all(state.table_mut(ins.table.name)?, ins.rows.iter().cloned())
}

/// [`insert_many`] with rows given by value: stored without a copy.
pub(crate) fn insert_owned(state: &mut State, table: &TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
    insert_all(state.table_mut(table.name)?, rows.into_iter())
}

/// Inserts rows, all or nothing.
fn insert_all(t: &mut Table, rows: impl ExactSizeIterator<Item = Vec<Value>>) -> Result<u64> {
    let n = rows.len();
    let snapshot_len = t.rows.len();
    let snapshot_next = t.next_id;
    t.rows.reserve(n);
    for row in rows {
        if let Err(e) = t.insert(row, true) {
            // Roll back the rows inserted by this statement.
            let added: Vec<usize> = (snapshot_len..t.rows.len()).collect();
            t.remove_rows(added);
            t.next_id = snapshot_next;
            return Err(e);
        }
    }
    Ok(n as u64)
}

/// Executes a write statement. Returns the result and the written table.
pub(crate) fn execute(state: &mut State, q: &Query<'_>) -> Result<(ExecResult, Option<String>)> {
    let done =
        |n: u64, table: &str| Ok((ExecResult { rows_affected: n, last_insert_id: None }, Some(table.to_owned())));
    match *q {
        Query::Insert(ins) => {
            let n = insert_many(state, ins)?;
            done(n, ins.table.name)
        }
        Query::Update(u) => {
            let n = update(state, u)?;
            done(n, u.table.name)
        }
        Query::Delete(d) => {
            let n = delete(state, d)?;
            done(n, d.table.name)
        }
        Query::InsertRow { table, values } => {
            let key = state.table_mut(table.name)?.insert(values.to_vec(), true)?;
            Ok((ExecResult { rows_affected: 1, last_insert_id: Some(key) }, Some(table.name.to_owned())))
        }
        Query::UpsertRow { table, values } => {
            state.table_mut(table.name)?.upsert(values.to_vec())?;
            done(1, table.name)
        }
        Query::UpdateRow { table, values } => {
            let t = state.table(table.name)?;
            if values.len() != table.columns.len() {
                return Err(Error::Conversion(format!("`{}` expects {} values", table.name, table.columns.len())));
            }
            match t.lookup(table.primary_key, &values[table.primary_key]) {
                Some(idx) => {
                    let n = state.table_mut(table.name)?.replace_rows(vec![(idx, values.to_vec())])?;
                    done(n, table.name)
                }
                None => Ok((ExecResult::default(), None)),
            }
        }
        Query::DeleteByKey { table, key } => {
            let t = state.table(table.name)?;
            match t.lookup(table.primary_key, key) {
                Some(idx) => {
                    let n = state.table_mut(table.name)?.remove_rows(vec![idx]);
                    done(n, table.name)
                }
                None => Ok((ExecResult::default(), None)),
            }
        }
        Query::CreateTable { table, if_not_exists } => {
            if state.tables.contains_key(table.name) {
                if if_not_exists {
                    return Ok((ExecResult::default(), None));
                }
                return Err(Error::TableExists(table.name.to_owned()));
            }
            state.clock += 1;
            state.tables.insert(table.name.to_owned(), Arc::new(Table::new(table, state.clock)));
            done(0, table.name)
        }
        Query::DropTable { name, if_exists } => {
            if state.tables.remove(name).is_none() && !if_exists {
                return Err(Error::NoSuchTable(name.to_owned()));
            }
            state.clock += 1;
            done(0, name)
        }
        Query::Raw { .. } | Query::Script { .. } => {
            Err(Error::Unsupported("raw SQL is not supported by the memory engine".into()))
        }
        Query::Select(_) | Query::FindByKey { .. } => {
            Err(Error::Unsupported("use `fetch_all` for statements returning rows".into()))
        }
    }
}

pub(crate) fn fetch(state: &State, q: &Query<'_>) -> Result<Vec<Vec<Value>>> {
    match *q {
        Query::Select(s) => Ok(select(state, s)?.into_values()),
        Query::FindByKey { table, key } => {
            let t = state.table(table.name)?;
            Ok(t.lookup(table.primary_key, key).map(|i| t.rows[i].clone()).into_iter().collect())
        }
        Query::Raw { .. } | Query::Script { .. } => {
            Err(Error::Unsupported("raw SQL is not supported by the memory engine".into()))
        }
        _ => Err(Error::Unsupported("statement does not return rows".into())),
    }
}

/// [`fetch`] decoded into `O`, straight from the stored rows when whole rows
/// are selected.
pub(crate) fn fetch_decode<O: FromRow>(state: &State, q: &Query<'_>) -> Result<Vec<O>> {
    match *q {
        Query::Select(s) => select(state, s)?.decode(),
        Query::FindByKey { table, key } => {
            let t = state.table(table.name)?;
            let Some(i) = t.lookup(table.primary_key, key) else {
                return Ok(Vec::new());
            };
            let parts = [Some(t.rows[i].as_slice())];
            let columns = [(0, t.schema.columns.len())];
            Ok(vec![O::from_row(&mut PartsRow::new(&parts, &columns), 0)?])
        }
        _ => decode(fetch(state, q)?),
    }
}

pub(crate) fn decode<O: FromRow>(rows: Vec<Vec<Value>>) -> Result<Vec<O>> {
    rows.into_iter().map(|r| O::from_row(&mut ValueRow(r), 0)).collect()
}

// ---------------------------------------------------------------------------
// Public handles
// ---------------------------------------------------------------------------

/// A thread-safe in-memory database. Cloning is cheap and shares the data.
///
/// ```
/// use memoire_orm::prelude::*;
///
/// #[derive(Model, Debug)]
/// struct Note { id: i64, text: String }
///
/// # memoire_orm::block_on(async {
/// let db = MemoryDb::new();
/// db.create_table::<Note>().await?;
/// Note { id: 0, text: "hello".into() }.insert(&db).await?;
/// assert_eq!(Note::count(&db).await?, 1);
/// # Ok::<(), memoire_orm::Error>(())
/// # }).unwrap();
/// ```
#[derive(Clone, Default)]
pub struct MemoryDb {
    shared: Arc<RwLock<State>>,
}

impl fmt::Debug for MemoryDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let st = self.shared.read();
        let mut names: Vec<_> = st.tables.iter().map(|(n, t)| (n.clone(), t.rows.len())).collect();
        names.sort();
        f.debug_struct("MemoryDb").field("tables", &names).finish()
    }
}

impl MemoryDb {
    /// Creates an empty database.
    pub fn new() -> Self {
        Self::default()
    }

    /// Names of the existing tables (sorted).
    pub fn table_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.shared.read().tables.keys().cloned().collect();
        v.sort();
        v
    }

    /// Number of rows in a table.
    pub fn row_count(&self, table: &str) -> Result<usize> {
        Ok(self.shared.read().table(table)?.rows.len())
    }
}

impl Executor for MemoryDb {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        fetch_decode(&self.shared.read(), &query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        execute(&mut self.shared.write(), &query).map(|(r, _)| r)
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        insert_owned(&mut self.shared.write(), table, rows)
    }
}

impl Database for MemoryDb {
    type Tx<'c> = MemoryTransaction;

    async fn begin(&self) -> Result<MemoryTransaction> {
        let snapshot = self.shared.read().clone();
        let base = snapshot.tables.iter().map(|(n, t)| (n.clone(), t.version)).collect();
        Ok(MemoryTransaction {
            shared: Arc::clone(&self.shared),
            base,
            local: Mutex::new(snapshot),
            dirty: Mutex::new(Set::default()),
        })
    }
}

/// A transaction on a [`MemoryDb`].
///
/// It works on a copy-on-write snapshot taken by `begin()`: its reads are
/// isolated from concurrent writers and its writes are invisible until
/// [`commit`](Transaction::commit). Committing fails with [`Error::Conflict`] if
/// another writer modified one of the tables this transaction wrote; retrying the
/// whole transaction is then safe. Dropping it without committing discards it.
pub struct MemoryTransaction {
    shared: Arc<RwLock<State>>,
    base: Map<String, u64>,
    local: Mutex<State>,
    dirty: Mutex<Set<String>>,
}

impl fmt::Debug for MemoryTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryTransaction").field("dirty", &*self.dirty.lock()).finish()
    }
}

impl Executor for MemoryTransaction {
    fn dialect(&self) -> Option<Dialect> {
        None
    }

    async fn fetch_all<O>(&self, query: Query<'_>) -> Result<Vec<O>>
    where
        O: FromRow + Send,
    {
        fetch_decode(&self.local.lock(), &query)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let (res, table) = execute(&mut self.local.lock(), &query)?;
        if let Some(t) = table {
            self.dirty.lock().insert(t);
        }
        Ok(res)
    }

    async fn insert_rows(&self, table: &'static TableSchema, rows: Vec<Vec<Value>>) -> Result<u64> {
        let n = insert_owned(&mut self.local.lock(), table, rows)?;
        self.dirty.lock().insert(table.name.to_owned());
        Ok(n)
    }
}

impl Transaction for MemoryTransaction {
    async fn commit(self) -> Result<()> {
        let dirty = self.dirty.into_inner();
        if dirty.is_empty() {
            return Ok(());
        }
        let mut local = self.local.into_inner();
        let mut db = self.shared.write();
        for name in &dirty {
            let now = db.tables.get(name).map(|t| t.version);
            if now != self.base.get(name).copied() {
                return Err(Error::Conflict(name.clone()));
            }
        }
        db.clock = db.clock.max(local.clock);
        for name in dirty {
            match local.tables.remove(&name) {
                Some(mut t) => {
                    // Re-stamp with a fresh version so concurrent snapshots conflict.
                    db.clock += 1;
                    Arc::make_mut(&mut t).version = db.clock;
                    db.tables.insert(name, t);
                }
                None => {
                    db.tables.remove(&name);
                }
            }
        }
        db.clock += 1;
        Ok(())
    }

    fn rollback(self) -> impl Future<Output = Result<()>> + Send {
        std::future::ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::eval::key_eq;
    use super::*;
    use crate::prelude::*;

    #[derive(Model, Debug, Clone)]
    #[memoire(table = "consistency")]
    struct C {
        id: i64,
        #[memoire(unique)]
        code: Option<String>,
        #[memoire(index)]
        group: Option<i32>,
        weight: Option<f64>,
    }

    /// Indexes and typed copies must always describe the rows exactly.
    fn assert_consistent(t: &Table) {
        for index in &t.indexes {
            let mut values: Vec<&Value> = t.rows.iter().map(|r| &r[index.col]).filter(|v| !v.is_null()).collect();
            values.push(&Value::I32(-1));
            for v in values {
                let expected: Vec<usize> = (0..t.rows.len()).filter(|&i| key_eq(&t.rows[i][index.col], v)).collect();
                assert_eq!(index.find_all(&t.rows, v), expected, "column {} value {v:?}", index.col);
            }
        }
        for m in &t.mirrors {
            let mut fresh = m.clone();
            fresh.rebuild(&t.rows);
            assert_eq!(fresh.state, m.state, "mirror {} states", m.col);
            match (&fresh.data, &m.data) {
                (MirrorData::Int(a), MirrorData::Int(b)) => assert_eq!(a, b),
                (MirrorData::Float(a), MirrorData::Float(b)) => assert_eq!(a, b),
                _ => panic!("mirror kind changed"),
            }
        }
    }

    #[test]
    fn indexes_and_mirrors_follow_every_change() {
        let mut t = Table::new(C::SCHEMA, 0);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let row = |id: i64, r: &mut dyn FnMut(u64) -> u64| {
            vec![
                Value::I64(id),
                if r(4) == 0 { Value::Null } else { Value::Text(format!("c{}", r(400))) },
                if r(5) == 0 { Value::Null } else { Value::I32(r(6) as i32) },
                if r(5) == 0 { Value::Null } else { Value::F64(r(100) as f64 / 4.0) },
            ]
        };
        for step in 0..1500 {
            match rnd(10) {
                0..=4 => {
                    let r = row(0, &mut rnd);
                    let _ = t.insert(r, true); // may hit the unique `code`
                }
                5 | 6 if !t.rows.is_empty() => {
                    let i = rnd(t.rows.len() as u64) as usize;
                    let id = t.rows[i][0].clone();
                    let mut r = row(0, &mut rnd);
                    r[0] = id;
                    let _ = t.replace_rows(vec![(i, r)]);
                }
                7 | 8 if !t.rows.is_empty() => {
                    let k = 1 + rnd(3) as usize;
                    let idxs = (0..k).map(|_| rnd(t.rows.len() as u64) as usize).collect();
                    t.remove_rows(idxs);
                }
                9 if t.rows.len() > 40 => {
                    // Large delete: the retain + rebuild path.
                    let idxs = (0..t.rows.len()).filter(|i| i % 3 == 0).collect();
                    t.remove_rows(idxs);
                }
                _ => {}
            }
            if step % 25 == 0 {
                assert_consistent(&t);
            }
        }
        assert_consistent(&t);
        assert!(t.rows.len() > 10);
    }
}
