//! An identity map, in the spirit of Doctrine's: each row is read from the
//! database once per map, and every later lookup of the same primary key
//! returns the same shared object, without a query.
//!
//! Use one map per unit of work (an HTTP request, a job, a test): it is a cache
//! that never expires, and it does not see writes made through other handles.
//! Rows are shared as [`Arc`]s; to change one, clone it, write it, and
//! [`remember`](IdentityMap::remember) the new version (or
//! [`forget`](IdentityMap::forget) it).
//!
//! ```
//! use std::sync::Arc;
//! use memoire_orm::identity::IdentityMap;
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
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new();
//! db.create_table::<User>().await?;
//! User { id: 0, name: "Ada".into() }.insert(&db).await?;
//!
//! let map = IdentityMap::new();
//! let a = map.find::<User, _>(&db, 1).await?.unwrap(); // one query
//! let b = map.find::<User, _>(&db, 1).await?.unwrap(); // none: the same object
//! assert!(Arc::ptr_eq(&a, &b));
//! assert_eq!(map.len(), 1);
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::Result;
use crate::executor::Executor;
use crate::model::Model;
use crate::relation::{Key, column_ref, fetch_in, key};
use crate::value::{Bind, Value};

type Entry = Arc<dyn Any + Send + Sync>;

/// One shared object per row: see the [module documentation](self).
#[derive(Default)]
pub struct IdentityMap {
    rows: Mutex<HashMap<(TypeId, Key), Entry>>,
}

impl std::fmt::Debug for IdentityMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityMap").field("rows", &self.len()).finish()
    }
}

impl IdentityMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(TypeId, Key), Entry>> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The row of `M` with this primary key, if the map holds it (no query).
    pub fn get<M: Model>(&self, key_value: impl Bind<M::Key>) -> Option<Arc<M>> {
        let k = key(&key_value.into_bind())?;
        self.lock().get(&(TypeId::of::<M>(), k)).cloned().and_then(|e| e.downcast::<M>().ok())
    }

    /// Keeps `row` in the map and returns it shared. A row already held for
    /// the same key is replaced (to keep the first one, use [`find`](Self::find)).
    pub fn remember<M: Model>(&self, row: M) -> Arc<M> {
        let shared = Arc::new(row);
        if let Some(k) = key(&shared.key()) {
            self.lock().insert((TypeId::of::<M>(), k), shared.clone());
        }
        shared
    }

    /// The row of `M` with this primary key: from the map, or read from the
    /// database once and kept (`EntityManager::find` in Doctrine). A missing
    /// row is not remembered.
    pub async fn find<M: Model, E: Executor>(&self, db: &E, key_value: impl Bind<M::Key>) -> Result<Option<Arc<M>>> {
        let value = key_value.into_bind();
        if let Some(found) = self.get_value::<M>(&value) {
            return Ok(Some(found));
        }
        Ok(M::query().filter(crate::model::pk_eq::<M>(value)).first(db).await?.map(|row| self.keep(row)))
    }

    /// The rows of `M` with these primary keys, in one query for the ones the
    /// map does not hold yet. Missing keys are skipped; the order follows `keys`.
    pub async fn find_many<M: Model, E: Executor, K: Bind<M::Key>>(
        &self,
        db: &E,
        keys: impl IntoIterator<Item = K>,
    ) -> Result<Vec<Arc<M>>> {
        let keys: Vec<Value> = keys.into_iter().map(Bind::into_bind).collect();
        let missing: Vec<Value> = keys.iter().filter(|k| self.get_value::<M>(k).is_none()).cloned().collect();
        let pk = column_ref(M::SCHEMA, M::SCHEMA.primary_key);
        for row in fetch_in(db, M::query(), pk, missing).await? {
            self.keep(row);
        }
        Ok(keys.iter().filter_map(|k| self.get_value::<M>(k)).collect())
    }

    /// Keeps the rows a query returned, each replaced by the object the map
    /// already holds for its key, if any: the same row is one object.
    pub fn merge<M: Model>(&self, rows: Vec<M>) -> Vec<Arc<M>> {
        rows.into_iter().map(|row| self.keep(row)).collect()
    }

    /// Forgets the row of `M` with this primary key (after you changed or
    /// deleted it). Returns `true` if the map held it.
    pub fn forget<M: Model>(&self, key_value: impl Bind<M::Key>) -> bool {
        match key(&key_value.into_bind()) {
            Some(k) => self.lock().remove(&(TypeId::of::<M>(), k)).is_some(),
            None => false,
        }
    }

    /// Forgets every row (`EntityManager::clear` in Doctrine).
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The number of rows held, all models together.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// `true` when no row is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get_value<M: Model>(&self, value: &Value) -> Option<Arc<M>> {
        let k = key(value)?;
        self.lock().get(&(TypeId::of::<M>(), k)).cloned().and_then(|e| e.downcast::<M>().ok())
    }

    /// The object held for the key of `row`, or `row`, now held.
    fn keep<M: Model>(&self, row: M) -> Arc<M> {
        let Some(k) = key(&row.key()) else { return Arc::new(row) };
        let mut rows = self.lock();
        let entry = rows.entry((TypeId::of::<M>(), k)).or_insert_with(|| Arc::new(row) as Entry);
        entry.clone().downcast::<M>().unwrap_or_else(|_| unreachable!("rows are keyed by their type"))
    }
}
