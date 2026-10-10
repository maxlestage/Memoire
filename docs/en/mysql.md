# MySQL & MariaDB

`MySqlPool` connects Memoire to MySQL or MariaDB through a pool of reusable connections. Your models and queries work exactly as on the other backends, and Memoire sets up the tables so that comparisons, sorting and `LIKE` behave the same way too.

## Enable the backend

```bash
cargo add memoire-orm --features mysql
cargo add tokio --features macros,rt-multi-thread
```

For encrypted connections (most hosted databases), use the `mysql-tls` feature instead (it includes `mysql`):

```bash
cargo add memoire-orm --features mysql-tls
```

The client is `mysql_async`, a pure Rust implementation of the MySQL protocol; TLS uses `rustls`. No C library or OpenSSL is needed.

| Requirement | Value |
|---|---|
| Async runtime | **Tokio** (`#[tokio::main]`, `#[tokio::test]`) |
| MySQL | 8.0 and newer (tested on 8.4) |
| MariaDB | 10.6 and newer (tested on 10.11 and 11) |

> **Tip** To try it locally, start a server with Docker: `docker run -d -p 3306:3306 -e MARIADB_ROOT_PASSWORD=root -e MARIADB_DATABASE=app -e MARIADB_USER=app -e MARIADB_PASSWORD=secret mariadb:11` (or the `mysql:8.4` image, with `MYSQL_` instead of `MARIADB_` in the variable names).

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
    let db = MySqlPool::connect("mysql://app:secret@localhost:3306/app").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);

    db.close().await // optional: closes the connections gracefully
}
```

| Method | What it does |
|---|---|
| `MySqlPool::connect(url)` | Connects with a `mysql://user:password@host:port/database` URL. One connection is opened right away, so a wrong address or password fails immediately |
| `MySqlPool::connect_with(builder)` | Connects with a `mysql_async::OptsBuilder`, for options the URL cannot express (TLS settings, socket options...) |
| `db.pool()` | The underlying `mysql_async::Pool` (escape hatch) |
| `db.close().await` | Closes every connection of the pool (for a clean shutdown) |

`MySqlPool` is cheap to clone: clones share the same pool. Connections are opened on demand and reused. By default the pool keeps between 10 and 100 connections (the `mysql_async` defaults); set other bounds with the `pool_min` and `pool_max` URL parameters, for example `mysql://app:secret@localhost:3306/app?pool_min=1&pool_max=5`, or with `OptsBuilder::pool_opts`. `pool_min` must not exceed `pool_max`, so a `pool_max` below 10 needs a `pool_min` too; on a small hosted plan with a low connection limit, set both.

## Encrypted connections (TLS)

With the `mysql-tls` feature, add TLS parameters to the URL:

| URL parameter | Effect |
|---|---|
| `require_ssl=true` | Encrypt the connection and verify the server certificate and host name |
| `verify_ca=false` | Accept a certificate that is not signed by a trusted authority (self-signed) |
| `verify_identity=false` | Do not check that the certificate matches the host name |

```rust
async fn connect_securely() -> memoire_orm::Result<MySqlPool> {
    MySqlPool::connect("mysql://app:secret@db.example.com:3306/app?require_ssl=true").await
}
```

For full control, build the options yourself with `mysql_async` (0.37) in your dependencies:

```rust
use mysql_async::{Opts, OptsBuilder, SslOpts};

async fn connect_with_options(url: &str) -> Result<MySqlPool, Box<dyn std::error::Error>> {
    let builder = OptsBuilder::from_opts(Opts::from_url(url)?).ssl_opts(SslOpts::default());
    Ok(MySqlPool::connect_with(builder).await?)
}
```

## What Memoire configures for you

| Setting | Why |
|---|---|
| Tables created with `DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin` | Full Unicode (including emoji), and case-sensitive, byte-ordered comparisons, like the other backends |
| `CLIENT_FOUND_ROWS` | `update` counts rows that *matched*, even if no value changed, so saving an unchanged row is not reported as `Error::NotFound` |
| 256 prepared statements cached per connection | Each distinct statement is parsed once per connection |

Because of the binary collation, `User::NAME.eq("alice")` does not match `"Alice"`, and `like`, `contains`, `starts_with` and `ends_with` are case-sensitive, exactly as on PostgreSQL. Use `ilike` or `icontains` to ignore case (they use `LOWER()`, which handles accented letters).

> **Note** This applies to tables created by Memoire (`create_table`, migrations). Tables you created yourself keep their own collation, which is often case-insensitive in MySQL.

## Types

| Rust type | MySQL type |
|---|---|
| `bool` | `BOOLEAN` (stored as `TINYINT(1)`: 0 or 1) |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INT` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `FLOAT` |
| `f64` | `DOUBLE` |
| `String` | `TEXT`, or `VARCHAR(255)` for primary keys, `unique`, `index` and foreign key columns |
| `Vec<u8>` | `BLOB`, or `VARBINARY(255)` for keyed columns |
| `uuid::Uuid` | `CHAR(36)` |
| `chrono::NaiveDateTime` | `DATETIME(6)` |
| `chrono::DateTime<Utc>` | `DATETIME(6)`, holding the UTC time |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME(6)` |
| `serde_json::Value`, `Json<T>` | `JSON` |
| `auto_increment` key | `BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY` (for `i64`) |

Things to know:

- **Keyed text columns hold at most 255 characters**, because MySQL cannot index unbounded `TEXT`. Use `#[memoire(sql_type = "VARCHAR(500)")]` for longer unique values.
- **Timestamps keep microseconds.** If you compare a value with what you read back, truncate it first: `Utc::now().trunc_subsecs(6)` (from `chrono::SubsecRound`).
- **`DECIMAL` values** (from your own tables or raw SQL) are read as exact text: decode them into `String`, `f64`, or `i64` for whole numbers.
- **Generated ids** come from `AUTO_INCREMENT` and are read back with `LAST_INSERT_ID()`.

## Transactions

`db.begin()` takes a connection from the pool for the transaction. Other queries keep running on other connections and only see committed data. The isolation level is the server's default (`REPEATABLE READ` for InnoDB).

- A dropped transaction is rolled back when its connection returns to the pool.
- Deadlocks and lock wait timeouts are reported as `Error::Conflict`: retry the whole transaction, as shown in [Transactions](transactions).

> **Warning** MySQL and MariaDB commit automatically before and after every schema change (`CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`...). A schema change inside a transaction cannot be rolled back, and it also commits the changes made before it. Keep schema changes out of your application transactions.

This matters for [migrations](migrations): if a migration fails after creating a table, that table stays. Keep each migration to a single schema change, so a failure is easy to clean up by hand before running the migration again.

## Upsert

`upsert` matches rows on the primary key only, as on every other backend. If the row you upsert has a new primary key but a `unique` value (an email, say) that already belongs to another row, it fails with `Error::UniqueViolation` and the other row is left untouched.

MySQL's own `INSERT ... ON DUPLICATE KEY UPDATE` fires when **any** unique key collides, so it would update that other row instead. Memoire therefore uses it only for tables without other `unique` columns, where it is a single atomic statement. For the others, `upsert` runs an `INSERT` and, when that insert collides on the primary key, an `UPDATE` by key. Both steps are safe when several programs upsert at the same time, and inside a transaction too: a failed statement does not abort a MySQL transaction.

Upserting rows with explicit ids into an `auto_increment` table is safe on MySQL: the counter moves past the highest id.

## Errors

| MySQL error | Memoire error |
|---|---|
| Duplicate entry (1062) | `Error::UniqueViolation { table, column }` (`column` is the key name, which Memoire names after the column) |
| Table doesn't exist (1146), unknown table (1051) | `Error::NoSuchTable(name)` |
| Table already exists (1050) | `Error::TableExists(name)` |
| `NOT NULL`, foreign key or `CHECK` violation | `Error::Constraint(message)` |
| Deadlock (1213), lock wait timeout (1205) | `Error::Conflict(table)`: retry the transaction |
| Cannot connect | `Error::Pool(message)` or `Error::Database(...)` |
| Anything else | `Error::Database(...)` with the original error |

## Things to know

- **NULL ordering:** like SQLite, MySQL puts NULLs first in ascending order. `nulls_first()`/`nulls_last()` are emulated with an `IS NULL` sort key, so they work everywhere.
- **Foreign keys** (`references`, `on_delete = "cascade"`) are enforced (InnoDB).
- **Raw SQL** uses `?` placeholders; migration scripts may contain several statements. See [Raw SQL](raw-sql).
- **Direct access:** `db.pool()` returns the `mysql_async::Pool`:

```rust
use mysql_async::prelude::Queryable;

async fn server_version(db: &MySqlPool) -> Result<Option<String>, mysql_async::Error> {
    let mut conn = db.pool().get_conn().await?;
    conn.query_first("SELECT VERSION()").await
}
```

## Generating MySQL SQL for another driver

Memoire can also just *write* MySQL SQL for you, without `MySqlPool`: for example to inspect a query, to prepare a schema file, or to run statements with another MySQL client library. `Dialect::MySql` is always available, whatever features you enable.

```rust
fn mysql_sql() {
    // A query, with its parameters in placeholder order.
    let query = Product::query().filter(Product::PRICE_CENTS.lt(1_000)).order_by(Product::NAME.asc()).limit(20);
    let (sql, params) = query.to_sql_with_params(Dialect::MySql);
    println!("{sql}\n{params:?}");

    // The statements used by find / insert / update / delete / upsert.
    println!("{}", Product::SCHEMA.sql(Dialect::MySql).insert);

    // CREATE TABLE.
    for statement in memoire_orm::sql::create_table(Product::SCHEMA, Dialect::MySql, false) {
        println!("{statement};");
    }
}
```

```text
SELECT `products`.`id`, `products`.`sku`, `products`.`name`, `products`.`price_cents` FROM `products` WHERE `products`.`price_cents` < ? ORDER BY `products`.`name` ASC LIMIT 20
[I64(1000)]
INSERT INTO `products` (`sku`, `name`, `price_cents`) VALUES (?, ?, ?)
CREATE TABLE `products` (
    `id` BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    `sku` VARCHAR(255) NOT NULL UNIQUE,
    `name` TEXT NOT NULL,
    `price_cents` BIGINT NOT NULL
) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
```

`memoire_orm::sql::render` returns any statement with its parameters (for example a model's `INSERT`), and rows returned by another driver can be decoded into models by putting their values in a `memoire_orm::ValueRow` and calling `FromRow::from_row`.

## Next steps

- [Transactions](transactions): retries and per-backend behaviour.
- [Migrations](migrations): schema changes on MySQL.
- [Raw SQL](raw-sql): MySQL-specific queries.
