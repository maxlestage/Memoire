# SQLite

`SqliteDb` guarda tus datos en un único archivo usando SQLite, la base de datos más desplegada del mundo. No hay ningún servidor que instalar ni que ejecutar: SQLite se compila dentro de tu programa. Encaja muy bien en aplicaciones de escritorio, herramientas de línea de comandos, servicios web pequeños y medianos, y pruebas que necesitan una base de datos SQL de verdad.

## Activar el backend

```bash
cargo add memoire-orm --features sqlite
```

La feature `sqlite` compila SQLite desde el código fuente (a través del crate `rusqlite`), así que necesitas un compilador de C en tiempo de compilación (GCC, Clang o MSVC, normalmente ya instalado). No hay que instalar nada en las máquinas que ejecutan tu programa.

## Abrir una base de datos

| Constructor | Úsalo para |
|---|---|
| `SqliteDb::open("app.db")` | Un archivo de base de datos, que se crea si no existe |
| `SqliteDb::open_in_memory()` | Una base de datos privada y temporal que desaparece al destruirse (pruebas) |
| `SqliteDb::from_connection(conn)` | Una `rusqlite::Connection` existente que hayas configurado tú |

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "bookmarks")]
pub struct Bookmark {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub url: String,
    pub title: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("bookmarks.db")?;
    db.create_table_if_not_exists::<Bookmark>().await?;

