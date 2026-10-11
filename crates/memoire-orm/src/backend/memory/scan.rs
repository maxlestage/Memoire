//! Vectorised single-table scans and sorts.
//!
//! A `WHERE` clause is split into its top-level `AND` terms. Each term then
//! filters the rows with a loop specialised for its column and constant
//! (integers, floats, text, ranges, `IS NULL`). On numeric columns the loop
//! reads the table's contiguous typed copy of the column ([`Mirror`]): a few
//! instructions and bytes per row instead of a walk through the general
//! evaluator. Any other term, or a value of an unexpected type (a float in an
//! integer column, `NaN`, ...), goes through [`Pred`], the general
//! three-valued evaluator, so the rows selected and the errors raised are the
//! ones row-by-row evaluation gives. (Terms are applied in order and a row is
//! dropped as soon as one term is not true, so later terms are not evaluated
//! on it, like `AND`'s short-circuit; databases make no promise about that
//! order either.)

use std::cmp::Ordering;

use crate::error::Result;
use crate::expr::{BinOp, Expr, Nulls, Order};
use crate::value::Value;

use super::eval::{Pred, Rows, Slot};
use super::{MIRROR_NULL, MIRROR_VALUE, Mirror, MirrorData, Table};

/// Splits `a AND b AND c` into `[a, b, c]`.
fn conjuncts<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::Binary { op: BinOp::And, left, right } => {
            conjuncts(left, out);
            conjuncts(right, out);
        }
        other => out.push(other),
    }
}

/// How a term is evaluated.
enum Kernel<'a> {
    Int { col: usize, op: BinOp, c: i64 },
    Float { col: usize, op: BinOp, c: f64 },
    Text { col: usize, op: BinOp, c: &'a str },
    IntRange { col: usize, lo: i64, hi: i64, negated: bool },
    FloatRange { col: usize, lo: f64, hi: f64, negated: bool },
    IsNull { col: usize, negated: bool },
    General,
}

struct Term<'a> {
    kernel: Kernel<'a>,
    /// Exact evaluation, for [`Kernel::General`] and unexpected value types.
    pred: Pred<'a>,
}

fn int(v: &Value) -> Option<i64> {
    match *v {
        Value::I32(x) => Some(i64::from(x)),
        Value::I64(x) => Some(x),
        _ => None,
    }
}

fn float(v: &Value) -> Option<f64> {
    match *v {
        Value::I32(x) => Some(f64::from(x)),
        Value::I64(x) => Some(x as f64),
        Value::F64(x) if !x.is_nan() => Some(x),
        _ => None,
    }
}

/// Outcome of a specialised test on one row.
const FAIL: u8 = 0;
const PASS: u8 = 1;
/// The value has an unexpected type: ask the general evaluator.
const ASK: u8 = 2;

/// Runs `fast` over every row (`all`: filling `sel`) or over the rows of `sel`
/// (keeping those that pass), in order; rows it cannot decide go to `general`.
/// The hot loop handles a byte per row, never an error.
#[inline(always)]
fn drive(
    sel: &mut Vec<u32>,
    all: Option<u32>,
    mut fast: impl FnMut(u32) -> u8,
    general: &dyn Fn(u32) -> Result<bool>,
) -> Result<()> {
    let mut decide = |i: u32| -> Result<bool> {
        Ok(match fast(i) {
            PASS => true,
            FAIL => false,
            _ => general(i)?,
        })
    };
    match all {
        Some(n) => {
            for i in 0..n {
                if decide(i)? {
                    sel.push(i);
                }
            }
        }
        None => {
            let mut kept = 0;
            for r in 0..sel.len() {
                let i = sel[r];
                sel[kept] = i;
                kept += usize::from(decide(i)?);
            }
            sel.truncate(kept);
        }
    }
    Ok(())
}

