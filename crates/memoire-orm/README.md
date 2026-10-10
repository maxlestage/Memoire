# Memoire ORM

[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![license](https://img.shields.io/crates/l/memoire-orm.svg)](#license)

**A blazing-fast, type-safe, async ORM written in pure Rust.**

- **Compile-time SQL** — `#[derive(Model)]` renders every primary-key statement
  (`find`, `insert`, `update`, `delete`, `upsert`) for PostgreSQL, SQLite and MySQL
  while your code compiles. The hot path never builds a SQL string.
- **Type-safe query builder** — `User::AGE.gt(18)` compiles, `User::AGE.gt("18")`
  does not. Combine conditions with `&`, `|` and `!`.
- **Zero-cost row mapping** — rows are decoded by position in one pass and values
  are moved (never cloned) into your structs.
- **One API, several backends** — an in-memory engine written in pure Rust (it even
  runs in WebAssembly), SQLite and PostgreSQL.
- **Batteries included** — transactions, migrations, joins, aggregates,
  `GROUP BY`, pagination, upserts, normalised errors.
- `#![forbid(unsafe_code)]`.

## Install

```toml
[dependencies]
memoire-orm = { version = "0.1", features = ["sqlite"] } # or "postgres", "full"
```

| Feature | Default | What you get |
|---|---|---|
| `memory` | ✓ | `MemoryDb`, a pure-Rust in-memory engine (also on `wasm32`) |
| `sqlite` | | `SqliteDb` (SQLite is compiled in, nothing to install) |
| `postgres` | | `PgPool` (tokio-postgres + connection pool, needs Tokio) |
| `chrono` / `uuid` / `json` | | date-time, UUID and JSON column types |
| `full` | | everything |

## Example

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
    let db = MemoryDb::new(); // or SqliteDb::open("app.db")?, PgPool::connect(url).await?
    db.create_table::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?; // alice.id is now set

    let adults = User::query()
        .filter(User::AGE.ge(18) & User::NAME.contains("li"))
        .order_by(User::NAME.asc())
        .limit(20)
        .all(&db)
        .await?;

    let tx = db.begin().await?;
    alice.name = "Alice Cooper".into();
    alice.update(&tx).await?;
    tx.commit().await?;

    println!("{adults:?}");
    Ok(())
}
```

More: joins (`inner_join`, `left_join`), `group_by` + aggregates, `paginate`,
`insert_many`, `update_many().increment(..)`, `Migrator`, raw SQL. See the
[documentation](https://github.com/maxlestage/Memoire/tree/master/docs/en) and the
[examples](https://github.com/maxlestage/Memoire/tree/master/crates/memoire-orm/examples).

## Performance

Measured against hand-written `rusqlite` code (same driver, cached prepared
statements — the floor any SQLite ORM can reach):

| Operation | Memoire | Raw rusqlite |
|---|---:|---:|
| Find by primary key | 987 ns | 840 ns |
| Select + decode 1 000 rows | 343 µs | 324 µs |
| Insert 1 000 rows in a transaction | **1.80 ms** | 7.98 ms |

Full results and methodology: [`benches/RESULTS.md`](benches/RESULTS.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
