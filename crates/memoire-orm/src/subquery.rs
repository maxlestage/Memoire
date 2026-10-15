//! Subqueries on backends that do not speak SQL.
//!
//! SQL databases run `x IN (SELECT ...)` themselves. For the in-memory engine,
//! Redis and MongoDB (no [`Dialect`](crate::Dialect)), the query builders run
//! each subquery first, on the same executor (so inside the same transaction),
//! and replace it with the `IN` list of its rows.

use std::future::Future;
use std::pin::Pin;

use crate::ast::{DeleteAst, Projection, Query, SelectAst, UpdateAst};
use crate::error::Result;
use crate::executor::{ExecResult, Executor};
use crate::expr::Expr;
use crate::row::FromRow;
use crate::value::Value;

type Boxed<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Replaces every subquery of `e` with the values it returns.
fn resolve_expr<'a, E: Executor>(db: &'a E, e: &'a mut Expr) -> Boxed<'a> {
    Box::pin(async move {
        match e {
            Expr::InSubquery { expr, query, negated } => {
                resolve_expr(db, expr).await?;
                resolve_select(db, query).await?;
                let list: Vec<Value> = db.fetch_all(Query::Select(query)).await?;
                let expr = std::mem::replace(&mut **expr, Expr::Value(Value::Null));
                *e = Expr::InList { expr: Box::new(expr), list, negated: *negated };
            }
            Expr::Binary { left, right, .. } => {
                resolve_expr(db, left).await?;
                resolve_expr(db, right).await?;
            }
            Expr::Not(inner) | Expr::IsNull { expr: inner, .. } | Expr::InList { expr: inner, .. } => {
                resolve_expr(db, inner).await?;
            }
            Expr::Between { expr, low, high, .. } => {
                resolve_expr(db, expr).await?;
                resolve_expr(db, low).await?;
                resolve_expr(db, high).await?;
            }
            Expr::Like { expr, pattern, .. } => {
                resolve_expr(db, expr).await?;
                resolve_expr(db, pattern).await?;
            }
            Expr::Aggregate { arg: Some(arg), .. } => resolve_expr(db, arg).await?,
            _ => {}
        }
        Ok(())
    })
}

async fn resolve_opt<E: Executor>(db: &E, e: &mut Option<Expr>) -> Result<()> {
    match e {
        Some(e) if e.has_subquery() => resolve_expr(db, e).await,
        _ => Ok(()),
    }
}

fn resolve_select<'a, E: Executor>(db: &'a E, ast: &'a mut SelectAst) -> Boxed<'a> {
    Box::pin(async move {
        if !ast.has_subquery() {
            return Ok(());
        }
        resolve_opt(db, &mut ast.filter).await?;
        resolve_opt(db, &mut ast.having).await?;
        for j in &mut ast.joins {
            if j.on.has_subquery() {
                resolve_expr(db, &mut j.on).await?;
            }
        }
        if let Projection::Exprs(exprs) = &mut ast.projection {
            for e in exprs.iter_mut().filter(|e| e.has_subquery()) {
                resolve_expr(db, e).await?;
            }
        }
        Ok(())
    })
}

/// Runs a `SELECT`, resolving its subqueries first on a backend without SQL.
pub(crate) async fn fetch<O: FromRow + Send, E: Executor>(db: &E, ast: &SelectAst) -> Result<Vec<O>> {
    if db.dialect().is_none() && ast.has_subquery() {
        let mut ast = ast.clone();
        resolve_select(db, &mut ast).await?;
        return db.fetch_all(Query::Select(&ast)).await;
    }
    db.fetch_all(Query::Select(ast)).await
}

/// Runs an `UPDATE`, resolving the subqueries of its filter first on a backend without SQL.
pub(crate) async fn update<E: Executor>(db: &E, ast: &UpdateAst) -> Result<ExecResult> {
    let nosql = db.dialect().is_none();
    if nosql && (ast.filter.as_ref().is_some_and(Expr::has_subquery) || ast.sets.iter().any(|(_, e)| e.has_subquery()))
    {
        let mut ast = ast.clone();
        resolve_opt(db, &mut ast.filter).await?;
        for (_, e) in &mut ast.sets {
            if e.has_subquery() {
                resolve_expr(db, e).await?;
            }
        }
        return db.execute(Query::Update(&ast)).await;
    }
    db.execute(Query::Update(ast)).await
}

/// Runs a `DELETE`, resolving the subqueries of its filter first on a backend without SQL.
pub(crate) async fn delete<E: Executor>(db: &E, ast: &DeleteAst) -> Result<ExecResult> {
    if db.dialect().is_none() && ast.filter.as_ref().is_some_and(Expr::has_subquery) {
        let mut ast = ast.clone();
        resolve_opt(db, &mut ast.filter).await?;
        return db.execute(Query::Delete(&ast)).await;
    }
    db.execute(Query::Delete(ast)).await
}