/// [`drive`] over a typed column copy; a full scan walks the arrays in step.
#[inline(always)]
fn drive_mirror<T: Copy>(
    sel: &mut Vec<u32>,
    all: Option<u32>,
    mirror: (&[T], &[u8], usize),
    test: impl Fn(T) -> bool,
    general: &dyn Fn(u32) -> Result<bool>,
) -> Result<()> {
    let (data, state, others) = mirror;
    if all.is_some() && others == 0 {
        // Only NULLs and values: no branch per row, every index is written and
        // the cursor moves on when the row passes.
        let start = sel.len();
        sel.resize(start + state.len(), 0);
        let out = &mut sel[start..];
        let mut kept = 0;
        for (i, (&st, &x)) in state.iter().zip(data).enumerate() {
            out[kept] = i as u32;
            kept += usize::from((st == MIRROR_VALUE) & test(x));
        }
        sel.truncate(start + kept);
        return Ok(());
    }
    if all.is_some() {
        for (i, (&st, &x)) in state.iter().zip(data).enumerate() {
            let pass = match st {
                MIRROR_VALUE => test(x),
                MIRROR_NULL => false,
                _ => general(i as u32)?,
            };
            if pass {
                sel.push(i as u32);
            }
        }
        return Ok(());
    }
    drive(
        sel,
        None,
        |i| match state[i as usize] {
            MIRROR_VALUE => u8::from(test(data[i as usize])),
            MIRROR_NULL => FAIL,
            _ => ASK,
        },
        general,
    )
}

