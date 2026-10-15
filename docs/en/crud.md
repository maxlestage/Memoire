# Create, read, update, delete

This page covers every way to write and read rows with a model: inserting one or many rows, loading by key, updating one row or many, upserting, deleting, and handling the errors these operations can return.

## The examples on this page

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
    pub visits: i32,
}

impl User {
    pub fn new(email: &str, name: &str) -> Self {
        User { id: 0, email: email.into(), name: name.into(), age: None, active: true, visits: 0 }
    }
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}
```

Every method takes `&db`, which can be any database handle (`MemoryDb`, `SqliteDb`, `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb`) or an open [transaction](transactions).

## Create

### Insert one row

```rust
let mut alice = User::new("alice@example.com", "Alice");
alice.insert(&db).await?;
println!("Alice got id {}", alice.id); // 1
```

`insert` borrows the row mutably because, for an `auto_increment` key, it writes the generated value back into the struct. The `id` you set before inserting (here `0`) is ignored.

For a model whose key is not auto-increment, the key you set is the one stored:

```rust
let mut rust = Tag { slug: "rust".into(), label: "Rust".into() };
rust.insert(&db).await?;
```

Inserting a second row with the same primary key or the same value in a `unique` column fails with `Error::UniqueViolation` (see "Handling errors" below).

### Insert many rows at once

`insert_many` inserts a whole slice with as few statements as possible and returns how many rows were inserted:

```rust
let users: Vec<User> = (1..=1000)
    .map(|i| User::new(&format!("user{i}@example.com"), &format!("User {i}")))
    .collect();
let inserted = User::insert_many(&db, &users).await?;
assert_eq!(inserted, 1000);
```

On SQL databases this sends one multi-row `INSERT ... VALUES (...), (...), ...` per batch. Batches are sized to stay under the database's limit on bound parameters (32,766 for SQLite, 65,535 for PostgreSQL and MySQL) and hold at most 10,000 rows, so even huge slices are handled automatically. Redis writes each batch with one atomic script, and MongoDB with one `insertMany` call.

Things to know about `insert_many`:

- Generated keys are **not** read back: the structs in your slice keep `id: 0`. Query the rows again if you need their ids, or insert one by one with `insert`.
- Each batch is atomic: if a row fails (for example a duplicate email), nothing from that batch is stored. (On a standalone MongoDB server, which has no transactions, Memoire removes the already inserted documents on a best-effort basis.)
- When the slice is bigger than one batch, earlier batches stay stored if a later one fails. Wrap the call in a [transaction](transactions) to make the whole operation all-or-nothing. A transaction also makes bulk inserts much faster on SQLite.

## Read

### By primary key

```rust
let maybe: Option<User> = User::find(&db, 1).await?; // None if there is no row 1
let alice: User = User::get(&db, 1).await?;          // Error::NotFound if there is no row 1
let tag = Tag::get(&db, "rust").await?;              // string keys work the same way
```

Use `find` when a missing row is a normal situation, and `get` when it is an error. The key must have the type of the primary key: `User::find(&db, "1")` does not compile.

### Several rows by primary key

`find_many` fetches a list of keys in one query (`WHERE id IN (...)`), instead of one `find` per key. Missing keys are skipped and the rows come back in no particular order:

```rust
let some: Vec<User> = User::find_many(&db, [3, 1, 42]).await?; // users 1 and 3
```

### Every row, or a count

```rust
let everyone: Vec<User> = User::all(&db).await?;
let how_many: u64 = User::count(&db).await?;
```

For anything more specific (filters, sorting, pagination, a few columns...), use the [query builder](queries): `User::query().filter(...).all(&db)`.

### Refresh a row

`reload` re-reads a row from the database, replacing every field of your struct. It is useful after changes made with `update_many` or by someone else:

```rust
let mut alice = User::get(&db, 1).await?;
User::update_many().increment(User::VISITS, 1).filter(User::ID.eq(alice.id)).execute(&db).await?;
alice.reload(&db).await?;
assert_eq!(alice.visits, 1);
```

`reload` fails with `Error::NotFound` if the row was deleted in the meantime.

## Update

### Change one row

Load the row, change fields, then call `update`:

```rust
let mut alice = User::get(&db, 1).await?;
alice.name = "Alice Liddell".into();
alice.age = Some(32);
alice.update(&db).await?;
```

`update` writes **every column** of the struct to the row with the same primary key. If no row has that key, it fails with `Error::NotFound`; it never creates a row.

> **Note** Because `update` writes all columns, two programs that load the same row, change different fields and both call `update` will overwrite each other's change (the last write wins). For counters and other concurrent changes, use `update_many` with `increment`, or a [transaction](transactions).

### Change only some columns

`update_only` writes the columns you list, and only those, to the row with the same primary key. The other columns keep the value stored in the database, so a concurrent change to them is not overwritten, and less data is sent:

```rust
let mut alice = User::get(&db, 1).await?;
alice.name = "Alice Liddell".into();
alice.age = Some(32);
alice.update_only(&db, (User::NAME, User::AGE)).await?; // one column: `User::NAME`
```

The columns must belong to the model: `alice.update_only(&db, Post::TITLE)` does not compile. Like `update`, it fails with `Error::NotFound` if the row does not exist.

### Change many rows

`update_many` changes every row that matches its filters, without loading them, and returns the number of updated rows:

```rust
let changed = User::update_many()
    .set(User::ACTIVE, false)
    .filter(User::AGE.lt(18))
    .execute(&db)
    .await?;
