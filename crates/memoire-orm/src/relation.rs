//! Relations between models and eager loading without N+1 queries.
//!
//! Relations are read from the foreign keys you already declare with
//! `#[memoire(references = "table(column)")]`: no extra attribute, no trait to
//! implement. A model `C` is a *child* of `P` when exactly one column of `C`
//! references the table of `P`.
//!
//! * [`Model::belonging_to`] builds the query for the children of one row.
//! * [`Model::find_parent`] fetches the row a child points to.
//! * [`LoadRelated`] loads the children, the parents or the many-to-many
//!   partners of a whole slice of rows in one query per level, whatever the
//!   number of rows (the "N+1 queries" problem disappears).
//! * [`LoadCounts`] counts related rows, [`LoadPivot`] returns the junction
//!   rows of a many-to-many relation with the partners, and
//!   [`Select::where_has`] (with `has`, `doesnt_have`...) filters on relations.
//!
//! # Polymorphic relations
//!
//! A model declared with `#[memoire(morph = "commentable")]` and the fields
//! `commentable_type: String` and `commentable_id` points to a row of any
//! model, named by its table name: [`Model::set_morph_parent`],
//! [`Model::morphed_by`] (`morphMany`), [`Model::morph_parent`] (`morphTo`)
//! and [`LoadMorph`] for slices.
//!
//! ```
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug, Clone, PartialEq)]
//! #[memoire(table = "users")]
//! struct User {
//!     #[memoire(primary_key, auto_increment)]
//!     id: i64,
//!     name: String,
//! }
//!
//! #[derive(Model, Debug, Clone, PartialEq)]
//! #[memoire(table = "posts")]
//! struct Post {
//!     #[memoire(primary_key, auto_increment)]
//!     id: i64,
//!     #[memoire(references = "users(id)", index)]
//!     author_id: i64,
//!     title: String,
//! }
//!
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new();
//! db.create_table::<User>().await?;
//! db.create_table::<Post>().await?;
//! for name in ["Alice", "Bob"] {
//!     User { id: 0, name: name.into() }.insert(&db).await?;
//! }
//! for (author_id, title) in [(1, "Hello"), (1, "Rust"), (2, "Bob's")] {
//!     Post { id: 0, author_id, title: title.into() }.insert(&db).await?;
//! }
//!
//! // One query for the users, one for all their posts.
//! let users = User::query().order_by(User::ID).all(&db).await?;
//! let posts: Vec<Vec<Post>> = users.load_children(&db).await?;
//! assert_eq!(posts[0].len(), 2);
//! assert_eq!(posts[1][0].title, "Bob's");
//!
//! // And the other way round.
//! let all_posts = Post::all(&db).await?;
//! let authors: Vec<Option<User>> = all_posts.load_parent(&db).await?;
//! assert_eq!(authors[2].as_ref().unwrap().name, "Bob");
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;

use crate::error::{Error, Result};
use crate::executor::Executor;
use crate::expr::{BinOp, ColumnRef, Expr, count_all};
use crate::model::Model;
use crate::query::Select;
use crate::schema::TableSchema;
use crate::value::Value;

/// A foreign key from a child model to a parent model, as column positions.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Link {
    /// Referencing column, in the child schema.
    pub child: usize,
    /// Referenced column, in the parent schema.
    pub parent: usize,
}

/// Finds the unique column of `C` that references the table of `P`.
pub(crate) fn link<C: Model, P: Model>() -> Result<Link> {
    let (child, parent) = (C::SCHEMA, P::SCHEMA);
    let mut found = child
        .columns
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.references.filter(|fk| fk.table == parent.name).map(|fk| (i, fk)));
    let Some((index, fk)) = found.next() else {
        return Err(Error::Relation(format!(
            "no column of `{}` references `{}`: add `#[memoire(references = \"{}({})\")]` to the foreign key field",
            child.name,
            parent.name,
            parent.name,
            parent.pk().name
        )));
    };
    if found.next().is_some() {
        return Err(Error::Relation(format!(
            "several columns of `{}` reference `{}`: filter on the column you mean, e.g. `{}::query().filter(...)`",
            child.name, parent.name, child.name
        )));
    }
    let referenced = parent.column_index(fk.column).ok_or_else(|| {
        Error::Relation(format!(
            "`{}.{}` references `{}({})`, which is not a column of the model",
            child.name, child.columns[index].name, parent.name, fk.column
        ))
    })?;
    Ok(Link { child: index, parent: referenced })
}

