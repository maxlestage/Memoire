# Redis

`RedisDb` stores your models in Redis, the in-memory data store. You use the same models, queries, transactions and migrations as with any other backend: Memoire keeps each table in a few Redis keys, writes every change atomically, and evaluates queries with its in-memory engine. It is a good fit for caches, sessions, shared application state and small-to-medium datasets.

## When to use it

| Great for | Think twice for |
|---|---|
| Sessions, tokens, carts, feature flags, caches | Large tables queried on arbitrary columns (each such query reads the whole table) |
| Data shared by several processes or servers | Data that must survive the loss of the Redis server, unless Redis persistence is configured |
| Small-to-medium datasets that must be fast to read by key | Redis Cluster (see "Limitations") |
| Apps that already run Redis and want typed access | Reporting and analytics |

## Enable the backend

```bash
cargo add memoire-orm --features redis
cargo add tokio --features macros,rt-multi-thread
```

For `rediss://` (TLS) URLs, use the `redis-tls` feature instead (it includes `redis`). The client is the `redis` crate; TLS uses `rustls`. `RedisDb` needs the **Tokio** runtime.

> **Tip** To try it locally: `docker run -d -p 6379:6379 redis:7`.

## Connecting

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "sessions")]
pub struct Session {
    #[memoire(primary_key)]
    pub token: String,
    pub user_id: i64,
    pub expires_at: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = RedisDb::connect("redis://127.0.0.1:6379").await?;
    db.create_table_if_not_exists::<Session>().await?;

    let mut session = Session { token: "4f9c2a".into(), user_id: 42, expires_at: 1_790_000_000 };
    session.insert(&db).await?;

    let found = Session::get(&db, "4f9c2a").await?;
    println!("token belongs to user {}", found.user_id);
    Ok(())
}
```

| Method | What it does |
|---|---|
| `RedisDb::connect(url)` | Connects and stores every key under the prefix `memoire` |
| `RedisDb::connect_with_prefix(url, "myapp")` | Same, with your own prefix, so several applications (or test runs) can share one Redis server |
| `db.connection()` | The underlying `redis::aio::ConnectionManager` (escape hatch) |

URLs look like `redis://host:6379`, `redis://:password@host:6379/2` (database 2) or `rediss://...` for TLS. The handle keeps one multiplexed connection that reconnects automatically; cloning it is cheap.

### TLS and Heroku Key-Value Store

With the `redis-tls` feature, `rediss://` URLs are encrypted and the server certificate is verified. Some hosted services, Heroku Key-Value Store among them, use self-signed certificates: append `#insecure` to the URL to keep the encryption but skip the certificate check.

```rust
async fn connect_on_heroku() -> Result<RedisDb, Box<dyn std::error::Error>> {
    // Heroku sets REDIS_URL (a rediss:// URL) when you add Heroku Key-Value Store.
    let url = std::env::var("REDIS_URL")?;
    let url = if url.starts_with("rediss://") && !url.contains('#') { format!("{url}#insecure") } else { url };
    Ok(RedisDb::connect_with_prefix(&url, "myapp").await?)
}
```

## How your data is stored

Each table uses a handful of keys. Their names share the `{table}` part, a Redis *hash tag*:

| Key | Redis type | Content |
|---|---|---|
| `memoire:tables` | set | The names of the existing tables |
| `memoire:{users}:rows` | hash | Primary key → the row as a JSON object |
| `memoire:{users}:u:email` | hash | One per `unique` column: value → primary key |
| `memoire:{users}:seq` | string | The last generated id (`auto_increment`) |
| `memoire:{users}:ver` | string | A version number, increased by every write |
| `memoire:{users}:meta` | hash | The column list and the table's keys |

Rows are readable JSON, so you can inspect them with `redis-cli` and other programs can read them:

```text
> HGET memoire:{users}:rows 1
{"id":1,"email":"alice@example.com","name":"Alice","age":31,"joined":"2026-10-10T14:30:00.123456789Z"}
```

Values are stored as JSON numbers, strings and booleans. `NULL` is `null`; bytes are hex strings; UUIDs and dates are text (timestamps keep nanosecond precision); JSON columns are embedded as they are.

