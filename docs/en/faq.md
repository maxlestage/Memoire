# FAQ

Answers to the questions people ask most often about Memoire ORM. Each answer links to the page with the full details.

## Which databases are supported?

Six backends, all with the same API and passing the same test suite:

| Backend | Feature | Page |
|---|---|---|
| `MemoryDb` | `memory` (default) | [In-memory engine](memory) |
| `SqliteDb` | `sqlite` | [SQLite](sqlite) |
| `PgPool` | `postgres` (`postgres-tls` for TLS) | [PostgreSQL](postgres) |
| `MySqlPool` | `mysql` (`mysql-tls` for TLS) | [MySQL & MariaDB](mysql) |
| `RedisDb` | `redis` (`redis-tls` for TLS) | [Redis](redis) |
| `MongoDb` | `mongodb` | [MongoDB](mongodb) |

## Which async runtime do I need?

The server backends (PostgreSQL, MySQL/MariaDB, Redis and MongoDB) need **Tokio**. The in-memory engine and SQLite never wait for the network, so their futures work with any executor (Tokio, async-std, smol...) or with no runtime at all. See [Installation](installation).

## Can I use Memoire from synchronous code?

Yes, with the in-memory engine and SQLite. Wrap the calls in `memoire_orm::block_on`, which runs a future on the current thread:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "settings")]
pub struct Setting {
    #[memoire(primary_key)]
    pub key: String,
    pub value: String,
}

fn read_setting(db: &SqliteDb, key: &str) -> memoire_orm::Result<Option<Setting>> {
    memoire_orm::block_on(Setting::find(db, key))
}
```

For the server backends, use a Tokio runtime (`#[tokio::main]`, or `tokio::runtime::Runtime::block_on` in synchronous code).

## Why are ids `i64`, and why do I write `id: 0`?

`i64` is the largest integer type that every supported database stores natively (`BIGINT`, SQLite's `INTEGER`), so it never overflows in practice and maps cleanly everywhere. `i32` and `i16` keys work too. Unsigned 64-bit integers are not supported because most databases have no portable unsigned 64-bit type.

For an `auto_increment` key, the database chooses the value: the `0` you write is a placeholder that `insert` ignores, and the generated id is written back into your struct. See [Models](models).

## How do I make a column nullable?

Make the field an `Option`: `pub age: Option<i32>`. `None` is stored as `NULL`, and the column is created without `NOT NULL`. To find rows where it is empty, use `User::AGE.is_null()`; `eq(None)` does not compile on purpose, because in SQL `= NULL` never matches. See [Query builder](queries).

## Is `LIKE` case-sensitive?

Yes, `like`, `contains`, `starts_with` and `ends_with` are case-sensitive on all six backends (Memoire configures SQLite and the MySQL tables it creates that way so every backend agrees). Use `ilike` or `icontains` to ignore case. On SQLite, case-insensitive matching only covers ASCII letters; the other backends also handle accented letters. See [Query builder](queries).

## Why do NULL values appear first on one database and last on another?

Databases disagree: PostgreSQL, Redis and the in-memory engine put NULLs last in ascending order; SQLite, MySQL and MongoDB put them first. Say what you want and Memoire produces it everywhere: `order_by(User::AGE.asc().nulls_last())` or `.nulls_first()`.

## Does Memoire support composite primary keys?

No: a model has exactly one primary-key field. For link tables, add a surrogate `id` key, and if you want the database to reject duplicate pairs, create a unique index over both columns with a SQL [migration](migrations) (or with `db.database()` on MongoDB).

## How do I store an enum?

Two options:

1. **Simple:** store a `String` or `i32` field and convert to and from your enum in your own code.
2. **Typed:** implement the `SqlType`, `ToValue`, `IntoValue` and `FromValue` traits for your enum, then use it directly as a field type. Filters stay type-checked: `Article::STATUS.eq(Status::Published)`.

The second option is shown step by step in [Models](models). On PostgreSQL you can also use a native enum type with a `String` field and `#[memoire(sql_type = "mood")]`, after creating the type in a migration.

## How do I store JSON?

Enable the `json` feature. Use `serde_json::Value` for free-form JSON, or `Json<T>` for any type implementing `serde::Serialize` and `serde::Deserialize`. It is stored as `JSONB` on PostgreSQL, `JSON` on MySQL, text on SQLite, embedded JSON on Redis and embedded documents on MongoDB. See [Models](models).

## Can I use UUIDs as primary keys?

Yes. Enable the `uuid` feature, add `uuid` with its `v4` feature to your dependencies, and generate the key yourself:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "documents")]
pub struct Document {
    #[memoire(primary_key)]
    pub id: uuid::Uuid,
    pub title: String,
}

