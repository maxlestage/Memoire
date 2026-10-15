# Query log

Wrap a database handle in `Logged` to record every statement run through it, its clones and its transactions: the SQL, the parameters, the duration and the number of rows. It is Laravel's `DB::enableQueryLog()` / `DB::listen()` and Doctrine's SQL logger, on every backend.

```rust
use memoire_orm::log::Logged;
use memoire_orm::prelude::*;

let db = Logged::new(SqliteDb::open("app.db")?); // or MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let users = User::query().filter(User::ACTIVE.eq(true)).all(&db).await?;

for q in db.queries() {
    println!("{q}");
}
// SELECT "users"."id", ... FROM "users" WHERE "users"."active" = ? -- [TRUE] (0.041 ms)
```

`Logged<D>` is itself a database: pass `&db` to model methods, queries and transactions as before.

## What an entry contains

| Field | Content |
|---|---|
| `sql` | The statement, in the backend's dialect (`BEGIN`, `COMMIT`, `ROLLBACK` for transactions) |
| `params` | The bound parameters, in order |
| `duration` | Time from the call to the result (`None` in WebAssembly) |
| `rows` | Rows returned by a query, or affected by a write; `None` if it failed |
| `error` | The error message, when the statement failed |
| `in_transaction` | Whether it ran inside a transaction |

The in-memory engine, Redis and MongoDB run no SQL: their log shows the equivalent statement in the SQLite dialect, which is just as useful to see what a piece of code does.

## Finding N+1 queries in a test

```rust
let db = Logged::new(MemoryDb::new());
// ... seed ...
db.clear();

let users = User::all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

assert_eq!(db.query_count(), 2, "one query per level, whatever the number of users");
```

| Method | Effect |
|---|---|
| `queries()` | Copy of the entries, oldest first |
| `take_queries()` | The entries, emptying the log |
| `query_count()` | Number of entries |
| `clear()` | Empties the log |
| `set_recording(false)` / `set_recording(true)` | Stops / resumes keeping entries |
| `listen(\|q\| ...)` | Calls a function with each statement as it runs |
| `Logged::with_limit(db, n)` | Keeps only the last `n` entries |

## Printing every statement

For a long-running program, keep nothing and print instead:

```rust
let db = Logged::with_limit(PgPool::connect(&url).await?, 0);
db.listen(|q| {
    if q.duration.is_some_and(|d| d.as_millis() > 100) {
        eprintln!("slow query: {q}");
    }
});
```

Listeners run on the task that ran the statement, right after it: keep them quick.

> **Tip** Logging renders every statement to text and copies its parameters, which costs a little on each call. Use it in development and tests, or with a listener that filters what it keeps.

## Next steps

- [Change notifications](events): be told about the writes, with the rows written.
- [Raw SQL](raw-sql): run your own statements.
