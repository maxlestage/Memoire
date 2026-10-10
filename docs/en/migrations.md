# Migrations

Migrations are numbered, ordered changes to your database schema: "1: create the users table", "2: add a bio column", and so on. Memoire records which ones already ran, so every database (your laptop, your tests, production) reaches the same schema by applying only what is missing.

## Your first migration

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub user_id: i64,
    pub title: String,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<User>())
        .add(Migration::new(2, "create_posts").create_table::<Post>())
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("app.db")?;

    let applied = migrations().run(&db).await?;
    println!("applied migrations: {applied:?}");

    // ... the rest of your program ...
    Ok(())
}
```

The first run prints `applied migrations: [1, 2]`. Every later run prints `applied migrations: []`, because both are already recorded. Running the migrator at startup is therefore safe and the most common setup.

The building blocks:

- `Migration::new(version, name)` creates a migration. The version is a unique number (`i64`) and decides the order; the name is a human-readable label.
- `Migrator::new().add(...)` collects migrations. The order of the `add` calls does not matter: migrations always run by ascending version.
- `migrator.run(&db)` applies the pending migrations and returns the versions it applied.

## Migration steps

A migration is a list of steps, run in the order you add them:

| Step | What it does | In-memory, Redis, MongoDB |
|---|---|---|
| `.create_table::<M>()` | `CREATE TABLE` (and its indexes) for model `M`; a collection and its indexes on MongoDB | Supported |
| `.drop_table("name")` | `DROP TABLE name` | Supported |
| `.sql("...")` | Runs a SQL script on any SQL database (SQLite, PostgreSQL, MySQL) | **Fails** with `Error::Unsupported` |
| `.sql_for(Dialect::Postgres, "...")` | Runs a SQL script only on that database (`Dialect::Sqlite`, `Dialect::Postgres` or `Dialect::MySql`); skipped elsewhere | Skipped |

One migration can mix several steps:

```rust
let release_2 = Migration::new(3, "comments_and_cleanup")
    .create_table::<Comment>()
    .drop_table("legacy_comments")
    .sql_for(Dialect::Postgres, "CREATE INDEX idx_posts_title ON posts (title)")
    .sql_for(Dialect::Sqlite, "CREATE INDEX idx_posts_title ON posts (title)")
    .sql_for(Dialect::MySql, "CREATE INDEX idx_posts_title ON posts (title(100))");
```

The MySQL version indexes the first 100 characters only, because MySQL cannot index a whole `TEXT` column: `sql_for` is how you handle such differences.

A `sql` or `sql_for` script may contain several statements separated by semicolons.

## Changing a table later

To change an existing table, add a **new** migration with the SQL for the change. Here we add a `bio` column. `sql_for` lets you write the statement for each database you support:

```rust
let add_bio = Migration::new(4, "add_user_bio")
    .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
    .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT")
    .sql_for(Dialect::MySql, "ALTER TABLE users ADD COLUMN bio TEXT");
```

Then add the field to the struct (`pub bio: Option<String>`), so the model matches the new schema.

> **Warning** Never edit a migration that has already run somewhere: databases that applied it will not run it again, so your change would never reach them. Always add a new migration with a higher version.

### Freezing the shape of old tables

`create_table::<User>()` creates the table from the struct **as it is in your current code**. After you add `bio` to `User`, migration 1 would create `users` with a `bio` column on a brand-new database, and migration 4 would then fail because the column already exists.

The fix is to keep a frozen copy of the struct as it was when migration 1 was written, and use that copy in the old migration:

```rust
/// `users` as it was in migration 1. Never change this module.
mod v1 {
    use memoire_orm::prelude::*;

    #[derive(Model)]
    #[memoire(table = "users")]
    pub struct User {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        #[memoire(unique)]
        pub email: String,
    }
}

