//! Versioned schema migrations.
//!
//! ```
//! use memoire_orm::prelude::*;
//!
//! #[derive(Model)]
//! #[memoire(table = "users")]
//! struct User { id: i64, name: String }
//!
//! # memoire_orm::block_on(async {
//! let db = MemoryDb::new();
//! let migrator = Migrator::new()
//!     .add(Migration::new(1, "create_users").create_table::<User>());
//!
//! assert_eq!(migrator.run(&db).await?, vec![1]);
//! assert!(migrator.run(&db).await?.is_empty()); // already applied
//! # Ok::<(), memoire_orm::Error>(())
//! # }).unwrap();
//! ```

use std::collections::HashSet;

use crate::ast::Query;
use crate::error::{Error, Result};
use crate::executor::{Database, Executor, Transaction};
use crate::model::Model;
use crate::schema::{Dialect, TableSchema};

/// A migration recorded in the `_memoire_migrations` table.
#[derive(memoire_orm_derive::Model, Debug, Clone, PartialEq, Eq)]
#[memoire(table = "_memoire_migrations")]
pub struct AppliedMigration {
    /// Migration version.
    #[memoire(primary_key)]
    pub version: i64,
    /// Migration name.
    pub name: String,
    /// When it was applied (Unix time in milliseconds).
    pub applied_at: i64,
}

#[derive(Debug, Clone)]
enum Step {
    CreateTable(&'static TableSchema),
    DropTable(String),
    Sql(String),
    DialectSql(Dialect, String),
}

/// One versioned migration: an ordered list of schema changes applied in a
/// single transaction.
#[derive(Debug, Clone)]
pub struct Migration {
    version: i64,
    name: String,
    steps: Vec<Step>,
}

impl Migration {
    /// A new, empty migration. Versions must be unique; they are applied in
    /// ascending order.
    pub fn new(version: i64, name: impl Into<String>) -> Self {
        Migration { version, name: name.into(), steps: Vec::new() }
    }

    /// Version number.
    pub fn version(&self) -> i64 {
        self.version
    }

    /// Name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Creates the table of a model (works on every backend).
    pub fn create_table<M: Model>(mut self) -> Self {
        self.steps.push(Step::CreateTable(M::SCHEMA));
        self
    }

    /// Drops a table (works on every backend).
    pub fn drop_table(mut self, name: impl Into<String>) -> Self {
        self.steps.push(Step::DropTable(name.into()));
        self
    }

    /// Runs a SQL script on every SQL database (not supported by the memory
    /// engine).
    pub fn sql(mut self, sql: impl Into<String>) -> Self {
        self.steps.push(Step::Sql(sql.into()));
        self
    }

    /// Runs a SQL script only on one dialect (skipped elsewhere, including on
    /// the memory engine).
    pub fn sql_for(mut self, dialect: Dialect, sql: impl Into<String>) -> Self {
        self.steps.push(Step::DialectSql(dialect, sql.into()));
        self
    }

    async fn apply<E: Executor>(&self, db: &E) -> Result<()> {
        for step in &self.steps {
            match step {
                Step::CreateTable(t) => {
                    db.execute(Query::CreateTable { table: t, if_not_exists: false }).await?;
                }
                Step::DropTable(name) => {
                    db.execute(Query::DropTable { name, if_exists: false }).await?;
                }
                Step::Sql(sql) => {
                    if db.dialect().is_none() {
                        return Err(Error::Unsupported(
                            "SQL migration steps need a SQL database; use `sql_for` to target one dialect".into(),
                        ));
                    }
                    db.execute(Query::Script { sql }).await?;
                }
                Step::DialectSql(d, sql) => {
                    if db.dialect() == Some(*d) {
                        db.execute(Query::Script { sql }).await?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Applies pending [`Migration`]s and records them in `_memoire_migrations`.
#[derive(Debug, Clone, Default)]
pub struct Migrator {
    migrations: Vec<Migration>,
}

fn now_millis() -> i64 {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        0
    }
}

impl Migrator {
    /// A migrator without migrations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a migration.
    pub fn add(mut self, migration: Migration) -> Self {
        self.migrations.push(migration);
        self
    }

    /// Registered migrations, sorted by version.
    pub fn migrations(&self) -> Vec<&Migration> {
        let mut v: Vec<&Migration> = self.migrations.iter().collect();
        v.sort_by_key(|m| m.version);
        v
    }

    fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for m in &self.migrations {
            if !seen.insert(m.version) {
                return Err(Error::Migration {
                    version: m.version,
                    name: m.name.clone(),
                    source: Box::new(Error::Conversion("duplicate migration version".into())),
                });
            }
        }
        Ok(())
    }

    /// Migrations already applied to the database, sorted by version.
    pub async fn applied<E: Executor>(&self, db: &E) -> Result<Vec<AppliedMigration>> {
        db.create_table_if_not_exists::<AppliedMigration>().await?;
        AppliedMigration::query().order_by(AppliedMigration::VERSION.asc()).all(db).await
    }

    /// Migrations not applied yet, sorted by version.
    pub async fn pending<E: Executor>(&self, db: &E) -> Result<Vec<&Migration>> {
        let applied: HashSet<i64> = self.applied(db).await?.into_iter().map(|m| m.version).collect();
        Ok(self.migrations().into_iter().filter(|m| !applied.contains(&m.version)).collect())
    }

    /// Applies every pending migration, each one in its own transaction, and
    /// returns the versions that were applied.
    pub async fn run<D: Database>(&self, db: &D) -> Result<Vec<i64>> {
        self.validate()?;
        let pending = self.pending(db).await?;
        let mut done = Vec::with_capacity(pending.len());
        for m in pending {
            let wrap = |e: Error| Error::Migration { version: m.version, name: m.name.clone(), source: Box::new(e) };
            let tx = db.begin().await.map_err(wrap)?;
            m.apply(&tx).await.map_err(wrap)?;
            let mut record = AppliedMigration { version: m.version, name: m.name.clone(), applied_at: now_millis() };
            record.insert(&tx).await.map_err(wrap)?;
            tx.commit().await.map_err(wrap)?;
            done.push(m.version);
        }
        Ok(done)
    }
}
