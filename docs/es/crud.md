# Crear, leer, actualizar, eliminar

Esta página cubre todas las formas de escribir y leer filas con un modelo: insertar una o muchas filas, cargar por clave, actualizar una fila o muchas, hacer upsert, eliminar y gestionar los errores que pueden devolver estas operaciones.

## Los ejemplos de esta página

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
    pub visits: i32,
}

impl User {
    pub fn new(email: &str, name: &str) -> Self {
        User { id: 0, email: email.into(), name: name.into(), age: None, active: true, visits: 0 }
    }
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}
```

Todos los métodos reciben `&db`, que puede ser cualquier manejador de base de datos (`MemoryDb`, `SqliteDb`, `PgPool`) o una [transacción](transactions) abierta.

## Crear

### Insertar una fila

```rust
let mut alice = User::new("alice@example.com", "Alice");
alice.insert(&db).await?;
println!("Alice got id {}", alice.id); // 1
```

`insert` toma prestada la fila de forma mutable porque, con una clave `auto_increment`, escribe el valor generado de vuelta en el struct. El `id` que pongas antes de insertar (aquí `0`) se ignora.

En un modelo cuya clave no es autoincremental, la clave que pongas es la que se guarda:

```rust
let mut rust = Tag { slug: "rust".into(), label: "Rust".into() };
rust.insert(&db).await?;
```

Insertar una segunda fila con la misma clave primaria o con el mismo valor en una columna `unique` falla con `Error::UniqueViolation` (consulta "Gestionar los errores" más abajo).

### Insertar muchas filas a la vez

`insert_many` inserta un slice completo con el menor número posible de sentencias y devuelve cuántas filas se han insertado:

```rust
let users: Vec<User> = (1..=1000)
    .map(|i| User::new(&format!("user{i}@example.com"), &format!("User {i}")))
    .collect();
let inserted = User::insert_many(&db, &users).await?;
assert_eq!(inserted, 1000);
```

En las bases de datos SQL esto envía un `INSERT ... VALUES (...), (...), ...` de varias filas por lote. El tamaño de los lotes se ajusta para no superar el límite de parámetros enlazados de la base de datos (32 766 en SQLite, 65 535 en PostgreSQL) y cada lote contiene como máximo 10 000 filas, así que incluso los slices enormes se gestionan automáticamente.

Lo que debes saber sobre `insert_many`:

- Las claves generadas **no** se leen de vuelta: los structs de tu slice conservan `id: 0`. Vuelve a consultar las filas si necesitas sus ids, o insértalas una a una con `insert`.
- Cada lote es atómico: si una fila falla (por ejemplo, un email duplicado), no se guarda nada de ese lote.
- Cuando el slice ocupa más de un lote, los lotes anteriores quedan guardados si uno posterior falla. Envuelve la llamada en una [transacción](transactions) para que toda la operación sea de todo o nada. Una transacción también hace que las inserciones masivas sean mucho más rápidas en SQLite.

## Leer

### Por clave primaria

```rust
let maybe: Option<User> = User::find(&db, 1).await?; // None si no existe la fila 1
let alice: User = User::get(&db, 1).await?;          // Error::NotFound si no existe la fila 1
let tag = Tag::get(&db, "rust").await?;              // las claves de texto funcionan igual
```

Usa `find` cuando que falte una fila sea una situación normal, y `get` cuando sea un error. La clave debe tener el tipo de la clave primaria: `User::find(&db, "1")` no compila.

### Todas las filas, o un recuento

```rust
let everyone: Vec<User> = User::all(&db).await?;
let how_many: u64 = User::count(&db).await?;
```

Para cualquier cosa más específica (filtros, ordenación, paginación, unas pocas columnas...), usa el [constructor de consultas](queries): `User::query().filter(...).all(&db)`.

### Refrescar una fila

`reload` vuelve a leer una fila de la base de datos y sustituye todos los campos de tu struct. Resulta útil después de cambios hechos con `update_many` o por otra persona:

```rust
let mut alice = User::get(&db, 1).await?;
User::update_many().increment(User::VISITS, 1).filter(User::ID.eq(alice.id)).execute(&db).await?;
alice.reload(&db).await?;
assert_eq!(alice.visits, 1);
```

`reload` falla con `Error::NotFound` si la fila se eliminó entretanto.

## Actualizar

### Cambiar una fila

Carga la fila, cambia los campos y luego llama a `update`:

```rust
let mut alice = User::get(&db, 1).await?;
alice.name = "Alice Liddell".into();
alice.age = Some(32);
alice.update(&db).await?;
```

`update` escribe **todas las columnas** del struct en la fila con la misma clave primaria. Si ninguna fila tiene esa clave, falla con `Error::NotFound`; nunca crea una fila.

> **Nota** Como `update` escribe todas las columnas, si dos programas cargan la misma fila, cambian campos distintos y ambos llaman a `update`, uno sobrescribirá el cambio del otro (gana la última escritura). Para contadores y otros cambios concurrentes, usa `update_many` con `increment`, o una [transacción](transactions).

### Cambiar muchas filas

`update_many` cambia todas las filas que coinciden con sus filtros, sin cargarlas, y devuelve el número de filas actualizadas:

```rust
let changed = User::update_many()
    .set(User::ACTIVE, false)
    .filter(User::AGE.lt(18))
    .execute(&db)
    .await?;