impl<'a> Term<'a> {
    fn compile(e: &'a Expr, layout: &[&'static str]) -> Result<Term<'a>> {
        let pred = Pred::compile(e, layout)?;
        let kernel = match &pred {
            Pred::Cmp { col: Slot { slot: 0, index }, op, value } => match value {
                Value::I32(_) | Value::I64(_) => {
                    Kernel::Int { col: *index, op: *op, c: int(value).unwrap_or_default() }
                }
                Value::F64(c) if !c.is_nan() => Kernel::Float { col: *index, op: *op, c: *c },
                Value::Text(c) => Kernel::Text { col: *index, op: *op, c },
                _ => Kernel::General,
            },
            Pred::Between { col: Slot { slot: 0, index }, low, high, negated } => {
                match (int(low), int(high), float(low), float(high)) {
                    (Some(lo), Some(hi), _, _) => Kernel::IntRange { col: *index, lo, hi, negated: *negated },
                    (_, _, Some(lo), Some(hi)) => Kernel::FloatRange { col: *index, lo, hi, negated: *negated },
                    _ => Kernel::General,
                }
            }
            Pred::IsNull { col: Slot { slot: 0, index }, negated } => Kernel::IsNull { col: *index, negated: *negated },
            _ => Kernel::General,
        };
        Ok(Term { kernel, pred })
    }

    /// Filters the rows: all of them into `sel` when `all` is set, else the
    /// rows already in `sel`.
    fn apply(&self, table: &Table, sel: &mut Vec<u32>, all: Option<u32>, layout: &[&'static str]) -> Result<()> {
        let rows = &table.rows;
        let general = |i: u32| self.pred.passes(&[Some(rows[i as usize].as_slice())], layout);
        let general: &dyn Fn(u32) -> Result<bool> = &general;

        // Typed column copy: `$x` is the value, `$test` decides.
        macro_rules! on_mirror {
            ($m:expr, $data:expr, |$x:ident| $test:expr) => {
                drive_mirror(sel, all, ($data.as_slice(), $m.state.as_slice(), $m.others), |$x| $test, general)
            };
        }
        // Rows: `$get` extracts the value (`None`: general path).
        macro_rules! on_rows {
            ($col:expr, |$v:ident| $get:expr, |$x:ident| $test:expr) => {
                drive(
                    sel,
                    all,
                    |i| match rows[i as usize].get($col) {
                        None | Some(Value::Null) => FAIL,
                        Some($v) => match $get {
                            Some($x) => u8::from($test),
                            None => ASK,
                        },
                    },
                    general,
                )
            };
        }
        // One loop per comparison operator: `$go!(<)` expands the scan with `<`.
        macro_rules! by_op {
            ($op:expr, $go:ident) => {
                match $op {
                    BinOp::Eq => $go!(==),
                    BinOp::Ne => $go!(!=),
                    BinOp::Lt => $go!(<),
                    BinOp::Le => $go!(<=),
                    BinOp::Gt => $go!(>),
                    BinOp::Ge => $go!(>=),
                    _ => drive(sel, all, |_| ASK, general),
                }
            };
        }

        match self.kernel {
            Kernel::Int { col, op, c } => match table.mirror(col) {
                Some(m @ Mirror { data: MirrorData::Int(d), .. }) => {
                    macro_rules! go { ($cmp:tt) => { on_mirror!(m, d, |x| x $cmp c) }; }
                    by_op!(op, go)
                }
                Some(m @ Mirror { data: MirrorData::Float(d), .. }) => {
                    let c = c as f64;
                    macro_rules! go { ($cmp:tt) => { on_mirror!(m, d, |x| x $cmp c) }; }
                    by_op!(op, go)
                }
                None => {
                    macro_rules! go { ($cmp:tt) => { on_rows!(col, |v| int(v), |x| x $cmp c) }; }
                    by_op!(op, go)
                }
            },
            Kernel::Float { col, op, c } => match table.mirror(col) {
                Some(m @ Mirror { data: MirrorData::Int(d), .. }) => {
                    macro_rules! go { ($cmp:tt) => { on_mirror!(m, d, |x| (x as f64) $cmp c) }; }
                    by_op!(op, go)
                }
                Some(m @ Mirror { data: MirrorData::Float(d), .. }) => {
                    macro_rules! go { ($cmp:tt) => { on_mirror!(m, d, |x| x $cmp c) }; }
                    by_op!(op, go)
                }
                None => {
                    macro_rules! go { ($cmp:tt) => { on_rows!(col, |v| float(v), |x| x $cmp c) }; }
                    by_op!(op, go)
                }
            },
            Kernel::Text { col, op, c } => {
                macro_rules! go {
                    ($cmp:tt) => {
                        on_rows!(
                            col,
                            |v| match v {
                                Value::Text(s) => Some(s.as_str()),
                                _ => None,
                            },
                            |x| x $cmp c
                        )
                    };
                }
                by_op!(op, go)
            }
            Kernel::IntRange { col, lo, hi, negated } => match table.mirror(col) {
                Some(m @ Mirror { data: MirrorData::Int(d), .. }) => {
                    on_mirror!(m, d, |x| (lo <= x && x <= hi) != negated)
                }
                Some(m @ Mirror { data: MirrorData::Float(d), .. }) => {
                    let (lo, hi) = (lo as f64, hi as f64);
                    on_mirror!(m, d, |x| (lo <= x && x <= hi) != negated)
                }
                None => on_rows!(col, |v| int(v), |x| (lo <= x && x <= hi) != negated),
            },
            Kernel::FloatRange { col, lo, hi, negated } => match table.mirror(col) {
                Some(m @ Mirror { data: MirrorData::Int(d), .. }) => {
                    on_mirror!(m, d, |x| (lo <= x as f64 && x as f64 <= hi) != negated)
                }
                Some(m @ Mirror { data: MirrorData::Float(d), .. }) => {
                    on_mirror!(m, d, |x| (lo <= x && x <= hi) != negated)
                }
                None => on_rows!(col, |v| float(v), |x| (lo <= x && x <= hi) != negated),
            },
            Kernel::IsNull { col, negated } => {
                drive(sel, all, |i| u8::from(rows[i as usize].get(col).is_none_or(Value::is_null) != negated), general)
            }
            Kernel::General => drive(sel, all, |_| ASK, general),
        }
    }
}

