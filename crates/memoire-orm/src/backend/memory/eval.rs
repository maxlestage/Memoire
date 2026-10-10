//! Expression evaluation for the in-memory engine (SQL three-valued logic).

use std::borrow::Cow;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use crate::error::{Error, Result};
use crate::expr::{AggFunc, BinOp, Expr, Nulls, Order};
use crate::value::Value;

static NULL: Value = Value::Null;

/// One table slice per table of the query (`None` for the NULL side of a
/// `LEFT JOIN`).
pub(crate) type Parts<'a> = [Option<&'a [Value]>];

/// Combined rows of a query, stored flat: row `i` is
/// `flat[i * stride .. (i + 1) * stride]` (one slot per table).
pub(crate) struct Rows<'a> {
    pub flat: Vec<Option<&'a [Value]>>,
    pub stride: usize,
}

impl<'a> Rows<'a> {
    pub fn with_capacity(stride: usize, rows: usize) -> Self {
        Rows { flat: Vec::with_capacity(stride * rows), stride }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.flat.len().checked_div(self.stride).unwrap_or(0)
    }

    #[inline]
    pub fn get(&self, i: usize) -> &Parts<'a> {
        &self.flat[i * self.stride..(i + 1) * self.stride]
    }
}

/// Members of a group, for aggregates.
#[derive(Clone, Copy)]
pub(crate) struct Group<'a, 'b> {
    pub rows: &'b Rows<'a>,
    pub members: &'b [usize],
}

/// Evaluation context.
pub(crate) struct Ctx<'a, 'b> {
    /// Table names, aligned with `parts`.
    pub layout: &'b [&'static str],
    /// Current row.
    pub parts: &'b Parts<'a>,
    /// Rows of the current group, for aggregates.
    pub group: Option<Group<'a, 'b>>,
}

/// A hashable wrapper used for indexes, `DISTINCT` and `GROUP BY`.
/// Integers and integral floats compare equal, like in SQL.
#[derive(Clone, Debug)]
pub(crate) struct Key(pub Value);

impl Key {
    fn tag(&self) -> (u8, KeyRepr<'_>) {
        match &self.0 {
            Value::Null => (0, KeyRepr::None),
            Value::Bool(b) => (1, KeyRepr::Int(*b as i64)),
            Value::I32(v) => (2, KeyRepr::Int(*v as i64)),
            Value::I64(v) => (2, KeyRepr::Int(*v)),
            Value::F64(f) => {
                if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                    (2, KeyRepr::Int(*f as i64))
                } else if f.is_nan() {
                    (3, KeyRepr::Bits(u64::MAX))
                } else {
                    (3, KeyRepr::Bits(f.to_bits()))
                }
            }
            Value::Text(s) => (4, KeyRepr::Str(s)),
            Value::Bytes(b) => (5, KeyRepr::Bytes(b)),
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => (6, KeyRepr::Bytes(u.as_bytes())),
            #[cfg(feature = "chrono")]
            Value::Timestamp(t) => (7, KeyRepr::Int(t.and_utc().timestamp_nanos_opt().unwrap_or(i64::MAX))),
            #[cfg(feature = "chrono")]
            Value::TimestampTz(t) => (7, KeyRepr::Int(t.timestamp_nanos_opt().unwrap_or(i64::MAX))),
            #[cfg(feature = "chrono")]
            Value::Date(d) => (8, KeyRepr::Int(chrono::Datelike::num_days_from_ce(d) as i64)),
            #[cfg(feature = "chrono")]
            Value::Time(t) => (9, KeyRepr::Str2(t.to_string())),
            #[cfg(feature = "json")]
            Value::Json(j) => (10, KeyRepr::Str2(j.to_string())),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum KeyRepr<'a> {
    None,
    Int(i64),
    Bits(u64),
    Str(&'a str),
    #[allow(dead_code)]
    Str2(String),
    Bytes(&'a [u8]),
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.tag() == other.tag()
    }
}

impl Eq for Key {}

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
    }
}

