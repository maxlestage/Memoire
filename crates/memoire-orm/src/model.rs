//! The [`Model`] trait implemented by `#[derive(Model)]`.

use std::future::Future;

use crate::ast::Query;
use crate::ast::UpdateAst;
use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::expr::{BinOp, Expr};
use crate::query::{Delete, ModelColumns, Select, Update};
use crate::relation::{column_ref, fetch_in, link};
use crate::row::FromRow;
use crate::schema::TableSchema;
use crate::value::{Bind, Value};

/// A Rust struct mapped to a database table.
///
/// Implement it with `#[derive(Model)]`; every method below is then available
/// on your type. All methods accept any [`Executor`]: a database handle or a
/// transaction.
///
/// ```
/// use memoire_orm::prelude::*;
///
/// #[derive(Model, Debug, Clone, PartialEq)]
/// #[memoire(table = "users")]
/// struct User {
///     #[memoire(primary_key, auto_increment)]
///     id: i64,
///     #[memoire(unique)]
///     email: String,
///     age: Option<i32>,
/// }
///
/// # memoire_orm::block_on(async {
/// let db = MemoryDb::new();
/// db.create_table::<User>().await?;
///
/// let mut ada = User { id: 0, email: "ada@example.com".into(), age: Some(36) };
/// ada.insert(&db).await?;
/// assert_eq!(ada.id, 1);
///
/// let found = User::find(&db, 1).await?;
/// assert_eq!(found, Some(ada));
/// # Ok::<(), memoire_orm::Error>(())
/// # }).unwrap();
/// ```
pub trait Model: FromRow + Send + Sync + Sized + 'static {
    /// Static description of the table, including pre-rendered SQL.
    const SCHEMA: &'static TableSchema;

    /// Rust type of the primary key.
    type Key;

    /// The primary key value of this row.
    fn key(&self) -> Value;

    /// Sets the primary key (used after an insert with a generated key).
    fn set_key(&mut self, value: Value) -> Result<()>;

    /// Every column value, in schema order.
    fn to_values(&self) -> Vec<Value>;

    /// The value of the column at `index` in [`TableSchema::columns`]
    /// (`NULL` when out of range). `#[derive(Model)]` reads only that field;
    /// the default implementation converts the whole row.
    fn value_at(&self, index: usize) -> Value {
        let mut values = self.to_values();
        if index < values.len() { values.swap_remove(index) } else { Value::Null }
    }

    /// Starts a `SELECT` on this table.
    #[inline]
    fn query() -> Select<Self> {
        Select::new()
    }

    /// Starts an `UPDATE` of every row matching the filters you add.
    #[inline]
    fn update_many() -> Update<Self> {
        Update::new()
    }

    /// Starts a `DELETE` of every row matching the filters you add.
    #[inline]
    fn delete_many() -> Delete<Self> {
        Delete::new()
    }

    /// Fetches a row by primary key.
    fn find<E: Executor>(db: &E, key: impl Bind<Self::Key>) -> impl Future<Output = Result<Option<Self>>> + Send {
        let key = key.into_bind();
        async move {
            let mut rows: Vec<Self> = db.fetch_all(Query::FindByKey { table: Self::SCHEMA, key: &key }).await?;
            Ok(rows.pop())
        }
    }

    /// Fetches a row by primary key, failing with [`Error::NotFound`].
    fn get<E: Executor>(db: &E, key: impl Bind<Self::Key>) -> impl Future<Output = Result<Self>> + Send {
        let fut = Self::find(db, key);
        async move { fut.await?.ok_or(Error::NotFound) }
    }

    /// Fetches the rows whose primary key is in `keys`, in one query
    /// (`WHERE pk IN (...)`, split under the database's parameter limit).
    /// Missing keys are skipped; the order of the rows is unspecified.
    fn find_many<E: Executor, K: Bind<Self::Key>>(
        db: &E,
        keys: impl IntoIterator<Item = K>,
    ) -> impl Future<Output = Result<Vec<Self>>> + Send {
        let keys: Vec<Value> = keys.into_iter().map(Bind::into_bind).collect();
        fetch_in(db, Self::query(), column_ref(Self::SCHEMA, Self::SCHEMA.primary_key), keys)
    }

    /// Fetches every row of the table.
    fn all<E: Executor>(db: &E) -> impl Future<Output = Result<Vec<Self>>> + Send {
        Self::query().all(db)
    }

    /// Counts the rows of the table.
    fn count<E: Executor>(db: &E) -> impl Future<Output = Result<u64>> + Send {
        Self::query().count(db)
    }

    /// Inserts this row. A generated primary key is written back into `self`.
    fn insert<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let values = self.to_values();
            let res = db.execute(Query::InsertRow { table: Self::SCHEMA, values: &values }).await?;
            if Self::SCHEMA.auto_increment() {
                match res.last_insert_id {
                    Some(id) => self.set_key(id)?,
                    None => {
                        return Err(Error::Conversion(format!(
                            "the database did not return the generated key of `{}`",
                            Self::SCHEMA.name
                        )));
                    }
                }
            }
            Ok(())
        }
    }

    /// Inserts many rows with as few statements as possible (one multi-row
    /// `INSERT` per batch). Generated keys are *not* read back; returns the
    /// number of inserted rows.
    fn insert_many<E: Executor>(db: &E, rows: &[Self]) -> impl Future<Output = Result<u64>> + Send {
        async move {
            if rows.is_empty() {
                return Ok(0);
            }
            let width = Self::SCHEMA.columns.len() - usize::from(Self::SCHEMA.auto_increment());
            let max_params = db.dialect().map_or(usize::MAX, |d| d.max_params());
            let batch = (max_params / width.max(1)).clamp(1, 10_000);
            let mut total = 0;
            for chunk in rows.chunks(batch) {
                total += db.insert_rows(Self::SCHEMA, chunk.iter().map(Model::to_values).collect()).await?;
            }
            Ok(total)
        }
    }

    /// Updates every column of this row, by primary key.
    ///
    /// Fails with [`Error::NotFound`] if the row does not exist.
    fn update<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let values = self.to_values();
            let res = db.execute(Query::UpdateRow { table: Self::SCHEMA, values: &values }).await?;
            if res.rows_affected == 0 { Err(Error::NotFound) } else { Ok(()) }
        }
    }

    /// Updates only some columns of this row, by primary key:
    /// `user.update_only(&db, (User::NAME, User::AGE))`. The other columns
    /// are left untouched in the database, so concurrent writers of other
    /// columns are not overwritten, and less data travels.
    ///
    /// Fails with [`Error::NotFound`] if the row does not exist. The primary
    /// key is never written; with nothing else to write, nothing is run.
    fn update_only<E: Executor>(
        &self,
        db: &E,
        columns: impl ModelColumns<Self>,
    ) -> impl Future<Output = Result<()>> + Send {
        let schema = Self::SCHEMA;
        let mut sets: Vec<(usize, Expr)> = Vec::new();
        for index in columns.indexes() {
            if index != schema.primary_key && !sets.iter().any(|(i, _)| *i == index) {
                sets.push((index, Expr::Value(self.value_at(index))));
            }
        }
        let filter =
            Expr::binary(BinOp::Eq, Expr::Column(column_ref(schema, schema.primary_key)), Expr::Value(self.key()));
        let ast = UpdateAst { table: schema, sets, filter: Some(filter) };
        async move {
            if ast.sets.is_empty() {
                return Ok(());
            }
            let res = db.execute(Query::Update(&ast)).await?;
            if res.rows_affected == 0 { Err(Error::NotFound) } else { Ok(()) }
        }
    }

    /// The children of `parent`: the rows of `Self` whose foreign key
    /// (`#[memoire(references = "...")]`) points to it, as a query you can
    /// refine. `Post::belonging_to(&alice).order_by(Post::ID).all(&db)`.
    ///
    /// To load the children of many rows at once, use
    /// [`LoadRelated::load_children`](crate::LoadRelated::load_children).
    ///
    /// # Panics
    ///
    /// If no column of `Self` references the table of `P`, or several do
    /// (filter on the foreign key column yourself then).
    fn belonging_to<P: Model>(parent: &P) -> Select<Self> {
        let link = link::<Self, P>().unwrap_or_else(|e| panic!("{e}"));
        Self::query().filter(Expr::binary(
            BinOp::Eq,
            Expr::Column(column_ref(Self::SCHEMA, link.child)),
            Expr::Value(parent.value_at(link.parent)),
        ))
    }

    /// The row of `P` that this row's foreign key points to (`None` when the
    /// key is `NULL` or dangling). Fails with [`Error::Relation`] if `Self`
    /// has no foreign key to `P`, or several.
    fn find_parent<P: Model, E: Executor>(&self, db: &E) -> impl Future<Output = Result<Option<P>>> + Send {
        let target = link::<Self, P>().map(|l| (l.parent, self.value_at(l.child)));
        async move {
            let (column, value) = target?;
            if value.is_null() {
                return Ok(None);
            }
            let filter = Expr::binary(BinOp::Eq, Expr::Column(column_ref(P::SCHEMA, column)), Expr::Value(value));
            P::query().filter(filter).first(db).await
        }
    }

    /// Inserts this row, or updates it if a row with the same primary key exists.
    fn upsert<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let values = self.to_values();
            db.execute(Query::UpsertRow { table: Self::SCHEMA, values: &values }).await?;
            Ok(())
        }
    }

    /// Deletes this row. Returns `false` if it did not exist.
    fn delete<E: Executor>(&self, db: &E) -> impl Future<Output = Result<bool>> + Send {
        async move {
            let key = self.key();
            let res = db.execute(Query::DeleteByKey { table: Self::SCHEMA, key: &key }).await?;
            Ok(res.rows_affected > 0)
        }
    }

    /// Deletes a row by primary key. Returns `false` if it did not exist.
    fn delete_by_key<E: Executor>(db: &E, key: impl Bind<Self::Key>) -> impl Future<Output = Result<bool>> + Send {
        let key = key.into_bind();
        async move {
            let res = db.execute(Query::DeleteByKey { table: Self::SCHEMA, key: &key }).await?;
            Ok(res.rows_affected > 0)
        }
    }

    /// Re-reads this row from the database.
    fn reload<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let key = self.key();
            let mut rows: Vec<Self> = db.fetch_all(Query::FindByKey { table: Self::SCHEMA, key: &key }).await?;
            *self = rows.pop().ok_or(Error::NotFound)?;
            Ok(())
        }
    }
}