/// Indexes of the rows of `table` for which `filter` is true, in table order
/// (all rows without a filter). Uses a primary-key or unique index lookup for
/// the candidates when the filter pins an indexed column.
pub(crate) fn matching(table: &Table, filter: Option<&Expr>) -> Result<Vec<u32>> {
    let n = table.rows.len() as u32;
    let Some(filter) = filter else {
        return Ok((0..n).collect());
    };
    let layout = [table.schema.name];
    let mut parts = Vec::new();
    conjuncts(filter, &mut parts);

    let mut sel: Vec<u32> = Vec::new();
    // An index lookup on the first conjunct that allows one. An `IN` list is
    // answered exactly by the index (same key equality), so that term is not
    // evaluated again on the candidates.
    let indexed = parts
        .iter()
        .enumerate()
        .find_map(|(i, e)| super::index_candidates(e, table).map(|c| (i, c)))
        // Equalities pinning every column of a multi-column index (no term skipped).
        .or_else(|| super::composite_candidates(filter, table).map(|c| (usize::MAX, c)));
    let skip = match &indexed {
        Some((i, _)) if matches!(parts.get(*i), Some(Expr::InList { negated: false, .. })) => Some(*i),
        _ => None,
    };
    let mut terms = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .map(|(_, e)| Term::compile(e, &layout))
        .collect::<Vec<_>>()
        .into_iter();
    match indexed {
        Some((_, candidates)) => sel.extend(candidates.into_iter().map(|i| i as u32)),
        // The first term scans the whole table and only keeps its matches.
        None => match terms.next() {
            Some(first) => first?.apply(table, &mut sel, Some(n), &layout)?,
            None => sel.extend(0..n),
        },
    }
    for term in terms {
        if sel.is_empty() {
            break;
        }
        term?.apply(table, &mut sel, None, &layout)?;
    }
    Ok(sel)
}

/// Number of rows of `table` for which `filter` is true.
pub(crate) fn count(table: &Table, filter: Option<&Expr>) -> Result<usize> {
    match filter {
        None => Ok(table.rows.len()),
        Some(_) => matching(table, filter).map(|sel| sel.len()),
    }
}

// ---------------------------------------------------------------------------
// ORDER BY one column
// ---------------------------------------------------------------------------

/// The comparison of `order_cmp` on extracted keys, ties broken by position.
#[inline(always)]
fn key_cmp<K: PartialOrd>(a: &(Option<K>, u32), b: &(Option<K>, u32), desc: bool, nulls_first: bool) -> Ordering {
    let ord = match (&a.0, &b.0) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => return if nulls_first { Ordering::Less } else { Ordering::Greater },
        (Some(_), None) => return if nulls_first { Ordering::Greater } else { Ordering::Less },
        (Some(x), Some(y)) => {
            let o = x.partial_cmp(y).unwrap_or(Ordering::Equal);
            if desc { o.reverse() } else { o }
        }
    };
    ord.then(a.1.cmp(&b.1))
}

/// Sorts `(key, position)` pairs and returns the first `need` positions (all
/// when `None`).
fn top<K: PartialOrd>(mut keys: Vec<(Option<K>, u32)>, need: Option<usize>, order: &Order) -> Vec<usize> {
    let (desc, nf) = (order.desc, nulls_first(order));
    let cmp = |a: &(Option<K>, u32), b: &(Option<K>, u32)| key_cmp(a, b, desc, nf);
    if let Some(need) = need.filter(|&k| k > 0 && k < keys.len()) {
        keys.select_nth_unstable_by(need - 1, cmp);
        keys.truncate(need);
    }
    keys.sort_unstable_by(cmp);
    keys.into_iter().map(|(_, i)| i as usize).collect()
}

/// `x` mapped to a `u64` with the same order.
#[inline]
fn int_bits(x: i64) -> u64 {
    (x as u64) ^ (1 << 63)
}

