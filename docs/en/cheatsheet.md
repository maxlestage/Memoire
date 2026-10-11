# Cheat sheet

Every common Memoire operation on one page. The examples assume `use memoire_orm::prelude::*;`, a database handle `db`, and the models `User` (`id`, `email`, `name`, `age: Option<i32>`, `active: bool`, `visits: i32`) and `Post` (`id`, `user_id`, `title`, `views: i32`).

## Setup

| Task | Code |
|---|---|
| In-memory engine (default feature) | `cargo add memoire-orm --git https://github.com/maxlestage/Memoire` |
| SQLite | `cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features sqlite` |
| PostgreSQL / with TLS | `--features postgres` / `--features postgres-tls` (plus Tokio) |
| MySQL, MariaDB / with TLS | `--features mysql` / `--features mysql-tls` (plus Tokio) |
| Redis / with TLS | `--features redis` / `--features redis-tls` (plus Tokio) |
| MongoDB | `--features mongodb` (plus Tokio) |
| Extra column types | `--features chrono,uuid,json` (or `full`) |
| Import everything | `use memoire_orm::prelude::*;` |
| Open in-memory | `let db = MemoryDb::new();` |
| Open in-memory, stored in a file | `MemoryDb::open("app.memoire")?`; encrypted: `MemoryDb::open_with(path, StorageOptions::new().password(pw))?` |
| Pinia-style store in `localStorage` | `let cart = Store::<Cart>::new("cart"); cart.update(\|c\| c.items.push(item));` (feature `web`) |
| Database in `localStorage` | `LocalStorageDb::open("app")?` (feature `web`) |
| Open SQLite file / memory | `SqliteDb::open("app.db")?` / `SqliteDb::open_in_memory()?` |
| Connect to PostgreSQL | `PgPool::connect(url).await?` or `PgPool::connect_with(url, 10).await?` |
| Connect to MySQL / MariaDB | `MySqlPool::connect("mysql://user:pass@host:3306/app").await?` |
| Connect to Redis | `RedisDb::connect(url).await?` or `RedisDb::connect_with_prefix(url, "myapp").await?` |
| Connect to MongoDB | `MongoDb::connect("mongodb://host:27017/app").await?` or `MongoDb::connect_with_database(uri, "app").await?` |
| Run without a runtime (memory, SQLite) | `memoire_orm::block_on(future)` |

Backend pages: [In-memory engine](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).

## Model attributes

| Attribute | Example |
|---|---|
| Table name | `#[memoire(table = "users")]` on the struct |
| Primary key | `#[memoire(primary_key)]` (a field named `id` is the key by default) |
| Generated key | `#[memoire(primary_key, auto_increment)]` (implicit for an integer `id` without attributes) |
| Unique / index | `#[memoire(unique)]`, `#[memoire(index)]` |
| Index over several fields | `#[memoire(unique(user_id, team_id), index(team_id, role))]` on the struct |
| Full-text index | `#[memoire(fulltext)]` |
| Column name | `#[memoire(column = "e_mail")]` |
| SQL type | `#[memoire(sql_type = "VARCHAR(120)")]` |
| Default | `#[memoire(default = "0")]` |
| Foreign key | `#[memoire(references = "users(id)", on_delete = "cascade")]` |
| Not stored | `#[memoire(skip)]` (field type must implement `Default`) |
| Nullable column | `age: Option<i32>` |
| Custom row type | `#[derive(FromRow)] struct Stats { user_id: i64, posts: i64 }` |

## Schema

| Task | Code |
|---|---|
| Create a table | `db.create_table::<User>().await?` |
| Create if missing | `db.create_table_if_not_exists::<User>().await?` |
| Drop a table | `db.drop_table::<User>().await?` / `db.drop_table_if_exists::<User>().await?` |
| See the DDL | `memoire_orm::sql::create_table(User::SCHEMA, Dialect::Postgres, false)` |
| See the primary-key SQL | `User::SCHEMA.sql(Dialect::Sqlite).insert` (also `find`, `update`, `delete`, `upsert`) |

## Create

| Task | Code |
|---|---|
| Insert one (id written back) | `user.insert(&db).await?` (`user` must be `mut`) |
| Insert many (returns count) | `User::insert_many(&db, &users).await?` |
| Insert or replace by key | `user.upsert(&db).await?` |

## Read

| Task | Code |
|---|---|
| By key, optional | `User::find(&db, 1).await?` → `Option<User>` |
| By key, required | `User::get(&db, 1).await?` → `User` or `Error::NotFound` |
| All rows | `User::all(&db).await?` |
| Count all rows | `User::count(&db).await?` |
| Several by key, one query | `User::find_many(&db, [1, 2, 3]).await?` → `Vec<User>` |
| Refresh a struct | `user.reload(&db).await?` |
| Start a query | `User::query()` |

