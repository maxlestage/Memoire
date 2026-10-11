<div align="center">

# Memoire ORM

**The blazing-fast, type-safe ORM written in pure Rust.**

[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![License: proprietary](https://img.shields.io/badge/license-proprietary-2a2a2e.svg)](#license)

**English** · [Français](README.fr.md) · [Español](README.es.md)

</div>

---

## Why Memoire?

| | |
|---|---|
| ⚡ **Compile-time SQL** | `#[derive(Model)]` renders every primary-key statement (`find`, `insert`, `update`, `delete`, `upsert`) for PostgreSQL, SQLite and MySQL while your code compiles. |
| 🛡️ **Type-safe queries** | `User::AGE.gt(18)` compiles, `User::AGE.gt("18")` does not. Combine conditions with `&`, `\|` and `!`. |
| 🧬 **Zero-cost row mapping** | Rows are decoded by position in a single pass; values are moved, never cloned. |
| 🗄️ **SQL and NoSQL, one API** | PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB and a pure-Rust in-memory engine (runs in WebAssembly!). |
| 🔋 **Batteries included** | Transactions, migrations, joins, aggregates, `GROUP BY`, pagination, upserts, normalised errors. |
| 🦀 **Safe** | `#![forbid(unsafe_code)]`, the same test suite passes on all six backends. |

## Quick look

```toml
[dependencies]
memoire-orm = { version = "0.1", features = ["sqlite"] }   # or "postgres", "mysql", "redis", "mongodb", "full"
tokio = { version = "1", features = ["full"] }
```

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
    let db = SqliteDb::open("app.db")?;          // or MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?;                    // alice.id is now set

    let adults = User::query()
        .filter(User::AGE.ge(18) & User::NAME.contains("li"))
        .order_by(User::NAME.asc())
        .limit(20)
        .all(&db)
        .await?;
    println!("{adults:#?}");
    Ok(())
}
```

Supported backends (one feature each):

| Backend | Feature | Handle | Notes |
|---|---|---|---|
| In-memory | `memory` (default) | `MemoryDb::new()` | pure Rust, runs in WebAssembly, perfect for tests |
| SQLite | `sqlite` | `SqliteDb::open("app.db")?` | embedded, nothing to install |
| PostgreSQL | `postgres` (+ `postgres-tls`) | `PgPool::connect(url).await?` | Heroku Postgres, Neon, Supabase… |
| MySQL / MariaDB | `mysql` (+ `mysql-tls`) | `MySqlPool::connect(url).await?` | MySQL 8+, MariaDB 10.6+ |
| Redis | `redis` (+ `redis-tls`) | `RedisDb::connect(url).await?` | NoSQL key-value, rows stored as JSON |
| MongoDB | `mongodb` | `MongoDb::connect(url).await?` | NoSQL documents, real transactions on replica sets |

Run the examples:

```bash
cargo run -p memoire-orm --example quickstart                    # in-memory engine
cargo run -p memoire-orm --example sqlite --features sqlite      # migrations + transactions
cargo run -p memoire-orm --example blog                          # joins, eager loading, GROUP BY
DATABASE_URL=postgres://… cargo run -p memoire-orm --example postgres --features postgres
```

## Performance

The in-memory engine (which also evaluates queries for the Redis and MongoDB backends) filters with
loops specialised for each column type, over contiguous copies of the numeric columns, and its
indexes never copy keys. Intel Xeon 2.8 GHz, criterion medians, 10 000-row table:

| In-memory engine | Before | Now |
|---|---:|---:|
| Equality on an `#[memoire(index)]` column | 140.9 µs | **1.1 µs** (×123) |
| `COUNT(*) WHERE` | 106.2 µs | **11.1 µs** (×9.6) |
| Filter + `ORDER BY` + `LIMIT 20` | 231.2 µs | **36.3 µs** (×6.4) |
| `update_many` with a filter | 177.0 µs | **35.8 µs** (×4.9) |
| `insert_many`, 1 000 rows | 422.9 µs | **246.0 µs** (×1.7) |

On SQLite, compared with hand-written `rusqlite` code (same driver, cached prepared statements: the
floor any SQLite ORM can reach):

| Operation | Memoire | Raw rusqlite |
|---|---:|---:|
| Find by primary key | 961 ns | 746 ns |
| Select + decode 1 000 rows | 315 µs | 308 µs |
| Insert one row | 6.26 µs | 5.92 µs |
| Insert 1 000 rows in a transaction | **1.95 ms** | 6.64 ms |

Details: [`crates/memoire-orm/benches/RESULTS.md`](crates/memoire-orm/benches/RESULTS.md) ·
reproduce with `cargo bench -p memoire-orm --features memory,sqlite`.

## Documentation

The full guide lives in [`docs/`](docs/en) (English, [French](docs/fr), [Spanish](docs/es)) and on the website.

| Getting started | Core concepts | Backends | Going further |
|---|---|---|---|
| [Introduction](docs/en/introduction.md) | [Models](docs/en/models.md) | [In-memory](docs/en/memory.md) | [Why it's fast](docs/en/performance.md) |
| [Installation](docs/en/installation.md) | [Query builder](docs/en/queries.md) | [SQLite](docs/en/sqlite.md) | [Benchmarks](docs/en/benchmarks.md) |
| [Quick start](docs/en/quick-start.md) | [CRUD](docs/en/crud.md) | [PostgreSQL](docs/en/postgres.md) | [Deploy from your phone](docs/en/deploy-heroku-mobile.md) |
| | [Joins & relations](docs/en/relations.md) | [MySQL & MariaDB](docs/en/mysql.md) | [Publish to crates.io](docs/en/publish-crates-io.md) |
| | [Transactions](docs/en/transactions.md) · [Migrations](docs/en/migrations.md) · [Raw SQL](docs/en/raw-sql.md) | [Redis](docs/en/redis.md) · [MongoDB (NoSQL)](docs/en/mongodb.md) | [Cheat sheet](docs/en/cheatsheet.md) · [FAQ](docs/en/faq.md) |

API reference: [docs.rs/memoire-orm](https://docs.rs/memoire-orm).

## Repository layout

```text
crates/memoire-orm/          the ORM (published on crates.io)
crates/memoire-orm-derive/   #[derive(Model)] and #[derive(FromRow)]
docs/{en,fr,es}/             documentation (rendered by the website)
site/                        presentation website, written with active (server + WebAssembly islands, PWA)
Dockerfile, heroku.yml, app.json   one-click Heroku deployment
.github/workflows/           CI, auto-merge, Heroku and GitHub Pages deployment, crates.io publishing
```

## The website

The presentation site is written in Rust with [active](https://maxlestage.github.io/Active/):
a small Rust server renders every page (complete SEO tags, in English, French and Spanish),
and only the interactive parts (the playground, the code tabs, the docs search) wake up in the
browser as WebAssembly. The playground runs the real in-memory engine in your browser. The site
is a PWA: install it on your phone's home screen, and once opened it keeps working offline,
playground included. The same server exports the whole site as static files for GitHub Pages.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # the version in site/Cargo.lock
cd site
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm
cargo run                                          # http://localhost:8080
BASE_PATH=/Memoire cargo run -- export dist-static  # static files, as on GitHub Pages
```

## Deploy from your phone 📱

No computer needed:

1. Tap the **Deploy to Heroku** button at the top of this page, pick an app name, tap **Deploy app**.
2. Or add the `HEROKU_API_KEY` and `HEROKU_APP_NAME` secrets in *Settings → Secrets and variables → Actions*:
   every push to `master` is then deployed automatically, and you can redeploy from the GitHub
   app (*Actions → Deploy to Heroku → Run workflow*).
3. Or publish the website **for free on GitHub Pages**: *Settings → Pages → Source: GitHub Actions*, then
   *Actions → Deploy to GitHub Pages → Run workflow*. It goes online at `https://<your-user>.github.io/Memoire/`.

Step-by-step guide: [docs/en/deploy-heroku-mobile.md](docs/en/deploy-heroku-mobile.md).

## Automation

- **CI** (`ci.yml`): formatting, clippy, the same test suite on **all six backends** (memory, SQLite,
  PostgreSQL, MySQL/MariaDB, Redis, MongoDB), every feature on its own, WebAssembly build, website build.
- **Auto-merge** (`ci.yml`): pull requests to `master` are merged automatically as soon as CI is green
  (and the Heroku and GitHub Pages deployments are triggered).
- **Deploy** (`deploy-heroku.yml`): every push to `master` deploys the website to Heroku.
- **GitHub Pages** (`pages.yml`): every push to `master` that changes the website or the docs publishes it on GitHub Pages.
- **Publish** (`publish.yml`): pushing a `v*` tag (or publishing a GitHub release) publishes the crates
  to crates.io — see [Publish to crates.io](docs/en/publish-crates-io.md).

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md).

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