println!("{changed} user(s) deactivated");
```

Las asignaciones disponibles:

| Método | SQL | Ejemplo |
|---|---|---|
| `set(column, value)` | `column = value` | `.set(User::NAME, "Bob")` |
| `set_null(column)` | `column = NULL` | `.set_null(User::AGE)` (solo para columnas `Option`) |
| `increment(column, n)` | `column = column + n` | `.increment(User::VISITS, 1)` |
| `decrement(column, n)` | `column = column - n` | `.decrement(User::VISITS, 1)` |
| `set_expr(column, expr)` | `column = <expression>` | `.set_expr(User::VISITS, User::VISITS.mul(2))` |

Las asignaciones y los filtros se pueden combinar libremente:

```rust
User::update_many()
    .set(User::ACTIVE, true)
    .increment(User::VISITS, 1)
    .set_null(User::AGE)
    .filter(User::EMAIL.ends_with("@example.com") & User::ACTIVE.eq(false))
    .execute(&db)
    .await?;
```

Igual que las consultas, una actualización puede mostrar su SQL:

```rust
let update = User::update_many()
    .set(User::ACTIVE, false)
    .increment(User::VISITS, 1)
    .filter(User::NAME.eq("Bob"));
println!("{}", update.to_sql(Dialect::Postgres));
// UPDATE "users" SET "active" = $1, "visits" = ("users"."visits" + $2) WHERE "users"."name" = $3
```

> **Atención** Sin `filter`, `update_many` cambia **todas las filas** de la tabla. Se necesita al menos una asignación: `update_many()` sin ningún `set`, `increment`, ... falla con `Error::Unsupported`.

Una actualización que violaría una restricción `unique` falla por completo y deja todas las filas intactas:

```rust
let err = User::update_many().set(User::EMAIL, "same@example.com").execute(&db).await.unwrap_err();
assert!(err.is_unique_violation());
```

## Upsert

`upsert` significa "insertar, o actualizar si ya existe". Inserta la fila o, si ya existe una fila con la misma clave primaria, sobrescribe todas sus columnas:

```rust
let tag = Tag { slug: "rust".into(), label: "Rust 🦀".into() };
tag.upsert(&db).await?; // crea "rust", o actualiza su label

