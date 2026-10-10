//! Typed columns and the expression tree used by `WHERE`, `ORDER BY`, `SET` and
//! projections.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{BitAnd, BitOr, Not};

use crate::value::{Bind, ColumnType, IntoValue, SqlType, Value};

/// Untyped reference to a column of a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColumnRef {
    /// Table name.
    pub table: &'static str,
    /// Column name.
    pub name: &'static str,
    /// Position of the column in the table schema.
    pub index: usize,
    /// Logical type of the column.
    pub kind: ColumnType,
}

/// A typed column of model `M` holding Rust values of type `T`.
///
/// `#[derive(Model)]` generates one associated constant per field
/// (`User::EMAIL`, `User::AGE`, ...). Every comparison is checked at compile
/// time against the column type.
pub struct Column<M, T> {
    col: ColumnRef,
    _marker: PhantomData<fn() -> (M, T)>,
}

impl<M, T> Clone for Column<M, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, T> Copy for Column<M, T> {}

impl<M, T> fmt::Debug for Column<M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Column({}.{})", self.col.table, self.col.name)
    }
}

/// Marker trait for numeric Rust types (enables `sum`, `avg` and arithmetic).
pub trait Numeric {}
macro_rules! numeric {
    ($($t:ty),*) => {$(impl Numeric for $t {})*};
}
numeric!(i8, i16, i32, i64, u8, u16, u32, f32, f64);

impl<M, T: SqlType> Column<M, T> {
    /// Creates a column reference. Used by the derive macro.
    pub const fn new(table: &'static str, name: &'static str, index: usize) -> Self {
        Column { col: ColumnRef { table, name, index, kind: T::KIND }, _marker: PhantomData }
    }
}

impl<M, T> Column<M, T> {
    /// The untyped column reference.
    #[inline]
    pub const fn column_ref(&self) -> ColumnRef {
        self.col
    }

    /// Column name.
    #[inline]
    pub const fn name(&self) -> &'static str {
        self.col.name
    }

    /// Turns the column into an expression.
    #[inline]
    pub fn expr(self) -> Expr {
        Expr::Column(self.col)
    }

    /// `ORDER BY column ASC`.
    #[inline]
    pub fn asc(self) -> Order {
        Order { expr: self.expr(), desc: false, nulls: None }
    }

    /// `ORDER BY column DESC`.
    #[inline]
    pub fn desc(self) -> Order {
        Order { expr: self.expr(), desc: true, nulls: None }
    }

    /// `column IS NULL`.
    #[inline]
    pub fn is_null(self) -> Expr {
        Expr::IsNull { expr: Box::new(self.expr()), negated: false }
    }

    /// `column IS NOT NULL`.
    #[inline]
    pub fn is_not_null(self) -> Expr {
        Expr::IsNull { expr: Box::new(self.expr()), negated: true }
    }

    /// `COUNT(column)` (counts non-`NULL` values).
    pub fn count(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Count, arg: Some(Box::new(self.expr())), distinct: false }
    }

    /// `COUNT(DISTINCT column)`.
    pub fn count_distinct(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Count, arg: Some(Box::new(self.expr())), distinct: true }
    }

    /// `MIN(column)`.
    pub fn min(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Min, arg: Some(Box::new(self.expr())), distinct: false }
    }

    /// `MAX(column)`.
    pub fn max(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Max, arg: Some(Box::new(self.expr())), distinct: false }
    }
}

