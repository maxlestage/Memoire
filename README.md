<div align="center">

[![Memoire ORM: the blazing-fast, pure-Rust ORM](https://raw.githubusercontent.com/maxlestage/Memoire/master/site/static/og-image.jpg)](https://memoire-7f4525f1fdaf.herokuapp.com/)

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
| 🔗 **Relations without N+1** | `users.load_children::<Post>(&db)`, `load_parent`, many-to-many `load_via`: one query per level, read from the foreign keys you already declare. |
| 🔎 **Search and indexes everywhere** | Full-text search (`Post::BODY.matches("rust orm")`) and unique indexes over several columns (`#[memoire(unique(user_id, team_id))]`), with the same behaviour on all six backends. |
| ⏱️ **Model lifecycle, Eloquent-style** | `#[memoire(timestamps, soft_deletes, hooks)]`: `created_at`/`updated_at` filled for you, `delete` that only hides the row (`with_trashed`, `restore`), hooks that can reject a write; `save`, `first_or_create`, `update_or_create`, `chunks`, `cursor_paginate` and a query log (`Logged`). |
| 📣 **Change notifications** | `Observed::new(db).subscribe(...)`: every write, on any backend, announced when it commits. |
| 🌐 **Browser storage** | `Store::<Cart>::new("cart")`, a Pinia-style store saved in `localStorage`, and `LocalStorageDb::open("app")`, a whole database in `localStorage`, in step across tabs (feature `web`). |
| 💾 **Embedded and durable** | `MemoryDb::open("app.memoire")`: the in-memory engine stored in a crash-safe file, optionally encrypted (AES-256-GCM). |
| 🔋 **Batteries included** | Transactions, migrations, joins, aggregates, `GROUP BY`, pagination, upserts, partial updates (`update_only`), `find_many`, normalised errors. |
| 🦀 **Safe** | `#![forbid(unsafe_code)]`, the same test suite passes on all six backends. |

## Quick look

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] }   # or "postgres", "mysql", "redis", "mongodb", "full"
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
| In-memory | `memory` (default) | `MemoryDb::new()` / `MemoryDb::open(path)` | pure Rust, runs in WebAssembly, perfect for tests; can be stored in a file (`encryption` to encrypt it) |
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

On PostgreSQL (local server), the relation helpers replace round trips:

| Operation | Before | Now |
|---|---:|---:|
| 100 users and their posts (N+1 queries → `load_children`) | 13.5 ms | **1.14 ms** (×12) |
| `paginate` (count and page, sequential → concurrent) | 342 µs | **176 µs** (×1.9) |

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
| | [Transactions](docs/en/transactions.md) · [Change notifications](docs/en/events.md) · [Migrations](docs/en/migrations.md) · [Raw SQL](docs/en/raw-sql.md) | [Redis](docs/en/redis.md) · [MongoDB (NoSQL)](docs/en/mongodb.md) · [Browser storage](docs/en/browser-storage.md) | [Cheat sheet](docs/en/cheatsheet.md) · [FAQ](docs/en/faq.md) · [vs Diesel, SeaORM & Nitrite](docs/en/comparison.md) |

API reference: [docs.rs/memoire-orm](https://docs.rs/memoire-orm).

## Repository layout

```text
crates/memoire-orm/          the ORM (published on crates.io)
crates/memoire-orm-derive/   #[derive(Model)] and #[derive(FromRow)]
docs/{en,fr,es}/             documentation (rendered by the website)
site/                        presentation website, written with active (server + WebAssembly islands, PWA)
RustConfig, Procfile, app.json     one-click Heroku deployment (emk/rust buildpack, heroku-build.sh)
Dockerfile, heroku.yml             Docker image (Heroku container stack, any Docker host)
.github/workflows/           CI, auto-merge, Heroku deployment, crates.io publishing
```

## The website

The presentation site is written in Rust with [Active](https://active-c1883a966650.herokuapp.com/):
a small Rust server renders every page (complete SEO tags, in English, French and Spanish),
and only the interactive parts (the playground, the code tabs, the docs search) wake up in the
browser as WebAssembly. The playground runs the real in-memory engine in your browser. The site
is a PWA: install it on your phone's home screen, and once opened it keeps working offline,
playground included. It is served by Heroku only, at <https://memoire-7f4525f1fdaf.herokuapp.com/>; the same server can also
export the whole site as static files, which the CI checks.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # the version in site/Cargo.lock
cd site
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm
cargo run                                          # http://localhost:8080
BASE_PATH=/Memoire cargo run -- export dist-static  # the whole site as static files
```

## Deploy from your phone 📱

No computer needed:

1. Open `https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire` on your phone, pick an app name, tap **Deploy app**.
2. Or add the `HEROKU_API_KEY` and `HEROKU_APP_NAME` secrets in *Settings → Secrets and variables → Actions*:
   every push to `master` is then deployed automatically, and you can redeploy from the GitHub
   app (*Actions → Deploy to Heroku → Run workflow*). Heroku builds the site with the
   [emk/rust buildpack](https://elements.heroku.com/buildpacks/emk/heroku-buildpack-rust); set the repository
   variable `HEROKU_BUILD` to `docker` to build the Docker image on GitHub instead.

From a computer: `heroku create --stack heroku-26 --buildpack emk/rust`, then `git push heroku master`.

Step-by-step guide: [docs/en/deploy-heroku-mobile.md](docs/en/deploy-heroku-mobile.md).

## Automation

- **CI** (`ci.yml`): formatting, clippy, the same test suite on **all six backends** (memory, SQLite,
  PostgreSQL, MySQL/MariaDB, Redis, MongoDB), every feature on its own, WebAssembly build, website build.
- **Auto-merge** (`ci.yml`): pull requests to `master` are merged automatically as soon as CI is green
  (and the Heroku deployment is triggered).
- **Deploy** (`deploy-heroku.yml`): every push to `master` deploys the website to Heroku, built there with the
  emk/rust buildpack (or as a Docker image on GitHub, with the repository variable `HEROKU_BUILD=docker`).
- **Publish** (`publish.yml`): once CI is green on `master`, a new version of the root `Cargo.toml` is tagged,
  released on GitHub (notes from `CHANGELOG.md`) and its crates published to crates.io; a `v*` tag or a
  GitHub release created by hand publishes too — see [Publish to crates.io](docs/en/publish-crates-io.md).

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
as part of one family of Rust projects: [Active](https://active-c1883a966650.herokuapp.com/), [Vitesse](https://vitesse-f16135654c3d.herokuapp.com/), [Securite](https://securite-2db4c1994be7.herokuapp.com/), [Memoire](https://memoire-7f4525f1fdaf.herokuapp.com/) and [Paquets](https://paquets-90dfc06b6fea.herokuapp.com/).
