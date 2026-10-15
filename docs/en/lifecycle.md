# Timestamps, soft deletes & hooks

Three behaviours you switch on per model with an attribute, in the spirit of Laravel's Eloquent (`$timestamps`, `SoftDeletes`, model events) and Doctrine's lifecycle callbacks (`prePersist`, `preUpdate`, `preRemove`...). They work the same way on every backend, and a model that does not ask for them behaves exactly as before.

```rust
use memoire_orm::lifecycle::Hooks;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "articles", timestamps, soft_deletes, hooks)]
pub struct Article {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub created_at: Option<i64>, // milliseconds since the Unix epoch
    pub updated_at: Option<i64>,
    pub deleted_at: Option<i64>,
}
```

## Automatic timestamps

`#[memoire(timestamps)]` on the struct uses the fields named `created_at` and `updated_at`. To use other names, mark the fields instead: `#[memoire(created_at)]` and `#[memoire(updated_at)]` (you can have only one of them).

| Operation | `created_at` | `updated_at` |
|---|---|---|
| `insert`, `save` of a new row | set if unset (`None`, `0` or `""`), written back into the struct | same |
| `update` | unchanged | now (in the database; the borrowed struct keeps its value) |
| `save` of a stored row | unchanged | now, written back into the struct |
| `update_only(&db, columns)` | unchanged | now, added to the columns |
| `touch(&mut self, &db)` | unchanged | now, and nothing else |
| `update_many()` | unchanged | now, unless you `set` it yourself |
| `insert_many` | set if unset | set if unset |
| `upsert` | set if unset | now |
| soft `delete`, `restore` | unchanged | now |

```rust
let mut a = Article { id: 0, title: "Hello".into(), slug: String::new(),
                      created_at: None, updated_at: None, deleted_at: None };
a.insert(&db).await?;
assert!(a.created_at.is_some() && a.updated_at == a.created_at);

a.title = "Hello, world".into();
a.save(&db).await?; // updated_at moves forward, in the database and in `a`
```

The value depends on the type of the field; any other type is rejected when the code compiles:

| Field type | Stored value |
|---|---|
| `i64` | milliseconds since the Unix epoch |
| `i32` | seconds since the Unix epoch |
| `f64` | seconds since the Unix epoch, with a fraction |
| `String` | RFC 3339 in UTC, `2026-10-15T08:30:00.123Z` (sorts like the time) |
| `chrono::DateTime<Utc>`, `NaiveDateTime` (UTC), `NaiveDate` | now (feature `chrono`) |

Each may be an `Option<...>`, which is the clearest way to say "not set yet". In WebAssembly, enable the `web` feature so the time comes from the browser's clock.

## Soft deletes

With `#[memoire(soft_deletes)]` (a nullable field named `deleted_at`, or any `Option<...>` timestamp field marked `#[memoire(soft_delete)]`), deleting a row only stamps that column, and the row disappears from every query built with `Model::query()`, including `find`, `get`, `count`, `paginate` and the relation loaders:

```rust
a.delete(&db).await?;                                     // UPDATE ... SET deleted_at = now
assert_eq!(Article::find(&db, a.id).await?, None);         // hidden
assert_eq!(Article::with_trashed().count(&db).await?, 1);  // still stored
let trash = Article::only_trashed().all(&db).await?;       // just the deleted rows

let mut a = trash[0].clone();
assert!(a.trashed());
a.restore(&db).await?;      // deleted_at = NULL again
a.force_delete(&db).await?; // DELETE, for good
```

| Method | Effect |
|---|---|
| `row.delete(&db)` / `Model::delete_by_key(&db, key)` | Soft delete; `false` if already deleted |
| `Model::delete_many().filter(...).execute(&db)` | Soft deletes the matching rows |
| `Model::delete_many().force()...` / `row.force_delete(&db)` | Real `DELETE` |
| `row.restore(&db)` | Clears `deleted_at` |
| `row.trashed()` | `true` if `deleted_at` is set |
| `Model::with_trashed()`, `query().with_trashed()` | Every row |
| `Model::only_trashed()`, `query().only_trashed()` | Only the deleted rows |
| `query().without_trashed()` | Back to the default |

The scope is applied to the whole condition: `filter(a).or_filter(b)` becomes `deleted_at IS NULL AND (a OR b)`, never a trashed row matching `b`. `update_many` and `reload` see every row, like a joined table.

> **Tip** A `unique` column still counts soft-deleted rows: a deleted user keeps its email. Restore the row, or `force_delete` it, before reusing the value.

## Lifecycle hooks

Add `hooks` to the struct attribute and implement `memoire_orm::lifecycle::Hooks`, overriding only the methods you need:

```rust
impl Hooks for Article {
    fn before_save(&mut self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(Error::Validation("an article needs a title".into()));
        }
        self.slug = self.title.to_lowercase().replace(' ', "-");
        Ok(())
    }

    fn after_delete(&self) {
        println!("article {} deleted", self.id);
    }
}
```

| Hook | Runs in | Doctrine / Eloquent |
|---|---|---|
| `before_save(&mut self)` | `insert`, `save` | `saving` |
| `before_insert(&mut self)` / `after_insert(&self)` | `insert`, `save` of a new row | `prePersist` / `postPersist`, `creating` / `created` |
| `before_update(&self)` / `after_update(&self)` | `update`, `update_only`, `save` of a stored row | `preUpdate` / `postUpdate`, `updating` / `updated` |
| `before_delete(&self)` / `after_delete(&self)` | `delete` (soft too), `force_delete` | `preRemove` / `postRemove`, `deleting` / `deleted` |

A `before_*` hook that returns an error cancels the write: nothing is sent to the database and the error comes back to the caller. `Error::Validation(message)` is meant for that (`err.is_validation()`).

Hooks are synchronous and see only the row. Writes of many rows at once (`insert_many`, `upsert`, `update_many`, `delete_many`) run none, like Eloquent's mass updates. To react to every write with database access (an audit log, a cache), use [change notifications](events) instead.

## `save`

`save(&mut self, &db)` is the one call for "store this row", like Eloquent's `save()`: a row with an `auto_increment` key that is still `0` is inserted, any other row is updated (and inserted if no row has its key). It runs `before_save`, then the insert or update hooks, and writes the timestamps back into the struct.

## Next steps

- [Create, read, update, delete](crud): `first_or_create`, `update_or_create` and the other writes.
- [Query builder](queries): chunks and cursor pagination.
- [Query log](query-log): see the statements these methods run.