macro_rules! cmp_methods {
    ($($(#[$doc:meta])* $name:ident => $op:ident, $col_name:ident;)*) => {
        impl<M, T: SqlType> Column<M, T> {
            $(
                $(#[$doc])*
                #[inline]
                pub fn $name<V: Bind<T::Base>>(self, value: V) -> Expr {
                    Expr::binary(BinOp::$op, self.expr(), Expr::Value(value.into_bind()))
                }

                $(#[$doc])*
                /// (column to column comparison, typically for joins).
                #[inline]
                pub fn $col_name<M2, T2: SqlType<Base = T::Base>>(self, other: Column<M2, T2>) -> Expr {
                    Expr::binary(BinOp::$op, self.expr(), other.expr())
                }
            )*
        }
    };
}

cmp_methods! {
    /// `column = value`
    eq => Eq, eq_col;
    /// `column <> value`
    ne => Ne, ne_col;
    /// `column < value`
    lt => Lt, lt_col;
    /// `column <= value`
    le => Le, le_col;
    /// `column > value`
    gt => Gt, gt_col;
    /// `column >= value`
    ge => Ge, ge_col;
}

impl<M, T: SqlType> Column<M, T> {
    /// `column IN (v1, v2, ...)`. An empty list is always false.
    pub fn is_in<I, V>(self, values: I) -> Expr
    where
        I: IntoIterator<Item = V>,
        V: Bind<T::Base>,
    {
        Expr::InList {
            expr: Box::new(self.expr()),
            list: values.into_iter().map(Bind::into_bind).collect(),
            negated: false,
        }
    }

    /// `column NOT IN (v1, v2, ...)`. An empty list is always true.
    pub fn not_in<I, V>(self, values: I) -> Expr
    where
        I: IntoIterator<Item = V>,
        V: Bind<T::Base>,
    {
        Expr::InList {
            expr: Box::new(self.expr()),
            list: values.into_iter().map(Bind::into_bind).collect(),
            negated: true,
        }
    }

    /// `column BETWEEN low AND high` (inclusive).
    pub fn between<A: Bind<T::Base>, B: Bind<T::Base>>(self, low: A, high: B) -> Expr {
        Expr::Between {
            expr: Box::new(self.expr()),
            low: Box::new(Expr::Value(low.into_bind())),
            high: Box::new(Expr::Value(high.into_bind())),
            negated: false,
        }
    }

    /// `column NOT BETWEEN low AND high`.
    pub fn not_between<A: Bind<T::Base>, B: Bind<T::Base>>(self, low: A, high: B) -> Expr {
        Expr::Between {
            expr: Box::new(self.expr()),
            low: Box::new(Expr::Value(low.into_bind())),
            high: Box::new(Expr::Value(high.into_bind())),
            negated: true,
        }
    }
}

/// Escapes `%`, `_` and `\` so a user string matches literally inside `LIKE`.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl<M, T: SqlType<Base = String>> Column<M, T> {
    fn like_expr(self, pattern: String, case_insensitive: bool, negated: bool, escaped: bool) -> Expr {
        Expr::Like {
            expr: Box::new(self.expr()),
            pattern: Box::new(Expr::Value(Value::Text(pattern))),
            case_insensitive,
            negated,
            escaped,
        }
    }

    /// `column LIKE pattern` (`%` and `_` are wildcards).
    ///
    /// Case sensitivity follows the database: SQLite and MySQL compare ASCII
    /// case-insensitively by default, PostgreSQL and the memory engine do not.
    /// Use [`ilike`](Self::ilike) for portable case-insensitive matching.
    pub fn like(self, pattern: impl Into<String>) -> Expr {
        self.like_expr(pattern.into(), false, false, false)
    }

    /// `column NOT LIKE pattern`.
    pub fn not_like(self, pattern: impl Into<String>) -> Expr {
        self.like_expr(pattern.into(), false, true, false)
    }

    /// Case-insensitive `LIKE` on every backend.
    pub fn ilike(self, pattern: impl Into<String>) -> Expr {
        self.like_expr(pattern.into(), true, false, false)
    }

    /// The column contains `needle` (wildcards in `needle` are escaped).
    pub fn contains(self, needle: impl AsRef<str>) -> Expr {
        self.like_expr(format!("%{}%", escape_like(needle.as_ref())), false, false, true)
    }

    /// Case-insensitive [`contains`](Self::contains).
    pub fn icontains(self, needle: impl AsRef<str>) -> Expr {
        self.like_expr(format!("%{}%", escape_like(needle.as_ref())), true, false, true)
    }

    /// The column starts with `prefix`.
    pub fn starts_with(self, prefix: impl AsRef<str>) -> Expr {
        self.like_expr(format!("{}%", escape_like(prefix.as_ref())), false, false, true)
    }

    /// The column ends with `suffix`.
    pub fn ends_with(self, suffix: impl AsRef<str>) -> Expr {
        self.like_expr(format!("%{}", escape_like(suffix.as_ref())), false, false, true)
    }
}

impl<M, T> Column<M, T>
where
    T: SqlType,
    T::Base: Numeric,
{
    /// `column + value`.
    pub fn add<V: Bind<T::Base>>(self, value: V) -> Expr {
        Expr::binary(BinOp::Add, self.expr(), Expr::Value(value.into_bind()))
    }

    /// `column - value`.
    pub fn sub<V: Bind<T::Base>>(self, value: V) -> Expr {
        Expr::binary(BinOp::Sub, self.expr(), Expr::Value(value.into_bind()))
    }

    /// `column * value`.
    pub fn mul<V: Bind<T::Base>>(self, value: V) -> Expr {
        Expr::binary(BinOp::Mul, self.expr(), Expr::Value(value.into_bind()))
    }

    /// `column / value`.
    pub fn div<V: Bind<T::Base>>(self, value: V) -> Expr {
        Expr::binary(BinOp::Div, self.expr(), Expr::Value(value.into_bind()))
    }

    /// `SUM(column)`.
    pub fn sum(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Sum, arg: Some(Box::new(self.expr())), distinct: false }
    }

    /// `AVG(column)`, always a floating point number.
    pub fn avg(self) -> Expr {
        Expr::Aggregate { func: AggFunc::Avg, arg: Some(Box::new(self.expr())), distinct: false }
    }
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `AND`
    And,
    /// `OR`
    Or,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
}

impl BinOp {
    /// SQL spelling of the operator.
    pub const fn as_sql(self) -> &'static str {
        match self {
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "AND",
            BinOp::Or => "OR",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        }
    }
}

/// Aggregate functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AggFunc {
    /// `COUNT`
    Count,
    /// `SUM`
    Sum,
    /// `AVG`
    Avg,
    /// `MIN`
    Min,
    /// `MAX`
    Max,
}