/// SQL comparison. `None` if either side is NULL or the types are not
/// comparable.
pub(crate) fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    use Value::*;
    match (a, b) {
        (Null, _) | (_, Null) => None,
        (Bool(x), Bool(y)) => Some(x.cmp(y)),
        (I32(x), I32(y)) => Some(x.cmp(y)),
        (I32(_) | I64(_), I32(_) | I64(_)) => Some(a.as_i64()?.cmp(&b.as_i64()?)),
        (I32(_) | I64(_) | F64(_), I32(_) | I64(_) | F64(_)) => a.as_f64()?.partial_cmp(&b.as_f64()?),
        (Text(x), Text(y)) => Some(x.as_str().cmp(y.as_str())),
        (Bytes(x), Bytes(y)) => Some(x.cmp(y)),
        #[cfg(feature = "uuid")]
        (Uuid(x), Uuid(y)) => Some(x.cmp(y)),
        #[cfg(feature = "uuid")]
        (Uuid(x), Text(y)) => ::uuid::Uuid::parse_str(y).ok().map(|y| x.cmp(&y)),
        #[cfg(feature = "uuid")]
        (Text(x), Uuid(y)) => ::uuid::Uuid::parse_str(x).ok().map(|x| x.cmp(y)),
        #[cfg(feature = "chrono")]
        (Timestamp(x), Timestamp(y)) => Some(x.cmp(y)),
        #[cfg(feature = "chrono")]
        (TimestampTz(x), TimestampTz(y)) => Some(x.cmp(y)),
        #[cfg(feature = "chrono")]
        (Timestamp(x), TimestampTz(y)) => Some(x.cmp(&y.naive_utc())),
        #[cfg(feature = "chrono")]
        (TimestampTz(x), Timestamp(y)) => Some(x.naive_utc().cmp(y)),
        #[cfg(feature = "chrono")]
        (Date(x), Date(y)) => Some(x.cmp(y)),
        #[cfg(feature = "chrono")]
        (Time(x), Time(y)) => Some(x.cmp(y)),
        #[cfg(feature = "json")]
        (Json(x), Json(y)) => {
            if x == y {
                Some(Ordering::Equal)
            } else {
                Some(x.to_string().cmp(&y.to_string()))
            }
        }
        _ => None,
    }
}

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::I32(_) | Value::I64(_) | Value::F64(_) => 2,
        Value::Text(_) => 3,
        Value::Bytes(_) => 4,
        #[allow(unreachable_patterns)]
        _ => 5,
    }
}

/// Total order used by `ORDER BY`: NULLs sort as the largest value (like
/// PostgreSQL) unless overridden.
pub(crate) fn order_cmp(a: &Value, b: &Value, order: &Order) -> Ordering {
    let nulls_first = match order.nulls {
        Some(Nulls::First) => true,
        Some(Nulls::Last) => false,
        None => order.desc,
    };
    let ord = match (a.is_null(), b.is_null()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return if nulls_first { Ordering::Less } else { Ordering::Greater },
        (false, true) => return if nulls_first { Ordering::Greater } else { Ordering::Less },
        (false, false) => compare(a, b).unwrap_or_else(|| type_rank(a).cmp(&type_rank(b))),
    };
    if order.desc { ord.reverse() } else { ord }
}

/// WHERE / HAVING / ON truthiness.
#[inline]
pub(crate) fn truthy(v: &Value) -> bool {
    match *v {
        Value::Bool(b) => b,
        Value::I32(i) => i != 0,
        Value::I64(i) => i != 0,
        _ => false,
    }
}

fn incomparable(a: &Value, b: &Value) -> Error {
    Error::Conversion(format!("cannot compare {} with {}", a.type_name(), b.type_name()))
}

