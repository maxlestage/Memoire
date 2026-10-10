//! Typed query builders: [`Select`], [`Update`] and [`Delete`].

use std::marker::PhantomData;

use crate::ast::{DeleteAst, Join, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::expr::{Column, Expr, Numeric, Order, count_all};
use crate::model::Model;
use crate::row::FromRow;
use crate::schema::Dialect;
use crate::sql;
use crate::value::{Bind, FromValue, SqlType, Value};

/// Output type of a join: appends `P` to the current output tuple.
pub trait Joined<P> {
    /// New output type.
    type Out: FromRow;
}

impl<A: Model, P: FromRow> Joined<P> for A {
    type Out = (A, P);
}
impl<A: FromRow, B: FromRow, P: FromRow> Joined<P> for (A, B) {
    type Out = (A, B, P);
}
impl<A: FromRow, B: FromRow, C: FromRow, P: FromRow> Joined<P> for (A, B, C) {
    type Out = (A, B, C, P);
}
impl<A: FromRow, B: FromRow, C: FromRow, D: FromRow, P: FromRow> Joined<P> for (A, B, C, D) {
    type Out = (A, B, C, D, P);
}

/// A set of typed columns selected with [`Select::columns`].
pub trait ColumnList {
    /// Decoded row type.
    type Output: FromRow;
    /// The column expressions.
    fn exprs(self) -> Vec<Expr>;
}

impl<M, T: FromRow> ColumnList for Column<M, T> {
    type Output = T;
    fn exprs(self) -> Vec<Expr> {
        vec![self.expr()]
    }
}

macro_rules! column_list_tuple {
    ($($m:ident $t:ident $i:tt),+) => {
        impl<$($m, $t: FromRow),+> ColumnList for ($(Column<$m, $t>,)+) {
            type Output = ($($t,)+);
            fn exprs(self) -> Vec<Expr> {
                vec![$(self.$i.expr()),+]
            }
        }
    };
}

column_list_tuple!(M0 T0 0);
column_list_tuple!(M0 T0 0, M1 T1 1);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2, M3 T3 3);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2, M3 T3 3, M4 T4 4);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2, M3 T3 3, M4 T4 4, M5 T5 5);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2, M3 T3 3, M4 T4 4, M5 T5 5, M6 T6 6);
column_list_tuple!(M0 T0 0, M1 T1 1, M2 T2 2, M3 T3 3, M4 T4 4, M5 T5 5, M6 T6 6, M7 T7 7);

/// Result type of `SUM` for a numeric Rust type (`i64` for integers, `f64` for
/// floats).
pub trait SumType {
    /// Decoded sum type.
    type Sum: FromValue + Send;
}
macro_rules! sum_type {
    ($sum:ty: $($t:ty),*) => {$(impl SumType for $t { type Sum = $sum; })*};
}
sum_type!(i64: i8, i16, i32, i64, u8, u16, u32);
sum_type!(f64: f32, f64);

/// One page of results, returned by [`Select::paginate`].
#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    /// Rows of this page.
    pub items: Vec<T>,
    /// 1-based page number.
    pub page: u64,
    /// Page size.
    pub per_page: u64,
    /// Total number of matching rows.
    pub total: u64,
}

impl<T> Page<T> {
    /// Number of pages.
    pub fn total_pages(&self) -> u64 {
        if self.per_page == 0 { 0 } else { self.total.div_ceil(self.per_page) }
    }

    /// `true` if there is a page after this one.
    pub fn has_next(&self) -> bool {
        self.page < self.total_pages()
    }

    /// `true` if there is a page before this one.
    pub fn has_prev(&self) -> bool {
        self.page > 1
    }
}

/// A `SELECT` statement producing rows of type `O`.
///
/// Built with `Model::query()`; every method consumes and returns the builder.
///
/// ```
/// # use memoire_orm::prelude::*;
/// # #[derive(Model)] #[memoire(table = "users")]
/// # struct User { id: i64, name: String, age: Option<i32> }
/// let q = User::query()
///     .filter(User::AGE.ge(18) & User::NAME.starts_with("A"))
///     .order_by(User::NAME.asc())
///     .limit(10);
/// assert_eq!(
///     q.to_sql(Dialect::Postgres),
///     r#"SELECT "users"."id", "users"."name", "users"."age" FROM "users" WHERE ("users"."age" >= $1 AND "users"."name" LIKE $2 ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 10"#
/// );
/// ```
#[must_use = "a query does nothing until it is executed with `.all(&db)`, `.first(&db)`, ..."]
pub struct Select<O> {
    ast: SelectAst,
    _out: PhantomData<fn() -> O>,
}