pub(crate) fn column_ref(table: &'static TableSchema, index: usize) -> ColumnRef {
    let c = &table.columns[index];
    ColumnRef { table: table.name, name: c.name, index, kind: c.kind }
}

/// A fast hasher for [`Key`]s (multiply-rotate on integers, FNV-1a on bytes):
/// relation keys come from the database, not from an attacker choosing
/// collisions, and SipHash dominated the cost of grouping.
#[derive(Default, Clone, Copy)]
pub(crate) struct KeyHasher(u64);

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0 ^ 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    fn write_i64(&mut self, n: i64) {
        self.write_u64(n as u64);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    fn write_isize(&mut self, n: isize) {
        self.write_u64(n as u64);
    }
}

pub(crate) type KeyMap<V> = HashMap<Key, V, std::hash::BuildHasherDefault<KeyHasher>>;

/// Hashable form of a key value. Integers of every width compare equal (an
/// `i32` foreign key matches an `i64` primary key), like in SQL.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Int(i64),
    Float(u64),
    Text(String),
    Bytes(Vec<u8>),
    Other(String),
}

/// `None` for `NULL`, which never matches anything.
pub(crate) fn key(value: &Value) -> Option<Key> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => Key::Int(i64::from(*b)),
        Value::I32(v) => Key::Int(i64::from(*v)),
        Value::I64(v) => Key::Int(*v),
        Value::F64(v) if v.fract() == 0.0 && v.abs() < 9.0e15 => Key::Int(*v as i64),
        Value::F64(v) => Key::Float(v.to_bits()),
        Value::Text(s) => Key::Text(s.clone()),
        Value::Bytes(b) => Key::Bytes(b.clone()),
        #[allow(unreachable_patterns)]
        other => Key::Other(format!("{}:{other}", other.type_name())),
    })
}

/// Distinct non-`NULL` values, in first-seen order.
fn distinct(values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut seen: std::collections::HashSet<Key, std::hash::BuildHasherDefault<KeyHasher>> = Default::default();
    values.into_iter().filter(|v| key(v).is_some_and(|k| seen.insert(k))).collect()
}

/// Runs `query` with `column IN (values)`, split into as many statements as the
/// database's bind parameter limit requires.
pub(crate) async fn fetch_in<O: Model, E: Executor>(
    db: &E,
    query: Select<O>,
    column: ColumnRef,
    values: Vec<Value>,
) -> Result<Vec<O>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    // Leave room for the parameters of the caller's own filters.
    let chunk = db.dialect().map_or(usize::MAX, |d| d.max_params() / 2).max(1);
    if values.len() <= chunk {
        let filter = Expr::InList { expr: Box::new(Expr::Column(column)), list: values, negated: false };
        return query.filter(filter).all(db).await;
    }
    let mut out = Vec::with_capacity(values.len());
    for part in values.chunks(chunk) {
        let filter = Expr::InList { expr: Box::new(Expr::Column(column)), list: part.to_vec(), negated: false };
        out.extend(query.clone().filter(filter).all(db).await?);
    }
    Ok(out)
}

/// Eager loading of related rows for a slice of models, in one query per
/// level instead of one per row.
///
/// Implemented for `[M]`, so it is available on slices and `Vec`s of models.
/// Relations come from `#[memoire(references = "table(column)")]` (see the
/// [module documentation](crate::relation)); a missing or ambiguous foreign key
/// is reported as [`Error::Relation`]. Large slices are split under the bind
/// parameter limit of the database.
pub trait LoadRelated<M: Model> {
    /// The children of every row (rows of `C` whose foreign key points to it),
    /// aligned with `self`: `result[i]` holds the children of `self[i]`,
    /// ordered by primary key. One query.
    ///
    /// When the same row appears twice in `self`, its children are attached to
    /// the first occurrence.
    fn load_children<C: Model>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<Vec<C>>>> + Send;

    /// Like [`load_children`](Self::load_children), starting from your own
    /// query on `C` (filters, ordering): `Post::query().filter(Post::PUBLISHED.eq(true)).order_by(Post::ID.desc())`.
    /// Children keep the order of the query. `LIMIT` and `OFFSET` apply to all
    /// the children at once, not per row.
    fn load_children_with<C: Model>(
        &self,
        db: &impl Executor,
        query: Select<C>,
    ) -> impl Future<Output = Result<Vec<Vec<C>>>> + Send;

    /// The parent of every row (the row of `P` its foreign key points to),
    /// aligned with `self`; `None` for a `NULL` key or a dangling one. One query.
    fn load_parent<P: Model + Clone>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<Option<P>>>> + Send;