Tag { slug: "orm".into(), label: "ORM".into() }.upsert(&db).await?;
```

En las bases de datos SQL es una sola sentencia (`INSERT ... ON CONFLICT (key) DO UPDATE` en SQLite y PostgreSQL), así que es seguro incluso cuando dos programas hacen upsert de la misma clave al mismo tiempo.

> **Atención** `upsert` siempre guarda la clave que hay en tu struct, incluso con una clave `auto_increment`. Hacer upsert de `User { id: 0, ... }` crea una fila cuyo id es `0`. Usa `insert` para las filas nuevas de modelos con autoincremento, y `upsert` para claves naturales (slugs, códigos, UUID) o filas cuyo id ya conoces. En PostgreSQL, ten en cuenta también que hacer upsert de filas nuevas con ids explícitos no hace avanzar la secuencia de ids, así que un `insert` posterior puede chocar con ellas.

## Eliminar

```rust
let alice = User::get(&db, 1).await?;
let deleted: bool = alice.delete(&db).await?; // true: la fila existía

let deleted_again = User::delete_by_key(&db, 1).await?; // false: ya no existe
```

Ambos devuelven `true` si se eliminó una fila y `false` si no había nada que eliminar; que falte una fila no es un error.

Para eliminar todas las filas que coinciden con un filtro, usa `delete_many`, que devuelve el número de filas eliminadas:

```rust
let removed = User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
println!("removed {removed} inactive user(s)");
```

> **Atención** `User::delete_many().execute(&db)` sin filtro elimina **todas las filas** de la tabla.

Si otras tablas hacen referencia a las filas eliminadas con `on_delete = "cascade"`, la base de datos también las elimina (en SQLite y PostgreSQL; el motor en memoria no aplica las claves foráneas).

## Gestionar los errores

Todas las operaciones devuelven `memoire_orm::Result<T>`, un alias de `Result<T, memoire_orm::Error>`. Los errores están normalizados: el mismo fallo produce la misma variante de error en todos los backends, así que puedes hacer `match` sobre ella sin saber qué base de datos usas.

| Error | Cuándo ocurre |
|---|---|
| `Error::NotFound` | `get`, `one`, `update` o `reload` no encontraron ninguna fila |
| `Error::UniqueViolation { table, column }` | Ya existe un valor de clave primaria o `unique` |
| `Error::Constraint(message)` | Ha fallado otra restricción: `NOT NULL`, clave foránea, `CHECK` |
| `Error::UnexpectedNull { column }` | Se ha leído un `NULL` en un campo que no es `Option` |
| `Error::Decode { column, message }` | No se ha podido convertir un valor al tipo del campo |
| `Error::Conversion(message)` | No se ha podido convertir un valor (fuera de una columna concreta) |
| `Error::NoSuchTable(name)` | La tabla no existe |
| `Error::TableExists(name)` | `create_table` sobre una tabla que ya existe |
| `Error::Unsupported(message)` | El backend no puede hacer esto (por ejemplo, SQL sin procesar en el motor en memoria) |
| `Error::Conflict(table)` | Una transacción ha chocado con otra concurrente; se puede reintentar con seguridad |
| `Error::TransactionFinished` | La transacción ya se había confirmado o revertido |
| `Error::Migration { version, name, source }` | Ha fallado una migración; `source` contiene la causa |
| `Error::Pool(message)` | El pool de PostgreSQL no ha podido proporcionar una conexión |
| `Error::Database(error)` | Cualquier otro error del driver de la base de datos |

Tres ayudantes cubren las comprobaciones más habituales: `is_not_found()`, `is_unique_violation()` e `is_conflict()`.

```rust
let mut duplicate = User::new("alice@example.com", "Another Alice");
match duplicate.insert(&db).await {
    Ok(()) => println!("created #{}", duplicate.id),
    Err(Error::UniqueViolation { column, .. }) => println!("this {column} is already taken"),
    Err(other) => return Err(other),
}
```

```text
this email is already taken
```

> **Consejo** `Error` está marcado como `#[non_exhaustive]`: cuando hagas `match` sobre él, conserva una rama comodín (`Err(other) => ...`) como arriba. Todos los errores implementan `std::error::Error` y `Display`, así que también funcionan con `anyhow`, `thiserror` y `Box<dyn Error>`.

## Próximos pasos

- [Constructor de consultas](queries): encuentra exactamente las filas que necesitas.
- [Transacciones](transactions): agrupa varias escrituras en un único cambio de todo o nada.
- [Uniones y relaciones](relations): trabaja con tablas relacionadas.
