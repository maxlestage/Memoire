//! Typed query builders: [`Select`], [`Update`] and [`Delete`].

use std::marker::PhantomData;

use crate::ast::{DeleteAst, Join, JoinKind, Projection, Query, SelectAst, UpdateAst};
use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::expr::{Column, ColumnRef, Expr, Numeric, Order, count_all};
use crate::lifecycle::now_for;
use crate::model::Model;
use crate::relation::column_ref;
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

/// Columns of model `M`, given to [`Model::update_only`]: one typed column
/// (`User::NAME`) or a tuple of them (`(User::NAME, User::AGE)`). Columns of
/// another model are rejected at compile time.
pub trait ModelColumns<M> {
    /// Positions of the columns in the schema of `M`.
    fn indexes(self) -> Vec<usize>;
}

impl<M, T> ModelColumns<M> for Column<M, T> {
    fn indexes(self) -> Vec<usize> {
        vec![self.column_ref().index]
    }
}

macro_rules! model_columns_tuple {
    ($($t:ident $i:tt),+) => {
        impl<M, $($t),+> ModelColumns<M> for ($(Column<M, $t>,)+) {
            fn indexes(self) -> Vec<usize> {
                vec![$(self.$i.column_ref().index),+]
            }
        }
    };
}

model_columns_tuple!(T0 0);
model_columns_tuple!(T0 0, T1 1);
model_columns_tuple!(T0 0, T1 1, T2 2);
model_columns_tuple!(T0 0, T1 1, T2 2, T3 3);
model_columns_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4);
model_columns_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
model_columns_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
model_columns_tuple!(T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);

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
    /// The soft delete scope, for a model with soft deletes.
    soft: Option<Box<SoftScope>>,
    _out: PhantomData<fn() -> O>,
}

/// Which rows of a model with soft deletes a query sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trashed {
    /// Only the rows that are not deleted (the default).
    Without,
    /// Every row.
    With,
    /// Only the deleted rows.
    Only,
}

/// The soft delete scope of a query: kept apart from the caller's filters so
/// that it always applies to all of them (`scope AND (a OR b)`), whatever
/// `or_filter` does, and can be lifted with `with_trashed`.
#[derive(Clone, Debug)]
struct SoftScope {
    column: ColumnRef,
    mode: Trashed,
    /// The caller's own `WHERE` condition.
    user: Option<Expr>,
}

impl SoftScope {
    fn filter(&self) -> Option<Expr> {
        let column = || Box::new(Expr::Column(self.column));
        let clause = match self.mode {
            Trashed::Without => Some(Expr::IsNull { expr: column(), negated: false }),
            Trashed::Only => Some(Expr::IsNull { expr: column(), negated: true }),
            Trashed::With => None,
        };
        match (clause, self.user.clone()) {
            (Some(c), Some(u)) => Some(c.and(u)),
            (c, u) => c.or(u),
        }
    }
}

impl<O> Clone for Select<O> {
    fn clone(&self) -> Self {
        Select { ast: self.ast.clone(), soft: self.soft.clone(), _out: PhantomData }
    }
}

impl<O> std::fmt::Debug for Select<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Select").field("ast", &self.ast).finish()
    }
}

impl<M: Model> Select<M> {
    /// `SELECT * FROM <M>`; for a model with [soft deletes](crate::lifecycle),
    /// `WHERE deleted_at IS NULL` (see [`with_trashed`](Self::with_trashed)).
    pub fn new() -> Self {
        let mut select = Select { ast: SelectAst::new(M::SCHEMA), soft: None, _out: PhantomData };
        if let Some(i) = M::DELETED_AT_INDEX {
            select.soft =
                Some(Box::new(SoftScope { column: column_ref(M::SCHEMA, i), mode: Trashed::Without, user: None }));
            select.sync_filter();
        }
        select
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
        Select { ast, soft: None, _out: PhantomData }
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
        Select { ast: self.ast, soft: self.soft, _out: PhantomData }
    }

    /// Writes the soft delete scope and the caller's filters into the statement.
    fn sync_filter(&mut self) {
        if let Some(soft) = &self.soft {
            self.ast.filter = soft.filter();
        }
    }

    /// Combines `condition` with the caller's filters.
    fn combine(mut self, condition: Expr, or: bool) -> Self {
        let slot = match &mut self.soft {
            Some(soft) => &mut soft.user,
            None => &mut self.ast.filter,
        };
        *slot = Some(match slot.take() {
            Some(prev) if or => prev.or(condition),
            Some(prev) => prev.and(condition),
            None => condition,
        });
        self.sync_filter();
        self
    }

    /// Adds a `WHERE` condition (combined with `AND` with previous ones).
    pub fn filter(self, condition: Expr) -> Self {
        self.combine(condition, false)
    }

