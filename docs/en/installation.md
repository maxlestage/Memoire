# Installation

This page shows how to add Memoire ORM to a Rust project, which Cargo features to enable for your database, and which async runtime you need.

## Add the crate

The crate is called `memoire-orm` on crates.io, and you import it in Rust as `memoire_orm`.

The quickest way is `cargo add`. Pick the line that matches the database you want to use:

```bash
# In-memory engine only (the default feature)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire

# SQLite
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features sqlite

# PostgreSQL (needs the Tokio runtime)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread

# PostgreSQL over TLS (hosted databases such as Heroku Postgres)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls

# MySQL / MariaDB (add "mysql-tls" for TLS)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mysql

# Redis (add "redis-tls" for rediss:// URLs)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features redis

# MongoDB
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mongodb
```

Every server backend (PostgreSQL, MySQL/MariaDB, Redis, MongoDB) also needs Tokio: `cargo add tokio --features macros,rt-multi-thread`.

You can also edit `Cargo.toml` by hand:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

> **Note** Tokio is only *required* for the server backends. The examples in this documentation use `#[tokio::main]` because it is the most common setup, but the memory and SQLite backends work without it (see "Async runtime" below).

## Feature flags

| Feature | Enabled by default | What it adds |
|---|---|---|
| `memory` | yes | `MemoryDb`, the pure-Rust in-memory engine (also works on `wasm32`) |
| `sqlite` | no | `SqliteDb`, an embedded SQLite database (SQLite is compiled in) |
| `postgres` | no | `PgPool`, a PostgreSQL connection pool (requires Tokio) |
| `postgres-tls` | no | Encrypted PostgreSQL connections with `rustls` (`sslmode=require`, `verify-full`...), needed by Heroku Postgres and most hosted databases. Implies `postgres` |
| `mysql` | no | `MySqlPool`, a MySQL and MariaDB connection pool (requires Tokio) |
| `mysql-tls` | no | Encrypted MySQL connections with `rustls` (`?require_ssl=true`). Implies `mysql` |
| `redis` | no | `RedisDb`, tables stored in Redis (requires Tokio; includes `memory`) |
| `redis-tls` | no | `rediss://` URLs with `rustls`; append `#insecure` for self-signed certificates. Implies `redis` |
| `mongodb` | no | `MongoDb`, collections in MongoDB through the official driver (requires Tokio; includes `memory`) |
| `encryption` | no | Encrypted `MemoryDb` files (AES-256-GCM, key derived from a password with Argon2id). Implies `memory` |
| `web` | no | Browser storage: `Store`, a Pinia-style state store, and `LocalStorageDb`, a database in `localStorage` (wasm32; `MemoryStorage` everywhere). Implies `memory` |
| `chrono` | no | Date and time columns: `NaiveDateTime`, `DateTime<Utc>`, `NaiveDate`, `NaiveTime` |
| `uuid` | no | `uuid::Uuid` columns |
| `json` | no | JSON columns: `serde_json::Value` and `Json<T>` for any `serde` type |
| `full` | no | All of the above |

Each backend is described on its own page: [In-memory engine](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis) and [MongoDB](mongodb).

Features add up, so you can combine them freely:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["postgres", "chrono", "uuid", "json"] }
```

If you do not need the in-memory engine at all, turn off the default features:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", default-features = false, features = ["sqlite"] }
```

> **Tip** Keep `memory` enabled even if production uses another database: it makes unit tests fast and setup-free. You can enable it only for tests with `[dev-dependencies]`.

### Extra crates for column types

Memoire does not re-export `chrono`, `uuid` or `serde_json`. To *name* those types in your structs, add the crates to your own `Cargo.toml` as well:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite", "chrono", "uuid", "json"] }
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }         # "v4" for Uuid::new_v4()
serde = { version = "1", features = ["derive"] }    # for Json<T>
serde_json = "1"
```

## Async runtime

Every database call in Memoire is an `async fn`, so you `.await` it. Which runtime you need depends on the backend:

| Backend | Runtime needed |
|---|---|
| `MemoryDb` | Any executor (Tokio, async-std, smol...) or none at all with `memoire_orm::block_on` |
| `SqliteDb` | Any executor or none at all with `memoire_orm::block_on` |
| `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb` | Tokio (their network I/O and connection pools run on Tokio) |

The memory and SQLite backends never wait for the network: their futures finish as soon as they are polled. That is why Memoire ships a tiny helper, `memoire_orm::block_on`, that runs a future on the current thread. It is handy in synchronous programs, command-line tools, tests and WebAssembly:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
struct Note {
    id: i64,
    text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;
        Note { id: 0, text: "hello".into() }.insert(&db).await?;
        println!("{} note(s)", Note::count(&db).await?);
        Ok(())
    })
}
```

> **Warning** Do not use `block_on` with `PgPool`, `MySqlPool`, `RedisDb` or `MongoDb`: they need a running Tokio runtime. Use `#[tokio::main]` (or `#[tokio::test]` in tests).

## WebAssembly

The `memory` backend compiles to `wasm32-unknown-unknown` with no extra setup:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

In the browser you can drive the futures with `memoire_orm::block_on` or with `wasm-bindgen-futures`. The other backends (SQLite, PostgreSQL, MySQL, Redis and MongoDB) are not available on `wasm32`. See [In-memory engine](memory) for details.

## Check that everything works

Create a new project and run a tiny program:

```bash
cargo new hello-memoire
cd hello-memoire
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread
```

Replace `src/main.rs` with:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "greetings")]
struct Greeting {
    id: i64,
    text: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Greeting>().await?;

    let mut hello = Greeting { id: 0, text: "Hello, Memoire!".into() };
    hello.insert(&db).await?;

    let found = Greeting::get(&db, hello.id).await?;
    println!("#{} says: {}", found.id, found.text);
    Ok(())
}
```

Then `cargo run` should print `#1 says: Hello, Memoire!`.

## Requirements

| Requirement | Value |
|---|---|
| Minimum Rust version (MSRV) | 1.88 |
| Build-time tools | A C compiler when the `sqlite` feature is enabled (SQLite is compiled from source) |
| Run-time dependencies | None for memory and SQLite; a reachable server for PostgreSQL, MySQL/MariaDB, Redis or MongoDB |
| License | Proprietary (© Maxime Nathan Lestage, all rights reserved): free to use as a dependency, see [LICENSE](https://maxlestage.github.io/Memoire/LICENSE.txt) |

## Next steps

- [Quick start](quick-start): build a small app from scratch.
- [Models](models): describe your tables.
- Backend setup: [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).