fn arith(op: BinOp, a: &Value, b: &Value) -> Result<Value> {
    use Value::*;
    if a.is_null() || b.is_null() {
        return Ok(Null);
    }
    let overflow = || Error::Conversion(format!("integer overflow in {a} {} {b}", op.as_sql()));
    match (a, b) {
        (I32(_) | I64(_), I32(_) | I64(_)) => {
            let (x, y) = (a.as_i64().unwrap_or(0), b.as_i64().unwrap_or(0));
            let r = match op {
                BinOp::Add => x.checked_add(y),
                BinOp::Sub => x.checked_sub(y),
                BinOp::Mul => x.checked_mul(y),
                BinOp::Div => {
                    if y == 0 {
                        return Err(Error::Conversion("division by zero".into()));
                    }
                    x.checked_div(y)
                }
                _ => unreachable!(),
            }
            .ok_or_else(overflow)?;
            if matches!((a, b), (I32(_), I32(_))) {
                Ok(i32::try_from(r).map(I32).unwrap_or(I64(r)))
            } else {
                Ok(I64(r))
            }
        }
        (I32(_) | I64(_) | F64(_), I32(_) | I64(_) | F64(_)) => {
            let (x, y) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
            Ok(F64(match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => {
                    if y == 0.0 {
                        return Err(Error::Conversion("division by zero".into()));
                    }
                    x / y
                }
                _ => unreachable!(),
            }))
        }
        _ => {
            Err(Error::Conversion(format!("cannot apply `{}` to {} and {}", op.as_sql(), a.type_name(), b.type_name())))
        }
    }
}

fn bool_or_null(b: Option<bool>) -> Value {
    match b {
        Some(b) => Value::Bool(b),
        None => Value::Null,
    }
}

/// Three-valued logic helper: `Some(bool)` or `None` for NULL.
fn tri(v: &Value) -> Option<bool> {
    if v.is_null() { None } else { Some(truthy(v)) }
}

