# MongoDB (NoSQL)

`MongoDb` stores your models as MongoDB documents, one collection per table, through the official MongoDB driver. Filters, sorting and pagination are translated into native MongoDB queries and run on the server; the rest of the query builder (joins, `GROUP BY`, aggregates) is completed by Memoire, so the whole API keeps working.

## Enable the backend

```bash
cargo add memoire-orm --features mongodb
cargo add tokio --features macros,rt-multi-thread
```

The client is the official `mongodb` driver (3.x), which encrypts connections with `rustls` when the URI asks for TLS (MongoDB Atlas always does). `MongoDb` needs the **Tokio** runtime.

## Connecting

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    #[memoire(index)]
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MongoDb::connect("mongodb://localhost:27017/app").await?;
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@example.com".into(), name: "Alice".into(), age: None };
    alice.insert(&db).await?;

    let adults = User::query().filter(User::AGE.ge(18)).order_by(User::NAME.asc()).all(&db).await?;
    println!("{} adult(s)", adults.len());
    Ok(())
}
```

| Method | What it does |
|---|---|
| `MongoDb::connect(uri)` | Connects with a `mongodb://` or `mongodb+srv://` URI that **names the database** (`.../app`); otherwise it fails with `Error::Pool` |
| `MongoDb::connect_with_database(uri, "app")` | Connects to the server of `uri` and uses the database `app` |
| `db.database()` | The driver's `mongodb::Database`, for native MongoDB features (escape hatch) |
| `db.supports_transactions()` | `true` on a replica set or sharded cluster (see "Transactions") |

The handle shares the driver's connection pool, and cloning it is cheap. For MongoDB Atlas, use the `mongodb+srv://user:password@cluster0.example.mongodb.net/app` URI shown in the Atlas dashboard.

### A local server with transactions

Transactions need a replica set, but a single-node replica set is enough for development:

```bash
docker run -d --name mongo -p 27017:27017 mongo:7 --replSet rs0
docker exec mongo mongosh --eval 'rs.initiate({_id:"rs0",members:[{_id:0,host:"127.0.0.1:27017"}]})'
```

Then connect with `mongodb://127.0.0.1:27017/app?directConnection=true`.

## How your data is stored

| Memoire | MongoDB |
|---|---|
| Table | Collection |
| Row | Document; the primary key is stored in `_id` |
| `NULL` | The field is absent |
| `#[memoire(unique)]` | Unique index named after the column, ignoring absent fields (many `NULL`s are allowed, as in SQL) |
| `#[memoire(index)]` | Regular index named after the column |
| `auto_increment` | A counter document in the `__memoire_counters` collection |
| `DateTime<Utc>`, `NaiveDateTime` | BSON date (**millisecond** precision) |
| `NaiveDate`, `NaiveTime` | ISO 8601 strings, which sort chronologically |
| `uuid::Uuid` | BSON binary, subtype 4 (UUID) |
| `Vec<u8>` | BSON binary |
| `serde_json::Value`, `Json<T>` | Embedded documents and arrays |

A user with no age looks like this in MongoDB:

```text
{ "_id": 1, "email": "alice@example.com", "name": "Alice" }
```

> **Warning** BSON dates keep milliseconds, not microseconds or nanoseconds. A `DateTime<Utc>` read back from MongoDB is truncated to the millisecond: truncate values before comparing them (`Utc::now().trunc_subsecs(3)`, from `chrono::SubsecRound`).

## How queries run

| Query | Where it runs |
|---|---|
| `find`, `get` by primary key | Server: `find` on `_id` |
| `filter`, `order_by`, `limit`, `offset`, `columns` on one collection | Server: one `find` with filter, sort, skip and limit |
| `count` | Server: `countDocuments` |
| `update_many` with `set`, `set_null`, `increment`, `decrement` | Server: one update with an aggregation pipeline |
| `delete_many` | Server: `deleteMany` |
| Joins | The filter is sent to the server; joined collections are fetched with `$in` on the join keys only; the join is done by Memoire |
| `group_by`, `sum`, `avg`, `min`, `max`, `distinct` | Matching documents are fetched with the filter applied on the server; the aggregation is done by Memoire |

Filters are translated so that they keep SQL's meaning of `NULL` (three-valued logic). A part of a filter that has no MongoDB equivalent (arithmetic such as `User::AGE.add(1).gt(...)`, for example) is checked by Memoire on the documents matched by the rest of the filter. A few examples:

| Memoire filter | MongoDB filter |
|---|---|
| `User::AGE.ge(18)` | `{ age: { $gte: 18 } }` |
| `!User::AGE.gt(18)` | `{ age: { $lte: 18 } }` (users without an age do not match) |
| `User::AGE.ne(3)` | `{ age: { $nin: [null, 3] } }` |
| `User::AGE.is_null()` | `{ age: null }` |
| `User::NAME.contains("li")` | `{ name: { $regex: "^.*li.*$", $options: "s" } }` |
| `User::ID.is_in([1, 2])` | `{ _id: { $in: [1, 2] } }` |

> **Tip** Add `#[memoire(index)]` to the fields you filter and sort on: MongoDB uses those indexes for the server-side part of every query.

## Query semantics

- **Comparisons and `LIKE` are case-sensitive**, and strings sort by their bytes. `ilike` and `icontains` ignore case, including accented letters.
- **NULL ordering follows MongoDB:** missing values come first in ascending order and last in descending order. Use `nulls_first()`/`nulls_last()` to choose explicitly (a placement different from MongoDB's own is applied by Memoire after fetching).
- **Primary keys, `unique` columns and `NOT NULL`** (non-`Option` fields) are enforced, with the usual errors.
- **Foreign keys are not enforced**, and `on_delete = "cascade"` does nothing.
- **Raw SQL** returns `Error::Unsupported`: use `db.database()` for native commands. Self-joins (a collection twice in one query) are not supported either.

## Transactions

On a **replica set** or a **sharded cluster** (including MongoDB Atlas), `db.begin()` opens a real multi-document transaction:

- reads inside the transaction see its own writes;
- a write conflict with another transaction is reported as `Error::Conflict` (by the conflicting statement or by `commit`): retry the whole transaction, as shown in [Transactions](transactions);
- dropping the transaction aborts it;
- creating and dropping collections happens outside the transaction, like schema changes on MySQL.

Memoire also wraps its own multi-document writes, `insert_many` and `update_many`, in a transaction, so they are all-or-nothing even when you do not use one yourself.

On a **standalone server**, MongoDB has no transactions:

| | Standalone server |
|---|---|
| `db.begin()` | Fails with `Error::Unsupported` |
| `db.supports_transactions()` | `false` |
| Migrations | Applied without a transaction |
| `insert_many` | If a document fails (for example a duplicate), Memoire removes the documents of that call that were already inserted (best effort) |
| `update_many` | Not all-or-nothing: if a document fails (for example a `unique` clash), the documents already updated stay updated |

## Migrations

`create_table` creates the collection and its indexes; `drop_table` drops the collection and its id counter. `sql` steps fail with `Error::Unsupported` and `sql_for` steps are skipped.

Because collections are created outside transactions, a migration that fails after a `create_table` step leaves that collection behind. Keep one schema change per migration, and drop the collection by hand before running a fixed migration again.

## Errors

| MongoDB situation | Memoire error |
|---|---|
| Duplicate key (E11000) | `Error::UniqueViolation { table, column }` |
| Write conflict, transient transaction error | `Error::Conflict(table)`: retry the transaction |
| Missing collection | `Error::NoSuchTable(name)` (Memoire remembers which collections exist, per handle) |
| Collection already exists | `Error::TableExists(name)` |
| URI without a database | `Error::Pool(message)` |
| Anything else | `Error::Database(...)` with the original driver error |

## Direct access

`db.database()` returns the driver's `mongodb::Database`, for aggregation pipelines, text search, change streams and anything else MongoDB offers. Add `mongodb` (3.x) to your dependencies:

```rust
use mongodb::bson::{doc, Document};

async fn top_authors(db: &MongoDb) -> Result<Vec<Document>, mongodb::error::Error> {
    let posts = db.database().collection::<Document>("posts");
    let mut cursor = posts
        .aggregate([
            doc! { "$group": { "_id": "$user_id", "views": { "$sum": "$views" } } },
            doc! { "$sort": { "views": -1 } },
            doc! { "$limit": 5 },
        ])
        .await?;
    let mut top = Vec::new();
    while cursor.advance().await? {
        top.push(cursor.deserialize_current()?);
    }
    Ok(top)
}
```

Remember that Memoire stores the primary key in `_id` and leaves `NULL` fields out of the documents.

## Next steps

- [Transactions](transactions): conflicts and retries.
- [Query builder](queries): everything you can filter, sort and aggregate.
- [Redis](redis): the other NoSQL backend.
