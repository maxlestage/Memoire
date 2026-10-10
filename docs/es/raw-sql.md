# SQL sin procesar

El constructor de consultas cubre las necesidades del día a día, pero a veces quieres escribir el SQL tú mismo: una funcionalidad específica de una base de datos, un informe complejo, un auto-join, un comando de mantenimiento. Esta página muestra cómo ejecutar SQL sin procesar de forma segura con SQLite y PostgreSQL y cómo decodificar sus resultados.

## Dos métodos

Cada backend SQL (y cada transacción) tiene dos métodos, proporcionados por el trait `Executor`:

| Método | Para | Devuelve |
|---|---|---|
| `db.raw::<T>(sql, &params)` | Sentencias que devuelven filas (`SELECT`, `... RETURNING`) | `Vec<T>` |
| `db.raw_execute(sql, &params)` | Sentencias que no devuelven filas (`INSERT`, `UPDATE`, `DELETE`, DDL) | Número de filas afectadas (`u64`) |

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

## Parámetros y `params!`

Nunca pegues valores en el texto SQL. Escribe marcadores de parámetro y pasa los valores por separado: la base de datos los recibe como datos, así que nunca pueden cambiar el significado de la sentencia (ni inyección SQL ni problemas con las comillas).

`params![...]` construye la lista de valores a partir de valores normales de Rust. Acepta cualquier cosa que Memoire pueda almacenar: números, `&str`, `String`, `bool`, `Option<T>`, slices de bytes y los tipos de `chrono`/`uuid`/`json` cuando esas features están activadas. Usa `&[]` cuando no haya parámetros.

```rust
let values: Vec<Value> = params![42, "alice", Some(3.5), None::<i32>, true];
```

La sintaxis de los marcadores depende de la base de datos:

| Base de datos | Marcadores | Ejemplo |
|---|---|---|
| SQLite | `?` (o numerados `?1`, `?2`) | `WHERE age > ? AND name = ?` |
| PostgreSQL | `$1`, `$2`, ... | `WHERE age > $1 AND name = $2` |
| MySQL / MariaDB | `?` | Consulta [MySQL y MariaDB](mysql) |

> **Atención** Los marcadores solo pueden representar **valores**, nunca nombres de tablas o de columnas. Si un nombre de tabla o de columna procede de la entrada del usuario, compruébalo contra una lista fija de nombres permitidos antes de ponerlo en el texto SQL.

## Decodificar los resultados

Las filas se decodifican **por posición**: la primera columna del resultado va al primer elemento o campo, y así sucesivamente. Elige el tipo que encaje con la forma de tu resultado:

| Tipo | Úsalo para |
|---|---|
| `i64`, `String`, `f64`, `bool`, ... | Una sola columna |
| `Option<T>` | Una sola columna que puede ser `NULL` |
| Una tupla como `(i64, String, Option<f64>)` | Varias columnas (hasta 8) |
| Un modelo como `User` | Todas las columnas del modelo, en el orden de declaración |
| Un struct con `#[derive(FromRow)]` | Campos con nombre para un resultado personalizado |
| `Value` | Cualquier valor individual, sea cual sea su tipo (`Value::I64`, `Value::Text`, `Value::Null`, ...) |

Los ejemplos siguientes usan este modelo:

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

### Escalares y tuplas

```rust
let total: Vec<i64> = db.raw("SELECT COUNT(*) FROM employees", &[]).await?;

let names: Vec<(i64, String)> = db
    .raw("SELECT id, name FROM employees WHERE salary >= ? ORDER BY id", &params![50_000])
    .await?;
```

### Modelos completos

Para decodificar en un modelo, selecciona sus columnas en el orden de declaración. La lista de columnas ya generada del modelo (consulta [Modelos](models)) te ahorra escribirlas:

```rust
let columns = Employee::SCHEMA.sql(Dialect::Sqlite).columns;
// "employees"."id", "employees"."name", "employees"."manager_id", "employees"."salary"

let sql = format!("SELECT {columns} FROM employees WHERE salary > ? ORDER BY salary DESC");
let well_paid: Vec<Employee> = db.raw(&sql, &params![60_000]).await?;
```

### Structs personalizados