    /// Many-to-many: the rows of `T` linked to every row through the junction
    /// model `J` (which references both `M` and `T`), aligned with `self` and
    /// in the order of the junction rows' primary keys (for an auto-increment
    /// key, the order the links were created). Two queries.
    fn load_via<J: Model, T: Model + Clone>(
        &self,
        db: &impl Executor,
    ) -> impl Future<Output = Result<Vec<Vec<T>>>> + Send;
}

impl<M: Model> LoadRelated<M> for [M] {
    fn load_children<C: Model>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<Vec<C>>>> + Send {
        self.load_children_with(
            db,
            C::query().order_by(Expr::Column(column_ref(C::SCHEMA, C::SCHEMA.primary_key)).asc()),
        )
    }

    async fn load_children_with<C: Model>(&self, db: &impl Executor, query: Select<C>) -> Result<Vec<Vec<C>>> {
        let link = link::<C, M>()?;
        let mut slots: KeyMap<usize> = KeyMap::with_capacity_and_hasher(self.len(), Default::default());
        let mut keys = Vec::with_capacity(self.len());
        for (i, row) in self.iter().enumerate() {
            let value = row.value_at(link.parent);
            if let Some(k) = key(&value)
                && let Entry::Vacant(e) = slots.entry(k)
            {
                e.insert(i);
                keys.push(value);
            }
        }
        let children = fetch_in(db, query, column_ref(C::SCHEMA, link.child), keys).await?;
        let mut out: Vec<Vec<C>> = (0..self.len()).map(|_| Vec::new()).collect();
        for child in children {
            if let Some(&i) = key(&child.value_at(link.child)).and_then(|k| slots.get(&k)) {
                out[i].push(child);
            }
        }
        Ok(out)
    }

    async fn load_parent<P: Model + Clone>(&self, db: &impl Executor) -> Result<Vec<Option<P>>> {
        let link = link::<M, P>()?;
        let refs: Vec<Option<Key>> = self.iter().map(|row| key(&row.value_at(link.child))).collect();
        let values = distinct(self.iter().map(|row| row.value_at(link.child)));
        let parents = fetch_in(db, P::query(), column_ref(P::SCHEMA, link.parent), values).await?;
        let by_key: KeyMap<P> =
            parents.into_iter().filter_map(|p| key(&p.value_at(link.parent)).map(|k| (k, p))).collect();
        Ok(refs.into_iter().map(|k| k.and_then(|k| by_key.get(&k).cloned())).collect())
    }

    async fn load_via<J: Model, T: Model + Clone>(&self, db: &impl Executor) -> Result<Vec<Vec<T>>> {
        let to_self = link::<J, M>()?;
        let to_target = link::<J, T>()?;
        let mut slots: KeyMap<usize> = KeyMap::with_capacity_and_hasher(self.len(), Default::default());
        let mut keys = Vec::with_capacity(self.len());
        for (i, row) in self.iter().enumerate() {
            let value = row.value_at(to_self.parent);
            if let Some(k) = key(&value)
                && let Entry::Vacant(e) = slots.entry(k)
            {
                e.insert(i);
                keys.push(value);
            }
        }
        let by_pk = J::query().order_by(Expr::Column(column_ref(J::SCHEMA, J::SCHEMA.primary_key)).asc());
        let junctions = fetch_in(db, by_pk, column_ref(J::SCHEMA, to_self.child), keys).await?;
        let targets = distinct(junctions.iter().map(|j| j.value_at(to_target.child)));
        let found = fetch_in(db, T::query(), column_ref(T::SCHEMA, to_target.parent), targets).await?;
        let by_key: KeyMap<T> =
            found.into_iter().filter_map(|t| key(&t.value_at(to_target.parent)).map(|k| (k, t))).collect();
        let mut out: Vec<Vec<T>> = (0..self.len()).map(|_| Vec::new()).collect();
        for j in &junctions {
            let slot = key(&j.value_at(to_self.child)).and_then(|k| slots.get(&k));
            let target = key(&j.value_at(to_target.child)).and_then(|k| by_key.get(&k));
            if let (Some(&i), Some(t)) = (slot, target) {
                out[i].push(t.clone());
            }
        }
        Ok(out)
    }
}

/// How `M` and `R` are related, for the relation filters.
#[derive(Clone, Copy, Debug)]
enum Rel {
    /// `R` has a foreign key to `M` (`M` has many `R`).
    Children(Link),
    /// `M` has a foreign key to `R` (`M` belongs to `R`).
    Parent(Link),
}

fn references(child: &TableSchema, parent: &TableSchema) -> bool {
    child.columns.iter().any(|c| c.references.is_some_and(|fk| fk.table == parent.name))
}

