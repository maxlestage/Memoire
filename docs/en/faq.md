# FAQ

Answers to the questions people ask most often about Memoire ORM. Each answer links to the page with the full details.

## Which databases are supported?

Three backends run queries today: the in-memory engine `MemoryDb` (default feature `memory`), SQLite with `SqliteDb` (feature `sqlite`) and PostgreSQL with `PgPool` (feature `postgres`, or `postgres-tls` for encrypted connections). They share one API, so the same models and queries work on all three.

MySQL/MariaDB, Redis and MongoDB backends: see their pages, [MySQL & MariaDB](mysql), [Redis](redis) and [MongoDB](mongodb).

## Which async runtime do I need?

Only PostgreSQL needs one, and it must be **Tokio**. The in-memory engine and SQLite never wait for the network, so their futures work with any executor (Tokio, async-std, smol...) or with no runtime at all. See [Installation](installation).

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

For PostgreSQL, use a Tokio runtime (`#[tokio::main]`, or `tokio::runtime::Runtime::block_on` in synchronous code).

## Why are ids `i64`, and why do I write `id: 0`?

`i64` is the largest integer type that every supported database stores natively (`BIGINT`, SQLite's `INTEGER`), so it never overflows in practice and maps cleanly everywhere. `i32` and `i16` keys work too. Unsigned 64-bit integers are not supported because SQL databases have no portable unsigned 64-bit type.

For an `auto_increment` key, the database chooses the value: the `0` you write is a placeholder that `insert` ignores, and the generated id is written back into your struct. See [Models](models).

## How do I make a column nullable?

Make the field an `Option`: `pub age: Option<i32>`. `None` is stored as `NULL`, and the column is created without `NOT NULL`. To find rows where it is empty, use `User::AGE.is_null()`; `eq(None)` does not compile on purpose, because in SQL `= NULL` never matches. See [Query builder](queries).

## Is `LIKE` case-sensitive?

Yes, `like`, `contains`, `starts_with` and `ends_with` are case-sensitive on the in-memory engine, SQLite and PostgreSQL (Memoire configures SQLite that way so every backend agrees). Use `ilike` or `icontains` to ignore case. On SQLite, case-insensitive matching only covers ASCII letters. See [Query builder](queries).

## Why do NULL values appear first on one database and last on another?

Databases disagree: PostgreSQL and the in-memory engine put NULLs last in ascending order, SQLite puts them first. Say what you want and Memoire produces it everywhere: `order_by(User::AGE.asc().nulls_last())` or `.nulls_first()`.

## Does Memoire support composite primary keys?

No: a model has exactly one primary-key field. For link tables, add a surrogate `id` key, and if you want the database to reject duplicate pairs, create a unique index over both columns with a SQL [migration](migrations).

## How do I store an enum?

Two options:

1. **Simple:** store a `String` or `i32` field and convert to and from your enum in your own code.
2. **Typed:** implement the `SqlType`, `ToValue`, `IntoValue` and `FromValue` traits for your enum, then use it directly as a field type. Filters stay type-checked: `Article::STATUS.eq(Status::Published)`.

The second option is shown step by step in [Models](models). On PostgreSQL you can also use a native enum type with a `String` field and `#[memoire(sql_type = "mood")]`, after creating the type in a migration.

## How do I store JSON?

Enable the `json` feature. Use `serde_json::Value` for free-form JSON, or `Json<T>` for any type implementing `serde::Serialize` and `serde::Deserialize`. It is stored as `JSONB` on PostgreSQL and as text on SQLite. See [Models](models).

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

UUIDs are stored as `UUID` on PostgreSQL and as text on SQLite.

## Which type should I use for timestamps?

Enable the `chrono` feature. Use `chrono::DateTime<Utc>` for moments in time (stored as `TIMESTAMPTZ` on PostgreSQL), `NaiveDateTime` for a date and time without time zone, `NaiveDate` for dates and `NaiveTime` for times of day. Two details:

- PostgreSQL keeps microseconds: compare with `Utc::now().trunc_subsecs(6)` if you check a value after reading it back.
- SQLite stores timestamps as text; for columns you filter or sort on, `NaiveDateTime` (holding UTC) sorts correctly as text. See [SQLite](sqlite).

## Do I need to set up connection pooling?

No. `PgPool` *is* a connection pool: create it once with `PgPool::connect` (up to 4 × CPU cores connections) or `PgPool::connect_with(url, size)`, and share clones of it. `SqliteDb` uses one shared connection, and `MemoryDb` needs none. See [PostgreSQL](postgres).

## How do I test code that uses Memoire?

Write your functions against the `Executor` (or `Database`) trait and give them a fresh `MemoryDb` in each test. It takes microseconds to create, needs no cleanup, and enforces the same keys and unique constraints as SQL databases. For behaviour that depends on SQL features (foreign key cascades, raw SQL), add a few tests with `SqliteDb::open_in_memory()`. See [In-memory engine](memory).

## Does it work in WebAssembly?

Yes, the in-memory engine compiles to `wasm32-unknown-unknown` with no extra setup, and runs its futures with `memoire_orm::block_on` or `wasm-bindgen-futures`. The SQLite and PostgreSQL backends are not available there. See [In-memory engine](memory).

## How can I see the SQL that Memoire runs?

Call `to_sql(Dialect::Postgres)` (or `Dialect::Sqlite`) on any query, update or delete builder; `to_sql_with_params` also returns the bound values. The statements used by `find`, `insert`, `update`, `delete` and `upsert` are in `User::SCHEMA.sql(dialect)`. See [Query builder](queries).

## Does Memoire create or update my tables automatically?

`db.create_table::<User>()` creates a table from a model, but Memoire never changes an existing table on its own. To evolve a schema, write [migrations](migrations): they are versioned, run in transactions, and are recorded so each one runs only once.

## Does Memoire use `unsafe` code?

No. The crate is compiled with `#![forbid(unsafe_code)]`, so it cannot contain any `unsafe` block. All values are sent to databases as bound parameters, never pasted into SQL text.

## How can I contribute?

Contributions of every size are welcome: bug reports, documentation fixes, translations and new features.

1. Fork `https://github.com/maxlestage/Memoire` and clone it.
2. Run the tests: `cargo test --workspace --features memory,sqlite` (set `MEMOIRE_PG_URL` to a PostgreSQL URL to include PostgreSQL).
3. Before opening a pull request, run `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and the tests.
4. Add behaviour tests to `crates/memoire-orm/tests/common/mod.rs`, so they run on every backend.
5. Documentation lives in `docs/en`, `docs/fr` and `docs/es`: update all three, or open an issue so someone can translate.

Pull requests to `master` are merged automatically once CI is green; open yours as a draft or add the `no-auto-merge` label if you want a review first. `CONTRIBUTING.md` in the repository has the details.

## Next steps

- [Quick start](quick-start): build a small app in ten minutes.
- [Cheat sheet](cheatsheet): every operation on one page.
- [Models](models): everything about `#[derive(Model)]`.
