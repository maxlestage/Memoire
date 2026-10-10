# Introducción

Memoire ORM es un ORM rápido, con tipado seguro y asíncrono, escrito en Rust puro. Describes tus tablas como structs normales de Rust y Memoire te ofrece una API tipada para crear, consultar, actualizar y eliminar filas, con el mismo código en todas las bases de datos compatibles.

## ¿Qué es un ORM?

Un ORM (Object-Relational Mapper, o mapeador objeto-relacional) es una biblioteca que traduce entre las filas de una tabla de la base de datos y los valores de tu lenguaje de programación. En lugar de escribir cadenas SQL a mano y copiar tú mismo cada columna en un struct, escribes:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?; // ahora alice.id vale 1

    let adults = User::query()
        .filter(User::AGE.ge(18))
        .order_by(User::NAME.asc())
        .all(&db)
        .await?;
    println!("{adults:?}");
    Ok(())
}
```

Memoire convierte `User::query().filter(User::AGE.ge(18))` en el SQL adecuado para tu base de datos, enlaza el valor `18` de forma segura como parámetro, ejecuta la sentencia y vuelve a convertir cada fila devuelta en un `User`.

## ¿Por qué Memoire?

- **Consultas con tipado seguro.** Cada campo obtiene una constante de columna tipada (`User::AGE`, `User::EMAIL`, ...). `User::AGE.gt(18)` compila; `User::AGE.gt("18")` es un error de compilación. Las erratas en los nombres de columna también son errores de compilación.
- **Una API, varias bases de datos.** El mismo modelo y las mismas consultas funcionan con el motor en memoria, SQLite y PostgreSQL (consulta las páginas de cada backend para los demás). Cambiar de base de datos es cuestión de modificar una sola línea.
- **Rápido por diseño.** El SQL de las operaciones por clave primaria se genera mientras tu código compila, las filas se decodifican en una sola pasada sin clonar nada y las sentencias se preparan una vez y se guardan en caché. Consulta [Por qué es rápido](performance).
- **Todo incluido.** Joins, agregados, `GROUP BY`, paginación, upserts, inserciones masivas, transacciones, migraciones y SQL sin procesar.
- **Seguro.** El crate es `#![forbid(unsafe_code)]` y los valores siempre se envían como parámetros enlazados, nunca se pegan en el texto SQL.
- **Funciona en todas partes.** El motor en memoria es Rust puro, no necesita runtime y compila a WebAssembly (`wasm32-unknown-unknown`).

## Resumen de funcionalidades

| Área | Qué obtienes |
|---|---|
| Modelos | `#[derive(Model)]` con claves primarias, autoincremento, `unique`, índices, valores por defecto, claves foráneas, campos renombrados y omitidos |
| Consultas | Filtros tipados combinados con `&`, `\|`, `!`; ayudantes `LIKE` con escapado automático; `IN`, `BETWEEN`, `IS NULL` |
| Resultados | `all`, `first`, `one`, `count`, `exists`, `paginate`, `sum`, `avg`, `min`, `max`, proyecciones de columnas tipadas |
| Escrituras | `insert`, `insert_many` (inserción por lotes de varias filas), `update`, `upsert`, `delete`, `update_many` con `increment`/`decrement`, `delete_many` |
| Relaciones | `inner_join` y `left_join`, que devuelven tuplas como `(User, Post)` y `(User, Option<Post>)` |
| Informes | `group_by`, `having`, `count_all()`, agregados decodificados en tuplas o en structs con `#[derive(FromRow)]` |
| Redes de seguridad | Transacciones (revertidas automáticamente al destruirse), migraciones versionadas, errores normalizados |
| Vías de escape | SQL sin procesar con `params![...]`, acceso directo a la conexión SQLite o al pool de PostgreSQL |

## Backends

| Backend | Tipo | Feature de Cargo | Runtime asíncrono | SQL sin procesar |
|---|---|---|---|---|
| `MemoryDb` | Motor en memoria, Rust puro | `memory` (por defecto) | Cualquiera, o ninguno (`block_on`) | No |
| `SqliteDb` | Base de datos SQLite embebida (en archivo o en memoria) | `sqlite` | Cualquiera, o ninguno (`block_on`) | Sí |
| `PgPool` | Cliente PostgreSQL con pool de conexiones | `postgres` (`postgres-tls` para TLS) | Tokio | Sí |

> **Nota** Backends MySQL/MariaDB, Redis y MongoDB: consulta sus páginas, [MySQL y MariaDB](mysql), [Redis](redis) y [MongoDB](mongodb).

## ¿Qué backend debería usar?

| Quieres... | Usa | Por qué |
|---|---|---|
| Aprender Memoire, hacer prototipos, escribir pruebas unitarias rápidas | `MemoryDb` | Sin configuración, sin archivos, rapidísimo, nada que limpiar |
| Ejecutarlo en el navegador (WebAssembly) | `MemoryDb` | Es el único backend que compila a `wasm32` |
| Distribuir una aplicación de escritorio, una herramienta CLI o un servidor pequeño con datos en disco | `SqliteDb` | Un solo archivo, ningún servidor que instalar, muy rápido en una sola máquina |
| Ejecutar un servicio web con varias instancias o muchos escritores concurrentes | `PgPool` | Un servidor de base de datos de verdad con pool de conexiones, acceso por red y una concurrencia sólida |

> **Consejo** Como todos los backends comparten la misma API, una configuración habitual es `SqliteDb` o `PgPool` en producción y `MemoryDb` en las pruebas unitarias. Escribe tus funciones contra los traits `Executor` o `Database` y pásales la base de datos que prefieras.

## Requisitos y licencia

- **Rust 1.88 o posterior** (la versión mínima de Rust compatible, o MSRV).
- **Licencia:** MIT OR Apache-2.0, a tu elección.
- La feature `sqlite` compila SQLite desde el código fuente, así que necesita un compilador de C en tiempo de compilación. No hay que instalar nada en tiempo de ejecución.

## Próximos pasos

- [Instalación](installation): añade Memoire a tu proyecto y elige tus features.
- [Inicio rápido](quick-start): construye una pequeña aplicación paso a paso en unos diez minutos.
- [Modelos](models): aprende todo lo que puede hacer `#[derive(Model)]`.