    let mut rust = Bookmark { id: 0, url: "https://www.rust-lang.org".into(), title: "Rust".into() };
    rust.insert(&db).await?;
    println!("{} bookmark(s) saved", Bookmark::count(&db).await?);
    Ok(())
}
```

La apertura es síncrona (sin `.await`) y devuelve un `memoire_orm::Result`.

`from_connection` está pensado para configuraciones especiales, como abrir un archivo en solo lectura o usar una extensión de cifrado. Necesita `rusqlite` en tus propias dependencias, con la misma versión mayor que Memoire (`rusqlite = "0.40"`):

```rust
fn read_only(path: &str) -> memoire_orm::Result<SqliteDb> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(Error::database)?;
    SqliteDb::from_connection(conn)
}
```

## Lo que Memoire configura por ti

| Ajuste | `open` (archivo) | `open_in_memory` | `from_connection` | Por qué |
|---|---|---|---|---|
| `journal_mode = WAL` | sí | | | Los lectores no bloquean al escritor ni al revés; escrituras más rápidas |
| `synchronous = NORMAL` | sí | | | Seguro con WAL y mucho más rápido que el valor por defecto |
| `busy_timeout = 5000` | sí | | | Espera hasta 5 segundos a que se libere un bloqueo en lugar de fallar al instante |
| `foreign_keys = ON` | sí | sí | sí | Aplica las restricciones `references` (SQLite las desactiva por defecto) |
| `case_sensitive_like = ON` | sí | sí | sí | Hace que `LIKE` se comporte como en PostgreSQL y en el motor en memoria |
| Caché de sentencias preparadas | 256 | 256 | 256 | Cada sentencia SQL distinta se compila una vez y se reutiliza |

> **Nota** Con WAL, SQLite mantiene dos archivos auxiliares junto a tu base de datos mientras está abierta (`app.db-wal` y `app.db-shm`). Son normales; copia los tres archivos si haces una copia de seguridad de una base de datos en uso, o usa la API de copias de seguridad de SQLite a través de `with_connection`.

## Conexiones y concurrencia

Un `SqliteDb` mantiene **una** conexión. Clonar un `SqliteDb` es barato y comparte esa conexión, así que puedes dar clones a todas las tareas o hilos. Las sentencias se ejecutan de una en una sobre ella, que es como SQLite rinde mejor en escritura.

Si tienes muchos lectores concurrentes sobre una base de datos en archivo, puedes abrir varios `SqliteDb` sobre el mismo archivo: gracias a WAL, leen en paralelo mientras uno de ellos escribe.

## Comportamiento asíncrono

SQLite se ejecuta dentro de tu proceso y nunca espera a la red, así que los futures de `SqliteDb` terminan en cuanto se ejecutan. Funcionan con cualquier runtime asíncrono, o sin ninguno mediante `memoire_orm::block_on`:

```rust
fn count_bookmarks() -> memoire_orm::Result<u64> {
    let db = SqliteDb::open("bookmarks.db")?;
    memoire_orm::block_on(Bookmark::count(&db))
}
```

Dentro de Tokio, una consulta se ejecuta directamente en el hilo de trabajo que la espera. La mayoría de las consultas tardan microsegundos, así que no hay problema. Para alguna consulta pesada ocasional (un informe grande, un `VACUUM`), pásala a un hilo bloqueante para que las demás tareas sigan ejecutándose:

```rust
async fn heavy_report(db: &SqliteDb) -> memoire_orm::Result<u64> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || memoire_orm::block_on(Bookmark::count(&db)))
        .await
        .expect("the report task panicked")
}
```

## Cómo se almacenan los valores

SQLite solo tiene unos pocos tipos de almacenamiento, así que Memoire convierte los tipos de Rust más ricos:

| Tipo de Rust | Se almacena como | Ejemplo |
|---|---|---|
| `bool` | `INTEGER` | `0` o `1` |
| Enteros (de `i8` a `i64`, de `u8` a `u32`) | `INTEGER` | `42` |
| `f32`, `f64` | `REAL` | `3.5` |
| `String` | `TEXT` | `'hello'` |
| `Vec<u8>` | `BLOB` | bytes sin procesar |
| `uuid::Uuid` | `TEXT` | `'67e55044-10b1-426f-9247-bb680e5fe0c8'` |
| `chrono::NaiveDateTime` | `TEXT` | `'2026-10-10 14:30:00.250'` |
| `chrono::DateTime<Utc>` | `TEXT` (RFC 3339) | `'2026-10-10T14:30:00.250Z'` |
| `chrono::NaiveDate` | `TEXT` | `'2026-10-10'` |
| `chrono::NaiveTime` | `TEXT` | `'14:30:00'` |
| `serde_json::Value`, `Json<T>` | `TEXT` | `'{"theme":"dark"}'` |

Al leer, la conversión inversa es automática, así que tus structs siempre reciben los tipos de Rust adecuados.

> **Nota** Las fechas y horas se almacenan como texto ISO 8601 de ancho fijo (precisión de nanosegundos, sufijo `Z` para `DateTime<Utc>`), así que el orden del texto coincide con el orden temporal: `gt`, `between` y `order_by` funcionan en todas las columnas de fecha y hora.

### Claves autoincrementales

Una clave `auto_increment` se convierte en `INTEGER PRIMARY KEY`, el row id integrado de SQLite. Las filas nuevas reciben el id más alto existente más uno, así que el id de la última fila insertada puede reutilizarse si esa fila se elimina. Si los ids nunca deben reutilizarse, conserva las filas eliminadas (por ejemplo, con un indicador `active`) en lugar de borrarlas.

## Transacciones

`db.begin()` reserva la conexión para la transacción hasta que se confirma, se revierte o se destruye. Por tanto, las transacciones se ejecutan de una en una y nunca entran en conflicto.

> **Atención** Mientras una transacción está abierta, una consulta lanzada sobre el propio manejador `SqliteDb` (`&db` en lugar de `&tx`) espera hasta que termine la transacción. Dentro de la misma tarea, eso es un interbloqueo (deadlock). Dentro de una transacción, pasa siempre `&tx`.

Las transacciones también son la clave para conseguir escrituras masivas rápidas en SQLite. Fuera de una transacción, cada escritura es una transacción en sí misma, con su propio trabajo de bloqueo y de journal; dentro de una, ese coste se paga una sola vez, en el commit:

```rust
async fn import(db: &SqliteDb, bookmarks: &[Bookmark]) -> memoire_orm::Result<u64> {
    let tx = db.begin().await?;
    let n = Bookmark::insert_many(&tx, bookmarks).await?;
    tx.commit().await?;
    Ok(n)
}
```

## Errores

Los errores de SQLite se traducen a las variantes de error comunes de Memoire:

| Situación en SQLite | Error de Memoire |
|---|---|
| `UNIQUE constraint failed: users.email` (o una clave primaria duplicada) | `Error::UniqueViolation { table: "users", column: "email" }` |
| Falla una restricción `NOT NULL`, `FOREIGN KEY` o `CHECK` | `Error::Constraint(message)` |
| `no such table: users` | `Error::NoSuchTable("users")` |
| `table users already exists` | `Error::TableExists("users")` |
| Cualquier otra cosa | `Error::Database(...)` con el error original de `rusqlite` |

## Lo que debes saber

- **`LIKE` distingue mayúsculas y minúsculas** (Memoire activa `case_sensitive_like`). `ilike` e `icontains` no las distinguen solo para las letras ASCII: `icontains("é")` no coincide con `"É"`.
- **Orden de los NULL:** SQLite coloca los NULL al principio en orden ascendente, a diferencia de PostgreSQL y del motor en memoria. Usa `nulls_first()`/`nulls_last()` para obtener resultados portables.
- **El SQL sin procesar** usa marcadores `?`. Consulta [SQL sin procesar](raw-sql).
- **Acceso directo:** `db.with_connection(|conn| ...)` te da la `rusqlite::Connection` para todo lo que Memoire no cubre (copias de seguridad, pragmas, funciones personalizadas). Consulta [SQL sin procesar](raw-sql).

## Próximos pasos

- [PostgreSQL](postgres): cuando necesitas un servidor de base de datos.
- [Transacciones](transactions): cómo se comparan las transacciones de SQLite con las de los demás backends.
- [Por qué es rápido](performance): consejos para las escrituras masivas.
