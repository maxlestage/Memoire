# Why it's fast

Memoire is designed so that the ORM layer costs as little as possible on top of the database itself. This page explains where the speed comes from, then gives practical tips to keep your own application fast.

## Where the speed comes from

### SQL generated at compile time

For the operations you run most often (`find`, `get`, `insert`, `update`, `delete`, `upsert`, `reload`), `#[derive(Model)]` writes the SQL while your code compiles, for PostgreSQL, SQLite and MySQL. At run time those statements are plain string constants: no SQL is built, formatted or allocated. You can see them in `User::SCHEMA.sql(Dialect::Postgres)` (see [Models](models)).

Queries built with the query builder are rendered at run time into a single pre-sized buffer, in one pass over a small syntax tree. Rendering a typical `SELECT` with a filter, a sort and a limit takes about half a microsecond.

### No copies of your parameters

Each field is converted once into a `Value`. From then on, the SQL renderer and the drivers only *borrow* those values: they are not copied again on the way to the database. On SQLite, text and byte values are handed to SQLite by reference.

### Rows decoded by position, values moved

When rows come back, Memoire decodes them **by position** in a single pass: column 0 goes into the first field, column 1 into the second, and so on. There is no lookup by column name and no intermediate map. Each value is *moved* into your struct rather than cloned.

### Prepared statements, cached

Every statement is prepared once per connection and reused:

| Backend | Statement cache |
|---|---|
| SQLite | Up to 256 statements per connection |
| PostgreSQL | Every statement per connection, reset after 1,024 distinct statements |
| MySQL / MariaDB | Up to 256 statements per connection |

Because the builder produces the same SQL text for queries of the same shape (values are always parameters), a query in a loop is parsed and planned only once.

### Bulk inserts in few statements

`insert_many` turns a slice of rows into multi-row `INSERT ... VALUES (...), (...), ...` statements. Each statement carries as many rows as the database allows: the number of bound parameters stays under the limit (32,766 on SQLite, 65,535 on PostgreSQL and MySQL), with at most 10,000 rows per statement. On Redis a batch is one atomic script, and on MongoDB one `insertMany` call. Inserting 1,000 rows in a transaction this way is about 4 times faster than a loop of single-row inserts with hand-written `rusqlite` code (see [Benchmarks](benchmarks)).

### A purpose-built in-memory engine

`MemoryDb` does not parse SQL at all: it executes the query structures directly.

- Hash indexes on the primary key and `unique` columns: lookups by key take constant time.
- Filters are compiled once per query (column positions resolved, long `IN` lists turned into hash sets, `LIKE` patterns pre-analysed) before scanning.
- Joins on column equality run as hash joins.
- `order_by` + `limit` selects the top rows without sorting everything.
- Transactions use copy-on-write snapshots: starting one is cheap, and a table is copied only when the transaction first writes to it.

### Redis: key lookups first

`RedisDb` reads only the rows it needs when you look up by primary key (`find`, or a filter with `eq`/`is_in` on the key) and counts with `HLEN`. Every write, even a batch of thousands of rows, is applied by one atomic Lua script. Other queries read the whole table and run on the in-memory engine. See [Redis](redis).

### MongoDB: the server does the filtering

`MongoDb` translates filters, sorting, `limit` and `offset` into one native `find`, counts with `countDocuments`, and runs `update_many`/`delete_many` as single server-side commands. Joins fetch only the documents whose keys match. See [MongoDB](mongodb).

### Lean by construction

The builders are thin typed wrappers: the column types and the result type exist only at compile time and cost nothing at run time. There is no reflection and no runtime schema discovery.

## Tips for fast applications

### Insert in bulk

```rust
// Slow: one statement (and one round trip) per row.
for user in &new_users {
    let mut user = user.clone();
    user.insert(&db).await?;
}

// Fast: a few multi-row statements.
User::insert_many(&db, &new_users).await?;
```

Use `insert` when you need each generated id back; use `insert_many` for everything else.

### Group writes in a transaction

On SQLite especially, every write outside a transaction is a transaction of its own. Wrapping many writes in one transaction is often an order of magnitude faster:

```rust
let tx = db.begin().await?;
User::insert_many(&tx, &new_users).await?;
User::update_many().set(User::ACTIVE, true).filter(User::AGE.ge(18)).execute(&tx).await?;
tx.commit().await?;
```

### Update and delete in the database

Do not load rows only to change them one by one. Let the database do it in one statement:

```rust
// One statement, however many rows match.
User::update_many().increment(User::VISITS, 1).filter(User::ACTIVE.eq(true)).execute(&db).await?;
User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
```

### Select only what you need

Loading whole rows means transferring and allocating every column. When you only need a few, use `columns`:

```rust
let emails: Vec<String> = User::query().columns(User::EMAIL).filter(User::ACTIVE.eq(true)).all(&db).await?;
```

### Ask precise questions

| Instead of | Use | Why |
|---|---|---|
| `query.all(&db).await?.len()` | `query.count(&db)` | Counts in the database, transfers one number |
| `query.count(&db).await? > 0` | `query.exists(&db)` | Stops at the first matching row |
| `query.all(&db).await?.into_iter().next()` | `query.first(&db)` | Adds `LIMIT 1` |
| Loading rows to add up a column | `query.sum(Column, &db)` | Aggregates in the database |

### Index the columns you search

Add `#[memoire(index)]` to columns you filter, join or sort on often, foreign keys in particular. `unique` columns are indexed automatically. MongoDB creates the same indexes on its collections. The in-memory engine keeps hash indexes on the primary key and `unique` columns, and Redis looks rows up by primary key directly; `index` has no effect on either.

### Avoid N+1 queries

Loading related rows with one query per parent is the most common performance problem in ORM code. Load all children with one `is_in` query and group them in memory, as shown in [Joins & relations](relations).

### Paginate with keys, not offsets

`offset(10_000)` makes the database walk over 10,000 rows before returning anything. For long lists and infinite scrolling, remember the last id you showed and continue from there:

```rust
async fn next_page<E: Executor>(db: &E, after_id: i64) -> memoire_orm::Result<Vec<User>> {
    User::query()
        .filter(User::ID.gt(after_id))
        .order_by(User::ID.asc())
        .limit(50)
        .all(db)
        .await
}
```

This stays fast on any page. Keep `paginate` (which also runs a `COUNT`) for screens that really need page numbers.

### Reuse the database handle

Open the database once at startup and share it: every handle (`MemoryDb`, `SqliteDb`, `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb`) is cheap to clone, and clones share the same connections. Opening a new pool per request throws away the connections and the statement caches. Size the PostgreSQL pool to what your server allows (see [PostgreSQL](postgres)).

### Measure in release mode

Debug builds of Rust code are many times slower. Benchmark and deploy with `cargo build --release`. To understand a slow query, print its SQL with `to_sql` and run it with `EXPLAIN` through [raw SQL](raw-sql).

## Next steps

- [Benchmarks](benchmarks): measured numbers and how to reproduce them.
- [Joins & relations](relations): loading related rows efficiently.
- [Transactions](transactions): grouping writes.
