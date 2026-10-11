# SQLite

`SqliteDb` stores your data in a single file using SQLite, the most widely deployed database in the world. There is no server to install or run: SQLite is compiled into your program. It is a great fit for desktop apps, command-line tools, small and medium web services, and tests that need a real SQL database.

## Enable the backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features sqlite
```

The `sqlite` feature compiles SQLite from source (through the `rusqlite` crate), so you need a C compiler at build time (GCC, Clang or MSVC, usually already installed). Nothing has to be installed on the machines that run your program.

## Opening a database

| Constructor | Use it for |
|---|---|
| `SqliteDb::open("app.db")` | A database file, created if it does not exist |
| `SqliteDb::open_in_memory()` | A private, temporary database that disappears when dropped (tests) |
| `SqliteDb::from_connection(conn)` | An existing `rusqlite::Connection` you configured yourself |

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "bookmarks")]
pub struct Bookmark {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub url: String,
    pub title: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("bookmarks.db")?;
    db.create_table_if_not_exists::<Bookmark>().await?;

    let mut rust = Bookmark { id: 0, url: "https://www.rust-lang.org".into(), title: "Rust".into() };
    rust.insert(&db).await?;
    println!("{} bookmark(s) saved", Bookmark::count(&db).await?);
    Ok(())
}
```

Opening is synchronous (no `.await`) and returns a `memoire_orm::Result`.

`from_connection` is for special setups, such as opening a read-only file or using an encryption extension. It needs `rusqlite` in your own dependencies, with the same major version as Memoire (`rusqlite = "0.40"`):

```rust
fn read_only(path: &str) -> memoire_orm::Result<SqliteDb> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(Error::database)?;
    SqliteDb::from_connection(conn)
}
```

## What Memoire configures for you

| Setting | `open` (file) | `open_in_memory` | `from_connection` | Why |
|---|---|---|---|---|
| `journal_mode = WAL` | yes | | | Readers do not block the writer and vice versa; faster writes |
| `synchronous = NORMAL` | yes | | | Safe with WAL and much faster than the default |
| `busy_timeout = 5000` | yes | | | Wait up to 5 seconds for a lock instead of failing immediately |
| `foreign_keys = ON` | yes | yes | yes | Enforce `references` constraints (SQLite disables them by default) |
| `case_sensitive_like = ON` | yes | yes | yes | Make `LIKE` behave as on PostgreSQL and the memory engine |
| Prepared statement cache | 256 | 256 | 256 | Each distinct SQL statement is compiled once and reused |

> **Note** With WAL, SQLite keeps two helper files next to your database while it is open (`app.db-wal` and `app.db-shm`). They are normal; copy all three files if you back up a database that is in use, or use the SQLite backup API through `with_connection`.

## Connections and concurrency

A `SqliteDb` holds **one** connection. Cloning a `SqliteDb` is cheap and shares that connection, so you can hand clones to every task or thread. Statements run one at a time on it, which is how SQLite performs best for writes.

If you have many concurrent readers on a file database, you can open several `SqliteDb` on the same file: thanks to WAL, they read in parallel while one of them writes.

## Async behaviour

SQLite runs inside your process and never waits for the network, so `SqliteDb` futures complete as soon as they run. They work with any async runtime, or without one through `memoire_orm::block_on`:

```rust
fn count_bookmarks() -> memoire_orm::Result<u64> {
    let db = SqliteDb::open("bookmarks.db")?;
    memoire_orm::block_on(Bookmark::count(&db))
}
```

Inside Tokio, a query runs directly on the worker thread that awaits it. Most queries take microseconds, so this is fine. For an occasional heavy query (a big report, a `VACUUM`), move it to a blocking thread so other tasks keep running:

```rust
async fn heavy_report(db: &SqliteDb) -> memoire_orm::Result<u64> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || memoire_orm::block_on(Bookmark::count(&db)))
        .await
        .expect("the report task panicked")
}
```

## How values are stored

SQLite has only a few storage types, so Memoire converts richer Rust types:

| Rust type | Stored as | Example |
|---|---|---|
| `bool` | `INTEGER` | `0` or `1` |
| Integers (`i8` to `i64`, `u8` to `u32`) | `INTEGER` | `42` |
| `f32`, `f64` | `REAL` | `3.5` |
| `String` | `TEXT` | `'hello'` |
| `Vec<u8>` | `BLOB` | raw bytes |
| `uuid::Uuid` | `TEXT` | `'67e55044-10b1-426f-9247-bb680e5fe0c8'` |
| `chrono::NaiveDateTime` | `TEXT` | `'2026-10-10 14:30:00.250000000'` |
| `chrono::DateTime<Utc>` | `TEXT` (RFC 3339) | `'2026-10-10T14:30:00.250000000Z'` |
| `chrono::NaiveDate` | `TEXT` | `'2026-10-10'` |
| `chrono::NaiveTime` | `TEXT` | `'14:30:00.000000000'` |
| `serde_json::Value`, `Json<T>` | `TEXT` | `'{"theme":"dark"}'` |

Reading converts back automatically, so your structs always get proper Rust types.

> **Note** Dates and times are stored as fixed-width ISO 8601 text (nanosecond precision, `Z` suffix for `DateTime<Utc>`), so text order matches time order: `gt`, `between` and `order_by` work on every date/time column.

### Auto-increment keys

An `auto_increment` key becomes `INTEGER PRIMARY KEY`, SQLite's built-in row id. New rows get the highest existing id plus one, so the id of the most recently inserted row can be reused if that row is deleted. If ids must never be reused, keep deleted rows (for example with an `active` flag) instead of deleting them.

## Transactions

`db.begin()` takes the connection for the transaction until it is committed, rolled back or dropped. Transactions therefore run one at a time and never conflict.

> **Warning** While a transaction is open, a query issued on the `SqliteDb` handle itself (`&db` instead of `&tx`) waits until the transaction ends. In the same task, that is a deadlock. Inside a transaction, always pass `&tx`.

Transactions are also the key to fast bulk writes on SQLite. Outside a transaction, every write is a transaction of its own, with its own locking and journal work; inside one, that cost is paid once, at commit:

```rust
async fn import(db: &SqliteDb, bookmarks: &[Bookmark]) -> memoire_orm::Result<u64> {
    let tx = db.begin().await?;
    let n = Bookmark::insert_many(&tx, bookmarks).await?;
    tx.commit().await?;
    Ok(n)
}
```

## Errors

SQLite errors are translated into Memoire's common error variants:

| SQLite situation | Memoire error |
|---|---|
| `UNIQUE constraint failed: users.email` (or a duplicate primary key) | `Error::UniqueViolation { table: "users", column: "email" }` |
| `NOT NULL`, `FOREIGN KEY` or `CHECK` constraint failed | `Error::Constraint(message)` |
| `no such table: users` | `Error::NoSuchTable("users")` |
| `table users already exists` | `Error::TableExists("users")` |
| Anything else | `Error::Database(...)` with the original `rusqlite` error |

## Things to know

- **`LIKE` is case-sensitive** (Memoire turns on `case_sensitive_like`). `ilike` and `icontains` ignore case for ASCII letters only: `icontains("é")` does not match `"É"`.
- **NULL ordering:** SQLite puts NULLs first in ascending order, like MySQL and MongoDB but unlike PostgreSQL, Redis and the memory engine. Use `nulls_first()`/`nulls_last()` for portable results.
- **Raw SQL** uses `?` placeholders. See [Raw SQL](raw-sql).
- **Direct access:** `db.with_connection(|conn| ...)` gives you the `rusqlite::Connection` for anything Memoire does not cover (backups, pragmas, custom functions). See [Raw SQL](raw-sql).

## Next steps

- [PostgreSQL](postgres): when you need a database server.
- [Transactions](transactions): how SQLite transactions compare with the other backends.
- [Why it's fast](performance): tips for bulk writes.