/// A SQL expression.
///
/// Expressions are usually built from typed columns (`User::AGE.gt(18)`) and
/// combined with `&` (AND), `|` (OR) and `!` (NOT).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Expr {
    /// A column.
    Column(ColumnRef),
    /// A bound value.
    Value(Value),
    /// `left op right`.
    Binary {
        /// Operator.
        op: BinOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// `NOT expr`.
    Not(Box<Expr>),
    /// `expr IS [NOT] NULL`.
    IsNull {
        /// Tested expression.
        expr: Box<Expr>,
        /// `IS NOT NULL` when true.
        negated: bool,
    },
    /// `expr [NOT] IN (...)`.
    InList {
        /// Tested expression.
        expr: Box<Expr>,
        /// Candidate values.
        list: Vec<Value>,
        /// `NOT IN` when true.
        negated: bool,
    },
    /// `expr [NOT] BETWEEN low AND high`.
    Between {
        /// Tested expression.
        expr: Box<Expr>,
        /// Lower bound (inclusive).
        low: Box<Expr>,
        /// Upper bound (inclusive).
        high: Box<Expr>,
        /// `NOT BETWEEN` when true.
        negated: bool,
    },
    /// `expr [NOT] LIKE pattern`.
    Like {
        /// Tested expression.
        expr: Box<Expr>,
        /// Pattern.
        pattern: Box<Expr>,
        /// Case-insensitive match.
        case_insensitive: bool,
        /// `NOT LIKE` when true.
        negated: bool,
        /// The pattern uses `\` as escape character.
        escaped: bool,
    },
    /// Aggregate function call; `arg = None` means `COUNT(*)`.
    Aggregate {
        /// Function.
        func: AggFunc,
        /// Argument.
        arg: Option<Box<Expr>>,
        /// `DISTINCT` modifier.
        distinct: bool,
    },
}

impl Expr {
    /// Builds a binary expression.
    #[inline]
    pub fn binary(op: BinOp, left: Expr, right: Expr) -> Expr {
        Expr::Binary { op, left: Box::new(left), right: Box::new(right) }
    }

    /// A bound value.
    #[inline]
    pub fn val(value: impl IntoValue) -> Expr {
        Expr::Value(value.into_value())
    }

    /// Always-true condition.
    pub fn always() -> Expr {
        Expr::Value(Value::Bool(true))
    }

    /// `self AND other`.
    #[inline]
    pub fn and(self, other: Expr) -> Expr {
        Expr::binary(BinOp::And, self, other)
    }