/// The relation between `M` and `R`: children first (a self-referencing model
/// filters on its children), then parent.
fn relation<M: Model, R: Model>() -> Result<Rel> {
    if references(R::SCHEMA, M::SCHEMA) {
        return link::<R, M>().map(Rel::Children);
    }
    if references(M::SCHEMA, R::SCHEMA) {
        return link::<M, R>().map(Rel::Parent);
    }
    Err(Error::Relation(format!(
        "`{}` and `{}` are not related: neither has a column with `#[memoire(references = \"...\")]` to the other",
        M::SCHEMA.name,
        R::SCHEMA.name
    )))
}

fn column(table: &'static TableSchema, index: usize) -> Expr {
    Expr::Column(column_ref(table, index))
}

fn in_subquery(expr: Expr, query: Select<Value>, negated: bool) -> Expr {
    Expr::InSubquery { expr: Box::new(expr), query: Box::new(query.into_ast()), negated }
}

/// The condition "this row of `M` has a related row of `R` in `related`"
/// (or has none, when `negated`).
fn has_related<M: Model, R: Model>(related: Select<R>, negated: bool) -> Expr {
    match relation::<M, R>().unwrap_or_else(|e| panic!("{e}")) {
        Rel::Children(l) => {
            let fk = column(R::SCHEMA, l.child);
            let keys = related.unordered().filter(not_null(fk.clone())).select_exprs::<Value>([fk]);
            in_subquery(column(M::SCHEMA, l.parent), keys, negated)
        }
        Rel::Parent(l) => {
            let pk = column(R::SCHEMA, l.parent);
            let keys = related.unordered().filter(not_null(pk.clone())).select_exprs::<Value>([pk]);
            let fk = column(M::SCHEMA, l.child);
            if negated {
                // A `NULL` foreign key points to nothing: such a row has no parent.
                Expr::IsNull { expr: Box::new(fk.clone()), negated: false }.or(in_subquery(fk, keys, true))
            } else {
                in_subquery(fk, keys, false)
            }
        }
    }
}

/// `expr IS NOT NULL`.
fn not_null(expr: Expr) -> Expr {
    Expr::IsNull { expr: Box::new(expr), negated: true }
}