impl<O> Clone for Select<O> {
    fn clone(&self) -> Self {
        Select { ast: self.ast.clone(), _out: PhantomData }
    }
}

impl<O> std::fmt::Debug for Select<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Select").field("ast", &self.ast).finish()
    }
}

impl<M: Model> Select<M> {
    /// `SELECT * FROM <M>`.
    pub fn new() -> Self {
        Select { ast: SelectAst::new(M::SCHEMA), _out: PhantomData }
    }
}

impl<M: Model> Default for Select<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<O> Select<O> {
    /// Wraps a hand-built AST. The caller guarantees that the projection
    /// decodes into `O`.
    pub fn from_ast(ast: SelectAst) -> Self {
        Select { ast, _out: PhantomData }
    }

    /// The underlying statement.
    pub fn ast(&self) -> &SelectAst {
        &self.ast
    }

    /// Consumes the builder and returns the statement.
    pub fn into_ast(self) -> SelectAst {
        self.ast
    }

    fn cast<P>(self) -> Select<P> {
        Select { ast: self.ast, _out: PhantomData }
    }

    /// Adds a `WHERE` condition (combined with `AND` with previous ones).
    pub fn filter(mut self, condition: Expr) -> Self {
        self.ast.filter = Some(match self.ast.filter.take() {
            Some(prev) => prev.and(condition),
            None => condition,
        });
        self
    }

    /// Adds a condition combined with `OR` with the previous ones.
    pub fn or_filter(mut self, condition: Expr) -> Self {
        self.ast.filter = Some(match self.ast.filter.take() {
            Some(prev) => prev.or(condition),
            None => condition,
        });
        self
    }

    /// Adds an `ORDER BY` term (`User::NAME.asc()`, `User::AGE.desc()`, or a
    /// bare column for ascending order).
    pub fn order_by(mut self, order: impl Into<Order>) -> Self {
        self.ast.order.push(order.into());
        self
    }

    /// `LIMIT n`.
    pub fn limit(mut self, n: u64) -> Self {
        self.ast.limit = Some(n);
        self
    }

    /// `OFFSET n`.
    pub fn offset(mut self, n: u64) -> Self {
        self.ast.offset = Some(n);
        self
    }

    /// Selects one page (1-based) of `per_page` rows.
    pub fn page(self, page: u64, per_page: u64) -> Self {
        let page = page.max(1);
        self.limit(per_page).offset((page - 1).saturating_mul(per_page))
    }

    /// `SELECT DISTINCT`.
    pub fn distinct(mut self) -> Self {
        self.ast.distinct = true;
        self
    }

    /// Adds a `GROUP BY` expression.
    pub fn group_by(mut self, expr: impl Into<Expr>) -> Self {
        self.ast.group_by.push(expr.into());
        self
    }

    /// Adds a `HAVING` condition.
    pub fn having(mut self, condition: Expr) -> Self {
        self.ast.having = Some(match self.ast.having.take() {
            Some(prev) => prev.and(condition),
            None => condition,
        });
        self
    }

    fn push_join<P: Model>(&mut self, kind: JoinKind, on: Expr, select: bool) {
        self.ast.joins.push(Join { kind, table: P::SCHEMA, on });
        if select {
            match &mut self.ast.projection {
                Projection::Tables(t) => t.push(P::SCHEMA),
                Projection::Exprs(e) => e.extend(P::SCHEMA.columns.iter().enumerate().map(|(i, c)| {
                    Expr::Column(crate::expr::ColumnRef { table: P::SCHEMA.name, name: c.name, index: i, kind: c.kind })
                })),
            }
        }
    }