## Filters

| SQL | Code |
|---|---|
| `=`, `<>` | `User::NAME.eq("Ada")`, `User::NAME.ne("Ada")` |
| `<`, `<=`, `>`, `>=` | `User::AGE.lt(18)`, `.le(18)`, `.gt(18)`, `.ge(18)` |
| `AND`, `OR`, `NOT` | `a & b`, `a \| b`, `!a` |
| Several filters (AND) | `.filter(a).filter(b)` |
| OR with previous filters | `.or_filter(c)` |
| `IN`, `NOT IN` | `User::ID.is_in([1, 2, 3])`, `User::ID.not_in(ids)` |
| `BETWEEN` (inclusive) | `User::AGE.between(18, 30)`, `.not_between(18, 30)` |
| `IS NULL`, `IS NOT NULL` | `User::AGE.is_null()`, `User::AGE.is_not_null()` |
| Column = column | `Post::USER_ID.eq_col(User::ID)` (also `ne_col`, `lt_col`, ...) |
| AND / OR of a list | `Expr::all(conditions)`, `Expr::any(conditions)` |
| Arithmetic | `User::VISITS.mul(2).gt(Expr::val(10))` (also `add`, `sub`, `div`) |

## Text matching

| Match | Code |
|---|---|
| Pattern (`%`, `_`) | `User::EMAIL.like("%@example.com")`, `.not_like(...)` |
| Pattern, any case | `User::EMAIL.ilike("%@EXAMPLE.com")` |
| Contains (escaped) | `User::NAME.contains("ad")`, `User::NAME.icontains("AD")` |
| Starts / ends with (escaped) | `User::NAME.starts_with("A")`, `User::EMAIL.ends_with(".org")` |
| Every word, whole words, any case | `Post::BODY.matches("rust orm")` |

## Ordering and paging

| Task | Code |
|---|---|
| Ascending / descending | `.order_by(User::NAME)`, `.order_by(User::NAME.asc())`, `.order_by(User::AGE.desc())` |
| NULL placement | `.order_by(User::AGE.desc().nulls_last())`, `.nulls_first()` |
| Limit / offset | `.limit(20)`, `.offset(40)` |
| Page (1-based) | `.page(3, 20)` |
| Page with total | `.paginate(&db, 3, 20).await?` → `Page { items, page, per_page, total }` |
| Distinct rows | `.distinct()` |

## Running a query

| Result | Code |
|---|---|
| Every row | `.all(&db).await?` → `Vec<T>` |
| First row | `.first(&db).await?` → `Option<T>` |
| Exactly one | `.one(&db).await?` → `T` or `Error::NotFound` |
| Count | `.count(&db).await?` → `u64` |
| Any match? | `.exists(&db).await?` → `bool` |

## Aggregates and projections

| Task | Code |
|---|---|
| Sum | `User::query().sum(User::VISITS, &db).await?` → `Option<i64>` (`f64` for floats) |
| Average | `.avg(User::AGE, &db).await?` → `Option<f64>` |
| Min / max | `.min(User::AGE, &db).await?`, `.max(User::NAME, &db).await?` |
| One column | `User::query().columns(User::EMAIL).all(&db).await?` → `Vec<String>` |
| Several columns | `.columns((User::ID, User::NAME))` → `Vec<(i64, String)>` (up to 8) |
| Count per group | `Post::query().group_by(Post::USER_ID).select_exprs([Post::USER_ID.expr(), count_all()])` → `(i64, i64)` rows |
| Filter groups | `.having(count_all().gt(Expr::val(1_i64)))` |
| Aggregate expressions | `count_all()`, `col.count()`, `col.count_distinct()`, `col.sum()`, `col.avg()`, `col.min()`, `col.max()` |

## Joins

| Task | Code | Rows |
|---|---|---|
| Inner join | `Post::query().inner_join::<User>(User::ID.eq_col(Post::USER_ID))` | `(Post, User)` |
| Left join | `User::query().left_join::<Post>(Post::USER_ID.eq_col(User::ID))` | `(User, Option<Post>)` |
| Join to filter only | `User::query().join::<Post>(JoinKind::Inner, cond).distinct()` | `User` |
| Columns across tables | `.inner_join::<User>(cond).columns((User::NAME, Post::TITLE))` | `(String, String)` |
| Children of one row | `Post::belonging_to(&user).all(&db).await?` | `Post` |
| Load children in one query | `users.load_children::<Post>(&db).await?` | `Vec<Vec<Post>>` |
| Load parents in one query | `posts.load_parent::<User>(&db).await?` | `Vec<Option<User>>` |
| Many-to-many in two queries | `posts.load_via::<PostTag, Tag>(&db).await?` | `Vec<Vec<Tag>>` |

