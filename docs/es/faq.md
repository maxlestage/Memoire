# Preguntas frecuentes

Respuestas a las preguntas que más se hacen sobre Memoire ORM. Cada respuesta enlaza con la página que contiene todos los detalles.

## ¿Qué bases de datos son compatibles?

Seis backends, todos con la misma API y superando la misma batería de pruebas:

| Backend | Feature | Página |
|---|---|---|
| `MemoryDb` | `memory` (por defecto) | [Motor en memoria](memory) |
| `SqliteDb` | `sqlite` | [SQLite](sqlite) |
| `PgPool` | `postgres` (`postgres-tls` para TLS) | [PostgreSQL](postgres) |
| `MySqlPool` | `mysql` (`mysql-tls` para TLS) | [MySQL y MariaDB](mysql) |
| `RedisDb` | `redis` (`redis-tls` para TLS) | [Redis](redis) |
| `MongoDb` | `mongodb` | [MongoDB](mongodb) |

## ¿Qué runtime asíncrono necesito?

Los backends de servidor (PostgreSQL, MySQL/MariaDB, Redis y MongoDB) necesitan **Tokio**. El motor en memoria y SQLite nunca esperan a la red, así que sus futures funcionan con cualquier ejecutor (Tokio, async-std, smol...) o sin ningún runtime. Consulta [Instalación](installation).

## ¿Puedo usar Memoire desde código síncrono?

Sí, con el motor en memoria y con SQLite. Envuelve las llamadas en `memoire_orm::block_on`, que ejecuta un future en el hilo actual:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "settings")]
pub struct Setting {
    #[memoire(primary_key)]
    pub key: String,
    pub value: String,
}

fn read_setting(db: &SqliteDb, key: &str) -> memoire_orm::Result<Option<Setting>> {
    memoire_orm::block_on(Setting::find(db, key))
}
```

Para los backends de servidor, usa un runtime de Tokio (`#[tokio::main]`, o `tokio::runtime::Runtime::block_on` en código síncrono).

## ¿Por qué los ids son `i64` y por qué escribo `id: 0`?

`i64` es el tipo entero más grande que todas las bases de datos compatibles almacenan de forma nativa (`BIGINT`, el `INTEGER` de SQLite), así que en la práctica nunca se desborda y encaja limpiamente en todas partes. Las claves `i32` e `i16` también funcionan. Los enteros sin signo de 64 bits no se admiten porque la mayoría de las bases de datos no tienen un tipo sin signo de 64 bits portable.

En una clave `auto_increment`, es la base de datos quien elige el valor: el `0` que escribes es un marcador que `insert` ignora, y el id generado se escribe de vuelta en tu struct. Consulta [Modelos](models).

## ¿Cómo hago que una columna admita nulos?

Haz que el campo sea un `Option`: `pub age: Option<i32>`. `None` se guarda como `NULL` y la columna se crea sin `NOT NULL`. Para encontrar las filas en las que está vacía, usa `User::AGE.is_null()`; `eq(None)` no compila a propósito, porque en SQL `= NULL` nunca coincide con nada. Consulta [Constructor de consultas](queries).

## ¿`LIKE` distingue mayúsculas y minúsculas?

Sí: `like`, `contains`, `starts_with` y `ends_with` distinguen mayúsculas y minúsculas en los seis backends (Memoire configura así SQLite y las tablas de MySQL que crea para que todos los backends coincidan). Usa `ilike` o `icontains` para no distinguirlas. En SQLite, la coincidencia sin distinguir mayúsculas solo cubre las letras ASCII; los demás backends también gestionan las letras acentuadas. Consulta [Constructor de consultas](queries).

## ¿Por qué los valores NULL aparecen al principio en una base de datos y al final en otra?

Las bases de datos no se ponen de acuerdo: PostgreSQL, Redis y el motor en memoria colocan los NULL al final en orden ascendente; SQLite, MySQL y MongoDB los colocan al principio. Indica lo que quieres y Memoire lo genera igual en todas partes: `order_by(User::AGE.asc().nulls_last())` o `.nulls_first()`.

## ¿Admite Memoire claves primarias compuestas?

No: un modelo tiene exactamente un campo de clave primaria. Para las tablas de enlace, añade una clave sustituta `id` y, si quieres que la base de datos rechace los pares duplicados, crea un índice único sobre ambas columnas con una [migración](migrations) SQL (o con `db.database()` en MongoDB).

## ¿Cómo guardo un enum?

Hay dos opciones:

1. **Sencilla:** guarda un campo `String` o `i32` y convierte de y hacia tu enum en tu propio código.
2. **Tipada:** implementa los traits `SqlType`, `ToValue`, `IntoValue` y `FromValue` para tu enum y úsalo directamente como tipo de campo. Los filtros siguen comprobándose por tipo: `Article::STATUS.eq(Status::Published)`.

La segunda opción se explica paso a paso en [Modelos](models). En PostgreSQL también puedes usar un tipo enum nativo con un campo `String` y `#[memoire(sql_type = "mood")]`, después de crear el tipo en una migración.

## ¿Cómo guardo JSON?

Activa la feature `json`. Usa `serde_json::Value` para JSON de forma libre, o `Json<T>` para cualquier tipo que implemente `serde::Serialize` y `serde::Deserialize`. Se guarda como `JSONB` en PostgreSQL, como `JSON` en MySQL, como texto en SQLite, como JSON incrustado en Redis y como documentos incrustados en MongoDB. Consulta [Modelos](models).

## ¿Puedo usar UUID como claves primarias?