    /// `INNER JOIN P ON condition`, adding `P` to the output:
    /// `Select<User>` becomes `Select<(User, Post)>`.
    pub fn inner_join<P: Model>(mut self, on: Expr) -> Select<<O as Joined<P>>::Out>
    where
        O: Joined<P>,
    {
        self.push_join::<P>(JoinKind::Inner, on, true);
        self.cast()
    }

    /// `LEFT JOIN P ON condition`, adding `Option<P>` to the output:
    /// `Select<User>` becomes `Select<(User, Option<Post>)>`.
    pub fn left_join<P: Model>(mut self, on: Expr) -> Select<<O as Joined<Option<P>>>::Out>
    where
        O: Joined<Option<P>>,
    {
        self.push_join::<P>(JoinKind::Left, on, true);
        self.cast()
    }

    /// Joins `P` only to filter on it; the output type does not change.
    /// Combine with [`distinct`](Self::distinct) to avoid duplicate rows.
    pub fn join<P: Model>(mut self, kind: JoinKind, on: Expr) -> Self {
        self.push_join::<P>(kind, on, false);
        self
    }

    /// Selects only some typed columns:
    /// `User::query().columns((User::ID, User::NAME))` yields `(i64, String)`.
    pub fn columns<C: ColumnList>(mut self, columns: C) -> Select<C::Output> {
        self.ast.projection = Projection::Exprs(columns.exprs());
        self.cast()
    }

    /// Selects arbitrary expressions decoded by position into `T`
    /// (aggregates, `GROUP BY` reports, ...).
    pub fn select_exprs<T: FromRow>(mut self, exprs: impl IntoIterator<Item = Expr>) -> Select<T> {
        self.ast.projection = Projection::Exprs(exprs.into_iter().collect());
        self.cast()
    }

    /// Renders the statement for a dialect (parameters are not inlined).
    pub fn to_sql(&self, dialect: Dialect) -> String {
        sql::render(&Query::Select(&self.ast), dialect).map(|r| r.sql.into_owned()).unwrap_or_default()
    }

    /// Renders the statement and returns its parameters.
    pub fn to_sql_with_params(&self, dialect: Dialect) -> (String, Vec<Value>) {
        match sql::render(&Query::Select(&self.ast), dialect) {
            Ok(r) => {
                let params = r.owned_params();
                (r.sql.into_owned(), params)
            }
            Err(_) => (String::new(), Vec::new()),
        }
    }

    fn is_grouped(&self) -> bool {
        self.ast.distinct
            || !self.ast.group_by.is_empty()
            || matches!(&self.ast.projection, Projection::Exprs(e) if e.iter().any(Expr::has_aggregate))
    }

    fn scalar_ast(&self, expr: Expr) -> SelectAst {
        SelectAst {
            from: self.ast.from,
            projection: Projection::Exprs(vec![expr]),
            joins: self.ast.joins.clone(),
            filter: self.ast.filter.clone(),
            group_by: Vec::new(),
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            distinct: false,
        }
    }
}

impl<O: FromRow + Send> Select<O> {
    /// Runs the query and returns every row.
    pub async fn all<E: Executor>(self, db: &E) -> Result<Vec<O>> {
        db.fetch_all(Query::Select(&self.ast)).await
    }

    /// Returns the first row, if any (adds `LIMIT 1`).
    pub async fn first<E: Executor>(mut self, db: &E) -> Result<Option<O>> {
        self.ast.limit = Some(1);
        let rows: Vec<O> = db.fetch_all(Query::Select(&self.ast)).await?;
        Ok(rows.into_iter().next())
    }

    /// Returns the first row or [`Error::NotFound`].
    pub async fn one<E: Executor>(self, db: &E) -> Result<O> {
        self.first(db).await?.ok_or(Error::NotFound)
    }

    /// Counts matching rows (`ORDER BY`, `LIMIT` and `OFFSET` are ignored).
    pub async fn count<E: Executor>(self, db: &E) -> Result<u64> {
        if self.is_grouped() {
            let mut ast = self.ast;
            ast.limit = None;
            ast.offset = None;
            let rows: Vec<O> = db.fetch_all(Query::Select(&ast)).await?;
            return Ok(rows.len() as u64);
        }
        let ast = self.scalar_ast(count_all());
        let rows: Vec<i64> = db.fetch_all(Query::Select(&ast)).await?;
        Ok(rows.first().copied().unwrap_or(0).max(0) as u64)
    }