/// Evaluates an expression.
pub(crate) fn eval<'a>(e: &'a Expr, ctx: &Ctx<'a, '_>) -> Result<Cow<'a, Value>> {
    match e {
        Expr::Column(c) => {
            let ti = ctx
                .layout
                .iter()
                .position(|t| *t == c.table)
                .ok_or_else(|| Error::Conversion(format!("table `{}` is not part of the query", c.table)))?;
            Ok(match ctx.parts[ti] {
                Some(row) => Cow::Borrowed(row.get(c.index).unwrap_or(&NULL)),
                None => Cow::Borrowed(&NULL),
            })
        }
        Expr::Value(v) => Ok(Cow::Borrowed(v)),
        Expr::Binary { op, left, right } => match op {
            BinOp::And => {
                let l = tri(&*eval(left, ctx)?);
                if l == Some(false) {
                    return Ok(Cow::Owned(Value::Bool(false)));
                }
                let r = tri(&*eval(right, ctx)?);
                Ok(Cow::Owned(match (l, r) {
                    (_, Some(false)) => Value::Bool(false),
                    (Some(true), Some(true)) => Value::Bool(true),
                    _ => Value::Null,
                }))
            }
            BinOp::Or => {
                let l = tri(&*eval(left, ctx)?);
                if l == Some(true) {
                    return Ok(Cow::Owned(Value::Bool(true)));
                }
                let r = tri(&*eval(right, ctx)?);
                Ok(Cow::Owned(match (l, r) {
                    (_, Some(true)) => Value::Bool(true),
                    (Some(false), Some(false)) => Value::Bool(false),
                    _ => Value::Null,
                }))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let (l, r) = (eval(left, ctx)?, eval(right, ctx)?);
                Ok(Cow::Owned(arith(*op, &l, &r)?))
            }
            cmp_op => {
                let (l, r) = (eval(left, ctx)?, eval(right, ctx)?);
                if l.is_null() || r.is_null() {
                    return Ok(Cow::Owned(Value::Null));
                }
                let ord = compare(&l, &r).ok_or_else(|| incomparable(&l, &r))?;
                let b = match cmp_op {
                    BinOp::Eq => ord == Ordering::Equal,
                    BinOp::Ne => ord != Ordering::Equal,
                    BinOp::Lt => ord == Ordering::Less,
                    BinOp::Le => ord != Ordering::Greater,
                    BinOp::Gt => ord == Ordering::Greater,
                    BinOp::Ge => ord != Ordering::Less,
                    _ => unreachable!(),
                };
                Ok(Cow::Owned(Value::Bool(b)))
            }
        },
        Expr::Not(inner) => Ok(Cow::Owned(bool_or_null(tri(&*eval(inner, ctx)?).map(|b| !b)))),
        Expr::IsNull { expr, negated } => Ok(Cow::Owned(Value::Bool(eval(expr, ctx)?.is_null() != *negated))),
        Expr::InList { expr, list, negated } => {
            let v = eval(expr, ctx)?;
            if list.is_empty() {
                return Ok(Cow::Owned(Value::Bool(*negated)));
            }
            if v.is_null() {
                return Ok(Cow::Owned(Value::Null));
            }
            let mut saw_null = false;
            let mut found = false;
            for item in list {
                if item.is_null() {
                    saw_null = true;
                } else if compare(&v, item) == Some(Ordering::Equal) {
                    found = true;
                    break;
                }
            }
            let res = if found {
                Some(true)
            } else if saw_null {
                None
            } else {
                Some(false)
            };
            Ok(Cow::Owned(bool_or_null(res.map(|b| b != *negated))))
        }
        Expr::Between { expr, low, high, negated } => {
            let (v, lo, hi) = (eval(expr, ctx)?, eval(low, ctx)?, eval(high, ctx)?);
            if v.is_null() || lo.is_null() || hi.is_null() {
                return Ok(Cow::Owned(Value::Null));
            }
            let a = compare(&v, &lo).ok_or_else(|| incomparable(&v, &lo))?;
            let b = compare(&v, &hi).ok_or_else(|| incomparable(&v, &hi))?;
            let inside = a != Ordering::Less && b != Ordering::Greater;
            Ok(Cow::Owned(Value::Bool(inside != *negated)))
        }
        Expr::Like { expr, pattern, case_insensitive, negated, escaped } => {
            let (v, p) = (eval(expr, ctx)?, eval(pattern, ctx)?);
            let (Some(s), Some(pat)) = (v.as_str(), p.as_str()) else {
                if v.is_null() || p.is_null() {
                    return Ok(Cow::Owned(Value::Null));
                }
                return Err(Error::Conversion(format!("LIKE expects text, found {}", v.type_name())));
            };
            let m = if *case_insensitive {
                like_match(&s.to_lowercase(), &pat.to_lowercase(), *escaped)
            } else {
                like_match(s, pat, *escaped)
            };
            Ok(Cow::Owned(Value::Bool(m != *negated)))
        }
        Expr::Aggregate { func, arg, distinct } => {
            let group = ctx
                .group
                .ok_or_else(|| Error::Unsupported("aggregate function outside of an aggregate query".into()))?;
            Ok(Cow::Owned(aggregate(*func, arg.as_deref(), *distinct, group, ctx.layout)?))
        }
        #[allow(unreachable_patterns)]
        _ => Err(Error::Unsupported("expression not supported by the memory engine".into())),
    }
}