println!("{changed} user(s) deactivated");
```

The available assignments:

| Method | SQL | Example |
|---|---|---|
| `set(column, value)` | `column = value` | `.set(User::NAME, "Bob")` |
| `set_null(column)` | `column = NULL` | `.set_null(User::AGE)` (only for `Option` columns) |
| `increment(column, n)` | `column = column + n` | `.increment(User::VISITS, 1)` |
| `decrement(column, n)` | `column = column - n` | `.decrement(User::VISITS, 1)` |
| `set_expr(column, expr)` | `column = <expression>` | `.set_expr(User::VISITS, User::VISITS.mul(2))` |

Assignments and filters can be combined freely:

```rust
User::update_many()
    .set(User::ACTIVE, true)
    .increment(User::VISITS, 1)
    .set_null(User::AGE)
    .filter(User::EMAIL.ends_with("@example.com") & User::ACTIVE.eq(false))
    .execute(&db)
    .await?;
```

Like queries, an update can show its SQL:

```rust
let update = User::update_many()
    .set(User::ACTIVE, false)
    .increment(User::VISITS, 1)
    .filter(User::NAME.eq("Bob"));
println!("{}", update.to_sql(Dialect::Postgres));
// UPDATE "users" SET "active" = $1, "visits" = ("users"."visits" + $2) WHERE "users"."name" = $3
```

> **Warning** Without `filter`, `update_many` changes **every row** of the table. At least one assignment is required: `update_many()` with no `set`, `increment`, ... fails with `Error::Unsupported`.

An update that would break a `unique` constraint fails as a whole, leaving every row untouched:

```rust
let err = User::update_many().set(User::EMAIL, "same@example.com").execute(&db).await.unwrap_err();
assert!(err.is_unique_violation());
```

## Upsert

`upsert` means "insert, or update if it already exists". It inserts the row, or, if a row with the same primary key exists, overwrites all its columns:

```rust
let tag = Tag { slug: "rust".into(), label: "Rust 🦀".into() };
tag.upsert(&db).await?; // creates "rust", or updates its label

Tag { slug: "orm".into(), label: "ORM".into() }.upsert(&db).await?;
```

It matches on the primary key only, and is safe even when two programs upsert the same key at the same time (`INSERT ... ON CONFLICT (key) DO UPDATE` on SQLite and PostgreSQL, `INSERT ... ON DUPLICATE KEY UPDATE` or an insert followed by an update on MySQL, a Lua script on Redis, `replaceOne` with `upsert` on MongoDB). If the row has a new key but a `unique` value that belongs to another row, `upsert` fails with `Error::UniqueViolation` on every backend.

> **Warning** `upsert` always stores the key that is in your struct, even for an `auto_increment` key. Upserting `User { id: 0, ... }` creates a row whose id is `0`. Use `insert` for new rows of auto-increment models, and `upsert` for natural keys (slugs, codes, UUIDs) or rows you know the id of. On PostgreSQL, also note that upserting new rows with explicit ids does not advance the id sequence, so a later `insert` can collide with them.

## Find or create, update or create, save

The everyday shortcuts of Eloquent, on any query of a model:

```rust
// The first match, or this new row, inserted (Eloquent's firstOrCreate).
let alice = User::query()
    .filter(User::EMAIL.eq("alice@example.com"))
    .first_or_create(&db, || User::new("alice@example.com", "Alice"))
    .await?;

