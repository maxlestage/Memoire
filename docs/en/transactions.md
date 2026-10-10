# Transactions

A transaction groups several changes so that they happen all together or not at all. This page shows how to open, commit and roll back transactions, how to retry when two transactions collide, and how each backend behaves.

## Why transactions?

Moving money from one account to another takes two updates: subtract from the first account, add to the second. If the program crashes, or the second update fails, between the two, money disappears. Inside a transaction, either both updates are saved or neither is.

Transactions also give you a consistent view of the data while you work, and they make bulk writes much faster on SQLite.

## Basic usage

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub owner: String,
    pub balance: i64,
}

/// Moves `amount` from one account to another.
/// Returns `false` (and changes nothing) if the source account has too little money.
async fn transfer<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let tx = db.begin().await?;

    let source = Account::get(&tx, from).await?;
    if source.balance < amount {
        tx.rollback().await?;
        return Ok(false);
    }

    Account::update_many()
        .decrement(Account::BALANCE, amount)
        .filter(Account::ID.eq(from))
        .execute(&tx)
        .await?;
    Account::update_many()
        .increment(Account::BALANCE, amount)
        .filter(Account::ID.eq(to))
        .execute(&tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}
```

The three steps:

1. `db.begin().await?` starts a transaction and returns a transaction handle.
2. Pass `&tx` instead of `&db` to every operation that belongs to the transaction. A transaction is an `Executor`, so every model method, query and raw SQL call accepts it.
3. `tx.commit().await?` saves everything. `tx.rollback().await?` throws everything away.

`commit` and `rollback` consume the transaction, so the compiler stops you from using it afterwards.

`begin` is part of the `Database` trait, implemented by `MemoryDb`, `SqliteDb` and `PgPool`. Writing `D: Database` makes `transfer` work with all three.

## Dropping a transaction rolls it back

If a transaction is dropped without `commit`, it is rolled back. This makes error handling simple: when `?` returns early, the transaction goes out of scope and its changes are discarded.

```rust
async fn create_two(db: &MemoryDb) -> memoire_orm::Result<()> {
    let tx = db.begin().await?;
    Account { id: 0, owner: "Ada".into(), balance: 10 }.insert(&tx).await?;
    Account::get(&tx, 999).await?; // fails with NotFound: `?` returns early...
    tx.commit().await              // ...so this line never runs, and Ada is not saved
}
```

> **Tip** You rarely need an explicit `rollback`: returning an error with `?` is enough. Call `rollback` when you decide to cancel without an error, as in `transfer` above.

## Functions that work with or without a transaction

Write helper functions against the `Executor` trait. They then accept a database handle *or* a transaction, and the caller decides:

```rust
async fn open_account<E: Executor>(db: &E, owner: &str) -> memoire_orm::Result<Account> {
    let mut account = Account { id: 0, owner: owner.into(), balance: 0 };
    account.insert(db).await?;
    Ok(account)
}

async fn open_family_accounts(db: &MemoryDb) -> memoire_orm::Result<()> {
    open_account(db, "standalone").await?; // runs on its own

    let tx = db.begin().await?;
    open_account(&tx, "parent").await?; // both are part of the same transaction
    open_account(&tx, "child").await?;
    tx.commit().await
}
```

## Behaviour on each backend

| | In-memory engine | SQLite | PostgreSQL |
|---|---|---|---|
| How it works | Private copy-on-write snapshot | Holds the single connection | Holds one pooled connection |
| Other code during the transaction | Runs normally, sees only committed data | **Waits** until the transaction ends | Runs on other connections, sees only committed data |
| Conflicting writers | Detected at `commit`: `Error::Conflict` | Impossible: transactions run one at a time | Waits for row locks; serialization failures and deadlocks give `Error::Conflict` |
| Dropped without commit | Changes discarded immediately | `ROLLBACK` runs immediately | `ROLLBACK` sent in the background (needs Tokio) |

### In-memory engine: snapshots and conflicts

`MemoryDb::begin` takes a snapshot of the database. The transaction reads and writes its own copy (tables are only copied when first written), so it never sees concurrent changes and nobody sees its changes before `commit`.

At commit time, Memoire checks whether another writer changed any table that this transaction also wrote. If so, the commit fails with `Error::Conflict` and nothing is applied: the first writer wins.

```rust
let t1 = db.begin().await?;
let t2 = db.begin().await?;
Account::update_many().increment(Account::BALANCE, 10).execute(&t1).await?;
Account::update_many().increment(Account::BALANCE, 20).execute(&t2).await?;