fn aggregate<'a>(
    func: AggFunc,
    arg: Option<&'a Expr>,
    distinct: bool,
    group: Group<'a, '_>,
    layout: &[&'static str],
) -> Result<Value> {
    let Some(arg) = arg else {
        // COUNT(*)
        return Ok(Value::I64(group.members.len() as i64));
    };
    let mut values: Vec<Value> = Vec::with_capacity(group.members.len());
    for &m in group.members {
        let parts = group.rows.get(m);
        let ctx = Ctx { layout, parts, group: None };
        let v = eval(arg, &ctx)?;
        if !v.is_null() {
            values.push(v.into_owned());
        }
    }
    if distinct {
        let mut seen = std::collections::HashSet::with_capacity(values.len());
        values.retain(|v| seen.insert(Key(v.clone())));
    }
    Ok(match func {
        AggFunc::Count => Value::I64(values.len() as i64),
        AggFunc::Sum => {
            if values.is_empty() {
                Value::Null
            } else if values.iter().all(|v| v.as_i64().is_some()) {
                let mut acc: i64 = 0;
                for v in &values {
                    acc = acc
                        .checked_add(v.as_i64().unwrap_or(0))
                        .ok_or_else(|| Error::Conversion("integer overflow in SUM".into()))?;
                }
                Value::I64(acc)
            } else {
                let mut acc = 0.0;
                for v in &values {
                    acc += v.as_f64().ok_or_else(|| Error::Conversion(format!("cannot SUM {}", v.type_name())))?;
                }
                Value::F64(acc)
            }
        }
        AggFunc::Avg => {
            if values.is_empty() {
                Value::Null
            } else {
                let mut acc = 0.0;
                for v in &values {
                    acc += v.as_f64().ok_or_else(|| Error::Conversion(format!("cannot AVG {}", v.type_name())))?;
                }
                Value::F64(acc / values.len() as f64)
            }
        }
        AggFunc::Min | AggFunc::Max => {
            let want = if func == AggFunc::Min { Ordering::Less } else { Ordering::Greater };
            let mut best: Option<Value> = None;
            for v in values {
                best = match best {
                    None => Some(v),
                    Some(b) => {
                        let ord = compare(&v, &b).ok_or_else(|| incomparable(&v, &b))?;
                        Some(if ord == want { v } else { b })
                    }
                };
            }
            best.unwrap_or(Value::Null)
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    Lit(char),
    One,
    Any,
}

/// SQL `LIKE` matching (`%` = any sequence, `_` = any char, `\` escapes when
/// `escaped` is set).
pub(crate) fn like_match(s: &str, pattern: &str, escaped: bool) -> bool {
    let mut toks: Vec<Tok> = Vec::with_capacity(pattern.len());
    let mut it = pattern.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' if escaped => toks.push(Tok::Lit(it.next().unwrap_or('\\'))),
            '%' => {
                if toks.last() != Some(&Tok::Any) {
                    toks.push(Tok::Any);
                }
            }
            '_' => toks.push(Tok::One),
            c => toks.push(Tok::Lit(c)),
        }
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut si, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < chars.len() {
        match toks.get(pi) {
            Some(Tok::Lit(c)) if *c == chars[si] => {
                si += 1;
                pi += 1;
            }
            Some(Tok::One) => {
                si += 1;
                pi += 1;
            }
            Some(Tok::Any) => {
                star = Some((pi, si));
                pi += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    toks[pi..].iter().all(|t| *t == Tok::Any)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_patterns() {
        assert!(like_match("alice", "a%", false));
        assert!(like_match("alice", "%li%", false));
        assert!(like_match("alice", "_lice", false));
        assert!(!like_match("alice", "_lic", false));
        assert!(like_match("", "%", false));
        assert!(!like_match("Alice", "a%", false));
        assert!(like_match("50%", "50\\%", true));
        assert!(!like_match("500", "50\\%", true));
        assert!(like_match("a_b", "a\\_b", true));
        assert!(!like_match("axb", "a\\_b", true));
        assert!(like_match("mississippi", "%iss%ppi", false));
    }

    #[test]
    fn keys_normalise_numbers() {
        assert_eq!(Key(Value::I32(3)), Key(Value::I64(3)));
        assert_eq!(Key(Value::F64(3.0)), Key(Value::I64(3)));
        assert_ne!(Key(Value::F64(3.5)), Key(Value::I64(3)));
        assert_ne!(Key(Value::Text("3".into())), Key(Value::I64(3)));
    }

    #[test]
    fn null_ordering() {
        let asc = Order { expr: Expr::always(), desc: false, nulls: None };
        assert_eq!(order_cmp(&Value::Null, &Value::I32(1), &asc), Ordering::Greater);
        let first = asc.clone().nulls_first();
        assert_eq!(order_cmp(&Value::Null, &Value::I32(1), &first), Ordering::Less);
    }
}