// Changes the first match, or inserts a new row with the change (updateOrCreate).
let bob = User::query()
    .filter(User::EMAIL.eq("bob@example.com"))
    .update_or_create(&db, || User::new("bob@example.com", "Bob"), |u| u.visits += 1)
    .await?;

// Inserts a new row (auto-increment key still 0), updates a stored one.
let mut carol = User::new("carol@example.com", "Carol");
carol.save(&db).await?; // INSERT
carol.age = Some(28);
carol.save(&db).await?; // UPDATE
```

`first_or_create` copes with a race: if another writer inserts the same row between the lookup and the insert, the insert fails on the `unique` column and the row now stored is returned. `save` and `update_or_create` run the [hooks and timestamps](lifecycle) of the model.

## Delete

```rust
let alice = User::get(&db, 1).await?;
let deleted: bool = alice.delete(&db).await?; // true: the row existed

let deleted_again = User::delete_by_key(&db, 1).await?; // false: already gone
```

Both return `true` if a row was deleted and `false` if there was nothing to delete; a missing row is not an error.

To delete every row matching a filter, use `delete_many`, which returns the number of deleted rows:

```rust
let removed = User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
println!("removed {removed} inactive user(s)");
```

> **Warning** `User::delete_many().execute(&db)` without a filter deletes **every row** of the table.

On a model with [soft deletes](lifecycle), `delete`, `delete_by_key` and `delete_many` only stamp `deleted_at`; `force_delete()` and `delete_many().force()` remove the rows.

If other tables reference the deleted rows with `on_delete = "cascade"`, the database deletes those too (on SQLite, PostgreSQL and MySQL; the in-memory engine, Redis and MongoDB do not enforce foreign keys).

## Handling errors

Every operation returns `memoire_orm::Result<T>`, an alias for `Result<T, memoire_orm::Error>`. Errors are normalised: the same mistake gives the same error variant on every backend, so you can match on it without knowing which database you use.

| Error | When it happens |
|---|---|
| `Error::NotFound` | `get`, `one`, `update` or `reload` found no row |
| `Error::UniqueViolation { table, column }` | A primary key or `unique` value already exists |
| `Error::Constraint(message)` | Another constraint failed: `NOT NULL`, foreign key, `CHECK` |
| `Error::UnexpectedNull { column }` | A `NULL` was read into a non-`Option` field |
| `Error::Decode { column, message }` | A value could not be converted into the field's type |
| `Error::Conversion(message)` | A value could not be converted (outside of a specific column) |
| `Error::NoSuchTable(name)` | The table does not exist |
| `Error::TableExists(name)` | `create_table` on a table that already exists |
| `Error::Unsupported(message)` | The backend cannot do this (for example raw SQL on Redis, or a transaction on a standalone MongoDB server) |
| `Error::Conflict(table)` | A transaction collided with a concurrent one; retrying is safe |
| `Error::Validation(message)` | A [hook](lifecycle) (or your own code) rejected the row before it was written |
| `Error::TransactionFinished` | The transaction was already committed or rolled back |
| `Error::Migration { version, name, source }` | A migration failed; `source` holds the cause |
| `Error::Pool(message)` | The PostgreSQL pool could not provide a connection |
| `Error::Database(error)` | Any other error from the database driver |

Four helpers cover the most common checks: `is_not_found()`, `is_unique_violation()`, `is_conflict()` and `is_validation()`.

```rust
let mut duplicate = User::new("alice@example.com", "Another Alice");
match duplicate.insert(&db).await {
    Ok(()) => println!("created #{}", duplicate.id),
    Err(Error::UniqueViolation { column, .. }) => println!("this {column} is already taken"),
    Err(other) => return Err(other),
}
```

```text
this email is already taken
```

> **Tip** `Error` is marked `#[non_exhaustive]`: when you `match` on it, keep a catch-all arm (`Err(other) => ...`) as above. Every error implements `std::error::Error` and `Display`, so it also works with `anyhow`, `thiserror` and `Box<dyn Error>`.

## Next steps

- [Timestamps, soft deletes & hooks](lifecycle): `created_at`, `updated_at`, `deleted_at` and code that runs around each write.
- [Query builder](queries): find exactly the rows you need.
- [Transactions](transactions): group several writes into one all-or-nothing change.
- [Joins & relations](relations): work with related tables.
