//! Backend-independent description of every statement Memoire can run.
//!
//! SQL backends render these structures to SQL (see [`crate::sql`]); the
//! in-memory engine interprets them directly, without ever producing SQL.

use crate::expr::{Expr, Order};
use crate::schema::TableSchema;
use crate::value::Value;

/// What a `SELECT` returns.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// Every column of each table, in order (models and joined models).
    Tables(Vec<&'static TableSchema>),
    /// Arbitrary expressions (columns, aggregates...).
    Exprs(Vec<Expr>),
}

/// Kind of join.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinKind {
    /// `INNER JOIN`
    Inner,
    /// `LEFT JOIN`
    Left,
}

/// One `JOIN` clause.
#[derive(Clone, Debug, PartialEq)]
pub struct Join {
    /// Kind of join.
    pub kind: JoinKind,
    /// Joined table.
    pub table: &'static TableSchema,
    /// Join condition.
    pub on: Expr,
}

/// A `SELECT` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectAst {
    /// Root table.
    pub from: &'static TableSchema,
    /// Selected values.
    pub projection: Projection,
    /// Joins, in order.
    pub joins: Vec<Join>,
    /// `WHERE` clause.
    pub filter: Option<Expr>,
    /// `GROUP BY` expressions.
    pub group_by: Vec<Expr>,
    /// `HAVING` clause.
    pub having: Option<Expr>,
    /// `ORDER BY` terms.
    pub order: Vec<Order>,
    /// `LIMIT`.
    pub limit: Option<u64>,
    /// `OFFSET`.
    pub offset: Option<u64>,
    /// `SELECT DISTINCT`.
    pub distinct: bool,
}

impl SelectAst {
    /// `SELECT <all columns of table> FROM table`.
    pub fn new(from: &'static TableSchema) -> Self {
        SelectAst {
            from,
            projection: Projection::Tables(vec![from]),
            joins: Vec::new(),
            filter: None,
            group_by: Vec::new(),
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            distinct: false,
        }
    }

    /// `true` if a condition or a selected expression holds a subquery
    /// ([`Expr::InSubquery`]).
    pub fn has_subquery(&self) -> bool {
        let projected = match &self.projection {
            Projection::Exprs(e) => e.iter().any(Expr::has_subquery),
            Projection::Tables(_) => false,
        };
        projected
            || self.filter.as_ref().is_some_and(Expr::has_subquery)
            || self.having.as_ref().is_some_and(Expr::has_subquery)
            || self.joins.iter().any(|j| j.on.has_subquery())
    }

    /// Tables visible to the query, root first then joins in order.
    pub fn tables(&self) -> impl Iterator<Item = &'static TableSchema> + '_ {
        std::iter::once(self.from).chain(self.joins.iter().map(|j| j.table))
    }
}

/// A multi-row `INSERT`.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertAst {
    /// Target table.
    pub table: &'static TableSchema,
    /// Full rows in schema order (an auto-increment key is ignored).
    pub rows: Vec<Vec<Value>>,
}

/// An `UPDATE ... SET ... WHERE ...`.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdateAst {
    /// Target table.
    pub table: &'static TableSchema,
    /// `(column index, new value expression)` pairs.
    pub sets: Vec<(usize, Expr)>,
    /// `WHERE` clause (every row when `None`).
    pub filter: Option<Expr>,
}

/// A `DELETE FROM ... WHERE ...`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeleteAst {
    /// Target table.
    pub table: &'static TableSchema,
    /// `WHERE` clause (every row when `None`).
    pub filter: Option<Expr>,
}

/// A statement handed to an [`Executor`](crate::Executor).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Query<'a> {
    /// General `SELECT`.
    Select(&'a SelectAst),
    /// Multi-row `INSERT`.
    Insert(&'a InsertAst),
    /// General `UPDATE`.
    Update(&'a UpdateAst),
    /// General `DELETE`.
    Delete(&'a DeleteAst),
    /// Fetch one row by primary key (pre-rendered SQL).
    FindByKey {
        /// Table.
        table: &'static TableSchema,
        /// Primary key value.
        key: &'a Value,
    },
    /// Insert one full row and return the generated key (pre-rendered SQL).
    InsertRow {
        /// Table.
        table: &'static TableSchema,
        /// Every column, in schema order.
        values: &'a [Value],
    },
    /// Insert or update one full row by primary key (pre-rendered SQL).
    UpsertRow {
        /// Table.
        table: &'static TableSchema,
        /// Every column, in schema order.
        values: &'a [Value],
    },
    /// Update one full row by primary key (pre-rendered SQL).
    UpdateRow {
        /// Table.
        table: &'static TableSchema,
        /// Every column, in schema order.
        values: &'a [Value],
    },
    /// Delete one row by primary key (pre-rendered SQL).
    DeleteByKey {
        /// Table.
        table: &'static TableSchema,
        /// Primary key value.
        key: &'a Value,
    },
    /// Raw SQL with positional parameters (SQL backends only).
    Raw {
        /// SQL text, using the placeholder syntax of the database.
        sql: &'a str,
        /// Parameters.
        params: &'a [Value],
    },
    /// A script of one or more SQL statements without parameters (SQL backends
    /// only), used by migrations.
    Script {
        /// SQL text.
        sql: &'a str,
    },
    /// `CREATE TABLE` (+ indexes) for a model.
    CreateTable {
        /// Table.
        table: &'static TableSchema,
        /// Add `IF NOT EXISTS`.
        if_not_exists: bool,
    },
    /// `DROP TABLE`.
    DropTable {
        /// Table name.
        name: &'a str,
        /// Add `IF EXISTS`.
        if_exists: bool,
    },
}
