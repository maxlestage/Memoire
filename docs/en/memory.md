# In-memory engine

`MemoryDb` is a database engine written in pure Rust that keeps every table in memory. It needs no file, no server and no async runtime, it runs the same queries as the other backends, and it even works in WebAssembly. It is enabled by default (feature `memory`).

## When to use it

| Great for | Not suited for |
|---|---|
| Unit and integration tests: fast, isolated, nothing to clean up | Data that must survive a restart (nothing is written to disk) |
| Prototypes and demos | Data larger than the available memory |
| Caches and per-process scratch data | Sharing data between several processes |
| Browser apps compiled to WebAssembly | Raw SQL (there is no SQL parser) |

## Getting started

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub slug: String,
    pub text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;

        let mut note = Note { id: 0, slug: "hello".into(), text: "Hello!".into() };
        note.insert(&db).await?;

        let found = Note::query().filter(Note::SLUG.eq("hello")).one(&db).await?;
        println!("{}", found.text);
        Ok(())
    })
}
```

No runtime is needed: the engine never waits for I/O, so `memoire_orm::block_on` runs its futures directly. It works just as well inside Tokio or any other executor.

### Sharing a database

`MemoryDb` is cheap to clone, and every clone sees the same data. Give a clone to each thread or task:

```rust
let db = MemoryDb::new();
let for_worker = db.clone(); // same tables, same rows
```

The data lives as long as at least one clone exists. It is safe to use from many threads at once: reads run in parallel and writes are serialised.

### Inspecting the database

| Method | Returns |
|---|---|
| `db.table_names()` | The names of all tables, sorted |
| `db.row_count("notes")` | The number of rows in a table (`Error::NoSuchTable` if it does not exist) |
| `format!("{db:?}")` | Every table with its row count, handy in test failures |

```rust
println!("{:?}", db.table_names());   // ["notes"]
println!("{}", db.row_count("notes")?); // 1
println!("{db:?}");                    // MemoryDb { tables: [("notes", 1)] }
```

## How it works

The engine does not generate or parse SQL. It interprets Memoire's query structures directly, with algorithms chosen for speed:

| Operation | Strategy |
|---|---|
| `find`, `get`, `update`, `delete` by key | Hash index on the primary key: constant time |
| Filters such as `eq` or `is_in` on the primary key, a `unique` or an `index` column | Hash index lookup instead of a full scan |
| Other filters | A vectorised pass: the condition is split into its `AND` terms and each one filters the rows with a loop specialised for its column and constant; numeric columns are read from a contiguous typed copy (about 1 ns per row). Other conditions (`OR`, `LIKE`, functions...) are compiled once per query |
| Joins on `a.column = b.column` | Hash join (or a direct lookup when the joined column is a key, `unique` or `index`) |
| Other join conditions | Nested loop |
| `order_by` + `limit` | Partial selection of the top rows on packed integer keys, instead of sorting everything |
| `count` | Counts the matching rows without building them |
| `group_by`, `distinct` | Hash maps |
| Transactions | Copy-on-write snapshots |

The [Redis](redis) backend evaluates queries with this same engine, and [MongoDB](mongodb) uses it to finish joins, `GROUP BY` and aggregates, so they share its semantics.

> **Note** `#[memoire(index)]` columns get a hash index here too, as on SQL databases and MongoDB. The indexes store row positions only, so keys are never copied.

## SQL semantics

The engine follows SQL rules, so tests on `MemoryDb` behave like production on the other backends:

- **Three-valued logic:** comparisons with `NULL` are neither true nor false; use `is_null()`.
- **NULL ordering like PostgreSQL:** NULLs come last in ascending order and first in descending order (`nulls_first()`/`nulls_last()` override this).
- **Case-sensitive `LIKE`**, like SQLite (as configured by Memoire) and PostgreSQL; `ilike` and `icontains` ignore case, including accented letters.
- **Aggregates** ignore `NULL` values; `SUM` of integers is an `i64`, `AVG` is an `f64`.
- **Constraints:** primary keys, `unique` columns and `NOT NULL` (non-`Option` fields) are enforced, with the same errors as SQL databases (`Error::UniqueViolation`, `Error::Constraint`).
- **Atomic writes:** a multi-row `update_many`, `insert_many` batch or `delete_many` either fully succeeds or changes nothing.

Differences from SQL databases to keep in mind:

| Feature | In-memory engine |
|---|---|
| Foreign keys (`references`) | Accepted but **not enforced**; `on_delete = "cascade"` does nothing |
| `default = "..."` | Ignored (Memoire's inserts always provide every value anyway) |
| Raw SQL (`raw`, `raw_execute`) | `Error::Unsupported` |
| Migrations | Only `create_table` and `drop_table` steps; `sql` fails, `sql_for` is skipped |
| Self-joins (a table twice in one query) | `Error::Unsupported` |
| Persistence | `MemoryDb::new()`: none, the data disappears with the last clone. `MemoryDb::open(path)`: stored in a file (below) |

## Storing the data in a file

`MemoryDb::open(path)` keeps everything that makes the engine fast (data in memory, indexes, snapshot transactions) and also writes every committed change to a file, which is read back the next time the file is opened:

```rust
let db = MemoryDb::open("app.memoire")?;
db.create_table_if_not_exists::<Note>().await?;
```

- **What is written:** one small record per changed row, appended when the statement or the transaction commits. A transaction that rolls back writes nothing.
- **Crashes:** each record carries a checksum. If the program stops in the middle of a write, the incomplete record is dropped at the next `open`; everything before it is kept.
- **Durability:** by default a write returns once the operating system has the data: it survives a crash of the program, not always a power cut. `MemoryDb::open_with(path, StorageOptions::new().sync(true))` also waits for the disk after every write (much slower: about 0.15 ms per write on the test machine, against 2 to 3 µs).
- **Size:** the file grows with every change. `db.compact()` rewrites it with only the current rows; it also happens on its own when the file holds far more changes than rows. Reopening a file of 20,000 rows takes about 40 ms.
- **Models:** tables come back the first time their model is used. Columns are matched by name: adding an `Option` field keeps the rows (the new column is `NULL`), removing a field drops that column. A new field that is not an `Option` needs a migration of the data.
- **One process at a time:** two programs must not open the same file at once.

### Encryption

With the `encryption` feature, the file can be encrypted with a password:

```rust
use memoire_orm::StorageOptions;

let db = MemoryDb::open_with("secrets.memoire", StorageOptions::new().password(password))?;
```

Each record is encrypted with AES-256-GCM, with a key derived from the password by Argon2id and a random salt kept in the file. Opening the file with another password fails with "wrong password", and nothing in it can be read without the password. Losing the password means losing the data.

## Transactions

Transactions are snapshots. `db.begin()` gives the transaction its own view of the database; tables are copied lazily, only when the transaction first writes to them. Changes become visible to others at `commit`, all at once.

If another writer modified a table that the transaction also wrote, `commit` fails with `Error::Conflict` and nothing is applied, so you can safely retry. Read-only transactions never conflict. See [Transactions](transactions) for a retry loop.

```rust
let tx = db.begin().await?;
Note { id: 0, slug: "draft".into(), text: "...".into() }.insert(&tx).await?;
assert_eq!(Note::count(&tx).await?, 2); // the transaction sees its own insert
assert_eq!(Note::count(&db).await?, 1); // others do not, yet
tx.commit().await?;
assert_eq!(Note::count(&db).await?, 2);
```

## Testing with `MemoryDb`

Write your application code against the `Executor` (or `Database`) trait, then test it with a fresh `MemoryDb` per test: every test gets its own isolated database in microseconds.

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

/// Application code: works with any database.
pub async fn register<E: Executor>(db: &E, email: &str) -> memoire_orm::Result<User> {
    let mut user = User { id: 0, email: email.to_lowercase() };
    user.insert(db).await?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> memoire_orm::Result<MemoryDb> {
        let db = MemoryDb::new();
        db.create_table::<User>().await?;
        Ok(db)
    }

    #[test]
    fn emails_are_unique_ignoring_case() {
        memoire_orm::block_on(async {
            let db = test_db().await?;
            register(&db, "ada@example.com").await?;
            let err = register(&db, "ADA@example.com").await.unwrap_err();
            assert!(err.is_unique_violation());
            Ok::<(), Error>(())
        })
        .unwrap();
    }
}
```

> **Tip** Plain `#[test]` functions with `block_on` are enough for the memory engine; `#[tokio::test]` works too if you prefer.

## WebAssembly

The memory engine compiles to `wasm32-unknown-unknown` without any extra configuration, which makes it possible to run a real query engine in the browser:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

In the browser, drive the futures with `memoire_orm::block_on` (the engine never actually waits, so it does not block) or with `wasm_bindgen_futures::spawn_local`. Only the `memory` feature works on `wasm32`: leave `sqlite` and `postgres` disabled for that target.

> **Note** On `wasm32-unknown-unknown` there is no system clock, so the `applied_at` column written by [migrations](migrations) is `0`.

## Next steps

- [SQLite](sqlite): the same API, with data saved to a file.
- [Transactions](transactions): snapshots, conflicts and retries in detail.
- [Why it's fast](performance): what makes the engine quick.