/// Filters on relations, in the spirit of Eloquent's `has`, `whereHas`,
/// `doesntHave` and `whereDoesntHave`. The relation is read from the foreign
/// keys (see the [module documentation](crate::relation)), in either
/// direction: `User::query().has::<Post>()` (a user has many posts) and
/// `Post::query().has::<User>()` (a post belongs to a user) both work.
///
/// Each filter is one `IN (SELECT ...)` [subquery](crate::Column::in_query):
/// one statement on SQL databases, two on the in-memory engine, Redis and MongoDB.
///
/// # Panics
///
/// When the two models are not related by a foreign key, or by several
/// (filter on the foreign key column with [`Column::in_query`](crate::Column::in_query) then).
impl<M: Model> Select<M> {
    /// The rows with at least one related row of `R`.
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model)] #[memoire(table = "users")]
    /// # struct User { id: i64, name: String }
    /// # #[derive(Model)] #[memoire(table = "posts")]
    /// # struct Post { id: i64, #[memoire(references = "users(id)")] author_id: i64, published: bool }
    /// let authors = User::query().has::<Post>();
    /// assert_eq!(
    ///     authors.to_sql(Dialect::Postgres),
    ///     r#"SELECT "users"."id", "users"."name" FROM "users" WHERE "users"."id" IN (SELECT "posts"."author_id" FROM "posts" WHERE "posts"."author_id" IS NOT NULL)"#
    /// );
    /// ```
    pub fn has<R: Model>(self) -> Self {
        let condition = has_related::<M, R>(R::query(), false);
        self.filter(condition)
    }

    /// The rows without any related row of `R`.
    pub fn doesnt_have<R: Model>(self) -> Self {
        let condition = has_related::<M, R>(R::query(), true);
        self.filter(condition)
    }

    /// The rows with at least one related row of `R` matching `scope`
    /// (`whereHas` in Eloquent).
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "users")]
    /// # struct User { #[memoire(primary_key, auto_increment)] id: i64, name: String }
    /// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "posts")]
    /// # struct Post { #[memoire(primary_key, auto_increment)] id: i64, #[memoire(references = "users(id)")] author_id: i64, published: bool }
    /// # memoire_orm::block_on(async {
    /// # let db = MemoryDb::new();
    /// # db.create_table::<User>().await?;
    /// # db.create_table::<Post>().await?;
    /// # for name in ["Ada", "Bob", "Cy"] { User { id: 0, name: name.into() }.insert(&db).await?; }
    /// # Post { id: 0, author_id: 1, published: true }.insert(&db).await?;
    /// # Post { id: 0, author_id: 2, published: false }.insert(&db).await?;
    /// let published = User::query()
    ///     .where_has::<Post>(|posts| posts.filter(Post::PUBLISHED.eq(true)))
    ///     .all(&db)
    ///     .await?;
    /// assert_eq!(published.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["Ada"]);
    ///
    /// let silent = User::query().doesnt_have::<Post>().all(&db).await?;
    /// assert_eq!(silent[0].name, "Cy");
    /// # Ok::<(), memoire_orm::Error>(())
    /// # }).unwrap();
    /// ```
    pub fn where_has<R: Model>(self, scope: impl FnOnce(Select<R>) -> Select<R>) -> Self {
        let condition = has_related::<M, R>(scope(R::query()), false);
        self.filter(condition)
    }

    /// The rows without any related row of `R` matching `scope`
    /// (`whereDoesntHave` in Eloquent).
    pub fn where_doesnt_have<R: Model>(self, scope: impl FnOnce(Select<R>) -> Select<R>) -> Self {
        let condition = has_related::<M, R>(scope(R::query()), true);
        self.filter(condition)
    }

    /// The rows with at least `n` related rows of `R` (`has('posts', '>=', n)`
    /// in Eloquent), counted with `GROUP BY ... HAVING COUNT(*) >= n`. For a
    /// parent relation (`M` belongs to `R`), a row has at most one: `n <= 1`
    /// is [`has`](Self::has), more matches nothing.
    pub fn has_at_least<R: Model>(self, n: u64) -> Self {
        if n == 0 {
            return self;
        }
        match relation::<M, R>().unwrap_or_else(|e| panic!("{e}")) {
            Rel::Children(l) => {
                let fk = column(R::SCHEMA, l.child);
                let n = i64::try_from(n).unwrap_or(i64::MAX);
                let keys = R::query()
                    .filter(not_null(fk.clone()))
                    .group_by(fk.clone())
                    .having(Expr::binary(BinOp::Ge, count_all(), Expr::Value(Value::I64(n))))
                    .select_exprs::<Value>([fk]);
                self.filter(in_subquery(column(M::SCHEMA, l.parent), keys, false))
            }
            Rel::Parent(_) if n == 1 => self.has::<R>(),
            Rel::Parent(_) => self.filter(Expr::Value(Value::Bool(false))),
        }
    }

    /// Many-to-many: the rows linked through the junction model `J` to at
    /// least one row of `T` matching `scope` (two nested subqueries).
    ///
    /// ```
    /// # use memoire_orm::prelude::*;
    /// # #[derive(Model)] #[memoire(table = "posts")]
    /// # struct Post { id: i64, title: String }
    /// # #[derive(Model)] #[memoire(table = "tags")]
    /// # struct Tag { id: i64, name: String }
    /// # #[derive(Model)] #[memoire(table = "post_tags")]
    /// # struct PostTag { id: i64, #[memoire(references = "posts(id)")] post_id: i64, #[memoire(references = "tags(id)")] tag_id: i64 }
    /// let about_rust = Post::query().where_has_via::<PostTag, Tag>(|tags| tags.filter(Tag::NAME.eq("rust")));
    /// assert!(about_rust.to_sql(Dialect::Sqlite).contains(r#""post_tags"."tag_id" IN (SELECT "tags"."id" FROM "tags" WHERE "tags"."name" = ?)"#));
    /// ```
    pub fn where_has_via<J: Model, T: Model>(self, scope: impl FnOnce(Select<T>) -> Select<T>) -> Self {
        let to_self = link::<J, M>().unwrap_or_else(|e| panic!("{e}"));
        let to_target = link::<J, T>().unwrap_or_else(|e| panic!("{e}"));
        let target_keys = scope(T::query()).unordered().select_exprs::<Value>([column(T::SCHEMA, to_target.parent)]);
        let fk = column(J::SCHEMA, to_self.child);
        let links = J::query()
            .filter(not_null(fk.clone()))
            .filter(in_subquery(column(J::SCHEMA, to_target.child), target_keys, false))
            .select_exprs::<Value>([fk]);
        self.filter(in_subquery(column(M::SCHEMA, to_self.parent), links, false))
    }

    /// Runs the query and loads the children of `C` of every row (eager
    /// loading, `with('posts')` in Eloquent): two queries, whatever the number
    /// of rows. See [`LoadRelated::load_children`]; nest by calling
    /// `load_children` on the children.
    pub async fn all_with<C: Model>(self, db: &impl Executor) -> Result<Vec<(M, Vec<C>)>> {
        let rows = self.all(db).await?;
        let children = rows.load_children::<C>(db).await?;
        Ok(rows.into_iter().zip(children).collect())
    }

    /// Runs the query and loads the parent of `P` of every row (eager
    /// loading of a `belongsTo`): two queries. See [`LoadRelated::load_parent`].
    pub async fn all_with_parent<P: Model + Clone>(self, db: &impl Executor) -> Result<Vec<(M, Option<P>)>> {
        let rows = self.all(db).await?;
        let parents = rows.load_parent::<P>(db).await?;
        Ok(rows.into_iter().zip(parents).collect())
    }

    /// Runs the query and counts the related rows of `R` of every row
    /// (`withCount` in Eloquent): two queries, whatever the number of rows.
    /// See [`LoadCounts::load_count`].
    pub async fn all_with_count<R: Model>(self, db: &impl Executor) -> Result<Vec<(M, u64)>> {
        let rows = self.all(db).await?;
        let counts = rows.load_count::<R>(db).await?;
        Ok(rows.into_iter().zip(counts).collect())
    }
}

