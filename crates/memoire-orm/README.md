# Memoire ORM

[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![License: proprietary](https://img.shields.io/badge/license-proprietary-2a2a2e.svg)](#license)

**A blazing-fast, type-safe, async ORM written in pure Rust.**

- **Compile-time SQL** — `#[derive(Model)]` renders every primary-key statement
  (`find`, `insert`, `update`, `delete`, `upsert`) for PostgreSQL, SQLite and MySQL
  while your code compiles. The hot path never builds a SQL string.
- **Type-safe query builder** — `User::AGE.gt(18)` compiles, `User::AGE.gt("18")`
  does not. Combine conditions with `&`, `|` and `!`.
- **Zero-cost row mapping** — rows are decoded by position in one pass and values
  are moved (never cloned) into your structs.
- **SQL and NoSQL, one API** — PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB and
  an in-memory engine written in pure Rust (it even runs in WebAssembly). The same test
  suite passes on all six.
- **Batteries included** — transactions, migrations, joins, aggregates,
  `GROUP BY`, pagination, upserts, normalised errors.
- `#![forbid(unsafe_code)]`.

## Install

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] } # or "postgres", "mysql", "redis", "mongodb", "full"
```

| Feature | Default | What you get |
|---|---|---|
| `memory` | ✓ | `MemoryDb`, a pure-Rust in-memory engine (also on `wasm32`) |
| `sqlite` | | `SqliteDb` (SQLite is compiled in, nothing to install) |
| `postgres` | | `PgPool` (tokio-postgres + connection pool, needs Tokio) |
| `postgres-tls` | | TLS for PostgreSQL with rustls (`sslmode=require`, `verify-full`…) — Heroku Postgres |
| `mysql` / `mysql-tls` | | `MySqlPool` for MySQL 8+ and MariaDB 10.6+ (mysql_async) |
| `redis` / `redis-tls` | | `RedisDb`: NoSQL key-value store, rows as JSON, atomic Lua writes |
| `mongodb` | | `MongoDb`: NoSQL documents, filters run in MongoDB, real transactions |
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
    let db = MemoryDb::new(); // or SqliteDb, PgPool, MySqlPool, RedisDb, MongoDb
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
documentation and the
examples.

## Performance

The in-memory engine filters with loops specialised for each column type, over contiguous copies
of the numeric columns, and its indexes never copy keys (10 000-row table, criterion medians):

| In-memory engine | Before | Now |
|---|---:|---:|
| Equality on an `#[memoire(index)]` column | 140.9 µs | **1.1 µs** (×123) |
| `COUNT(*) WHERE` | 106.2 µs | **11.1 µs** (×9.6) |
| Filter + `ORDER BY` + `LIMIT 20` | 231.2 µs | **36.3 µs** (×6.4) |

Measured against hand-written `rusqlite` code (same driver, cached prepared
statements — the floor any SQLite ORM can reach):

| Operation | Memoire | Raw rusqlite |
|---|---:|---:|
| Find by primary key | 961 ns | 746 ns |
| Select + decode 1 000 rows | 315 µs | 308 µs |
| Insert 1 000 rows in a transaction | **1.95 ms** | 6.64 ms |

Full results and methodology: [`benches/RESULTS.md`](benches/RESULTS.md).

## License

Memoire ORM is proprietary software: © 2026 Maxime Nathan Lestage, all rights
reserved. It is not open source. The source code is published on crates.io
and GitHub so that it can be read, audited and built, and you may use it
freely as a dependency of your own programs, commercial ones included.
Modifying it or redistributing its source needs the author's written
permission. Full terms: [LICENSE](LICENSE).

Contributions (code, documentation, ideas) are welcome and become part of
the project under the same terms: by submitting one, you assign its rights
to the author, as the license states.

## Credits

Designed and developed by Maxime Nathan Lestage,
as part of one family of Rust projects: [Active](https://maxlestage.github.io/Active/), [Vitesse](https://maxlestage.github.io/Vitesse/), [Securite](https://maxlestage.github.io/Securite/), [Memoire](https://maxlestage.github.io/Memoire/), [Paquets](https://maxlestage.github.io/Paquets/) and [FerroSwift](https://maxlestage.github.io/FerroSwift/).