    /// `self OR other`.
    #[inline]
    pub fn or(self, other: Expr) -> Expr {
        Expr::binary(BinOp::Or, self, other)
    }

    /// `NOT self`.
    #[inline]
    pub fn negate(self) -> Expr {
        Expr::Not(Box::new(self))
    }

    /// `self = other` (untyped).
    pub fn eq(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Eq, self, other.into())
    }

    /// `self <> other` (untyped).
    pub fn ne(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Ne, self, other.into())
    }

    /// `self > other` (untyped).
    pub fn gt(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Gt, self, other.into())
    }

    /// `self >= other` (untyped).
    pub fn ge(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Ge, self, other.into())
    }

    /// `self < other` (untyped).
    pub fn lt(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Lt, self, other.into())
    }

    /// `self <= other` (untyped).
    pub fn le(self, other: impl Into<Expr>) -> Expr {
        Expr::binary(BinOp::Le, self, other.into())
    }

    /// `ORDER BY self ASC`.
    pub fn asc(self) -> Order {
        Order { expr: self, desc: false, nulls: None }
    }

    /// `ORDER BY self DESC`.
    pub fn desc(self) -> Order {
        Order { expr: self, desc: true, nulls: None }
    }

    /// AND of every expression (`true` when empty).
    pub fn all(exprs: impl IntoIterator<Item = Expr>) -> Expr {
        exprs.into_iter().reduce(Expr::and).unwrap_or_else(Expr::always)
    }

    /// OR of every expression (`false` when empty).
    pub fn any(exprs: impl IntoIterator<Item = Expr>) -> Expr {
        exprs.into_iter().reduce(Expr::or).unwrap_or(Expr::Value(Value::Bool(false)))
    }

    /// `true` if the expression contains an aggregate function.
    pub fn has_aggregate(&self) -> bool {
        match self {
            Expr::Aggregate { .. } => true,
            Expr::Column(_) | Expr::Value(_) => false,
            Expr::Binary { left, right, .. } => left.has_aggregate() || right.has_aggregate(),
            Expr::Not(e) | Expr::IsNull { expr: e, .. } | Expr::InList { expr: e, .. } => e.has_aggregate(),
            Expr::Between { expr, low, high, .. } => {
                expr.has_aggregate() || low.has_aggregate() || high.has_aggregate()
            }
            Expr::Like { expr, pattern, .. } => expr.has_aggregate() || pattern.has_aggregate(),
        }
    }
}

/// `COUNT(*)`.
pub fn count_all() -> Expr {
    Expr::Aggregate { func: AggFunc::Count, arg: None, distinct: false }
}

impl From<Value> for Expr {
    fn from(v: Value) -> Self {
        Expr::Value(v)
    }
}

impl<M, T> From<Column<M, T>> for Expr {
    fn from(c: Column<M, T>) -> Self {
        c.expr()
    }
}

impl BitAnd for Expr {
    type Output = Expr;
    fn bitand(self, rhs: Expr) -> Expr {
        self.and(rhs)
    }
}

impl BitOr for Expr {
    type Output = Expr;
    fn bitor(self, rhs: Expr) -> Expr {
        self.or(rhs)
    }
}

impl Not for Expr {
    type Output = Expr;
    fn not(self) -> Expr {
        self.negate()
    }
}

/// Placement of `NULL`s in an ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nulls {
    /// `NULLS FIRST`
    First,
    /// `NULLS LAST`
    Last,
}

/// One `ORDER BY` term.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    /// Sorted expression.
    pub expr: Expr,
    /// Descending order.
    pub desc: bool,
    /// Explicit `NULL` placement (database default when `None`).
    pub nulls: Option<Nulls>,
}

impl Order {
    /// Puts `NULL`s first.
    pub fn nulls_first(mut self) -> Self {
        self.nulls = Some(Nulls::First);
        self
    }

    /// Puts `NULL`s last.
    pub fn nulls_last(mut self) -> Self {
        self.nulls = Some(Nulls::Last);
        self
    }
}

impl<M, T> From<Column<M, T>> for Order {
    fn from(c: Column<M, T>) -> Self {
        c.asc()
    }
}