    /// `true` if at least one row matches.
    pub async fn exists<E: Executor>(self, db: &E) -> Result<bool> {
        if self.is_grouped() {
            return Ok(!self.limit(1).all(db).await?.is_empty());
        }
        let pk = self.ast.from.primary_key;
        let col = &self.ast.from.columns[pk];
        let mut ast = self.scalar_ast(Expr::Column(crate::expr::ColumnRef {
            table: self.ast.from.name,
            name: col.name,
            index: pk,
            kind: col.kind,
        }));
        ast.limit = Some(1);
        let rows: Vec<Value> = db.fetch_all(Query::Select(&ast)).await?;
        Ok(!rows.is_empty())
    }

    /// Returns one page of results plus the total count (two queries).
    pub async fn paginate<E: Executor>(self, db: &E, page: u64, per_page: u64) -> Result<Page<O>> {
        let page = page.max(1);
        let total = self.clone().count(db).await?;
        let items = self.page(page, per_page).all(db).await?;
        Ok(Page { items, page, per_page, total })
    }

    async fn aggregate<R: FromRow + Send, E: Executor>(self, expr: Expr, db: &E) -> Result<R> {
        let ast = self.scalar_ast(expr);
        let mut rows: Vec<R> = db.fetch_all(Query::Select(&ast)).await?;
        rows.pop().ok_or(Error::NotFound)
    }

    /// `SUM(column)` over the matching rows (`None` when no row matches).
    pub async fn sum<M2, T, E>(self, column: Column<M2, T>, db: &E) -> Result<Option<<T::Base as SumType>::Sum>>
    where
        T: SqlType,
        T::Base: Numeric + SumType,
        E: Executor,
    {
        let v: Value = self.aggregate(column.sum(), db).await?;
        Option::from_value(v)
    }

    /// `AVG(column)` over the matching rows.
    pub async fn avg<M2, T, E>(self, column: Column<M2, T>, db: &E) -> Result<Option<f64>>
    where
        T: SqlType,
        T::Base: Numeric,
        E: Executor,
    {
        let v: Value = self.aggregate(column.avg(), db).await?;
        Option::from_value(v)
    }

    /// `MIN(column)` over the matching rows.
    pub async fn min<M2, T, E>(self, column: Column<M2, T>, db: &E) -> Result<Option<T::Base>>
    where
        T: SqlType,
        T::Base: FromValue + Send,
        E: Executor,
    {
        let v: Value = self.aggregate(column.min(), db).await?;
        Option::from_value(v)
    }

    /// `MAX(column)` over the matching rows.
    pub async fn max<M2, T, E>(self, column: Column<M2, T>, db: &E) -> Result<Option<T::Base>>
    where
        T: SqlType,
        T::Base: FromValue + Send,
        E: Executor,
    {
        let v: Value = self.aggregate(column.max(), db).await?;
        Option::from_value(v)
    }
}

/// An `UPDATE` statement on model `M`, built with `Model::update_many()`.
///
/// ```
/// # use memoire_orm::prelude::*;
/// # #[derive(Model)] #[memoire(table = "users")]
/// # struct User { id: i64, name: String, visits: i32, active: bool }
/// let q = User::update_many()
///     .set(User::ACTIVE, false)
///     .increment(User::VISITS, 1)
///     .filter(User::NAME.eq("bob"));
/// assert_eq!(
///     q.to_sql(Dialect::Sqlite),
///     r#"UPDATE "users" SET "active" = ?, "visits" = ("users"."visits" + ?) WHERE "users"."name" = ?"#
/// );
/// ```
#[must_use = "an update does nothing until `.execute(&db)` is called"]
pub struct Update<M> {
    ast: UpdateAst,
    _m: PhantomData<fn() -> M>,
}

impl<M> Clone for Update<M> {
    fn clone(&self) -> Self {
        Update { ast: self.ast.clone(), _m: PhantomData }
    }
}

impl<M> std::fmt::Debug for Update<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Update").field("ast", &self.ast).finish()
    }
}

