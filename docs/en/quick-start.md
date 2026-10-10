# Quick start

In this ten-minute tutorial you will build a tiny task list program from scratch. You will start with the in-memory engine, learn the everyday operations one at a time, and finally store your tasks in a SQLite file by changing a single line.

## What you will build

A command-line program that:

1. creates a `tasks` table,
2. adds a few tasks,
3. lists the open tasks, most important first,
4. updates and deletes tasks,
5. computes a few statistics,
6. and finally keeps everything in a `tasks.db` file.

You only need Rust 1.88 or newer. No database server is required.

## Step 1: create the project

```bash
cargo new tasks
cd tasks
cargo add memoire-orm
cargo add tokio --features macros,rt-multi-thread
```

`memoire-orm` comes with the in-memory engine enabled by default. Tokio is the async runtime that will drive our `async fn main`.

## Step 2: describe the table with a struct

Open `src/main.rs` and start with the model. A *model* is a normal Rust struct with `#[derive(Model)]`: each field becomes a column.

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tasks")]
pub struct Task {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub priority: i32,
    pub done: bool,
    pub note: Option<String>,
}

impl Task {
    fn new(title: &str, priority: i32) -> Self {
        Task { id: 0, title: title.to_string(), priority, done: false, note: None }
    }
}
```

What each piece means:

| Code | Meaning |
|---|---|
| `use memoire_orm::prelude::*;` | Imports everything you normally need (`Model`, `MemoryDb`, `Executor`, ...) |
| `#[derive(Model)]` | Generates the database code for `Task` at compile time |
| `#[memoire(table = "tasks")]` | The table name. Without it, the table would be called `task` |
| `#[memoire(primary_key, auto_increment)]` | `id` identifies each row, and the database picks its value |
| `Option<String>` | A nullable column: `None` is stored as `NULL` |

> **Tip** `#[derive(Model)]` also creates one *typed column constant* per field: `Task::ID`, `Task::TITLE`, `Task::PRIORITY`, `Task::DONE` and `Task::NOTE`. You will use them to build queries in a moment.

## Step 3: open a database and create the table

```rust
#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Task>().await?;

    // the next steps go here

    Ok(())
}
```

`MemoryDb::new()` creates an empty database that lives in memory. `create_table::<Task>()` creates the `tasks` table with the columns of `Task`. Every database operation is `async`, so you `.await` it, and returns a `memoire_orm::Result`, so `?` passes errors up.

## Step 4: insert rows

```rust
let mut task = Task::new("Write the docs", 3);
task.insert(&db).await?;
println!("created task #{}", task.id);
```

This prints `created task #1`. Because `id` is `auto_increment`, the value you put in the struct (`0`) is ignored and the database generates one. `insert` takes `&mut self` so it can write the new id back into your struct.

Let's add a few more:

```rust
let more = [("Fix the bike", 1), ("Call grandma", 2), ("Learn Memoire", 3)];
for (title, priority) in more {
    let mut task = Task::new(title, priority);
    task.insert(&db).await?;
    println!("created task #{}: {}", task.id, task.title);
}
```

```text
created task #2: Fix the bike
created task #3: Call grandma
created task #4: Learn Memoire
```

## Step 5: read rows

Fetch one row by its primary key:

```rust
let task = Task::get(&db, 2).await?;
println!("task 2 is {:?}", task.title);

let missing = Task::find(&db, 42).await?;
println!("task 42: {missing:?}");
```

```text
task 2 is "Fix the bike"
task 42: None
```

`get` returns an error (`Error::NotFound`) when the row does not exist, while `find` returns an `Option`. Use whichever reads better in your code.

To search with conditions, start a query with `Task::query()` and chain what you need:

```rust
let todo = Task::query()
    .filter(Task::DONE.eq(false))
    .order_by(Task::PRIORITY.desc())
    .order_by(Task::TITLE.asc())
    .all(&db)
    .await?;
for task in &todo {
    println!("[{}] {} (#{})", task.priority, task.title, task.id);
}
```