/// `x` (not NaN) mapped to a `u64` with the same order; `-0.0` sorts as `0.0`.
#[inline]
fn float_bits(x: f64) -> u64 {
    let b = (x + 0.0).to_bits();
    if b >> 63 == 1 { !b } else { b | (1 << 63) }
}

/// One integer whose order is the `ORDER BY` order: NULL placement, then the
/// value (reversed for DESC), then the position, which breaks ties.
#[inline]
fn packed(bits: Option<u64>, pos: usize, desc: bool, nulls_first: bool) -> u128 {
    let (rank, key) = match bits {
        None => (if nulls_first { 0u128 } else { 2 }, 0),
        Some(b) => (1, if desc { !b } else { b }),
    };
    (rank << 96) | (u128::from(key) << 32) | pos as u128
}

/// [`top`] on packed keys: plain integer comparisons.
fn top_packed(mut keys: Vec<u128>, need: Option<usize>) -> Vec<usize> {
    if let Some(need) = need.filter(|&k| k > 0 && k < keys.len()) {
        keys.select_nth_unstable(need - 1);
        keys.truncate(need);
    }
    keys.sort_unstable();
    keys.into_iter().map(|k| k as u32 as usize).collect()
}

fn nulls_first(order: &Order) -> bool {
    match order.nulls {
        Some(Nulls::First) => true,
        Some(Nulls::Last) => false,
        None => order.desc,
    }
}