/// Counts of related rows for a slice of models, in one grouped query
/// (`withCount` / `loadCount` in Eloquent), without loading the related rows.
///
/// ```
/// # use memoire_orm::prelude::*;
/// # use memoire_orm::relation::LoadCounts;
/// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "users")]
/// # struct User { #[memoire(primary_key, auto_increment)] id: i64, name: String }
/// # #[derive(Model, Debug, Clone, PartialEq)] #[memoire(table = "posts")]
/// # struct Post { #[memoire(primary_key, auto_increment)] id: i64, #[memoire(references = "users(id)")] author_id: i64, published: bool }
/// # memoire_orm::block_on(async {
/// # let db = MemoryDb::new();
/// # db.create_table::<User>().await?;
/// # db.create_table::<Post>().await?;
/// # for name in ["Ada", "Bob"] { User { id: 0, name: name.into() }.insert(&db).await?; }
/// # for (a, p) in [(1, true), (1, false), (1, true)] { Post { id: 0, author_id: a, published: p }.insert(&db).await?; }
/// let users = User::query().order_by(User::ID).all(&db).await?;
/// assert_eq!(users.load_count::<Post>(&db).await?, [3, 0]);
/// let published = users.load_count_with(&db, Post::query().filter(Post::PUBLISHED.eq(true))).await?;
/// assert_eq!(published, [2, 0]);
/// # Ok::<(), memoire_orm::Error>(())
/// # }).unwrap();
/// ```
pub trait LoadCounts<M: Model> {
    /// The number of children of `C` of every row (rows of `C` whose foreign
    /// key points to it), aligned with `self`.
    fn load_count<C: Model>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<u64>>> + Send;

    /// Like [`load_count`](Self::load_count), counting only the children
    /// matching your query on `C` (its order and limit are ignored).
    fn load_count_with<C: Model>(
        &self,
        db: &impl Executor,
        query: Select<C>,
    ) -> impl Future<Output = Result<Vec<u64>>> + Send;
}

impl<M: Model> LoadCounts<M> for [M] {
    fn load_count<C: Model>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<u64>>> + Send {
        self.load_count_with(db, C::query())
    }

    async fn load_count_with<C: Model>(&self, db: &impl Executor, query: Select<C>) -> Result<Vec<u64>> {
        let link = link::<C, M>()?;
        let mut slots: KeyMap<Vec<usize>> = KeyMap::with_capacity_and_hasher(self.len(), Default::default());
        let mut keys = Vec::with_capacity(self.len());
        for (i, row) in self.iter().enumerate() {
            let value = row.value_at(link.parent);
            if let Some(k) = key(&value) {
                match slots.entry(k) {
                    Entry::Vacant(e) => {
                        e.insert(vec![i]);
                        keys.push(value);
                    }
                    Entry::Occupied(mut e) => e.get_mut().push(i),
                }
            }
        }
        let mut out = vec![0u64; self.len()];
        if keys.is_empty() {
            return Ok(out);
        }
        let fk = column(C::SCHEMA, link.child);
        let query = query.unordered();
        let chunk = db.dialect().map_or(usize::MAX, |d| d.max_params() / 2).max(1);
        for part in keys.chunks(chunk) {
            let filter = Expr::InList { expr: Box::new(fk.clone()), list: part.to_vec(), negated: false };
            let counts: Vec<(Value, i64)> = query
                .clone()
                .filter(filter)
                .group_by(fk.clone())
                .select_exprs::<(Value, i64)>([fk.clone(), count_all()])
                .all(db)
                .await?;
            for (value, n) in counts {
                if let Some(rows) = key(&value).and_then(|k| slots.get(&k)) {
                    for &i in rows {
                        out[i] = n.max(0) as u64;
                    }
                }
            }
        }
        Ok(out)
    }
}

