# Change notifications

Wrap a database handle in `Observed` to be told about every write made through it: refresh a cache, push live updates to a web page, keep an audit log, or start work in the background. It works the same way on every backend.

```rust
use memoire_orm::prelude::*;

let db = Observed::new(SqliteDb::open("app.db")?); // or MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let _subscription = db.subscribe(|change| {
    println!("{:?} on {}: {} row(s), key {:?}", change.kind, change.table, change.rows, change.key);
});

let mut alice = User { id: 0, name: "Alice".into() };
alice.insert(&db).await?; // Insert on users: 1 row(s), key Some(I64(1))
alice.delete(&db).await?; // Delete on users: 1 row(s), key Some(I64(1))
```

`Observed<D>` is itself a database: pass `&db` to model methods, queries and transactions as before. Its clones share the listeners.

## What a change contains

| Field | Content |
|---|---|
| `table` | The table written (empty for raw SQL) |
| `kind` | `Insert`, `Update`, `Upsert`, `Delete`, `CreateTable`, `DropTable` or `Sql` |
| `rows` | Number of rows affected |
| `key` | The primary key, for `insert`, `update`, `upsert`, `delete` and `delete_by_key`; `None` for statements over many rows (`insert_many`, `update_many`, `update_only`, `delete_many`) |
| `row` | Every column as written, in schema order, for `insert`, `update` and `upsert` (with the generated key) |

Reads, and writes that change nothing (an `update_many` matching no row, deleting a missing key), are not announced.

## Transactions

Changes made in a transaction are announced when it commits, in order, and never if it rolls back or is dropped:

```rust
let tx = db.begin().await?;
alice.insert(&tx).await?; // nothing yet
tx.commit().await?;       // now: Insert on users
```

## Unsubscribing and channels

`subscribe` returns a `Subscription`: the listener stays registered until the subscription is dropped or `unsubscribe()` is called. Listeners run on the task that made the write, right after it, so keep them quick. For slow work, receive the changes on a channel and handle them elsewhere:

```rust
let (subscription, changes) = db.channel();
std::thread::spawn(move || {
    for change in changes {
        // send an e-mail, refresh a search index...
    }
});
```

`db.listener_count()` tells how many listeners are registered.

## Limits

- Only writes made **through the observed handle** (and its clones and transactions) are seen. Another program writing to the same database is not, nor is code that uses the wrapped handle directly (`db.inner()`).
- A change is announced after the write succeeded; for a transaction, after the commit succeeded.
- Raw SQL (`raw_execute`, migration scripts) is announced as `ChangeKind::Sql`, without a table name.

## Next steps

- [Transactions](transactions): commit, rollback and retries.
- [In-memory engine](memory): a database that can live in a file.
- [Cheat sheet](cheatsheet): everything on one page.