    /// Adds a condition combined with `OR` with the previous ones.
    ///
    /// On a model with [soft deletes](crate::lifecycle), the scope still
    /// applies to the whole condition: `deleted_at IS NULL AND (a OR b)`.
    pub fn or_filter(self, condition: Expr) -> Self {
        self.combine(condition, true)
    }

    /// Includes the soft-deleted rows (`withTrashed()` in Eloquent). No effect
    /// on a model without [soft deletes](crate::lifecycle).
    pub fn with_trashed(mut self) -> Self {
        if let Some(soft) = &mut self.soft {
            soft.mode = Trashed::With;
        }
        self.sync_filter();
        self
    }

    /// Only the soft-deleted rows (`onlyTrashed()` in Eloquent). On a model
    /// without [soft deletes](crate::lifecycle), no row matches.
    pub fn only_trashed(mut self) -> Self {
        match &mut self.soft {
            Some(soft) => {
                soft.mode = Trashed::Only;
                self.sync_filter();
                self
            }
            None => self.filter(Expr::Value(Value::Bool(false))),
        }
    }

    /// Leaves the soft-deleted rows out again after
    /// [`with_trashed`](Self::with_trashed) (the default of [`Model::query`]).
    pub fn without_trashed(mut self) -> Self {
        if let Some(soft) = &mut self.soft {
            soft.mode = Trashed::Without;
        }
        self.sync_filter();
        self
    }

    /// Applies `scope` to the query: a reusable piece of query, like a local
    /// scope in Eloquent or a criteria in Doctrine.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model)] #[memoire(table = "users")]
    /// # struct User { id: i64, name: String, active: bool, age: Option<i32> }
    /// use memoire_orm::Select;
    ///
    /// fn active(q: Select<User>) -> Select<User> {
    ///     q.filter(User::ACTIVE.eq(true))
    /// }
    /// let q = User::query().apply(active).order_by(User::NAME);
    /// # let _ = q;
    /// ```
    pub fn apply(self, scope: impl FnOnce(Self) -> Self) -> Self {
        scope(self)
    }