/// Positions in `sel` of the first `need` rows (all when `None`) in the
/// `ORDER BY` order of column `col` of `table`. `None` when the column's values
/// do not allow the fast path (the caller then uses the general comparison).
pub(crate) fn sort_selection(
    table: &Table,
    sel: &[u32],
    col: usize,
    order: &Order,
    need: Option<usize>,
) -> Option<Vec<usize>> {
    if let Some(m) = table.mirror(col) {
        if sel.iter().any(|&i| m.state[i as usize] > MIRROR_VALUE) {
            return None;
        }
        let (desc, nf) = (order.desc, nulls_first(order));
        let present = |i: u32| m.state[i as usize] == MIRROR_VALUE;
        let keys: Vec<u128> = match &m.data {
            MirrorData::Int(d) => sel
                .iter()
                .enumerate()
                .map(|(p, &i)| packed(present(i).then(|| int_bits(d[i as usize])), p, desc, nf))
                .collect(),
            MirrorData::Float(d) => sel
                .iter()
                .enumerate()
                .map(|(p, &i)| packed(present(i).then(|| float_bits(d[i as usize])), p, desc, nf))
                .collect(),
        };
        return Some(top_packed(keys, need));
    }
    let rows = &table.rows;
    let value = |i: u32| rows[i as usize].get(col).unwrap_or(&Value::Null);
    let first = sel.iter().map(|&i| value(i)).find(|v| !v.is_null())?;
    match first {
        Value::Text(_) => Some(top(
            sel.iter()
                .enumerate()
                .map(|(p, &i)| match value(i) {
                    Value::Null => Some((None, p as u32)),
                    Value::Text(s) => Some((Some(s.as_str()), p as u32)),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            need,
            order,
        )),
        _ => None,
    }
}

/// [`sort_selection`] for joined rows: keys read through `slot`.
pub(crate) fn sort_rows(rows: &Rows<'_>, slot: Slot, order: &Order, need: Option<usize>) -> Option<Vec<usize>> {
    let n = rows.len();
    let first = (0..n).map(|i| slot.get(rows.get(i))).find(|v| !v.is_null())?;
    let (desc, nf) = (order.desc, nulls_first(order));
    match first {
        Value::I32(_) | Value::I64(_) => Some(top_packed(
            (0..n)
                .map(|i| match slot.get(rows.get(i)) {
                    Value::Null => Some(packed(None, i, desc, nf)),
                    v => int(v).map(|x| packed(Some(int_bits(x)), i, desc, nf)),
                })
                .collect::<Option<_>>()?,
            need,
        )),
        Value::F64(_) => Some(top_packed(
            (0..n)
                .map(|i| match slot.get(rows.get(i)) {
                    Value::Null => Some(packed(None, i, desc, nf)),
                    Value::F64(x) if !x.is_nan() => Some(packed(Some(float_bits(*x)), i, desc, nf)),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            need,
        )),
        Value::Text(_) => Some(top(
            (0..n)
                .map(|i| match slot.get(rows.get(i)) {
                    Value::Null => Some((None, i as u32)),
                    Value::Text(s) => Some((Some(s.as_str()), i as u32)),
                    _ => None,
                })
                .collect::<Option<_>>()?,
            need,
            order,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ColumnRef, Expr};
    use crate::prelude::*;
    use crate::value::ColumnType;

    #[derive(Model, Debug, Clone)]
    #[memoire(table = "scan_t")]
    struct T {
        id: i64,
        a: Option<i32>,
        b: Option<f64>,
        s: Option<String>,
    }

    fn col(index: usize, kind: ColumnType) -> Expr {
        let c = &T::SCHEMA.columns[index];
        Expr::Column(ColumnRef { table: T::SCHEMA.name, name: c.name, index, kind })
    }

    fn bin(op: BinOp, l: Expr, r: Value) -> Expr {
        Expr::Binary { op, left: Box::new(l), right: Box::new(Expr::Value(r)) }
    }

    /// Rows with NULLs, i32/i64 mixes, NaN, and values of the "wrong" type.
    fn table(odd: bool) -> Table {
        let mut rows = Vec::new();
        for i in 0..60i64 {
            let a = match i % 7 {
                0 => Value::Null,
                1 => Value::I64(i - 30),
                _ => Value::I32((i * 13 % 41) as i32 - 20),
            };
            let b = match i % 5 {
                0 => Value::Null,
                _ => Value::F64((i as f64) * 0.75 - 20.0),
            };
            let s = match i % 4 {
                0 => Value::Null,
                _ => Value::Text(format!("v{:02}", i * 7 % 23)),
            };
            rows.push(vec![Value::I64(i + 1), a, b, s]);
        }
        // Edge cases of the packed sort keys.
        rows[10][2] = Value::F64(-0.0);
        rows[11][2] = Value::F64(0.0);
        rows[17][1] = Value::I64(i64::MIN);
        rows[13][1] = Value::I64(i64::MAX);
        rows[14][2] = Value::F64(f64::INFINITY);
        rows[16][2] = Value::F64(f64::NEG_INFINITY);
        if odd {
            rows[3][1] = Value::F64(2.5); // float in an integer column
            rows[4][2] = Value::F64(f64::NAN);
            rows[5][2] = Value::I64(7); // integer in a float column
        }
        Table::from_rows(T::SCHEMA, rows, 60)
    }

    /// Row-by-row reference: the general evaluator on the whole filter.
    fn reference(t: &Table, filter: &Expr) -> Result<Vec<u32>> {
        let layout = [t.schema.name];
        let pred = Pred::compile(filter, &layout)?;
        let mut out = Vec::new();
        for (i, row) in t.rows.iter().enumerate() {
            if pred.passes(&[Some(row.as_slice())], &layout)? {
                out.push(i as u32);
            }
        }
        Ok(out)
    }

    fn check(t: &Table, filter: &Expr) {
        let fast = matching(t, Some(filter));
        let slow = reference(t, filter);
        match (&fast, &slow) {
            (Ok(f), Ok(s)) => assert_eq!(f, s, "filter {filter:?}"),
            (Err(_), Err(_)) => {}
            _ => panic!("filter {filter:?}: fast {fast:?} vs reference {slow:?}"),
        }
    }

    #[test]
    fn filters_match_the_general_evaluator() {
        let ops = [BinOp::Eq, BinOp::Ne, BinOp::Lt, BinOp::Le, BinOp::Gt, BinOp::Ge];
        let consts = [Value::I32(3), Value::I64(-5), Value::F64(2.5), Value::F64(-0.0), Value::Null];
        for odd in [false, true] {
            let t = table(odd);
            for op in ops {
                for c in &consts {
                    check(&t, &bin(op, col(1, ColumnType::Int), c.clone()));
                    check(&t, &bin(op, col(2, ColumnType::Double), c.clone()));
                    // Constant on the left.
                    check(
                        &t,
                        &Expr::Binary {
                            op,
                            left: Box::new(Expr::Value(c.clone())),
                            right: Box::new(col(1, ColumnType::Int)),
                        },
                    );
                }
                check(&t, &bin(op, col(3, ColumnType::Text), Value::Text("v10".into())));
            }
            for (lo, hi) in
                [(Value::I32(-3), Value::I64(9)), (Value::F64(-1.5), Value::I32(4)), (Value::I32(5), Value::I32(1))]
            {
                for negated in [false, true] {
                    for c in [1usize, 2] {
                        let kind = if c == 1 { ColumnType::Int } else { ColumnType::Double };
                        check(
                            &t,
                            &Expr::Between {
                                expr: Box::new(col(c, kind)),
                                low: Box::new(Expr::Value(lo.clone())),
                                high: Box::new(Expr::Value(hi.clone())),
                                negated,
                            },
                        );
                    }
                }
            }
            for negated in [false, true] {
                for c in 1..4 {
                    check(&t, &Expr::IsNull { expr: Box::new(col(c, ColumnType::Text)), negated });
                }
            }
        }
        // Several terms, an OR and a NOT (general path), on regular data.
        let t = table(false);
        let a_gt = bin(BinOp::Gt, col(1, ColumnType::Int), Value::I32(-4));
        let b_le = bin(BinOp::Le, col(2, ColumnType::Double), Value::F64(10.0));
        let s_ne = bin(BinOp::Ne, col(3, ColumnType::Text), Value::Text("v03".into()));
        check(&t, &(a_gt.clone() & b_le.clone() & s_ne.clone()));
        check(&t, &((a_gt.clone() | b_le.clone()) & s_ne.clone()));
        check(&t, &(!a_gt & b_le));
        // An index lookup for the candidates, then the other terms.
        let pk = bin(BinOp::Eq, col(0, ColumnType::BigInt), Value::I64(9));
        check(&t, &(pk & s_ne));
    }

    #[test]
    fn sorts_match_order_cmp() {
        for odd in [false, true] {
            let t = table(odd);
            let sel: Vec<u32> = (0..t.rows.len() as u32).filter(|i| i % 3 != 0).collect();
            for c in 1..4 {
                for desc in [false, true] {
                    for nulls in [None, Some(Nulls::First), Some(Nulls::Last)] {
                        let order = Order { expr: col(c, ColumnType::Text), desc, nulls };
                        let mut expected: Vec<usize> = (0..sel.len()).collect();
                        expected.sort_by(|&x, &y| {
                            let (vx, vy) = (&t.rows[sel[x] as usize][c], &t.rows[sel[y] as usize][c]);
                            super::super::eval::order_cmp(vx, vy, &order).then(x.cmp(&y))
                        });
                        for need in [None, Some(5), Some(1000)] {
                            if let Some(got) = sort_selection(&t, &sel, c, &order, need) {
                                let want: Vec<usize> =
                                    expected.iter().copied().take(need.unwrap_or(usize::MAX)).collect();
                                assert_eq!(got, want, "column {c} desc {desc} nulls {nulls:?} need {need:?} odd {odd}");
                            } else {
                                assert!(odd, "the fast path only steps aside for odd values");
                            }
                        }
                    }
                }
            }
        }
    }
}