/// The current model, used by the application.
#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub bio: Option<String>,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<v1::User>())
        .add(
            Migration::new(2, "add_user_bio")
                .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
                .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT")
                .sql_for(Dialect::MySql, "ALTER TABLE users ADD COLUMN bio TEXT"),
        )
}
```

Now a new database and an old one both end up with the same schema. Alternatively, you can paste the `CREATE TABLE` statement produced by `memoire_orm::sql::create_table` into a `sql_for` step.

## Where Memoire records migrations

Applied migrations are stored in a table called `_memoire_migrations`, created automatically the first time you call `run`, `pending` or `applied`:

| Column | Type | Content |
|---|---|---|
| `version` | `BIGINT` (primary key) | The migration version |
| `name` | `TEXT` | The migration name |
| `applied_at` | `BIGINT` | When it ran, in milliseconds since the Unix epoch |

You can inspect it from Rust:

```rust
async fn status<D: Database>(db: &D) -> memoire_orm::Result<()> {
    let migrator = migrations();

    for done in migrator.applied(db).await? {
        println!("applied: {} {} (at {} ms)", done.version, done.name, done.applied_at);
    }
    for todo in migrator.pending(db).await? {
        println!("pending: {} {}", todo.version(), todo.name());
    }
    Ok(())
}
```

`applied` returns `memoire_orm::migrate::AppliedMigration` rows; `pending` returns the `Migration`s that have not run yet, sorted by version.

> **Note** Only the version is used to decide whether a migration ran. Renaming a migration does not make it run again.

## How `run` works and what happens on failure

`run` does the following:

1. Checks that no two migrations share a version (otherwise it fails before touching the database).
2. Reads `_memoire_migrations` to find the pending migrations.
3. For each pending migration, in ascending version order, it opens a **transaction**, runs every step, records the migration in `_memoire_migrations`, and commits. (On a standalone MongoDB server, which has no transactions, the steps run without one.)
4. Returns the list of applied versions.

If a step fails:

- that migration's transaction is rolled back: none of its steps and no record are kept;
- the migrations applied before it stay applied (they were committed);
- the following migrations are not attempted;
- `run` returns `Error::Migration { version, name, source }`, where `source` is the underlying error.

```rust
match migrations().run(&db).await {
    Ok(applied) => println!("schema up to date (applied {applied:?})"),
    Err(Error::Migration { version, name, source }) => {
        eprintln!("migration {version} ({name}) failed: {source}");
    }
    Err(other) => return Err(other),
}
```

Fix the migration and run again: it continues where it stopped.

How much of a failed migration is undone depends on the backend:

| Backend | Schema changes of a failed migration |
|---|---|
| SQLite, PostgreSQL, in-memory, Redis | Rolled back: nothing is left half-applied |
| MySQL / MariaDB | **Kept**: MySQL commits every schema change immediately |
| MongoDB | **Kept**: collections are created and dropped outside transactions |

> **Warning** On MySQL and MongoDB, a migration that fails after a `create_table` (or any other schema change) leaves that change in place, while the migration is not recorded as applied. Undo it by hand before running the fixed migration again. Keeping one schema change per migration makes this easy.

## Migrations without SQL: in-memory, Redis and MongoDB

The in-memory engine, Redis and MongoDB have no SQL, so they only understand `create_table` and `drop_table` steps. A `sql` step makes the migration fail with `Error::Unsupported`, and `sql_for` steps are skipped. Redis and MongoDB have no fixed columns, so adding a field to a model needs no migration there: new rows have the field, and rows written before read it as empty (`NULL`), so declare new fields as `Option<T>`. (This does not create indexes for a new `unique` or `index` field. The in-memory engine starts empty at every run, so it has nothing to migrate.)

For unit tests on `MemoryDb`, the simplest approach is usually to create the tables straight from your current models instead of running the migrator:

```rust
async fn test_db() -> memoire_orm::Result<MemoryDb> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;
    db.create_table::<Post>().await?;
    Ok(db)
}
```

## Next steps

- [Transactions](transactions): how the transaction around each migration behaves.
- [Raw SQL](raw-sql): run SQL outside of migrations.
- [Models](models): attributes that shape the generated `CREATE TABLE`.