async fn create(db: &MemoryDb, title: &str) -> memoire_orm::Result<Document> {
    let mut doc = Document { id: uuid::Uuid::new_v4(), title: title.into() };
    doc.insert(db).await?;
    Ok(doc)
}
```

UUIDs are stored as `UUID` on PostgreSQL, `CHAR(36)` on MySQL, text on SQLite and Redis, and BSON binary (subtype 4) on MongoDB.

## Which type should I use for timestamps?

Enable the `chrono` feature. Use `chrono::DateTime<Utc>` for moments in time (stored as `TIMESTAMPTZ` on PostgreSQL), `NaiveDateTime` for a date and time without time zone, `NaiveDate` for dates and `NaiveTime` for times of day. Two details:

- Precision depends on the backend: nanoseconds on the in-memory engine, SQLite and Redis, microseconds on PostgreSQL and MySQL, milliseconds on MongoDB. If you compare a value with what you read back, truncate it first, for example `Utc::now().trunc_subsecs(3)` (from `chrono::SubsecRound`) for code that must work everywhere.
- Comparisons and sorting work on every backend, including SQLite and Redis, where timestamps are stored as fixed-width text.

## Do I need to set up connection pooling?

No. `PgPool` and `MySqlPool` *are* connection pools: create them once and share clones of them (`PgPool::connect_with(url, size)` sets the PostgreSQL pool size). `MongoDb` uses the driver's own pool, `RedisDb` one multiplexed connection that reconnects automatically, `SqliteDb` one shared connection, and `MemoryDb` needs none.

## How do I test code that uses Memoire?

Write your functions against the `Executor` (or `Database`) trait and give them a fresh `MemoryDb` in each test. It takes microseconds to create, needs no cleanup, and enforces the same keys and unique constraints as SQL databases. For behaviour that depends on your production database (foreign key cascades, raw SQL, transactions on MongoDB), add a few integration tests against that database, or `SqliteDb::open_in_memory()` for SQL features. See [In-memory engine](memory).

## Does it work in WebAssembly?

Yes, the in-memory engine compiles to `wasm32-unknown-unknown` with no extra setup, and runs its futures with `memoire_orm::block_on` or `wasm-bindgen-futures`. The other backends are not available there. See [In-memory engine](memory).

## How can I see the SQL that Memoire runs?

Call `to_sql(Dialect::Postgres)` (or `Dialect::Sqlite`, `Dialect::MySql`) on any query, update or delete builder; `to_sql_with_params` also returns the bound values. The statements used by `find`, `insert`, `update`, `delete` and `upsert` are in `User::SCHEMA.sql(dialect)`. Redis and MongoDB do not use SQL; their pages explain what they do instead. See [Query builder](queries).

## Does Memoire create or update my tables automatically?

`db.create_table::<User>()` creates a table from a model, but Memoire never changes an existing table on its own. To evolve a schema, write [migrations](migrations): they are versioned, run in transactions, and are recorded so each one runs only once.

## Does Memoire use `unsafe` code?

No. The crate is compiled with `#![forbid(unsafe_code)]`, so it cannot contain any `unsafe` block. All values are sent to databases as bound parameters, never pasted into SQL text.

## How can I contribute?

Contributions of every size are welcome: bug reports, documentation fixes, translations and new features.

1. Fork `https://github.com/maxlestage/Memoire` and clone it.
2. Run the tests: `cargo test --workspace --features memory,sqlite`. To include the server backends, enable all features and set `MEMOIRE_PG_URL`, `MEMOIRE_MYSQL_URL`, `MEMOIRE_REDIS_URL` and `MEMOIRE_MONGO_URL` to test servers (MongoDB as a replica set).
3. Before opening a pull request, run `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and the tests.
4. Add behaviour tests to `crates/memoire-orm/tests/common/mod.rs`, so they run on every backend.
5. Documentation lives in `docs/en`, `docs/fr` and `docs/es`: update all three, or open an issue so someone can translate.

Pull requests to `master` are merged automatically once CI is green; open yours as a draft or add the `no-auto-merge` label if you want a review first. `CONTRIBUTING.md` in the repository has the details.

## Next steps

- [Quick start](quick-start): build a small app in ten minutes.
- [Cheat sheet](cheatsheet): every operation on one page.
- [Models](models): everything about `#[derive(Model)]`.