```text
[3] Learn Memoire (#4)
[3] Write the docs (#1)
[2] Call grandma (#3)
[1] Fix the bike (#2)
```

Read it like a sentence: "tasks where done is false, ordered by priority (highest first) then by title, all of them". Nothing is sent to the database until the final `.all(&db)`.

Conditions combine with `&` (and), `|` (or) and `!` (not):

```rust
let urgent = Task::query()
    .filter(Task::PRIORITY.ge(3) & Task::TITLE.contains("Memoire"))
    .first(&db)
    .await?;
println!("urgent: {:?}", urgent.map(|t| t.title));
```

This prints `urgent: Some("Learn Memoire")`. `first` returns the first matching row, if any.

> **Note** The column constants are typed. `Task::PRIORITY.ge(3)` compiles because `priority` is an `i32`, but `Task::PRIORITY.ge("3")` is rejected by the compiler. Mistakes are caught before your program runs.

## Step 6: update rows

To change one row, modify the struct and call `update`:

```rust
let mut bike = Task::get(&db, 2).await?;
bike.done = true;
bike.note = Some("new chain".to_string());
bike.update(&db).await?;
```

To change many rows at once without loading them, use `update_many`:

```rust
let changed = Task::update_many()
    .increment(Task::PRIORITY, 1)
    .filter(Task::DONE.eq(false) & Task::PRIORITY.lt(3))
    .execute(&db)
    .await?;
println!("bumped {changed} task(s)");
```

This prints `bumped 1 task(s)`: only "Call grandma" was open with a priority below 3, so it goes from 2 to 3.

## Step 7: count and compute statistics

```rust
let total = Task::count(&db).await?;
let open = Task::query().filter(Task::DONE.eq(false)).count(&db).await?;
let average = Task::query().avg(Task::PRIORITY, &db).await?;
println!("{open} open task(s) out of {total}, average priority {:.2}", average.unwrap_or(0.0));
```

```text
3 open task(s) out of 4, average priority 2.50
```

`avg` returns an `Option<f64>` because the average of zero rows does not exist.

## Step 8: delete rows

```rust
let removed = Task::delete_many().filter(Task::DONE.eq(true)).execute(&db).await?;
println!("removed {removed} finished task(s)");
```

This prints `removed 1 finished task(s)`. To delete a single row you already loaded, call `task.delete(&db).await?` instead.

> **Warning** `delete_many()` and `update_many()` without any `filter` affect **every row** of the table. That is sometimes what you want, so Memoire allows it, but double-check your filters.

## Step 9: peek at the SQL

Curious about what Memoire sends to the database? Any query can show its SQL:

```rust
let query = Task::query().filter(Task::DONE.eq(false)).order_by(Task::PRIORITY.desc()).limit(10);
println!("{}", query.to_sql(Dialect::Sqlite));
println!("{}", query.to_sql(Dialect::Postgres));
```

```text
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = ? ORDER BY "tasks"."priority" DESC LIMIT 10
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = $1 ORDER BY "tasks"."priority" DESC LIMIT 10
```

The value `false` is not inside the SQL text: it is sent separately as a parameter (`?` or `$1`). This is what protects you from SQL injection.

## Step 10: save your tasks in a SQLite file

The in-memory database disappears when the program stops. Let's keep the tasks in a file. First enable the `sqlite` feature:

```bash
cargo add memoire-orm --features sqlite
```

Now move the application logic into a function that accepts *any* database. The `Executor` trait is implemented by every backend (and by transactions), so the same function works with `MemoryDb`, `SqliteDb` and `PgPool`. Here is the complete program:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tasks")]
pub struct Task {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub priority: i32,
    pub done: bool,
    pub note: Option<String>,
}

impl Task {
    fn new(title: &str, priority: i32) -> Self {
        Task { id: 0, title: title.to_string(), priority, done: false, note: None }
    }
}

