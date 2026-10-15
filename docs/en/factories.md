# Factories & seeders

Tests and demos need data. Model factories describe a valid row once, then build as many as you want, in the spirit of Laravel's factories and seeders and Rails' fixtures. They live in `memoire_orm::factory` and work on every backend.

## A factory

Implement `Factory` with a `definition(n)`: a valid row, made unique by the sequence number `n` (1, 2, 3... per model, for the whole process). Leave an `auto_increment` key at `0`.

```rust
use memoire_orm::factory::Factory;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub admin: bool,
}

impl Factory for User {
    fn definition(n: u64) -> Self {
        User { id: 0, email: format!("user{n}@example.com"), admin: false }
    }
}
```

## Building rows

```rust
let one = User::factory().create_one(&db).await?;                     // inserted, id set
let many = User::factory().count(10).create(&db).await?;              // 10 rows, in order
let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
let mixed = User::factory().count(4).sequence(|u, i| u.admin = i % 2 == 0).create(&db).await?;
let unsaved: Vec<User> = User::factory().count(3).make()?;            // not stored
let posts = Post::factory().count(3).for_parent(&one).create(&db).await?; // author_id = one.id
```

| Method | Effect |
|---|---|
| `count(n)` | how many rows (1 by default) |
| `state(\|row\| ...)` | changes every row (a Laravel "state"); states apply in order |
| `sequence(\|row, i\| ...)` | changes each row according to its position in the batch |
| `for_parent(&parent)` | points the foreign key to `parent` (Laravel's `for`) |
| `make()`, `make_one()` | the rows, not stored |
| `create(&db)`, `create_one(&db)` | the rows, inserted with `insert`: hooks, timestamps and generated keys apply |

`Factory::reset_sequence()` starts the numbers of a model again at 1, for tests that compare generated values.

## Seeders

A seeder is a named piece of setup. `seed` runs it in a transaction, so a seeder that fails writes nothing; a tuple of seeders runs them in order:

```rust
use memoire_orm::factory::{Seeder, seed};

struct Users;
impl Seeder for Users {
    async fn run<E: Executor>(&self, db: &E) -> Result<()> {
        User::factory().count(50).create(db).await?;
        User::factory().state(|u| u.admin = true).create_one(db).await?;
        Ok(())
    }
}

struct Catalog;
impl Seeder for Catalog {
    async fn run<E: Executor>(&self, db: &E) -> Result<()> {
        Product::factory().count(200).create(db).await?;
        Ok(())
    }
}

seed(&db, &(Users, Catalog)).await?;
```

With `MemoryDb::new()` a test gets a fresh database in microseconds: create the tables, seed, assert.

## Next steps

- [Relation filters, counts & polymorphism](relations-advanced)
- [Optimistic locking & identity map](optimistic-locking)
