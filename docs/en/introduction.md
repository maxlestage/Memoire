# Introduction

Memoire ORM is a fast, type-safe, asynchronous ORM written in pure Rust. You describe your tables as ordinary Rust structs, and Memoire gives you a typed API to create, query, update and delete rows, with the same code on six databases: an in-memory engine, SQLite, PostgreSQL, MySQL/MariaDB, Redis and MongoDB.

## What is an ORM?

An ORM (Object-Relational Mapper) is a library that translates between the rows of a database table and the values of your programming language. Instead of writing SQL strings by hand and copying every column into a struct yourself, you write:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?; // alice.id is now 1

    let adults = User::query()
        .filter(User::AGE.ge(18))
        .order_by(User::NAME.asc())
        .all(&db)
        .await?;
    println!("{adults:?}");
    Ok(())
}
```

Memoire turns `User::query().filter(User::AGE.ge(18))` into the right SQL for your database (or a native MongoDB filter), binds the value `18` safely as a parameter, runs the statement and turns each returned row back into a `User`.

## Why Memoire?

- **Type-safe queries.** Every field gets a typed column constant (`User::AGE`, `User::EMAIL`, ...). `User::AGE.gt(18)` compiles; `User::AGE.gt("18")` is a compile error. Typos in column names are compile errors too.
- **One API, six databases.** The same model and the same queries run on the in-memory engine, SQLite, PostgreSQL, MySQL/MariaDB, Redis and MongoDB, and they all pass the same test suite. Switching databases is a one-line change.
- **Fast by design.** The SQL for primary-key operations is generated while your code compiles, rows are decoded in a single pass without cloning, and statements are prepared once and cached. See [Why it's fast](performance).
- **Batteries included.** Joins, aggregates, `GROUP BY`, pagination, upserts, bulk inserts, transactions, migrations and raw SQL.
- **Safe.** The crate is `#![forbid(unsafe_code)]`, and values are always sent as bound parameters, never pasted into SQL text.
- **Runs everywhere.** The in-memory engine is pure Rust, needs no runtime and compiles to WebAssembly (`wasm32-unknown-unknown`).

## Feature overview

| Area | What you get |
|---|---|
| Models | `#[derive(Model)]` with primary keys, auto-increment, `unique`, indexes, defaults, foreign keys, renamed and skipped fields |
| Queries | Typed filters combined with `&`, `\|`, `!`; `LIKE` helpers with automatic escaping; `IN`, `BETWEEN`, `IS NULL` |
| Results | `all`, `first`, `one`, `count`, `exists`, `paginate`, `sum`, `avg`, `min`, `max`, typed column projections |
| Writes | `insert`, `insert_many` (multi-row batching), `update`, `upsert`, `delete`, `update_many` with `increment`/`decrement`, `delete_many` |
| Relations | `inner_join` and `left_join` returning tuples like `(User, Post)` and `(User, Option<Post>)` |
| Reports | `group_by`, `having`, `count_all()`, aggregates decoded into tuples or `#[derive(FromRow)]` structs |
| Safety nets | Transactions (rolled back automatically when dropped), versioned migrations, normalised errors |
| Escape hatches | Raw SQL with `params![...]` on SQL databases, direct access to every driver (SQLite connection, PostgreSQL and MySQL pools, Redis connection, MongoDB database) |

## Backends

| Backend | Type | Cargo feature | Async runtime | Raw SQL |
|---|---|---|---|---|
| `MemoryDb` | In-memory engine, pure Rust | `memory` (default) | Any, or none (`block_on`) | No |
| `SqliteDb` | Embedded SQLite database (file or memory) | `sqlite` | Any, or none (`block_on`) | Yes |
| `PgPool` | PostgreSQL client with a connection pool | `postgres` (`postgres-tls` for TLS) | Tokio | Yes |
| `MySqlPool` | MySQL and MariaDB client with a connection pool | `mysql` (`mysql-tls` for TLS) | Tokio | Yes |
| `RedisDb` | Tables stored in Redis, queries evaluated by the in-memory engine | `redis` (`redis-tls` for TLS) | Tokio | No |
| `MongoDb` | One collection per table, queries translated to MongoDB filters | `mongodb` | Tokio | No |

Each backend has its own page: [In-memory engine](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis) and [MongoDB](mongodb).

## Which backend should I use?

| You want to... | Use | Why |
|---|---|---|
| Learn Memoire, prototype, write fast unit tests | `MemoryDb` | Zero setup, no files, extremely fast, nothing to clean up |
| Run in the browser (WebAssembly) | `MemoryDb` | The only backend that compiles to `wasm32` |
| Ship a desktop app, CLI tool or small server with data on disk | `SqliteDb` | One file, no server to install, very fast for a single machine |
| Run a web service with several instances or many concurrent writers | `PgPool` | A real database server with a connection pool, network access and strong concurrency |
| Use an existing MySQL or MariaDB database, or a host that offers MySQL | `MySqlPool` | A full SQL server with a connection pool, offered by almost every hosting provider |
| Store sessions, caches or small shared datasets, accessed mostly by key | `RedisDb` | Very fast key lookups, data shared between processes, typed and atomic |
| Work with an existing MongoDB database or MongoDB Atlas | `MongoDb` | Documents you can also use from other MongoDB tools, filters run on the server |

> **Tip** Because every backend shares the same API, a common setup is `SqliteDb`, `PgPool` or another server backend in production and `MemoryDb` in unit tests. Write your functions against the `Executor` or `Database` traits and pass whichever database you like.

## Requirements and license

- **Rust 1.88 or newer** (the minimum supported Rust version, or MSRV).
- **License:** proprietary, © Maxime Nathan Lestage, all rights reserved. Free to use as a dependency of your own programs, commercial ones included; the source is published so that it can be audited and built. See [LICENSE](https://github.com/maxlestage/Memoire/blob/master/LICENSE).
- The `sqlite` feature compiles SQLite from source, so it needs a C compiler at build time. Every other backend is pure Rust; TLS uses `rustls`, so OpenSSL is never needed.

## Next steps

- [Installation](installation): add Memoire to your project and pick your features.
- [Quick start](quick-start): build a small app step by step in about ten minutes.
- [Models](models): learn everything `#[derive(Model)]` can do.
