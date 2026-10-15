//! The [`Model`] trait implemented by `#[derive(Model)]`.

use std::future::Future;

use crate::ast::Query;
use crate::ast::UpdateAst;
use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::expr::{BinOp, Expr};
use crate::lifecycle::{HookFns, is_unset, now_for, now_millis, timestamp_value};
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

    /// Sets the field of the column at `index` in [`TableSchema::columns`],
    /// converting `value` to its Rust type. `#[derive(Model)]` implements it;
    /// the default fails with [`Error::Unsupported`]. Used by the automatic
    /// timestamps and soft deletes to write back into the struct.
    fn set_value_at(&mut self, index: usize, value: Value) -> Result<()> {
        let _ = value;
        Err(Error::Unsupported(format!("`{}` cannot set the column at position {index}", Self::SCHEMA.name)))
    }

    /// Position of the column filled with the creation time on insert
    /// (`#[memoire(created_at)]`, or the `created_at` field with
    /// `#[memoire(timestamps)]`). See [`lifecycle`](crate::lifecycle).
    const CREATED_AT_INDEX: Option<usize> = None;

    /// Position of the column refreshed on every write of the row
    /// (`#[memoire(updated_at)]`, or the `updated_at` field with
    /// `#[memoire(timestamps)]`). See [`lifecycle`](crate::lifecycle).
    const UPDATED_AT_INDEX: Option<usize> = None;

    /// Position of the soft delete column (`#[memoire(soft_delete)]`, or the
    /// `deleted_at` field with `#[memoire(soft_deletes)]`): when set,
    /// [`delete`](Self::delete) stamps it instead of removing the row, and
    /// [`query`](Self::query) leaves stamped rows out. See [`lifecycle`](crate::lifecycle).
    const DELETED_AT_INDEX: Option<usize> = None;

    /// The lifecycle hooks of this model, `Some` with `#[memoire(hooks)]` and
    /// an implementation of [`Hooks`](crate::lifecycle::Hooks).
    fn hooks() -> Option<HookFns<Self>> {
        None
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
            if Self::DELETED_AT_INDEX.is_some() {
                // Soft-deleted rows are left out, like by every other query.
                return Self::query().filter(pk_eq::<Self>(key)).first(db).await;
            }
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

    /// Inserts this row. A generated primary key is written back into `self`,
    /// and so are the [timestamps](crate::lifecycle) when the model has them
    /// (an unset `created_at`/`updated_at`, `NULL` or `0`, gets the current
    /// time). Runs the `before_save`, `before_insert` and `after_insert`
    /// [hooks](crate::lifecycle::Hooks).
    fn insert<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            if let Some(h) = Self::hooks() {
                (h.before_save)(self)?;
            }
            insert_row(self, db).await
        }
    }

    /// Inserts this row if it is new, updates it otherwise, like Eloquent's
    /// `save()`: a row is new when its primary key is generated
    /// (`auto_increment`) and still unset (`0`); with a key of your own, the
    /// row is updated, or inserted when no row has that key.
    ///
    /// Unlike [`update`](Self::update), `updated_at` is written back into
    /// `self`, and the `before_save` [hook](crate::lifecycle::Hooks) runs (it
    /// may modify the row) before `before_insert` or `before_update`.
    fn save<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let hooks = Self::hooks();
            if let Some(h) = hooks {
                (h.before_save)(self)?;
            }
            let auto = Self::SCHEMA.auto_increment();
            if auto && is_unset(&self.key()) {
                return insert_row(self, db).await;
            }
            if let Some(h) = hooks {
                (h.before_update)(self)?;
            }
            if let Some(i) = Self::UPDATED_AT_INDEX {
                self.set_value_at(i, now_for::<Self>(i))?;
            }
            let values = self.to_values();
            let res = db.execute(Query::UpdateRow { table: Self::SCHEMA, values: &values }).await?;
            if res.rows_affected == 0 {
                if auto {
                    return Err(Error::NotFound);
                }
                return insert_row(self, db).await;
            }
            if let Some(h) = hooks {
                (h.after_update)(self);
            }
            Ok(())
        }
    }

    /// Sets `updated_at` to the current time, in the database and in `self`.
    /// Returns `false` (and does nothing) when the model has no `updated_at`
    /// column; fails with [`Error::NotFound`] if the row does not exist.
    fn touch<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<bool>> + Send {
        async move {
            let Some(i) = Self::UPDATED_AT_INDEX else { return Ok(false) };
            let now = now_for::<Self>(i);
            self.set_value_at(i, now.clone())?;
            let ast = UpdateAst {
                table: Self::SCHEMA,
                sets: vec![(i, Expr::Value(now))],
                filter: Some(pk_eq::<Self>(self.key())),
            };
            let res = db.execute(Query::Update(&ast)).await?;
            if res.rows_affected == 0 { Err(Error::NotFound) } else { Ok(true) }
        }
    }

    /// Inserts many rows with as few statements as possible (one multi-row
    /// `INSERT` per batch). Generated keys are *not* read back; returns the
    /// number of inserted rows. Unset timestamps are filled in the inserted
    /// rows (not in `rows`); no [hook](crate::lifecycle::Hooks) runs.
    fn insert_many<E: Executor>(db: &E, rows: &[Self]) -> impl Future<Output = Result<u64>> + Send {
        async move {
            if rows.is_empty() {
                return Ok(0);
            }
            let width = Self::SCHEMA.columns.len() - usize::from(Self::SCHEMA.auto_increment());
            let max_params = db.dialect().map_or(usize::MAX, |d| d.max_params());
            let batch = (max_params / width.max(1)).clamp(1, 10_000);
            let stamps = new_row_stamps::<Self>();
            let mut total = 0;
            for chunk in rows.chunks(batch) {
                let values = chunk
                    .iter()
                    .map(|row| {
                        let mut values = row.to_values();
                        for (i, now) in &stamps {
                            if is_unset(&values[*i]) {
                                values[*i] = now.clone();
                            }
                        }
                        values
                    })
                    .collect();
                total += db.insert_rows(Self::SCHEMA, values).await?;
            }
            Ok(total)
        }
    }

    /// Updates every column of this row, by primary key.
    ///
    /// Fails with [`Error::NotFound`] if the row does not exist. The database
    /// gets the current time in `updated_at` when the model has one (`self`,
    /// borrowed, keeps its value: use [`save`](Self::save) to get it back).
    /// Runs the `before_update` and `after_update` [hooks](crate::lifecycle::Hooks).
    fn update<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let hooks = Self::hooks();
            if let Some(h) = hooks {
                (h.before_update)(self)?;
            }
            let mut values = self.to_values();
            if let Some(i) = Self::UPDATED_AT_INDEX {
                values[i] = now_for::<Self>(i);
            }
            let res = db.execute(Query::UpdateRow { table: Self::SCHEMA, values: &values }).await?;
            if res.rows_affected == 0 {
                return Err(Error::NotFound);
            }
            if let Some(h) = hooks {
                (h.after_update)(self);
            }
            Ok(())
        }
    }

    /// Updates only some columns of this row, by primary key:
    /// `user.update_only(&db, (User::NAME, User::AGE))`. The other columns
    /// are left untouched in the database, so concurrent writers of other
    /// columns are not overwritten, and less data travels.
    ///
    /// Fails with [`Error::NotFound`] if the row does not exist. The primary
    /// key is never written; with nothing else to write, nothing is run.
    /// `updated_at`, when the model has one, is set to the current time in the
    /// database; the update [hooks](crate::lifecycle::Hooks) run.
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
        if !sets.is_empty()
            && let Some(i) = Self::UPDATED_AT_INDEX
            && !sets.iter().any(|(j, _)| *j == i)
        {
            sets.push((i, Expr::Value(now_for::<Self>(i))));
        }
        let ast = UpdateAst { table: schema, sets, filter: Some(filter) };
        async move {
            if ast.sets.is_empty() {
                return Ok(());
            }
            let hooks = Self::hooks();
            if let Some(h) = hooks {
                (h.before_update)(self)?;
            }
            let res = db.execute(Query::Update(&ast)).await?;
            if res.rows_affected == 0 {
                return Err(Error::NotFound);
            }
            if let Some(h) = hooks {
                (h.after_update)(self);
            }
            Ok(())
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
    ///
    /// The row is written as it is, except for the timestamps: `updated_at`
    /// gets the current time, and so does `created_at` when it is unset. No
    /// [hook](crate::lifecycle::Hooks) runs.
    fn upsert<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send {
        async move {
            let mut values = self.to_values();
            for (i, now) in new_row_stamps::<Self>() {
                if Some(i) == Self::UPDATED_AT_INDEX || is_unset(&values[i]) {
                    values[i] = now;
                }
            }
            db.execute(Query::UpsertRow { table: Self::SCHEMA, values: &values }).await?;
            Ok(())
        }
    }

    /// Deletes this row. Returns `false` if it did not exist.
    ///
    /// With [soft deletes](crate::lifecycle), the row is kept and its
    /// `deleted_at` column set to the current time (in the database: `self`,
    /// borrowed, is unchanged); `false` then also means it was already
    /// deleted. [`force_delete`](Self::force_delete) removes it for good. Runs
    /// the `before_delete` and `after_delete` [hooks](crate::lifecycle::Hooks).
    fn delete<E: Executor>(&self, db: &E) -> impl Future<Output = Result<bool>> + Send {
        async move {
            let hooks = Self::hooks();
            if let Some(h) = hooks {
                (h.before_delete)(self)?;
            }
            let deleted = match Self::DELETED_AT_INDEX {
                Some(_) => soft_delete_where::<Self, E>(db, Some(pk_eq::<Self>(self.key()))).await? > 0,
                None => {
                    let key = self.key();
                    db.execute(Query::DeleteByKey { table: Self::SCHEMA, key: &key }).await?.rows_affected > 0
                }
            };
            if deleted && let Some(h) = hooks {
                (h.after_delete)(self);
            }
            Ok(deleted)
        }
    }

    /// Deletes a row by primary key. Returns `false` if it did not exist.
    /// With [soft deletes](crate::lifecycle), stamps `deleted_at` instead. No
    /// [hook](crate::lifecycle::Hooks) runs (there is no row in hand).
    fn delete_by_key<E: Executor>(db: &E, key: impl Bind<Self::Key>) -> impl Future<Output = Result<bool>> + Send {
        let key = key.into_bind();
        async move {
            if Self::DELETED_AT_INDEX.is_some() {
                return Ok(soft_delete_where::<Self, E>(db, Some(pk_eq::<Self>(key))).await? > 0);
            }
            let res = db.execute(Query::DeleteByKey { table: Self::SCHEMA, key: &key }).await?;
            Ok(res.rows_affected > 0)
        }
    }

    /// Removes this row from the database, even with
    /// [soft deletes](crate::lifecycle) (`forceDelete()` in Eloquent). Returns
    /// `false` if it did not exist. Runs the delete [hooks](crate::lifecycle::Hooks).
    fn force_delete<E: Executor>(&self, db: &E) -> impl Future<Output = Result<bool>> + Send {
        async move {
            let hooks = Self::hooks();
            if let Some(h) = hooks {
                (h.before_delete)(self)?;
            }
            let key = self.key();
            let deleted = db.execute(Query::DeleteByKey { table: Self::SCHEMA, key: &key }).await?.rows_affected > 0;
            if deleted && let Some(h) = hooks {
                (h.after_delete)(self);
            }
            Ok(deleted)
        }
    }

    /// Brings back a soft-deleted row: `deleted_at` becomes `NULL` (and
    /// `updated_at` the current time), in the database and in `self`. Returns `false` if the row was not deleted (or
    /// does not exist); fails with [`Error::Unsupported`] for a model without
    /// soft deletes.
    fn restore<E: Executor>(&mut self, db: &E) -> impl Future<Output = Result<bool>> + Send {
        async move {
            let Some(d) = Self::DELETED_AT_INDEX else {
                return Err(Error::Unsupported(format!("`{}` has no soft delete column", Self::SCHEMA.name)));
            };
            let now = Self::UPDATED_AT_INDEX.map(|u| (u, now_for::<Self>(u)));
            let mut sets = vec![(d, Expr::Value(Value::Null))];
            sets.extend(now.iter().map(|(u, v)| (*u, Expr::Value(v.clone()))));
            let deleted = Expr::IsNull { expr: Box::new(Expr::Column(column_ref(Self::SCHEMA, d))), negated: true };
            let filter = pk_eq::<Self>(self.key()).and(deleted);
            let ast = UpdateAst { table: Self::SCHEMA, sets, filter: Some(filter) };
            let restored = db.execute(Query::Update(&ast)).await?.rows_affected > 0;
            if restored {
                self.set_value_at(d, Value::Null)?;
                if let Some((u, v)) = now {
                    self.set_value_at(u, v)?;
                }
            }
            Ok(restored)
        }
    }

    /// `true` if this row is soft deleted (its `deleted_at` is set). Always
    /// `false` for a model without soft deletes.
    fn trashed(&self) -> bool {
        Self::DELETED_AT_INDEX.is_some_and(|i| !self.value_at(i).is_null())
    }

    /// A query over every row, soft-deleted ones included
    /// (`Model::withTrashed()` in Eloquent). Same as [`query`](Self::query) for
    /// a model without soft deletes.
    fn with_trashed() -> Select<Self> {
        Self::query().with_trashed()
    }

    /// A query over the soft-deleted rows only (none for a model without soft deletes).
    fn only_trashed() -> Select<Self> {
        Self::query().only_trashed()
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

/// `primary key = key`.
pub(crate) fn pk_eq<M: Model>(key: Value) -> Expr {
    Expr::binary(BinOp::Eq, Expr::Column(column_ref(M::SCHEMA, M::SCHEMA.primary_key)), Expr::Value(key))
}

/// The timestamps a new row of `M` gets: `(column, now)`, one clock reading
/// for both columns.
pub(crate) fn new_row_stamps<M: Model>() -> Vec<(usize, Value)> {
    let columns = [M::CREATED_AT_INDEX, M::UPDATED_AT_INDEX];
    if columns.iter().all(Option::is_none) {
        return Vec::new();
    }
    let ms = now_millis();
    columns.into_iter().flatten().map(|i| (i, timestamp_value(M::SCHEMA.columns[i].kind, ms))).collect()
}

/// Inserts one row: `before_insert`, timestamps, `INSERT`, generated key, `after_insert`.
pub(crate) async fn insert_row<M: Model, E: Executor>(row: &mut M, db: &E) -> Result<()> {
    let hooks = M::hooks();
    if let Some(h) = hooks {
        (h.before_insert)(row)?;
    }
    for (i, now) in new_row_stamps::<M>() {
        if is_unset(&row.value_at(i)) {
            row.set_value_at(i, now)?;
        }
    }
    let values = row.to_values();
    let res = db.execute(Query::InsertRow { table: M::SCHEMA, values: &values }).await?;
    if M::SCHEMA.auto_increment() {
        match res.last_insert_id {
            Some(id) => row.set_key(id)?,
            None => {
                return Err(Error::Conversion(format!(
                    "the database did not return the generated key of `{}`",
                    M::SCHEMA.name
                )));
            }
        }
    }
    if let Some(h) = hooks {
        (h.after_insert)(row);
    }
    Ok(())
}

/// Soft deletes the rows of `M` matching `filter` that are not deleted yet:
/// `deleted_at` (and `updated_at`) get the current time. Returns the number of
/// rows deleted.
pub(crate) async fn soft_delete_where<M: Model, E: Executor>(db: &E, filter: Option<Expr>) -> Result<u64> {
    let Some(d) = M::DELETED_AT_INDEX else {
        return Err(Error::Unsupported(format!("`{}` has no soft delete column", M::SCHEMA.name)));
    };
    let ms = now_millis();
    let mut sets = vec![(d, Expr::Value(timestamp_value(M::SCHEMA.columns[d].kind, ms)))];
    if let Some(u) = M::UPDATED_AT_INDEX {
        sets.push((u, Expr::Value(timestamp_value(M::SCHEMA.columns[u].kind, ms))));
    }
    let alive = Expr::IsNull { expr: Box::new(Expr::Column(column_ref(M::SCHEMA, d))), negated: false };
    let ast = UpdateAst {
        table: M::SCHEMA,
        sets,
        filter: Some(match filter {
            Some(f) => f.and(alive),
            None => alive,
        }),
    };
    Ok(db.execute(Query::Update(&ast)).await?.rows_affected)
}
