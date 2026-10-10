# Inicio rápido

En este tutorial de diez minutos vas a construir desde cero un pequeño programa de lista de tareas. Empezarás con el motor en memoria, aprenderás las operaciones del día a día una por una y, al final, guardarás tus tareas en un archivo SQLite cambiando una sola línea.

## Qué vas a construir

Un programa de línea de comandos que:

1. crea una tabla `tasks`,
2. añade algunas tareas,
3. lista las tareas pendientes, empezando por las más importantes,
4. actualiza y elimina tareas,
5. calcula algunas estadísticas,
6. y, por último, lo guarda todo en un archivo `tasks.db`.

Solo necesitas Rust 1.88 o posterior. No hace falta ningún servidor de base de datos.

## Paso 1: crear el proyecto

```bash
cargo new tasks
cd tasks
cargo add memoire-orm
cargo add tokio --features macros,rt-multi-thread
```

`memoire-orm` viene con el motor en memoria activado por defecto. Tokio es el runtime asíncrono que ejecutará nuestro `async fn main`.

## Paso 2: describir la tabla con un struct

Abre `src/main.rs` y empieza por el modelo. Un *modelo* es un struct normal de Rust con `#[derive(Model)]`: cada campo se convierte en una columna.

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

Qué significa cada parte:

| Código | Significado |
|---|---|
| `use memoire_orm::prelude::*;` | Importa todo lo que sueles necesitar (`Model`, `MemoryDb`, `Executor`, ...) |
| `#[derive(Model)]` | Genera el código de base de datos para `Task` en tiempo de compilación |
| `#[memoire(table = "tasks")]` | El nombre de la tabla. Sin él, la tabla se llamaría `task` |
| `#[memoire(primary_key, auto_increment)]` | `id` identifica cada fila y es la base de datos quien elige su valor |
| `Option<String>` | Una columna que admite nulos: `None` se guarda como `NULL` |

> **Consejo** `#[derive(Model)]` también crea una *constante de columna tipada* por cada campo: `Task::ID`, `Task::TITLE`, `Task::PRIORITY`, `Task::DONE` y `Task::NOTE`. Las usarás para construir consultas dentro de un momento.

## Paso 3: abrir una base de datos y crear la tabla

```rust
#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Task>().await?;

    // los siguientes pasos van aquí

    Ok(())
}
```

`MemoryDb::new()` crea una base de datos vacía que vive en memoria. `create_table::<Task>()` crea la tabla `tasks` con las columnas de `Task`. Todas las operaciones de base de datos son `async`, así que les haces `.await`, y devuelven un `memoire_orm::Result`, de modo que `?` propaga los errores hacia arriba.

## Paso 4: insertar filas

```rust
let mut task = Task::new("Write the docs", 3);
task.insert(&db).await?;
println!("created task #{}", task.id);
```

Esto imprime `created task #1`. Como `id` es `auto_increment`, el valor que pones en el struct (`0`) se ignora y la base de datos genera uno. `insert` recibe `&mut self` para poder escribir el nuevo id de vuelta en tu struct.

Añadamos unas cuantas más:

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

## Paso 5: leer filas

Obtén una fila por su clave primaria:

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

`get` devuelve un error (`Error::NotFound`) cuando la fila no existe, mientras que `find` devuelve un `Option`. Usa el que se lea mejor en tu código.

Para buscar con condiciones, empieza una consulta con `Task::query()` y encadena lo que necesites:

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

Léelo como una frase: "las tareas cuyo done es false, ordenadas por prioridad (la más alta primero) y luego por título, todas ellas". No se envía nada a la base de datos hasta el `.all(&db)` final.

Las condiciones se combinan con `&` (y), `|` (o) y `!` (no):

```rust
let urgent = Task::query()
    .filter(Task::PRIORITY.ge(3) & Task::TITLE.contains("Memoire"))
    .first(&db)
    .await?;
println!("urgent: {:?}", urgent.map(|t| t.title));
```

Esto imprime `urgent: Some("Learn Memoire")`. `first` devuelve la primera fila que coincide, si la hay.

> **Nota** Las constantes de columna son tipadas. `Task::PRIORITY.ge(3)` compila porque `priority` es un `i32`, pero el compilador rechaza `Task::PRIORITY.ge("3")`. Los errores se detectan antes de que tu programa se ejecute.

## Paso 6: actualizar filas

Para cambiar una fila, modifica el struct y llama a `update`:

```rust
let mut bike = Task::get(&db, 2).await?;
bike.done = true;
bike.note = Some("new chain".to_string());
bike.update(&db).await?;
```

Para cambiar muchas filas a la vez sin cargarlas, usa `update_many`:

```rust
let changed = Task::update_many()
    .increment(Task::PRIORITY, 1)
    .filter(Task::DONE.eq(false) & Task::PRIORITY.lt(3))
    .execute(&db)
    .await?;
println!("bumped {changed} task(s)");
```

Esto imprime `bumped 1 task(s)`: solo "Call grandma" estaba pendiente con una prioridad inferior a 3, así que pasa de 2 a 3.

## Paso 7: contar y calcular estadísticas

```rust
let total = Task::count(&db).await?;
let open = Task::query().filter(Task::DONE.eq(false)).count(&db).await?;
let average = Task::query().avg(Task::PRIORITY, &db).await?;
println!("{open} open task(s) out of {total}, average priority {:.2}", average.unwrap_or(0.0));
```

```text
3 open task(s) out of 4, average priority 2.50
```

`avg` devuelve un `Option<f64>` porque la media de cero filas no existe.

## Paso 8: eliminar filas

```rust
let removed = Task::delete_many().filter(Task::DONE.eq(true)).execute(&db).await?;
println!("removed {removed} finished task(s)");
```

Esto imprime `removed 1 finished task(s)`. Para eliminar una sola fila que ya has cargado, llama en su lugar a `task.delete(&db).await?`.

> **Atención** `delete_many()` y `update_many()` sin ningún `filter` afectan a **todas las filas** de la tabla. A veces es justo lo que quieres, por eso Memoire lo permite, pero revisa bien tus filtros.

## Paso 9: echar un vistazo al SQL

¿Tienes curiosidad por saber qué envía Memoire a la base de datos? Cualquier consulta puede mostrar su SQL:

```rust
let query = Task::query().filter(Task::DONE.eq(false)).order_by(Task::PRIORITY.desc()).limit(10);
println!("{}", query.to_sql(Dialect::Sqlite));
println!("{}", query.to_sql(Dialect::Postgres));
```

```text
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = ? ORDER BY "tasks"."priority" DESC LIMIT 10
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = $1 ORDER BY "tasks"."priority" DESC LIMIT 10
```

El valor `false` no está dentro del texto SQL: se envía por separado como parámetro (`?` o `$1`). Esto es lo que te protege de la inyección SQL.

## Paso 10: guardar tus tareas en un archivo SQLite

La base de datos en memoria desaparece cuando el programa se detiene. Guardemos las tareas en un archivo. Primero activa la feature `sqlite`:

```bash
cargo add memoire-orm --features sqlite
```

Ahora mueve la lógica de la aplicación a una función que acepte *cualquier* base de datos. Todos los backends (y las transacciones) implementan el trait `Executor`, así que la misma función sirve para `MemoryDb`, `SqliteDb` y `PgPool`. Este es el programa completo:

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

/// Toda la lógica de la aplicación. Funciona con cualquier base de datos.
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

Solo `main` menciona SQLite. Han cambiado dos detalles respecto a la versión en memoria:

- `SqliteDb::open("tasks.db")?` abre el archivo y lo crea si hace falta.
- `create_table_if_not_exists` no falla cuando la tabla ya existe de una ejecución anterior.

Ejecútalo dos veces:

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

La segunda ejecución encontró las tres tareas guardadas por la primera: tus datos ahora viven en `tasks.db`.

> **Consejo** Para volver al motor en memoria (en las pruebas, por ejemplo), sustituye las dos líneas de `main` por `let db = MemoryDb::new(); db.create_table::<Task>().await?;`. No cambia nada más.

## Lo que has aprendido

| Tarea | Código |
|---|---|
| Describir una tabla | `#[derive(Model)]` sobre un struct |
| Crear la tabla | `db.create_table::<Task>().await?` |
| Insertar una fila | `task.insert(&db).await?` |
| Leer por clave | `Task::get(&db, id)` o `Task::find(&db, id)` |
| Buscar | `Task::query().filter(...).order_by(...).all(&db)` |
| Actualizar | `task.update(&db)` o `Task::update_many()...execute(&db)` |
| Eliminar | `task.delete(&db)` o `Task::delete_many()...execute(&db)` |
| Contar y agregar | `Task::count(&db)`, `.avg(Task::PRIORITY, &db)` |
| Inspeccionar el SQL | `query.to_sql(Dialect::Sqlite)` |

## Próximos pasos

- [Modelos](models): todos los atributos, tipos de columna y elementos generados.
- [Constructor de consultas](queries): todos los filtros, la ordenación, la paginación y los agregados.
- [Transacciones](transactions): aplica varios cambios en bloque, todo o nada.
- [Migraciones](migrations): haz evolucionar tu esquema de forma segura con el tiempo.
