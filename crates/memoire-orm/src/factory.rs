//! Model factories and seeders, for tests, demos and development databases, in
//! the spirit of Laravel's factories and seeders and Rails' fixtures.
//!
//! * [`Factory`] — implement `definition(n)` once per model: a valid row, made
//!   unique by the sequence number `n`. [`Factory::factory`] then builds as many
//!   rows as you want, with [states](FactoryBuilder::state),
//!   [sequences](FactoryBuilder::sequence) and a [parent](FactoryBuilder::for_parent),
//!   in memory ([`make`](FactoryBuilder::make)) or stored
//!   ([`create`](FactoryBuilder::create)).
//! * [`Seeder`] — a named piece of setup that fills a database; [`seed`] runs one
//!   (or a tuple of them, in order) inside a transaction.
//!
//! ```
//! use memoire_orm::factory::{Factory, Seeder, seed};
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model, Debug, Clone, PartialEq)]
//! #[memoire(table = "users")]
//! struct User {
//!     #[memoire(primary_key, auto_increment)]
//!     id: i64,
//!     #[memoire(unique)]
//!     email: String,
//!     admin: bool,
//! }
//!
//! impl Factory for User {
//!     fn definition(n: u64) -> Self {
//!         User { id: 0, email: format!("user{n}@example.com"), admin: false }
//!     }
//! }
//!
//! struct Users;
//! impl Seeder for Users {
//!     async fn run<E: Executor>(&self, db: &E) -> Result<()> {
//!         User::factory().count(3).create(db).await?;
//!         User::factory().state(|u| u.admin = true).create_one(db).await?;
//!         Ok(())
//!     }
//! }
//!
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new();
//! db.create_table::<User>().await?;
//! seed(&db, &Users).await?;
//! assert_eq!(User::count(&db).await?, 4);
//! assert_eq!(User::query().filter(User::ADMIN.eq(true)).count(&db).await?, 1);
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```

use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;

use crate::error::Result;
use crate::executor::{Database, Executor, Transaction};
use crate::model::Model;
use crate::relation::link;

/// The last sequence number handed out, per model.
static SEQUENCES: Mutex<Option<HashMap<TypeId, u64>>> = Mutex::new(None);

fn next_numbers<M: 'static>(count: u64) -> u64 {
    let mut guard = SEQUENCES.lock().unwrap_or_else(|e| e.into_inner());
    let last = guard.get_or_insert_with(HashMap::new).entry(TypeId::of::<M>()).or_insert(0);
    let first = *last + 1;
    *last += count;
    first
}

/// A model that can make example rows of itself.
///
/// `definition(n)` returns a valid row; `n` is a sequence number, unique per
/// model for the whole process (1, 2, 3...), to fill unique columns. Leave an
/// `auto_increment` key at `0`: the database fills it on insert.
pub trait Factory: Model {
    /// A valid row, the `n`-th made by this factory.
    fn definition(n: u64) -> Self;

    /// A builder of rows of this model, one row unless you set a
    /// [`count`](FactoryBuilder::count).
    fn factory() -> FactoryBuilder<Self> {
        FactoryBuilder { count: 1, states: Vec::new(), error: None }
    }

    /// Starts the sequence numbers of this model again at 1 (for tests that
    /// compare generated values).
    fn reset_sequence() {
        let mut guard = SEQUENCES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(map) = guard.as_mut() {
            map.remove(&TypeId::of::<Self>());
        }
    }
}

type State<M> = Box<dyn Fn(&mut M, usize) + Send + Sync>;

/// Builds rows of `M` from its [`Factory::definition`], returned by
/// [`Factory::factory`].
#[must_use = "a factory makes nothing until `.make()` or `.create(&db)` is called"]
pub struct FactoryBuilder<M> {
    count: usize,
    states: Vec<State<M>>,
    error: Option<crate::Error>,
}

impl<M> std::fmt::Debug for FactoryBuilder<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryBuilder").field("count", &self.count).field("states", &self.states.len()).finish()
    }
}

impl<M: Factory> FactoryBuilder<M> {
    /// The number of rows to build.
    pub fn count(mut self, n: usize) -> Self {
        self.count = n;
        self
    }

    /// Changes every row after its definition (a "state" in Laravel):
    /// `.state(|u| u.admin = true)`. States apply in the order they are added.
    pub fn state(mut self, state: impl Fn(&mut M) + Send + Sync + 'static) -> Self {
        self.states.push(Box::new(move |row, _| state(row)));
        self
    }

    /// Changes each row according to its position in this batch (0, 1, 2...):
    /// `.sequence(|u, i| u.admin = i % 2 == 0)` alternates admins and users.
    pub fn sequence(mut self, state: impl Fn(&mut M, usize) + Send + Sync + 'static) -> Self {
        self.states.push(Box::new(state));
        self
    }

    /// Points the foreign key of every row to `parent` (`for()` in Laravel):
    /// `Post::factory().count(3).for_parent(&alice)`. The foreign key is the
    /// column of `M` that references the table of `P` (see [`relation`](crate::relation));
    /// without one, or with several, [`make`](Self::make) and
    /// [`create`](Self::create) fail with [`Error::Relation`](crate::Error::Relation).
    pub fn for_parent<P: Model>(mut self, parent: &P) -> Self {
        match link::<M, P>() {
            Ok(l) => {
                let value = parent.value_at(l.parent);
                self.states.push(Box::new(move |row, _| {
                    let _ = row.set_value_at(l.child, value.clone());
                }));
            }
            Err(e) => self.error = Some(e),
        }
        self
    }

    /// The rows, not stored.
    pub fn make(self) -> Result<Vec<M>> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let first = next_numbers::<M>(self.count as u64);
        Ok((0..self.count)
            .map(|i| {
                let mut row = M::definition(first + i as u64);
                for state in &self.states {
                    state(&mut row, i);
                }
                row
            })
            .collect())
    }

    /// One row, not stored (whatever the [`count`](Self::count)).
    pub fn make_one(self) -> Result<M> {
        let mut rows = self.count(1).make()?;
        Ok(rows.pop().expect("one row"))
    }

    /// The rows, inserted one by one with [`Model::insert`] (hooks and
    /// timestamps apply, generated keys are written back), in order.
    pub async fn create<E: Executor>(self, db: &E) -> Result<Vec<M>> {
        let mut rows = self.make()?;
        for row in &mut rows {
            row.insert(db).await?;
        }
        Ok(rows)
    }

    /// One row, inserted (whatever the [`count`](Self::count)).
    pub async fn create_one<E: Executor>(self, db: &E) -> Result<M> {
        let mut row = self.make_one()?;
        row.insert(db).await?;
        Ok(row)
    }
}

/// A piece of setup that fills a database: reference data, a demo, a test
/// scenario. Run it with [`seed`]; a tuple of seeders runs them in order.
pub trait Seeder: Sync {
    /// Writes the rows.
    fn run<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send;
}

macro_rules! seeder_tuple {
    ($($s:ident $i:tt),+) => {
        impl<$($s: Seeder),+> Seeder for ($($s,)+) {
            async fn run<E: Executor>(&self, db: &E) -> Result<()> {
                $(self.$i.run(db).await?;)+
                Ok(())
            }
        }
    };
}

seeder_tuple!(A 0);
seeder_tuple!(A 0, B 1);
seeder_tuple!(A 0, B 1, C 2);
seeder_tuple!(A 0, B 1, C 2, D 3);
seeder_tuple!(A 0, B 1, C 2, D 3, F 4);
seeder_tuple!(A 0, B 1, C 2, D 3, F 4, G 5);

impl<S: Seeder> Seeder for &S {
    fn run<E: Executor>(&self, db: &E) -> impl Future<Output = Result<()>> + Send {
        (**self).run(db)
    }
}

/// Runs `seeder` in a transaction: either every row is written, or none
/// (`php artisan db:seed`).
pub async fn seed<D: Database, S: Seeder>(db: &D, seeder: &S) -> Result<()> {
    let tx = db.begin().await?;
    seeder.run(&tx).await?;
    tx.commit().await
}