impl<M: Model> Default for Update<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> Update<M> {
    /// `UPDATE <M>` without any assignment yet.
    pub fn new() -> Self {
        Update { ast: UpdateAst { table: M::SCHEMA, sets: Vec::new(), filter: None }, _m: PhantomData }
    }

    /// Adds an assignment; assigning the same column again replaces it.
    fn assign(mut self, index: usize, expr: Expr) -> Self {
        match self.ast.sets.iter_mut().find(|(i, _)| *i == index) {
            Some(slot) => slot.1 = expr,
            None => self.ast.sets.push((index, expr)),
        }
        self
    }

    /// `SET column = value`.
    pub fn set<T, V: Bind<T>>(self, column: Column<M, T>, value: V) -> Self {
        self.assign(column.column_ref().index, Expr::Value(value.into_bind()))
    }

    /// `SET column = NULL`.
    pub fn set_null<T>(self, column: Column<M, Option<T>>) -> Self {
        self.assign(column.column_ref().index, Expr::Value(Value::Null))
    }

    /// `SET column = <expression>`.
    pub fn set_expr<T>(self, column: Column<M, T>, expr: Expr) -> Self {
        self.assign(column.column_ref().index, expr)
    }

    /// `SET column = column + by`.
    pub fn increment<T>(self, column: Column<M, T>, by: impl Bind<T::Base>) -> Self
    where
        T: SqlType,
        T::Base: Numeric,
    {
        self.set_expr(column, column.add(by))
    }

    /// `SET column = column - by`.
    pub fn decrement<T>(self, column: Column<M, T>, by: impl Bind<T::Base>) -> Self
    where
        T: SqlType,
        T::Base: Numeric,
    {
        self.set_expr(column, column.sub(by))
    }

    /// Adds a `WHERE` condition (combined with `AND`). Without any filter,
    /// every row is updated.
    pub fn filter(mut self, condition: Expr) -> Self {
        self.ast.filter = Some(match self.ast.filter.take() {
            Some(prev) => prev.and(condition),
            None => condition,
        });
        self
    }

    /// The underlying statement.
    pub fn ast(&self) -> &UpdateAst {
        &self.ast
    }

    /// Renders the statement for a dialect.
    pub fn to_sql(&self, dialect: Dialect) -> String {
        sql::render(&Query::Update(&self.ast), dialect).map(|r| r.sql.into_owned()).unwrap_or_default()
    }

    /// Runs the update and returns the number of updated rows.
    pub async fn execute<E: Executor>(self, db: &E) -> Result<u64> {
        Ok(db.execute(Query::Update(&self.ast)).await?.rows_affected)
    }
}

/// A `DELETE` statement on model `M`, built with `Model::delete_many()`.
#[must_use = "a delete does nothing until `.execute(&db)` is called"]
pub struct Delete<M> {
    ast: DeleteAst,
    _m: PhantomData<fn() -> M>,
}

impl<M> Clone for Delete<M> {
    fn clone(&self) -> Self {
        Delete { ast: self.ast.clone(), _m: PhantomData }
    }
}

impl<M> std::fmt::Debug for Delete<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Delete").field("ast", &self.ast).finish()
    }
}

impl<M: Model> Default for Delete<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> Delete<M> {
    /// `DELETE FROM <M>` (every row until a filter is added).
    pub fn new() -> Self {
        Delete { ast: DeleteAst { table: M::SCHEMA, filter: None }, _m: PhantomData }
    }

    /// Adds a `WHERE` condition (combined with `AND`).
    pub fn filter(mut self, condition: Expr) -> Self {
        self.ast.filter = Some(match self.ast.filter.take() {
            Some(prev) => prev.and(condition),
            None => condition,
        });
        self
    }

    /// The underlying statement.
    pub fn ast(&self) -> &DeleteAst {
        &self.ast
    }

    /// Renders the statement for a dialect.
    pub fn to_sql(&self, dialect: Dialect) -> String {
        sql::render(&Query::Delete(&self.ast), dialect).map(|r| r.sql.into_owned()).unwrap_or_default()
    }

    /// Runs the delete and returns the number of deleted rows.
    pub async fn execute<E: Executor>(self, db: &E) -> Result<u64> {
        Ok(db.execute(Query::Delete(&self.ast)).await?.rows_affected)
    }
}