## How operations run

| Operation | What happens in Redis |
|---|---|
| `insert`, `upsert`, `update`, `delete` | One atomic Lua script for the row and its `unique` indexes |
| `insert_many` | One atomic Lua script for the whole batch |
| `find`, `get`, `reload` | One `HGET` |
| `filter` on the primary key with `eq` or `is_in` | Reads only those rows (`HGET`/`HMGET`) |
| `count()` without filter | `HLEN` |
| Every other query (filters on other columns, sorting, joins, aggregates...) | Reads the whole table(s) involved, then evaluates the query with Memoire's [in-memory engine](memory) |
| `update_many`, `delete_many` | Reads the table, computes the changes with the in-memory engine, then writes them only if the table did not change meanwhile |

Every write is a single Lua script that first checks its conditions (the table exists, `unique` values are free, the row or table has not changed) and only then writes. A write is therefore all or nothing, even when it touches many rows and indexes.

Writes that depend on data read just before (`update`, `delete`, `update_many`...) are optimistic: if another client changed the data in between, Memoire retries the statement automatically, up to 64 times, before giving up with `Error::Conflict`.

> **Tip** Lookups by primary key are the fast path. Design keys you look up by, such as a session token or a username, as the primary key rather than a `unique` column: a filter on a `unique` column reads the whole table.

## Query semantics

Because queries are evaluated by the in-memory engine, Redis behaves exactly like [`MemoryDb`](memory):

- comparisons follow SQL's three-valued logic (`NULL` never matches a comparison);
- NULLs come last in ascending order and first in descending order;
- `like`, `contains`... are case-sensitive; `ilike` and `icontains` ignore case, including accented letters;
- primary keys, `unique` columns and `NOT NULL` (non-`Option` fields) are enforced, with the usual errors (`Error::UniqueViolation`, `Error::Constraint`);
- foreign keys are not enforced and `on_delete = "cascade"` does nothing;
- raw SQL (`raw`, `raw_execute`) returns `Error::Unsupported`, and so do self-joins.

## Transactions

`db.begin()` starts a transaction that reads each table once, the first time it uses it, and then works on that private snapshot. At `commit`, every change is written by one atomic Lua script. If another client modified one of the tables the transaction wrote, the commit fails with `Error::Conflict` and nothing is written: retry the whole transaction (see [Transactions](transactions)).

- Conflicts are detected per table: two transactions writing different rows of the same table still conflict.
- A transaction that only reads never conflicts.
- Dropping a transaction discards it; nothing was written.
- Creating or dropping tables inside a transaction takes effect at commit, so a failed [migration](migrations) leaves nothing behind.

## Migrations

`create_table` and `drop_table` steps work, and each migration is applied atomically. `sql` steps fail with `Error::Unsupported` (there is no SQL), and `sql_for` steps are skipped.

## Limitations

- **Redis Cluster is not supported**, because one write can touch keys of several tables, which a cluster may store on different nodes. A standalone server, Redis Sentinel and single-shard managed services work.
- **Everything lives in memory.** Configure Redis persistence (RDB snapshots or AOF) if the data must survive a restart.
- **Big tables with non-key queries** transfer the whole table to your application for each query. Keep such tables small, or query them by primary key.
- **No expiry per row.** Rows do not expire on their own; delete old rows with `delete_many`, or use the raw connection for expiring keys of your own.

## Direct access

`db.connection()` returns the `redis` connection manager, for anything Memoire does not cover. Add `redis` (1.x, with the `tokio-comp` feature) to your dependencies to send commands:

```rust
async fn raw_row(db: &RedisDb, id: i64) -> redis::RedisResult<Option<String>> {
    let mut conn = db.connection();
    redis::cmd("HGET").arg("memoire:{users}:rows").arg(id).query_async(&mut conn).await
}
```

> **Warning** Do not modify Memoire's keys directly: the `unique` indexes, versions and ids must stay consistent with the rows. Read them freely, and write through Memoire.

## Next steps

- [In-memory engine](memory): the query engine Redis relies on.
- [Transactions](transactions): conflicts and retries.
- [MongoDB](mongodb): another NoSQL backend, with queries run by the server.
