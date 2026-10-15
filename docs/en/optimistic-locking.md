# Optimistic locking & identity map

Two tools from Doctrine (and ActiveRecord's `lock_version`) for applications where several people or jobs write the same rows.

## Optimistic locking

Alice and Bob open the same account at the same time. Alice saves; then Bob saves, and silently overwrites Alice's change. Optimistic locking prevents this lost update: mark an integer field `#[memoire(version)]`, and every write checks that the row still has the version you read.

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub balance: i64,
    #[memoire(version)]
    pub version: i32,
}

let mut alice = Account::get(&db, 1).await?;
let mut bob = Account::get(&db, 1).await?;

alice.balance -= 30;
alice.save(&db).await?;                   // version 0 -> 1, written back into `alice`

bob.balance += 10;
match bob.save(&db).await {
    Err(e) if e.is_stale() => {
        bob.reload(&db).await?;           // read the new version, apply the change again
        bob.balance += 10;
        bob.save(&db).await?;
    }
    other => other?,
}
```

| Operation | With `#[memoire(version)]` |
|---|---|
| `save`, `update`, `update_only` | `UPDATE ... SET ..., version = version + 1 WHERE id = ? AND version = ?`; `Error::StaleRow` if the version changed |
| `delete`, `force_delete` (and soft deletes) | only if the version is unchanged, `Error::StaleRow` otherwise |
| `update_many()` | increments the version of every updated row, so copies read before are stale |
| `insert`, `upsert`, `insert_many` | write the version as it is (start at `0`) |

`save` writes the new version back into the struct. `update` and `update_only` borrow it, so they cannot: after them, `reload` the row (or use `save`). A missing row is still `Error::NotFound`; `Error::is_stale()` tells the two apart. The version field may be `i16`, `i32`, `i64` or an `Option` of them (a `NULL` version becomes 1); any other type is rejected at compile time.

## Identity map

An `IdentityMap` reads each row once and hands out the same shared object (`Arc<M>`) for every later lookup of its key, like Doctrine's `EntityManager::find`. Use one per unit of work: a request, a job, a test.

```rust
use memoire_orm::identity::IdentityMap;

let map = IdentityMap::new();
let a = map.find::<User, _>(&db, 1).await?;           // one query
let b = map.find::<User, _>(&db, 1).await?;           // no query: the same Arc
let team = map.find_many::<User, _, _>(&db, [1, 2, 3]).await?; // one query, for 2 and 3 only
let rows = map.merge(User::query().all(&db).await?);  // a row already held stays the same object

map.forget::<User>(1);   // after you changed or deleted it
map.clear();             // EntityManager::clear
```

The map is a cache that never expires and does not see writes made elsewhere: keep it short-lived, and `remember` (or `forget`) a row after you write it.

## Next steps

- [Transactions](transactions): make several writes all or nothing.
- [Factories & seeders](factories)
