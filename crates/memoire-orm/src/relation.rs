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
use crate::expr::{ColumnRef, Expr};
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
struct KeyHasher(u64);

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

type KeyMap<V> = HashMap<Key, V, std::hash::BuildHasherDefault<KeyHasher>>;

/// Hashable form of a key value. Integers of every width compare equal (an
/// `i32` foreign key matches an `i64` primary key), like in SQL.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Int(i64),
    Float(u64),
    Text(String),
    Bytes(Vec<u8>),
    Other(String),
}

/// `None` for `NULL`, which never matches anything.
fn key(value: &Value) -> Option<Key> {
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
