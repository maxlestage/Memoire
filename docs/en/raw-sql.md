# Raw SQL

The query builder covers everyday needs, but sometimes you want to write SQL yourself: a database-specific feature, a complex report, a self-join, a maintenance command. This page shows how to run raw SQL safely with SQLite, PostgreSQL and MySQL/MariaDB, and how to decode its results.

## Two methods

Every SQL backend (`SqliteDb`, `PgPool`, `MySqlPool`) and its transactions have two methods, provided by the `Executor` trait:

| Method | For | Returns |
|---|---|---|
| `db.raw::<T>(sql, &params)` | Statements that return rows (`SELECT`, `... RETURNING`) | `Vec<T>` |
| `db.raw_execute(sql, &params)` | Statements that do not return rows (`INSERT`, `UPDATE`, `DELETE`, DDL) | Number of affected rows (`u64`) |

```rust
use memoire_orm::prelude::*;

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open_in_memory()?;
    db.raw_execute("CREATE TABLE cities (name TEXT NOT NULL, population INTEGER)", &[]).await?;

    let inserted = db
        .raw_execute(
            "INSERT INTO cities (name, population) VALUES (?, ?), (?, ?)",
            &params!["Paris", 2_100_000, "Lyon", 520_000],
        )
        .await?;
    assert_eq!(inserted, 2);

    let big: Vec<(String, i64)> = db
        .raw("SELECT name, population FROM cities WHERE population > ? ORDER BY name", &params![1_000_000])
        .await?;
    assert_eq!(big, vec![("Paris".to_string(), 2_100_000)]);
    Ok(())
}
```

## Parameters and `params!`

Never paste values into the SQL text. Write placeholders and pass the values separately: the database receives them as data, so they can never change the meaning of the statement (no SQL injection, no quoting problems).

`params![...]` builds the list of values from ordinary Rust values. It accepts anything Memoire can store: numbers, `&str`, `String`, `bool`, `Option<T>`, byte slices, and the `chrono`/`uuid`/`json` types when those features are enabled. Use `&[]` when there are no parameters.

```rust
let values: Vec<Value> = params![42, "alice", Some(3.5), None::<i32>, true];
```

The placeholder syntax depends on the database:

| Database | Placeholders | Example |
|---|---|---|
| SQLite | `?` (or numbered `?1`, `?2`) | `WHERE age > ? AND name = ?` |
| PostgreSQL | `$1`, `$2`, ... | `WHERE age > $1 AND name = $2` |
| MySQL / MariaDB | `?` | See [MySQL & MariaDB](mysql) |

> **Warning** Placeholders can only stand for **values**, never for table or column names. If a table or column name comes from user input, check it against a fixed list of allowed names before putting it into the SQL text.

## Decoding the results

Rows are decoded **by position**: the first column of the result goes into the first element or field, and so on. Choose the type that fits the shape of your result:

| Type | Use it for |
|---|---|
| `i64`, `String`, `f64`, `bool`, ... | A single column |
| `Option<T>` | A single column that may be `NULL` |
| A tuple such as `(i64, String, Option<f64>)` | Several columns (up to 8) |
| A model such as `User` | Every column of the model, in declaration order |
| A `#[derive(FromRow)]` struct | Named fields for a custom result |
| `Value` | Any single value, whatever its type (`Value::I64`, `Value::Text`, `Value::Null`, ...) |

The examples below use this model:

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "employees")]
pub struct Employee {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub manager_id: Option<i64>,
    pub salary: i64,
}

#[derive(FromRow, Debug)]
pub struct TeamSize {
    pub manager: String,
    pub reports: i64,
}
```

### Scalars and tuples

```rust
let total: Vec<i64> = db.raw("SELECT COUNT(*) FROM employees", &[]).await?;

let names: Vec<(i64, String)> = db
    .raw("SELECT id, name FROM employees WHERE salary >= ? ORDER BY id", &params![50_000])
    .await?;
```

### Whole models

To decode into a model, select its columns in declaration order. The pre-rendered column list of the model (see [Models](models)) saves you from typing them:

```rust
let columns = Employee::SCHEMA.sql(Dialect::Sqlite).columns;
// "employees"."id", "employees"."name", "employees"."manager_id", "employees"."salary"

let sql = format!("SELECT {columns} FROM employees WHERE salary > ? ORDER BY salary DESC");
let well_paid: Vec<Employee> = db.raw(&sql, &params![60_000]).await?;
```

### Custom structs

`#[derive(FromRow)]` gives readable names to a custom result. Here a self-join, which the query builder cannot express, counts each manager's reports:

```rust
let teams: Vec<TeamSize> = db
    .raw(
        "SELECT m.name, COUNT(e.id)
         FROM employees m
         JOIN employees e ON e.manager_id = m.id
         GROUP BY m.id, m.name
         ORDER BY m.name",
        &[],
    )
    .await?;
for team in &teams {
    println!("{} manages {} people", team.manager, team.reports);
}
```

### `NULL` and type errors

- Reading `NULL` into a non-`Option` type fails with `Error::UnexpectedNull`: use `Option<T>` for nullable columns and for `LEFT JOIN` results.
- A value that cannot be converted (for example text into `i64`) fails with `Error::Decode`, naming the column position (`#0`, `#1`, ...).
- Integer sizes are flexible: an `INTEGER` column can be read as `i32` or `i64` as long as the value fits.