/// The `(type, id)` columns of a polymorphic model, or an error.
pub(crate) fn morph_columns<C: Model>() -> Result<(usize, usize)> {
    C::MORPH_INDEX.ok_or_else(|| {
        Error::Relation(format!(
            "`{}` has no polymorphic relation: add `#[memoire(morph = \"name\")]` with the fields `name_type` and `name_id`",
            C::SCHEMA.name
        ))
    })
}

/// `C.type = <table of P>` (the name a polymorphic row gives to `P`).
pub(crate) fn morph_type_is<C: Model, P: Model>(type_col: usize) -> Expr {
    Expr::binary(BinOp::Eq, column(C::SCHEMA, type_col), Expr::Value(Value::Text(P::SCHEMA.name.to_owned())))
}

/// The rows of `self` grouped by key: the distinct non-`NULL` values of
/// `column` and, for each, the positions of the rows holding it.
fn slots_of<M: Model>(rows: &[M], col: usize) -> (KeyMap<usize>, Vec<Value>) {
    let mut slots: KeyMap<usize> = KeyMap::with_capacity_and_hasher(rows.len(), Default::default());
    let mut keys = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let value = row.value_at(col);
        if let Some(k) = key(&value)
            && let Entry::Vacant(e) = slots.entry(k)
        {
            e.insert(i);
            keys.push(value);
        }
    }
    (slots, keys)
}

/// Polymorphic relations (`morphMany` / `morphTo` in Eloquent), for a slice
/// of models, one query per call.
///
/// A polymorphic model declares `#[memoire(morph = "commentable")]` and the
/// fields `commentable_type: String` and `commentable_id`: each row points to
/// a row of any model, named by its **table name**. See the
/// [module documentation](crate::relation#polymorphic-relations).
pub trait LoadMorph<M: Model> {
    /// `morphMany`: the rows of the polymorphic model `C` pointing to every
    /// row of `self`, aligned with `self`, ordered by primary key.
    fn load_morph_many<C: Model>(&self, db: &impl Executor) -> impl Future<Output = Result<Vec<Vec<C>>>> + Send;

    /// `morphTo`, for the parents of type `P`: the row of `P` each row of
    /// `self` (a polymorphic model) points to; `None` when it points to
    /// another model, to nothing, or to a missing row.
    fn load_morph_parent<P: Model + Clone>(
        &self,
        db: &impl Executor,
    ) -> impl Future<Output = Result<Vec<Option<P>>>> + Send;
}

impl<M: Model> LoadMorph<M> for [M] {
    async fn load_morph_many<C: Model>(&self, db: &impl Executor) -> Result<Vec<Vec<C>>> {
        let (type_col, id_col) = morph_columns::<C>()?;
        let pk = M::SCHEMA.primary_key;
        let (slots, keys) = slots_of(self, pk);
        let query =
            C::query().filter(morph_type_is::<C, M>(type_col)).order_by(column(C::SCHEMA, C::SCHEMA.primary_key).asc());
        let children = fetch_in(db, query, column_ref(C::SCHEMA, id_col), keys).await?;
        let mut out: Vec<Vec<C>> = (0..self.len()).map(|_| Vec::new()).collect();
        for child in children {
            if let Some(&i) = key(&child.value_at(id_col)).and_then(|k| slots.get(&k)) {
                out[i].push(child);
            }
        }
        Ok(out)
    }

    async fn load_morph_parent<P: Model + Clone>(&self, db: &impl Executor) -> Result<Vec<Option<P>>> {
        let (type_col, id_col) = morph_columns::<M>()?;
        let name = Value::Text(P::SCHEMA.name.to_owned());
        let refs: Vec<Option<Key>> = self
            .iter()
            .map(|row| if row.value_at(type_col) == name { key(&row.value_at(id_col)) } else { None })
            .collect();
        let values = distinct(self.iter().filter(|row| row.value_at(type_col) == name).map(|row| row.value_at(id_col)));
        let pk = P::SCHEMA.primary_key;
        let parents = fetch_in(db, P::query(), column_ref(P::SCHEMA, pk), values).await?;
        let by_key: KeyMap<P> = parents.into_iter().filter_map(|p| key(&p.value_at(pk)).map(|k| (k, p))).collect();
        Ok(refs.into_iter().map(|k| k.and_then(|k| by_key.get(&k).cloned())).collect())
    }
}