t1.commit().await?;                       // the first writer wins
let err = t2.commit().await.unwrap_err(); // the second one is rejected
assert!(err.is_conflict());
```

Good to know:

- Conflicts are detected **per table**, not per row: two transactions writing different rows of the same table still conflict.
- A transaction that only reads never conflicts.
- Any write to the table counts, including writes made directly on `db` outside a transaction.

### Retrying on conflict

A conflict is not a bug: it means "someone else changed this data while you were working; start again from fresh data". Because a failed transaction applies nothing, retrying is always safe. Wrap the whole transaction in a loop:

```rust
async fn transfer_with_retry<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match transfer(db, from, to, amount).await {
            Err(err) if err.is_conflict() && attempts < 5 => continue, // try again with fresh data
            result => return result,
        }
    }
}
```

Retry the *whole* function, including the reads: the new attempt must start a new transaction and see the latest data. The same loop works on PostgreSQL, where serialization failures and deadlocks are reported as `Error::Conflict` too.

### SQLite: one transaction at a time

A `SqliteDb` uses a single connection. A transaction takes that connection for itself until it is committed, rolled back or dropped, so SQLite transactions never conflict: they simply run one after the other.

> **Warning** While a SQLite transaction is open, it holds the single connection. A query issued on the `SqliteDb` handle itself (`&db` instead of `&tx`) waits until the transaction ends, so in the same task it waits forever: a deadlock. Inside a transaction, always pass `&tx`:
>
> ```rust,ignore
> let tx = db.begin().await?;
> Account::count(&db).await?; // never returns: use `&tx` here
> ```

Other tasks or threads that use the same `SqliteDb` simply wait until the transaction finishes, so keep transactions short. For bulk inserts, wrapping thousands of inserts in one transaction is dramatically faster than inserting them one by one.

### PostgreSQL: pooled connections

`PgPool::begin` takes a connection from the pool and keeps it until the transaction ends. Other queries keep running on other connections and only see committed data.

- The isolation level is PostgreSQL's default, `READ COMMITTED`. To raise it, run `SET TRANSACTION` as the first statement of the transaction:

  ```rust
  async fn strict(db: &PgPool) -> memoire_orm::Result<()> {
      let tx = db.begin().await?;
      tx.raw_execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", &[]).await?;
      // ... reads and writes ...
      tx.commit().await
  }
  ```

  At `REPEATABLE READ` and `SERIALIZABLE`, PostgreSQL may abort a transaction that collides with another one. Memoire reports it as `Error::Conflict`, so the retry loop above applies unchanged. Deadlocks are reported the same way.
- A dropped transaction is rolled back in the background on the Tokio runtime. Without a runtime, the connection is closed instead, which also cancels the transaction.
- Using `&db` instead of `&tx` inside a transaction does not block, but it runs on another connection: it does not see the transaction's uncommitted changes and is not part of it.

## Limitations

- **No nested transactions.** A transaction cannot start another transaction (there is no savepoint API). Pass the existing `&tx` to helper functions instead.
- `Error::TransactionFinished` is returned if a transaction is used after it was finished, which the API normally makes impossible.

## Next steps

- [Migrations](migrations): every migration runs in its own transaction.
- [In-memory engine](memory), [SQLite](sqlite) and [PostgreSQL](postgres): backend details.
- [Why it's fast](performance): transactions and bulk writes.