## Raw SQL in transactions

A transaction is an `Executor` too, so `raw` and `raw_execute` work inside it and take part in it:

```rust
let tx = db.begin().await?;
tx.raw_execute("UPDATE employees SET salary = salary + ? WHERE manager_id IS NULL", &params![1_000])
    .await?;
Employee::update_many().increment(Employee::SALARY, 500).execute(&tx).await?;
tx.commit().await?;
```

## PostgreSQL notes

The same code works on PostgreSQL with `$n` placeholders:

```rust
async fn pg_example(db: &PgPool) -> memoire_orm::Result<()> {
    let rows: Vec<(String, i64)> = db
        .raw("SELECT name, salary FROM employees WHERE salary > $1 ORDER BY name", &params![50_000])
        .await?;
    println!("{rows:?}");
    Ok(())
}
```

A few type rules are useful to know:

- Integer parameters adapt to the column: `params![7]` can be compared with a `SMALLINT`, `INTEGER` or `BIGINT` (with a range check).
- `COUNT(*)` is a `BIGINT`: read it as `i64`. `SUM` of a `BIGINT` column is a `NUMERIC`.
- `NUMERIC` values are decoded as text without losing precision: read them as `String` (exact) or `f64`. You can also cast in SQL: `SUM(salary)::bigint`.
- When PostgreSQL cannot guess a parameter's type, add a cast: `SELECT $1::text`.
- A type Memoire does not know (for example `INTERVAL`) gives an error: cast it to text in SQL (`duration::text`).

## MySQL and MariaDB notes

MySQL uses `?` placeholders, like SQLite:

```rust
async fn mysql_example(db: &MySqlPool) -> memoire_orm::Result<()> {
    let rows: Vec<(String, i64)> = db
        .raw("SELECT name, salary FROM employees WHERE salary > ? ORDER BY name", &params![50_000])
        .await?;
    let total: Vec<i64> = db.raw("SELECT SUM(salary) FROM employees", &[]).await?;
    println!("{rows:?} {total:?}");
    Ok(())
}
```

- `DECIMAL` values (including `SUM` of an integer column) are decoded as exact text: read them as `String`, `f64`, or `i64` when they hold whole numbers.
- `raw` and `raw_execute` run one statement at a time; migration scripts (`Migration::sql`) may contain several.
- `BOOLEAN` columns are `TINYINT(1)`: they read as `bool` or as an integer.

## Not available on the in-memory engine, Redis and MongoDB

`MemoryDb`, `RedisDb` and `MongoDb` do not understand SQL: `raw` and `raw_execute` return `Error::Unsupported`. Code that must also run on these backends (tests on `MemoryDb`, for example) should stick to the query builder. For native Redis commands or MongoDB aggregation pipelines, use their escape hatches below.

## Escape hatches

When you need a feature of the underlying driver itself, every backend lets you reach it:

| Backend | Escape hatch | Gives you |
|---|---|---|
| `SqliteDb` | `db.with_connection(\|conn\| ...)` | The `rusqlite::Connection` |
| `PgPool` | `db.pool()` | The `deadpool_postgres::Pool` |
| `MySqlPool` | `db.pool()` | The `mysql_async::Pool` (see [MySQL & MariaDB](mysql)) |
| `RedisDb` | `db.connection()` | A `redis::aio::ConnectionManager` (see [Redis](redis)) |
| `MongoDb` | `db.database()` | The `mongodb::Database` (see [MongoDB](mongodb)) |

### The SQLite connection

`SqliteDb::with_connection` runs a closure with the `rusqlite` connection, for example to read a pragma or run maintenance commands:

```rust
async fn maintenance(db: &SqliteDb) -> memoire_orm::Result<()> {
    let mode: String = db
        .with_connection(|conn| conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)))
        .await
        .map_err(Error::database)?;
    println!("journal mode: {mode}");

    db.with_connection(|conn| conn.execute_batch("VACUUM")).await.map_err(Error::database)?;
    Ok(())
}
```

`Error::database` wraps any driver error into `Error::Database`. Do not call `db` methods from inside the closure: the connection is already in use there.

### The PostgreSQL pool

`PgPool::pool()` returns the underlying `deadpool_postgres::Pool`, for monitoring or for `tokio-postgres` features such as `LISTEN`/`NOTIFY` or `COPY`:

```rust
async fn pool_tools(db: &PgPool) -> memoire_orm::Result<()> {
    let status = db.pool().status();
    println!("{} connection(s) open, {} idle, max {}", status.size, status.available, status.max_size);

    let client = db.pool().get().await.map_err(|e| Error::Pool(e.to_string()))?;
    client.batch_execute("SET application_name = 'my-app'").await.map_err(Error::database)?;
    Ok(())
}
```

## Next steps

- [Query builder](queries): most queries do not need raw SQL.
- [SQLite](sqlite), [PostgreSQL](postgres) and [MySQL & MariaDB](mysql): backend details and type mappings.
- [Migrations](migrations): run SQL scripts as versioned schema changes.