/// Many-to-many with the junction rows (the "pivot", `withPivot` in
/// Eloquent): each partner comes with the row of the junction model that
/// links it, so the extra columns of the junction (a role, a date, a
/// quantity) are at hand.
pub trait LoadPivot<M: Model> {
    /// Like [`LoadRelated::load_via`], each partner `T` paired with its
    /// junction row `J`, aligned with `self`, in the order of the junction
    /// rows' primary keys. Two queries.
    fn load_via_pivot<J: Model, T: Model + Clone>(
        &self,
        db: &impl Executor,
    ) -> impl Future<Output = Result<Vec<Vec<(T, J)>>>> + Send;

    /// Like [`load_via_pivot`](Self::load_via_pivot), starting from your own
    /// query on the junction model (`wherePivot` / `orderByPivot` in
    /// Eloquent): `PostTag::query().filter(PostTag::ROLE.eq("main"))`.
    fn load_via_pivot_with<J: Model, T: Model + Clone>(
        &self,
        db: &impl Executor,
        junctions: Select<J>,
    ) -> impl Future<Output = Result<Vec<Vec<(T, J)>>>> + Send;
}

impl<M: Model> LoadPivot<M> for [M] {
    fn load_via_pivot<J: Model, T: Model + Clone>(
        &self,
        db: &impl Executor,
    ) -> impl Future<Output = Result<Vec<Vec<(T, J)>>>> + Send {
        self.load_via_pivot_with(db, J::query().order_by(column(J::SCHEMA, J::SCHEMA.primary_key).asc()))
    }

    async fn load_via_pivot_with<J: Model, T: Model + Clone>(
        &self,
        db: &impl Executor,
        junctions: Select<J>,
    ) -> Result<Vec<Vec<(T, J)>>> {
        let to_self = link::<J, M>()?;
        let to_target = link::<J, T>()?;
        let (slots, keys) = slots_of(self, to_self.parent);
        let junctions = fetch_in(db, junctions, column_ref(J::SCHEMA, to_self.child), keys).await?;
        let targets = distinct(junctions.iter().map(|j| j.value_at(to_target.child)));
        let found = fetch_in(db, T::query(), column_ref(T::SCHEMA, to_target.parent), targets).await?;
        let by_key: KeyMap<T> =
            found.into_iter().filter_map(|t| key(&t.value_at(to_target.parent)).map(|k| (k, t))).collect();
        let mut out: Vec<Vec<(T, J)>> = (0..self.len()).map(|_| Vec::new()).collect();
        for j in junctions {
            let slot = key(&j.value_at(to_self.child)).and_then(|k| slots.get(&k));
            let target = key(&j.value_at(to_target.child)).and_then(|k| by_key.get(&k));
            if let (Some(&i), Some(t)) = (slot, target) {
                out[i].push((t.clone(), j));
            }
        }
        Ok(out)
    }
}

/// Polls two futures concurrently on the current task and returns both
/// outputs. On a connection pool the two statements run on two connections at
/// the same time; on a single connection they are pipelined.
pub(crate) async fn join<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    use std::task::Poll;
    let (mut a, mut b) = (std::pin::pin!(a), std::pin::pin!(b));
    let (mut ra, mut rb) = (None, None);
    std::future::poll_fn(move |cx| {
        if ra.is_none()
            && let Poll::Ready(v) = a.as_mut().poll(cx)
        {
            ra = Some(v);
        }
        if rb.is_none()
            && let Poll::Ready(v) = b.as_mut().poll(cx)
        {
            rb = Some(v);
        }
        if ra.is_some() && rb.is_some() { Poll::Ready((ra.take().unwrap(), rb.take().unwrap())) } else { Poll::Pending }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_widths_share_a_key() {
        assert_eq!(key(&Value::I32(7)), key(&Value::I64(7)));
        assert_eq!(key(&Value::F64(7.0)), key(&Value::I64(7)));
        assert_ne!(key(&Value::F64(7.5)), key(&Value::I64(7)));
        assert_eq!(key(&Value::Null), None);
    }

    #[test]
    fn distinct_drops_nulls_and_duplicates() {
        let v = distinct([Value::I64(1), Value::Null, Value::I32(1), Value::I64(2)]);
        assert_eq!(v, vec![Value::I64(1), Value::I64(2)]);
    }
}