/// All the application logic. It works with any database.
async fn run<E: Executor>(db: &E) -> memoire_orm::Result<()> {
    println!("tasks already stored: {}", Task::count(db).await?);

    for (title, priority) in [("Write the docs", 3), ("Fix the bike", 1), ("Call grandma", 2)] {
        let mut task = Task::new(title, priority);
        task.insert(db).await?;
    }

    Task::update_many()
        .set(Task::DONE, true)
        .filter(Task::TITLE.eq("Fix the bike"))
        .execute(db)
        .await?;

    let todo = Task::query()
        .filter(Task::DONE.eq(false))
        .order_by(Task::PRIORITY.desc())
        .order_by(Task::ID.asc())
        .all(db)
        .await?;
    for task in &todo {
        println!("[{}] {} (#{})", task.priority, task.title, task.id);
    }
    println!("tasks stored now: {}", Task::count(db).await?);
    Ok(())
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("tasks.db")?;
    db.create_table_if_not_exists::<Task>().await?;
    run(&db).await
}
```

Only `main` mentions SQLite. Two details changed compared to the memory version:

- `SqliteDb::open("tasks.db")?` opens the file, creating it if needed.
- `create_table_if_not_exists` does not fail when the table already exists from a previous run.

Run it twice:

```text
$ cargo run
tasks already stored: 0
[3] Write the docs (#1)
[2] Call grandma (#3)
tasks stored now: 3

$ cargo run
tasks already stored: 3
[3] Write the docs (#1)
[3] Write the docs (#4)
[2] Call grandma (#3)
[2] Call grandma (#6)
tasks stored now: 6
```

The second run found the three tasks saved by the first one: your data now lives in `tasks.db`.

> **Tip** To go back to the in-memory engine (in tests, for example), replace the two lines in `main` with `let db = MemoryDb::new(); db.create_table::<Task>().await?;`. Nothing else changes.

### Using another database

The same `run` function works with every backend. Enable the feature, and change the line that opens the database:

| Database | Feature | Open it in `main` |
|---|---|---|
| In-memory | `memory` (default) | `let db = MemoryDb::new();` |
| SQLite | `sqlite` | `let db = SqliteDb::open("tasks.db")?;` |
| PostgreSQL | `postgres` | `let db = PgPool::connect("postgres://user:password@localhost/tasks").await?;` |
| MySQL / MariaDB | `mysql` | `let db = MySqlPool::connect("mysql://user:password@localhost:3306/tasks").await?;` |
| Redis | `redis` | `let db = RedisDb::connect("redis://127.0.0.1:6379").await?;` |
| MongoDB | `mongodb` | `let db = MongoDb::connect("mongodb://localhost:27017/tasks").await?;` |

For example, with MongoDB:

```rust
#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MongoDb::connect("mongodb://localhost:27017/tasks").await?;
    db.create_table_if_not_exists::<Task>().await?;
    run(&db).await
}
```

The server backends need a running server; each backend page explains how to start one locally.

## What you learned

| Task | Code |
|---|---|
| Describe a table | `#[derive(Model)]` on a struct |
| Create the table | `db.create_table::<Task>().await?` |
| Insert a row | `task.insert(&db).await?` |
| Read by key | `Task::get(&db, id)` or `Task::find(&db, id)` |
| Search | `Task::query().filter(...).order_by(...).all(&db)` |
| Update | `task.update(&db)` or `Task::update_many()...execute(&db)` |
| Delete | `task.delete(&db)` or `Task::delete_many()...execute(&db)` |
| Count and aggregate | `Task::count(&db)`, `.avg(Task::PRIORITY, &db)` |
| Inspect SQL | `query.to_sql(Dialect::Sqlite)` |

## Next steps

- [Models](models): every attribute, column type and generated item.
- [Query builder](queries): all the filters, ordering, pagination and aggregates.
- [Transactions](transactions): make several changes all-or-nothing.
- [Migrations](migrations): evolve your schema safely over time.