Sí. Activa la feature `uuid`, añade `uuid` con su feature `v4` a tus dependencias y genera tú mismo la clave:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "documents")]
pub struct Document {
    #[memoire(primary_key)]
    pub id: uuid::Uuid,
    pub title: String,
}

async fn create(db: &MemoryDb, title: &str) -> memoire_orm::Result<Document> {
    let mut doc = Document { id: uuid::Uuid::new_v4(), title: title.into() };
    doc.insert(db).await?;
    Ok(doc)
}
```

Los UUID se guardan como `UUID` en PostgreSQL, como `CHAR(36)` en MySQL, como texto en SQLite y Redis, y como binario BSON (subtipo 4) en MongoDB.

## ¿Qué tipo debería usar para las marcas de tiempo?

Activa la feature `chrono`. Usa `chrono::DateTime<Utc>` para instantes concretos (se guardan como `TIMESTAMPTZ` en PostgreSQL), `NaiveDateTime` para una fecha y hora sin zona horaria, `NaiveDate` para fechas y `NaiveTime` para horas del día. Dos detalles:

- La precisión depende del backend: nanosegundos en el motor en memoria, SQLite y Redis, microsegundos en PostgreSQL y MySQL, milisegundos en MongoDB. Si comparas un valor con lo que lees de vuelta, trúncalo antes; por ejemplo, `Utc::now().trunc_subsecs(3)` (de `chrono::SubsecRound`) para código que deba funcionar en todas partes.
- Las comparaciones y la ordenación funcionan en todos los backends, incluidos SQLite y Redis, donde las marcas de tiempo se guardan como texto de ancho fijo.

## ¿Tengo que configurar un pool de conexiones?

No. `PgPool` y `MySqlPool` *son* pools de conexiones: créalos una sola vez y comparte clones de ellos (`PgPool::connect_with(url, size)` define el tamaño del pool de PostgreSQL). `MongoDb` usa el pool propio del driver, `RedisDb` una conexión multiplexada que se reconecta automáticamente, `SqliteDb` una única conexión compartida, y `MemoryDb` no necesita ninguna.

## ¿Cómo pruebo el código que usa Memoire?

Escribe tus funciones contra el trait `Executor` (o `Database`) y dales un `MemoryDb` nuevo en cada prueba. Se crea en microsegundos, no hay que limpiar nada después y aplica las mismas claves y restricciones de unicidad que las bases de datos SQL. Para el comportamiento que depende de tu base de datos de producción (borrados en cascada por claves foráneas, SQL sin procesar, transacciones en MongoDB), añade algunas pruebas de integración contra esa base de datos, o con `SqliteDb::open_in_memory()` para las funcionalidades de SQL. Consulta [Motor en memoria](memory).

## ¿Funciona en WebAssembly?

Sí: el motor en memoria compila a `wasm32-unknown-unknown` sin ninguna configuración adicional, y ejecuta sus futures con `memoire_orm::block_on` o con `wasm-bindgen-futures`. Los demás backends no están disponibles ahí. Consulta [Motor en memoria](memory).

## ¿Cómo puedo ver el SQL que ejecuta Memoire?

Llama a `to_sql(Dialect::Postgres)` (o `Dialect::Sqlite`, `Dialect::MySql`) sobre cualquier constructor de consulta, de update o de delete; `to_sql_with_params` devuelve además los valores enlazados. Las sentencias que usan `find`, `insert`, `update`, `delete` y `upsert` están en `User::SCHEMA.sql(dialect)`. Redis y MongoDB no usan SQL; sus páginas explican qué hacen en su lugar. Consulta [Constructor de consultas](queries).

## ¿Memoire crea o actualiza mis tablas automáticamente?

`db.create_table::<User>()` crea una tabla a partir de un modelo, pero Memoire nunca modifica por su cuenta una tabla existente. Para hacer evolucionar un esquema, escribe [migraciones](migrations): están versionadas, se ejecutan en transacciones y quedan registradas para que cada una se ejecute una sola vez.

## ¿Memoire usa código `unsafe`?

No. El crate se compila con `#![forbid(unsafe_code)]`, así que no puede contener ningún bloque `unsafe`. Todos los valores se envían a las bases de datos como parámetros enlazados, nunca se pegan en el texto SQL.

## ¿Cómo puedo contribuir?

Las contribuciones de cualquier tamaño son bienvenidas: informes de bugs, correcciones de la documentación, traducciones y nuevas funcionalidades.

1. Haz un fork de `https://maxlestage.github.io/Memoire/` y clónalo.
2. Ejecuta las pruebas: `cargo test --workspace --features memory,sqlite`. Para incluir los backends de servidor, activa todas las features y define `MEMOIRE_PG_URL`, `MEMOIRE_MYSQL_URL`, `MEMOIRE_REDIS_URL` y `MEMOIRE_MONGO_URL` con servidores de prueba (MongoDB como replica set).
3. Antes de abrir una pull request, ejecuta `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` y las pruebas.
4. Añade las pruebas de comportamiento a `crates/memoire-orm/tests/common/mod.rs`, para que se ejecuten en todos los backends.
5. La documentación está en `docs/en`, `docs/fr` y `docs/es`: actualiza las tres, o abre una issue para que alguien pueda traducirla.

Las pull requests a `master` se fusionan automáticamente en cuanto la CI está en verde; abre la tuya como borrador (draft) o añade la etiqueta `no-auto-merge` si quieres una revisión antes. El archivo `CONTRIBUTING.md` del repositorio tiene todos los detalles.

## Próximos pasos

- [Inicio rápido](quick-start): construye una pequeña aplicación en diez minutos.
- [Chuleta](cheatsheet): todas las operaciones en una sola página.
- [Modelos](models): todo sobre `#[derive(Model)]`.
