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

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};

use crate::ast::{DeleteAst, InsertAst, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::executor::{Database, ExecResult, Executor, Transaction};
use crate::expr::{BinOp, ColumnRef, Expr};
use crate::row::{FromRow, ValueRow};
use crate::schema::{Dialect, TableSchema};
use crate::value::Value;

use eval::{Ctx, Group, Key, Pred, Rows, Slot, eval, order_cmp, slot_of, truthy};
#[cfg(feature = "mongodb")]
pub(crate) use eval::{Tok as LikeTok, tokenize as like_tokens};

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

#[derive(Clone)]
pub(crate) struct Table {
    pub(crate) schema: &'static TableSchema,
    pub(crate) rows: Vec<Vec<Value>>,
    pk: Map<Key, usize>,
    /// `(column index, value -> row)` for `UNIQUE` columns.
    uniques: Vec<(usize, Map<Key, usize>)>,
    pub(crate) next_id: i64,
    version: u64,
}

impl Table {
    /// Builds a table from existing rows (used by the Redis and MongoDB
    /// backends to evaluate queries on loaded data).
    #[cfg_attr(not(any(feature = "redis", feature = "mongodb")), allow(dead_code))]
    pub(crate) fn from_rows(schema: &'static TableSchema, rows: Vec<Vec<Value>>, last_id: i64) -> Self {
        let mut t = Table::new(schema, 0);
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
        Table {
            schema,
            rows: Vec::new(),
            pk: Map::default(),
            uniques: schema
                .columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.unique && !c.primary_key)
                .map(|(i, _)| (i, Map::default()))
                .collect(),
            next_id: 1,
            version,
        }
    }

    fn name(&self) -> &'static str {
        self.schema.name
    }

    fn violation(&self, col: usize) -> Error {
        Error::UniqueViolation { table: self.name().to_owned(), column: self.schema.columns[col].name.to_owned() }
    }

    fn lookup(&self, col: usize, value: &Value) -> Option<usize> {
        let key = Key(value.clone());
        if col == self.schema.primary_key {
            self.pk.get(&key).copied()
        } else {
            self.uniques.iter().find(|(c, _)| *c == col).and_then(|(_, m)| m.get(&key).copied())
        }
    }

    fn has_index(&self, col: usize) -> bool {
        col == self.schema.primary_key || self.uniques.iter().any(|(c, _)| *c == col)
    }

    fn check_row(&self, row: &[Value]) -> Result<()> {
        validate_row(self.schema, row)
    }

    fn index_row(&mut self, idx: usize) {
        let row = &self.rows[idx];
        self.pk.insert(Key(row[self.schema.primary_key].clone()), idx);
        for (col, map) in &mut self.uniques {
            if !row[*col].is_null() {
                map.insert(Key(row[*col].clone()), idx);
            }
        }
    }

    fn unindex_row(&mut self, idx: usize) {
        let row = &self.rows[idx];
        self.pk.remove(&Key(row[self.schema.primary_key].clone()));
        for (col, map) in &mut self.uniques {
            if !row[*col].is_null() {
                map.remove(&Key(row[*col].clone()));
            }
        }
    }

    fn rebuild_indexes(&mut self) {
        self.pk.clear();
        for (_, m) in &mut self.uniques {
            m.clear();
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
        // Build every index key once: used for the uniqueness check, then moved
        // into the indexes.
        let pk_key = Key(row[pk].clone());
        if self.pk.contains_key(&pk_key) {
            return Err(self.violation(pk));
        }
        let mut unique_keys = Vec::with_capacity(self.uniques.len());
        for (col, map) in &self.uniques {
            if row[*col].is_null() {
                unique_keys.push(None);
                continue;
            }
            let k = Key(row[*col].clone());
            if map.contains_key(&k) {
                return Err(self.violation(*col));
            }
            unique_keys.push(Some(k));
        }
        let key = pk_key.0.clone();
        self.bump_next_id(&key);
        let idx = self.rows.len();
        self.rows.push(row);
        self.pk.insert(pk_key, idx);
        for ((_, map), k) in self.uniques.iter_mut().zip(unique_keys) {
            if let Some(k) = k {
                map.insert(k, idx);
            }
        }
        Ok(key)
    }

    fn upsert(&mut self, row: Vec<Value>) -> Result<()> {
        self.check_row(&row)?;
        let pk = self.schema.primary_key;
        match self.pk.get(&Key(row[pk].clone())).copied() {
            Some(idx) => self.replace_rows(vec![(idx, row)]).map(|_| ()),
            None => self.insert(row, false).map(|_| ()),
        }
    }

    /// Atomically replaces rows, validating unique constraints first.
    fn replace_rows(&mut self, changes: Vec<(usize, Vec<Value>)>) -> Result<u64> {
        if changes.is_empty() {
            return Ok(0);
        }
        for (_, row) in &changes {
            self.check_row(row)?;
        }
        let changed: Set<usize> = changes.iter().map(|(i, _)| *i).collect();
        let pk = self.schema.primary_key;
        let indexed: Vec<usize> = std::iter::once(pk).chain(self.uniques.iter().map(|(c, _)| *c)).collect();
        for col in indexed {
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
        for (idx, _) in &changes {
            self.unindex_row(*idx);
        }
        let n = changes.len() as u64;
        let mut touched = Vec::with_capacity(changes.len());
        for (idx, row) in changes {
            self.bump_next_id(&row[pk]);
            self.rows[idx] = row;
            touched.push(idx);
        }
        for idx in touched {
            self.index_row(idx);
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
                if idx != last {
                    // The former last row moved to `idx`.
                    self.index_row(idx);
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

/// If `filter` pins an indexed column of the root table to constants, returns
/// the candidate row indexes.
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
            Some(table.lookup(col, v).into_iter().collect())
        }
        Expr::InList { expr, list, negated: false } => {
            let col = indexed_col(expr)?;
            let mut out: Vec<usize> = list.iter().filter_map(|v| table.lookup(col, v)).collect();
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

fn select(state: &State, ast: &SelectAst) -> Result<Vec<Vec<Value>>> {
    let layout: Vec<&'static str> = ast.tables().map(|t| t.name).collect();
    for (i, name) in layout.iter().enumerate() {
        if layout[..i].contains(name) {
            return Err(Error::Unsupported(format!(
                "table `{name}` appears twice in the query (self-joins need table aliases)"
            )));
        }
    }
    let tables: Vec<&Table> = layout.iter().map(|n| state.table(n)).collect::<Result<_>>()?;
    let stride = layout.len();
    let root = tables[0];

    let pred = ast.filter.as_ref().map(|f| Pred::compile(f, &layout)).transpose()?;

    // 1. Root rows (index lookup when possible).
    let candidates = ast.filter.as_ref().and_then(|f| index_candidates(f, root));
    let root_rows: Box<dyn Iterator<Item = &[Value]>> = match &candidates {
        Some(idx) => Box::new(idx.iter().map(|&i| root.rows[i].as_slice())),
        None => Box::new(root.rows.iter().map(Vec::as_slice)),
    };

    let rows = if ast.joins.is_empty() {
        // Single table: filter while scanning.
        let mut rows = Rows::with_capacity(1, if pred.is_some() { 0 } else { root.rows.len() });
        for r in root_rows {
            let parts = [Some(r)];
            if let Some(p) = &pred
                && !p.passes(&parts, &layout)?
            {
                continue;
            }
            rows.flat.push(Some(r));
        }
        rows
    } else {
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
                // Hash join: build on the joined table (or reuse its unique index).
                let use_index = jt.has_index(jcol);
                let mut built: Map<Key, Vec<usize>> = Map::default();
                if !use_index {
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
                        if use_index {
                            if let Some(m) = jt.lookup(jcol, v) {
                                emit(&mut next, base, slot, Some(&jt.rows[m]));
                                matched = true;
                            }
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
        return select_grouped(ast, &layout, &rows);
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
        if let Some(slots) = column_slots {
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

    if ast.distinct {
        let mut seen: Set<Vec<Key>> = Set::default();
        let mut out = Vec::new();
        for &i in &order {
            let row = project(rows.get(i))?;
            if seen.insert(row.iter().cloned().map(Key).collect()) {
                out.push(row);
            }
        }
        return Ok(out.into_iter().skip(offset).take(take).collect());
    }

    order.iter().skip(offset).take(take).map(|&i| project(rows.get(i))).collect()
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
    let layout = [table.name()];
    let Some(filter) = filter else {
        return Ok((0..table.rows.len()).collect());
    };
    let candidates = index_candidates(filter, table);
    let pred = Pred::compile(filter, &layout)?;
    let mut out = Vec::new();
    let mut test = |i: usize| -> Result<()> {
        let parts = [Some(table.rows[i].as_slice())];
        if pred.passes(&parts, &layout)? {
            out.push(i);
        }
        Ok(())
    };
    match candidates {
        Some(c) => c.into_iter().try_for_each(&mut test)?,
        None => (0..table.rows.len()).try_for_each(&mut test)?,
    }
    Ok(out)
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
    let t = state.table_mut(ins.table.name)?;
    // All or nothing: validate on a scratch copy only if a row fails.
    let snapshot_len = t.rows.len();
    let snapshot_next = t.next_id;
    for row in &ins.rows {
        if let Err(e) = t.insert(row.clone(), true) {
            // Roll back the rows inserted by this statement.
            let added: Vec<usize> = (snapshot_len..t.rows.len()).collect();
            t.remove_rows(added);
            t.next_id = snapshot_next;
            return Err(e);
        }
    }
    Ok(ins.rows.len() as u64)
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
        Query::Select(s) => select(state, s),
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
        let rows = fetch(&self.shared.read(), &query)?;
        decode(rows)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        execute(&mut self.shared.write(), &query).map(|(r, _)| r)
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
        let rows = fetch(&self.local.lock(), &query)?;
        decode(rows)
    }

    async fn execute(&self, query: Query<'_>) -> Result<ExecResult> {
        let (res, table) = execute(&mut self.local.lock(), &query)?;
        if let Some(t) = table {
            self.dirty.lock().insert(t);
        }
        Ok(res)
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