    /// Applies `then` only when `condition` holds (`when()` in Laravel), to
    /// build a query without breaking the chain.
    pub fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition { then(self) } else { self }
    }

    /// Applies `then` with the value when there is one: optional filters,
    /// such as the parameters of an HTTP request.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model)] #[memoire(table = "users")]
    /// # struct User { id: i64, name: String, age: Option<i32> }
    /// let min_age: Option<i32> = Some(18);
    /// let name: Option<&str> = None;
    /// let q = User::query()
    ///     .when_some(min_age, |q, age| q.filter(User::AGE.ge(age)))
    ///     .when_some(name, |q, name| q.filter(User::NAME.eq(name)));
    /// assert_eq!(q.to_sql(Dialect::Sqlite), r#"SELECT "users"."id", "users"."name", "users"."age" FROM "users" WHERE "users"."age" >= ?"#);
    /// ```
    pub fn when_some<T>(self, value: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self {
        match value {
            Some(v) => then(self, v),
            None => self,
        }
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

    /// Returns one page of results plus the total count. The two queries run
    /// concurrently: on a connection pool they use two connections at once, so
    /// a page costs one round trip instead of two.
    pub async fn paginate<E: Executor>(self, db: &E, page: u64, per_page: u64) -> Result<Page<O>> {
        let page = page.max(1);
        let (total, items) = crate::relation::join(self.clone().count(db), self.page(page, per_page).all(db)).await;
        Ok(Page { items: items?, page, per_page, total: total? })
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

impl<M: Model> Select<M> {
    /// The first matching row, or the row made by `make`, inserted
    /// (`firstOrCreate` in Eloquent). Returns the row, found or created.
    ///
    /// If the insert fails on a unique constraint because another writer
    /// created the row in the meantime, that row is returned instead.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "tags")]
    /// # struct Tag { #[memoire(primary_key, auto_increment)] id: i64, #[memoire(unique)] name: String }
    /// # memoire_orm::block_on(async {
    /// # let db = MemoryDb::new();
    /// # db.create_table::<Tag>().await?;
    /// let rust = Tag::query()
    ///     .filter(Tag::NAME.eq("rust"))
    ///     .first_or_create(&db, || Tag { id: 0, name: "rust".into() })
    ///     .await?;
    /// let again = Tag::query()
    ///     .filter(Tag::NAME.eq("rust"))
    ///     .first_or_create(&db, || Tag { id: 0, name: "rust".into() })
    ///     .await?;
    /// assert_eq!(rust, again);
    /// assert_eq!(Tag::count(&db).await?, 1);
    /// # Ok::<(), memoire_orm::Error>(())
    /// # }).unwrap();
    /// ```
    pub async fn first_or_create<E: Executor>(self, db: &E, make: impl FnOnce() -> M) -> Result<M> {
        if let Some(found) = self.clone().first(db).await? {
            return Ok(found);
        }
        let mut row = make();
        match row.insert(db).await {
            Ok(()) => Ok(row),
            Err(e) if e.is_unique_violation() => match self.first(db).await? {
                Some(found) => Ok(found),
                None => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    /// Updates the first matching row with `change`, or inserts the row made
    /// by `make` with `change` applied to it (`updateOrCreate` in Eloquent).
    /// Returns the row as stored; the update goes through [`Model::save`], so
    /// its hooks and `updated_at` apply.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "settings")]
    /// # struct Setting { #[memoire(primary_key, auto_increment)] id: i64, #[memoire(unique)] key: String, value: String }
    /// # memoire_orm::block_on(async {
    /// # let db = MemoryDb::new();
    /// # db.create_table::<Setting>().await?;
    /// for value in ["dark", "light"] {
    ///     Setting::query()
    ///         .filter(Setting::KEY.eq("theme"))
    ///         .update_or_create(&db, || Setting { id: 0, key: "theme".into(), value: String::new() }, |s| {
    ///             s.value = value.into()
    ///         })
    ///         .await?;
    /// }
    /// let all = Setting::all(&db).await?;
    /// assert_eq!((all.len(), all[0].value.as_str()), (1, "light"));
    /// # Ok::<(), memoire_orm::Error>(())
    /// # }).unwrap();
    /// ```
    pub async fn update_or_create<E: Executor>(
        self,
        db: &E,
        make: impl FnOnce() -> M,
        change: impl FnOnce(&mut M),
    ) -> Result<M> {
        let mut row = match self.first(db).await? {
            Some(found) => found,
            None => {
                let mut row = make();
                change(&mut row);
                row.insert(db).await?;
                return Ok(row);
            }
        };
        change(&mut row);
        row.save(db).await?;
        Ok(row)
    }

    /// Reads the matching rows in chunks of `size`, by primary key
    /// (`chunkById` / `lazyById` in Laravel, `toIterable()` in Doctrine): each
    /// chunk is one query, `WHERE pk > <last key of the previous chunk>
    /// ORDER BY pk LIMIT size`, so memory stays bounded and rows written
    /// meanwhile are neither skipped nor read twice. The query's own
    /// `ORDER BY`, `LIMIT` and `OFFSET` are replaced.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model, Debug, Clone)] #[memoire(table = "events")]
    /// # struct Event { #[memoire(primary_key, auto_increment)] id: i64, kind: String }
    /// # memoire_orm::block_on(async {
    /// # let db = MemoryDb::new();
    /// # db.create_table::<Event>().await?;
    /// # for _ in 0..5 { Event { id: 0, kind: "click".into() }.insert(&db).await?; }
    /// let mut chunks = Event::query().filter(Event::KIND.eq("click")).chunks(2);
    /// let mut sizes = Vec::new();
    /// while let Some(events) = chunks.next(&db).await? {
    ///     sizes.push(events.len()); // process up to 2 rows at a time
    /// }
    /// assert_eq!(sizes, [2, 2, 1]);
    /// # Ok::<(), memoire_orm::Error>(())
    /// # }).unwrap();
    /// ```
    pub fn chunks(mut self, size: u64) -> Chunks<M> {
        self.ast.order.clear();
        self.ast.limit = None;
        self.ast.offset = None;
        Chunks { query: self, size: size.max(1), after: None, done: false }
    }

    /// One page of rows after the cursor `after` (a primary key value, `None`
    /// for the first page), by ascending primary key: keyset pagination,
    /// `cursorPaginate` in Laravel. Unlike [`paginate`](Self::paginate), no
    /// row is counted or skipped, so every page costs the same, and rows
    /// inserted meanwhile do not shift the pages. Pass
    /// [`next_cursor`](CursorPage::next_cursor) to get the next page.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model, Debug, Clone)] #[memoire(table = "items")]
    /// # struct Item { #[memoire(primary_key, auto_increment)] id: i64, name: String }
    /// # memoire_orm::block_on(async {
    /// # let db = MemoryDb::new();
    /// # db.create_table::<Item>().await?;
    /// # for n in 0..5 { Item { id: 0, name: format!("item {n}") }.insert(&db).await?; }
    /// let first = Item::query().cursor_paginate(&db, 2, None).await?;
    /// assert_eq!(first.items.len(), 2);
    /// let second = Item::query().cursor_paginate(&db, 2, first.next_cursor).await?;
    /// assert_eq!(second.items[0].id, 3);
    /// # Ok::<(), memoire_orm::Error>(())
    /// # }).unwrap();
    /// ```
    pub async fn cursor_paginate<E: Executor>(
        mut self,
        db: &E,
        per_page: u64,
        after: Option<Value>,
    ) -> Result<CursorPage<M>> {
        let per_page = per_page.max(1);
        let pk = Expr::Column(column_ref(M::SCHEMA, M::SCHEMA.primary_key));
        self.ast.order.clear();
        self.ast.offset = None;
        if let Some(after) = after {
            self = self.filter(pk.clone().gt(Expr::Value(after)));
        }
        let mut items = self.order_by(pk.asc()).limit(per_page.saturating_add(1)).all(db).await?;
        let next_cursor = if items.len() as u64 > per_page {
            items.truncate(per_page as usize);
            items.last().map(Model::key)
        } else {
            None
        };
        Ok(CursorPage { items, per_page, next_cursor })
    }
}

/// The rows of a query read in chunks, returned by [`Select::chunks`].
///
/// Call [`next`](Self::next) until it returns `None`.
#[must_use = "chunks are read with `.next(&db)`"]
pub struct Chunks<M> {
    query: Select<M>,
    size: u64,
    after: Option<Value>,
    done: bool,
}

impl<M> std::fmt::Debug for Chunks<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunks")
            .field("size", &self.size)
            .field("after", &self.after)
            .field("done", &self.done)
            .finish()
    }
}

impl<M: Model> Chunks<M> {
    /// The next chunk (at most `size` rows, never empty), or `None` when every
    /// row was read.
    pub async fn next<E: Executor>(&mut self, db: &E) -> Result<Option<Vec<M>>> {
        if self.done {
            return Ok(None);
        }
        let pk = Expr::Column(column_ref(M::SCHEMA, M::SCHEMA.primary_key));
        let mut query = self.query.clone();
        if let Some(after) = self.after.take() {
            query = query.filter(pk.clone().gt(Expr::Value(after)));
        }
        let rows = query.order_by(pk.asc()).limit(self.size).all(db).await?;
        if (rows.len() as u64) < self.size {
            self.done = true;
        }
        match rows.last() {
            Some(last) => {
                self.after = Some(last.key());
                Ok(Some(rows))
            }
            None => {
                self.done = true;
                Ok(None)
            }
        }
    }

    /// Calls `each` on every chunk, in order, stopping at the first error.
    /// Returns the number of rows read.
    pub async fn for_each<E: Executor>(mut self, db: &E, mut each: impl FnMut(Vec<M>) -> Result<()>) -> Result<u64> {
        let mut total = 0;
        while let Some(rows) = self.next(db).await? {
            total += rows.len() as u64;
            each(rows)?;
        }
        Ok(total)
    }
}

/// One page of a keyset pagination, returned by [`Select::cursor_paginate`].
#[derive(Clone, Debug, PartialEq)]
pub struct CursorPage<T> {
    /// Rows of this page.
    pub items: Vec<T>,
    /// Page size.
    pub per_page: u64,
    /// The cursor of the next page (the primary key of the last row), `None`
    /// on the last page.
    pub next_cursor: Option<Value>,
}

impl<T> CursorPage<T> {
    /// `true` if there is a page after this one.
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
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
    /// `UPDATE <M>` without any assignment yet; for a model with an
    /// `updated_at` [timestamp](crate::lifecycle), it is set to the current
    /// time (unless you [`set`](Self::set) it yourself).
    pub fn new() -> Self {
        let sets = M::UPDATED_AT_INDEX.map(|i| (i, Expr::Value(now_for::<M>(i)))).into_iter().collect();
        Update { ast: UpdateAst { table: M::SCHEMA, sets, filter: None }, _m: PhantomData }
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
    force: bool,
    _m: PhantomData<fn() -> M>,
}

impl<M> Clone for Delete<M> {
    fn clone(&self) -> Self {
        Delete { ast: self.ast.clone(), force: self.force, _m: PhantomData }
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
        Delete { ast: DeleteAst { table: M::SCHEMA, filter: None }, force: false, _m: PhantomData }
    }

    /// Removes the rows for good, even on a model with
    /// [soft deletes](crate::lifecycle) (`forceDelete()` in Eloquent).
    pub fn force(mut self) -> Self {
        self.force = true;
        self
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
    ///
    /// On a model with [soft deletes](crate::lifecycle), the matching rows
    /// that are not deleted yet get `deleted_at` instead (an `UPDATE`, which
    /// [`to_sql`](Self::to_sql) does not show), unless [`force`](Self::force)
    /// was called.
    pub async fn execute<E: Executor>(self, db: &E) -> Result<u64> {
        if M::DELETED_AT_INDEX.is_some() && !self.force {
            return crate::model::soft_delete_where::<M, E>(db, self.ast.filter).await;
        }
        Ok(db.execute(Query::Delete(&self.ast)).await?.rows_affected)
    }
}
