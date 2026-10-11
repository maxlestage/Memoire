# PostgreSQL

`PgPool` connects Memoire to a PostgreSQL server through a pool of reusable connections. It is the backend of choice for web services: several application instances can share one database, many requests can run at the same time, and hosted offers (Heroku Postgres, Amazon RDS, Supabase, Neon...) are easy to find.

## Enable the backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread
```

For hosted databases, which require encrypted connections, use the `postgres-tls` feature instead (it includes `postgres`):

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls
```

The client is pure Rust: `tokio-postgres` for the protocol, `deadpool` for pooling and, with `postgres-tls`, `rustls` for encryption. No C library or OpenSSL is needed.

> **Note** `PgPool` needs the **Tokio** runtime: use `#[tokio::main]` in your program and `#[tokio::test]` in tests. `memoire_orm::block_on` does not work with PostgreSQL.

## Connecting

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "products")]
pub struct Product {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = PgPool::connect("postgres://app:secret@localhost:5432/shop").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);
    Ok(())
}
```

The connection string can be a URL, `postgres://user:password@host:port/database?option=value`, or the `key=value` format: `host=localhost user=app password=secret dbname=shop`.

| Constructor | Pool size | Notes |
|---|---|---|
| `PgPool::connect(url)` | Up to 4 × the number of CPU cores | Opens one connection right away, so a wrong URL or password fails immediately |
| `PgPool::connect_with(url, max_size)` | Up to `max_size` | Same, with your own limit |
| `PgPool::connect_with_rustls(url, max_size, config)` | Up to `max_size` | TLS with your own `rustls` configuration (feature `postgres-tls`) |
| `PgPool::from_pool(pool)` | Whatever you configured | Wraps a `deadpool_postgres::Pool` you built yourself |

`PgPool` is cheap to clone: clones share the same pool. Create it once at startup and pass clones (or references) around.

## Choosing the pool size

Each connection is a process on the database server, so more is not always better:

- The default (4 × CPU cores of the application machine) suits a single application instance talking to its own server.
- Hosted plans limit the number of connections, sometimes to only a few dozen. Divide that limit between your application instances and keep a few connections for maintenance tools: `PgPool::connect_with(&url, 10)`.
- When every connection is busy, new queries wait until one is free.

Connections are opened on demand, up to the maximum, and reused.

## Encrypted connections (TLS)

With the `postgres-tls` feature, `connect` and `connect_with` follow the `sslmode` parameter of the connection string, like the official `psql` client:

| `sslmode` | Behaviour |
|---|---|
| `disable` | Plain text, never encrypted |
| `prefer` (default) | Use TLS if the server supports it, otherwise plain text |
| `require` | Always encrypted; the server certificate is **not** verified |
| `verify-ca`, `verify-full` | Always encrypted, and the server certificate is verified against the Mozilla root certificates |

```rust
async fn connect_securely() -> memoire_orm::Result<PgPool> {
    PgPool::connect("postgres://app:secret@db.example.com/shop?sslmode=verify-full").await
}
```

Without the `postgres-tls` feature, connections are plain text, and a connection string with `sslmode=require`, `verify-ca` or `verify-full` fails with `Error::Unsupported`, telling you to enable the feature.

> **Tip** Use `verify-full` when your provider's certificate is signed by a public authority: it protects against impostor servers. Many hosted databases (Heroku Postgres among them) use certificates that public authorities did not sign; use `require` for those, which still encrypts all traffic.

### Custom certificates

If your server uses a certificate signed by your own authority, build a `rustls::ClientConfig` that trusts it and pass it to `connect_with_rustls`. You need `rustls` (0.23) in your dependencies:

```toml
[dependencies]
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
```

```rust
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

async fn connect_with_my_ca(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from_pem_file("my-ca.pem")?)?;

    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(PgPool::connect_with_rustls(url, 10, config).await?)
}
```

Client certificates work the same way, with `with_client_auth_cert` instead of `with_no_client_auth`.

### Heroku Postgres

Heroku puts the connection string of your database in the `DATABASE_URL` environment variable, and Heroku Postgres only accepts encrypted connections. With the `postgres-tls` feature, connecting is one line:

```rust
async fn heroku() -> Result<PgPool, Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")?;
    // `prefer` (the default) already uses TLS; `sslmode=require` makes it mandatory.
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    Ok(PgPool::connect_with(&url, 10).await?)
}
```

See [Deploy to Heroku from your phone](deploy-heroku-mobile) for a complete deployment.

### Building your own pool

For full control over the pool (timeouts, recycling, a different TLS stack), build a `deadpool_postgres::Pool` and wrap it with `from_pool`. This needs `deadpool-postgres` (0.14) and `tokio-postgres` (0.7) in your dependencies:

```rust
use std::time::Duration;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};

fn custom_pool(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager = Manager::from_config(
        config,
        tokio_postgres::NoTls,
        ManagerConfig { recycling_method: RecyclingMethod::Fast },
    );
    let pool = Pool::builder(manager)
        .max_size(8)
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(Duration::from_secs(5))) // fail instead of waiting forever
        .build()?;
    Ok(PgPool::from_pool(pool))
}
```

Unlike `connect`, `from_pool` does not open a connection right away: errors appear on the first query.

## Prepared statements

Every statement Memoire runs is prepared once per connection and kept in a cache, so PostgreSQL parses and plans it only once. Primary-key operations reuse SQL generated at compile time, and builder queries with the same shape produce the same SQL text, so they share a cache entry. To protect long-running processes, a connection's cache is emptied when it exceeds 1,024 statements.

## Types

### How columns are created

| Rust type | PostgreSQL type |
|---|---|
| `bool` | `BOOLEAN` |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INTEGER` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `REAL` |
| `f64` | `DOUBLE PRECISION` |
| `String` | `TEXT` |
| `Vec<u8>` | `BYTEA` |
| `uuid::Uuid` | `UUID` |
| `chrono::NaiveDateTime` | `TIMESTAMP` |
| `chrono::DateTime<Utc>` | `TIMESTAMPTZ` |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME` |
| `serde_json::Value`, `Json<T>` | `JSONB` |
| `auto_increment` key | `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` (for `i64`) |

### How values are read

| PostgreSQL type | Read as |
|---|---|
| `BOOLEAN` | `bool` |
| `SMALLINT`, `INTEGER`, `BIGINT` | Any integer type the value fits in |
| `REAL`, `DOUBLE PRECISION` | `f32`, `f64` |
| `NUMERIC` | `String` (exact) or `f64` |
| `TEXT`, `VARCHAR`, `CHAR`, `NAME`, `citext`, enum types | `String` |
| `BYTEA` | `Vec<u8>` |
| `UUID` | `uuid::Uuid` with the `uuid` feature, otherwise `String` |
| `TIMESTAMP`, `TIMESTAMPTZ`, `DATE`, `TIME` | `chrono` types (requires the `chrono` feature) |
| `JSON`, `JSONB` | `serde_json::Value` or `Json<T>` with the `json` feature, otherwise `String` |

`NUMERIC` (exact decimals, common for money) has no native Rust type, so Memoire reads it as text without any rounding: decode it into a `String` to keep every digit, or into an `f64` for calculations. Writing a `String` or `f64` into a `NUMERIC` column works too. Other types (`INTERVAL`, arrays, geometric types...) are not supported directly: cast them to text in [raw SQL](raw-sql), for example `duration::text`.

> **Tip** `TIMESTAMPTZ` stores microseconds while `chrono` keeps nanoseconds. If you compare a value with what you read back, truncate it first: `Utc::now().trunc_subsecs(6)` (from `chrono::SubsecRound`).

## Errors

PostgreSQL error codes are translated into Memoire's common variants:

| PostgreSQL error | Memoire error |
|---|---|
| `unique_violation` (23505) | `Error::UniqueViolation { table, column }` |
| `not_null_violation`, `foreign_key_violation`, `check_violation` | `Error::Constraint(message)` |
| `undefined_table` (42P01) | `Error::NoSuchTable(name)` |
| `duplicate_table` (42P07) | `Error::TableExists(name)` |
| `serialization_failure` (40001), `deadlock_detected` (40P01) | `Error::Conflict(table)`: retry the transaction |
| Pool cannot connect (wrong host, password, TLS handshake...) | `Error::Pool(message)` |
| Anything else | `Error::Database(...)` with the original error |

## Things to know

- **Transactions** hold one pooled connection until they finish; a dropped transaction is rolled back in the background. See [Transactions](transactions).
- **`LIKE` is case-sensitive** and `ilike`/`icontains` use PostgreSQL's native `ILIKE`.
- **NULL ordering:** NULLs come last in ascending order and first in descending order.
- **Raw SQL** uses `$1`, `$2`, ... placeholders. See [Raw SQL](raw-sql).
- **Upserting explicit ids** into an `auto_increment` table does not advance PostgreSQL's identity sequence, so a later `insert` can collide with them. Use `insert` for new rows of auto-increment models.
- **Direct access:** `db.pool()` returns the `deadpool_postgres::Pool`, for monitoring (`db.pool().status()`) or `tokio-postgres` features such as `LISTEN`/`NOTIFY`.

## Next steps

- [Deploy to Heroku from your phone](deploy-heroku-mobile): run a Memoire app with Heroku Postgres.
- [Transactions](transactions): isolation levels and retries on PostgreSQL.
- [Raw SQL](raw-sql): PostgreSQL-specific queries.