## Update and delete

| Task | Code |
|---|---|
| Save one row (all columns) | `user.update(&db).await?` (`Error::NotFound` if missing) |
| Save some columns | `user.update_only(&db, (User::NAME, User::AGE)).await?` |
| Update many | `User::update_many().set(User::ACTIVE, false).filter(...).execute(&db).await?` → `u64` |
| Set NULL | `.set_null(User::AGE)` |
| Increment / decrement | `.increment(User::VISITS, 1)`, `.decrement(User::VISITS, 1)` |
| Set from an expression | `.set_expr(User::VISITS, User::VISITS.mul(2))` |
| Delete one row | `user.delete(&db).await?` → `bool` |
| Delete by key | `User::delete_by_key(&db, 1).await?` → `bool` |
| Delete many | `User::delete_many().filter(...).execute(&db).await?` → `u64` |

## Change notifications

| Task | Code |
|---|---|
| Watch the writes | `let db = Observed::new(db); let _sub = db.subscribe(\|change\| println!("{change:?}"));` |
| Receive them on a channel | `let (_sub, changes) = db.channel();` |
| Stop | drop the subscription, or `sub.unsubscribe()` |

## Transactions

| Task | Code |
|---|---|
| Begin | `let tx = db.begin().await?;` |
| Use it | pass `&tx` instead of `&db` |
| Commit / roll back | `tx.commit().await?` / `tx.rollback().await?` |
| Automatic rollback | drop `tx` (for example by returning early with `?`) |
| Retry on conflict | `Err(e) if e.is_conflict() => continue` in a loop around the whole transaction |
| Generic code | `async fn f<E: Executor>(db: &E)` (handle or transaction), `async fn g<D: Database>(db: &D)` (can `begin`) |

## Migrations

| Task | Code |
|---|---|
| Define | `Migration::new(1, "create_users").create_table::<User>()` |
| Other steps | `.drop_table("old")`, `.sql("...")`, `.sql_for(Dialect::Postgres, "...")` |
| Run pending | `Migrator::new().add(m1).add(m2).run(&db).await?` → applied versions |
| Inspect | `migrator.applied(&db).await?`, `migrator.pending(&db).await?` |

## Raw SQL and driver access

| Task | Code |
|---|---|
| Query | `db.raw::<(i64, String)>("SELECT id, name FROM users WHERE age > ?", &params![18]).await?` |
| Execute | `db.raw_execute("DELETE FROM users WHERE active = ?", &params![false]).await?` → `u64` |
| No parameters | `db.raw::<i64>("SELECT COUNT(*) FROM users", &[]).await?` |
| Available on | SQLite, PostgreSQL, MySQL/MariaDB (in-memory, Redis and MongoDB return `Error::Unsupported`) |
| Placeholders | SQLite and MySQL `?`, PostgreSQL `$1`, `$2`, ... |
| SQLite connection | `db.with_connection(\|conn\| ...).await` |
| PostgreSQL / MySQL pool | `db.pool()` |
| Redis connection | `db.connection()` |
| MongoDB database | `db.database()` |
| MongoDB transactions available? | `db.supports_transactions()` |

## Errors

| Check | Code |
|---|---|
| Missing row | `err.is_not_found()` / `Error::NotFound` |
| Duplicate key or unique value | `err.is_unique_violation()` / `Error::UniqueViolation { table, column }` |
| Transaction conflict | `err.is_conflict()` / `Error::Conflict(table)` |
| Other constraint | `Error::Constraint(message)` |
| NULL into a non-`Option` | `Error::UnexpectedNull { column }` |
| Not supported by the backend | `Error::Unsupported(message)` |

## Inspecting SQL

| Task | Code |
|---|---|
| Query SQL | `User::query().filter(...).to_sql(Dialect::Postgres)` |
| SQL and parameters | `.to_sql_with_params(Dialect::Sqlite)` → `(String, Vec<Value>)` |
| Update / delete SQL | `User::update_many().set(...).to_sql(Dialect::MySql)`, `User::delete_many().to_sql(...)` |

## Next steps

- [Query builder](queries): the details behind each line.
- [FAQ](faq): answers to common questions.
- [Quick start](quick-start): see everything in action.