`#[derive(FromRow)]` da nombres legibles a un resultado personalizado. Aquí, un auto-join, que el constructor de consultas no puede expresar, cuenta los subordinados de cada responsable:

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

### `NULL` y errores de tipo

- Leer `NULL` en un tipo que no es `Option` falla con `Error::UnexpectedNull`: usa `Option<T>` para las columnas que admiten nulos y para los resultados de un `LEFT JOIN`.
- Un valor que no se puede convertir (por ejemplo, texto a `i64`) falla con `Error::Decode`, que indica la posición de la columna (`#0`, `#1`, ...).
- Los tamaños de los enteros son flexibles: una columna `INTEGER` se puede leer como `i32` o como `i64` siempre que el valor quepa.

## SQL sin procesar en transacciones

Una transacción también es un `Executor`, así que `raw` y `raw_execute` funcionan dentro de ella y forman parte de ella:

```rust
let tx = db.begin().await?;
tx.raw_execute("UPDATE employees SET salary = salary + ? WHERE manager_id IS NULL", &params![1_000])
    .await?;
Employee::update_many().increment(Employee::SALARY, 500).execute(&tx).await?;
tx.commit().await?;
```

## Notas sobre PostgreSQL

El mismo código funciona en PostgreSQL con marcadores `$n`:

```rust
async fn pg_example(db: &PgPool) -> memoire_orm::Result<()> {
    let rows: Vec<(String, i64)> = db
        .raw("SELECT name, salary FROM employees WHERE salary > $1 ORDER BY name", &params![50_000])
        .await?;
    println!("{rows:?}");
    Ok(())
}
```

Conviene conocer algunas reglas sobre los tipos:

- Los parámetros enteros se adaptan a la columna: `params![7]` se puede comparar con un `SMALLINT`, un `INTEGER` o un `BIGINT` (con comprobación de rango).
- `COUNT(*)` es un `BIGINT`: léelo como `i64`. El `SUM` de una columna `BIGINT` es un `NUMERIC`.
- Los valores `NUMERIC` se decodifican como texto sin perder precisión: léelos como `String` (exacto) o como `f64`. También puedes hacer el cast en SQL: `SUM(salary)::bigint`.
- Cuando PostgreSQL no puede deducir el tipo de un parámetro, añade un cast: `SELECT $1::text`.
- Un tipo que Memoire no conoce (por ejemplo, `INTERVAL`) produce un error: conviértelo a texto en SQL (`duration::text`).

## No disponible en el motor en memoria

`MemoryDb` no entiende SQL: `raw` y `raw_execute` devuelven `Error::Unsupported`. El código que también deba ejecutarse en el motor en memoria (pruebas, WebAssembly) debería limitarse al constructor de consultas.

## Vías de escape

Cuando necesitas una funcionalidad del propio driver subyacente, ambos backends SQL te permiten acceder a él.

### La conexión SQLite

`SqliteDb::with_connection` ejecuta un closure con la conexión de `rusqlite`, por ejemplo para leer un pragma o ejecutar comandos de mantenimiento:

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

`Error::database` envuelve cualquier error del driver en `Error::Database`. No llames a métodos de `db` desde dentro del closure: ahí la conexión ya está en uso.

### El pool de PostgreSQL

`PgPool::pool()` devuelve el `deadpool_postgres::Pool` subyacente, para monitorización o para funcionalidades de `tokio-postgres` como `LISTEN`/`NOTIFY` o `COPY`:

```rust
async fn pool_tools(db: &PgPool) -> memoire_orm::Result<()> {
    let status = db.pool().status();
    println!("{} connection(s) open, {} idle, max {}", status.size, status.available, status.max_size);

    let client = db.pool().get().await.map_err(|e| Error::Pool(e.to_string()))?;
    client.batch_execute("SET application_name = 'my-app'").await.map_err(Error::database)?;
    Ok(())
}
```

## Próximos pasos

- [Constructor de consultas](queries): la mayoría de las consultas no necesitan SQL sin procesar.
- [SQLite](sqlite) y [PostgreSQL](postgres): detalles de cada backend y correspondencia de tipos.
- [Migraciones](migrations): ejecuta scripts SQL como cambios de esquema versionados.
