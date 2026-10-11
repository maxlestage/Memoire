# Instalación

Esta página explica cómo añadir Memoire ORM a un proyecto de Rust, qué features de Cargo debes activar para tu base de datos y qué runtime asíncrono necesitas.

## Añadir el crate

El crate se llama `memoire-orm` en crates.io y en Rust lo importas como `memoire_orm`.

La forma más rápida es `cargo add`. Elige la línea que corresponda a la base de datos que quieres usar:

```bash
# Solo el motor en memoria (la feature por defecto)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire

# SQLite
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features sqlite

# PostgreSQL (necesita el runtime Tokio)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread

# PostgreSQL sobre TLS (bases de datos alojadas como Heroku Postgres)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls

# MySQL / MariaDB (añade "mysql-tls" para TLS)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mysql

# Redis (añade "redis-tls" para las URL rediss://)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features redis

# MongoDB
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mongodb
```

Todos los backends de servidor (PostgreSQL, MySQL/MariaDB, Redis, MongoDB) también necesitan Tokio: `cargo add tokio --features macros,rt-multi-thread`.

También puedes editar `Cargo.toml` a mano:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

> **Nota** Tokio solo es *obligatorio* para los backends de servidor. Los ejemplos de esta documentación usan `#[tokio::main]` porque es la configuración más habitual, pero los backends en memoria y SQLite funcionan sin él (consulta "Runtime asíncrono" más abajo).

## Features de Cargo

| Feature | Activada por defecto | Qué añade |
|---|---|---|
| `memory` | sí | `MemoryDb`, el motor en memoria en Rust puro (también funciona en `wasm32`) |
| `sqlite` | no | `SqliteDb`, una base de datos SQLite embebida (SQLite se compila dentro) |
| `postgres` | no | `PgPool`, un pool de conexiones PostgreSQL (requiere Tokio) |
| `postgres-tls` | no | Conexiones PostgreSQL cifradas con `rustls` (`sslmode=require`, `verify-full`...), necesarias para Heroku Postgres y la mayoría de bases de datos alojadas. Implica `postgres` |
| `mysql` | no | `MySqlPool`, un pool de conexiones MySQL y MariaDB (requiere Tokio) |
| `mysql-tls` | no | Conexiones MySQL cifradas con `rustls` (`?require_ssl=true`). Implica `mysql` |
| `redis` | no | `RedisDb`, tablas almacenadas en Redis (requiere Tokio; incluye `memory`) |
| `redis-tls` | no | URL `rediss://` con `rustls`; añade `#insecure` al final para los certificados autofirmados. Implica `redis` |
| `mongodb` | no | `MongoDb`, colecciones en MongoDB a través del driver oficial (requiere Tokio; incluye `memory`) |
| `encryption` | no | Archivos `MemoryDb` cifrados (AES-256-GCM, clave derivada de una contraseña con Argon2id). Implica `memory` |
| `chrono` | no | Columnas de fecha y hora: `NaiveDateTime`, `DateTime<Utc>`, `NaiveDate`, `NaiveTime` |
| `uuid` | no | Columnas `uuid::Uuid` |
| `json` | no | Columnas JSON: `serde_json::Value` y `Json<T>` para cualquier tipo `serde` |
| `full` | no | Todo lo anterior |

Cada backend se describe en su propia página: [Motor en memoria](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL y MariaDB](mysql), [Redis](redis) y [MongoDB](mongodb).

Las features se suman, así que puedes combinarlas libremente:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["postgres", "chrono", "uuid", "json"] }
```

Si no necesitas el motor en memoria en absoluto, desactiva las features por defecto:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", default-features = false, features = ["sqlite"] }
```

> **Consejo** Mantén `memory` activada aunque en producción uses otra base de datos: hace que las pruebas unitarias sean rápidas y no requieran configuración. Puedes activarla solo para las pruebas con `[dev-dependencies]`.

### Crates adicionales para los tipos de columna

Memoire no reexporta `chrono`, `uuid` ni `serde_json`. Para poder *nombrar* esos tipos en tus structs, añade también esos crates a tu propio `Cargo.toml`:

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite", "chrono", "uuid", "json"] }
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }         # "v4" para Uuid::new_v4()
serde = { version = "1", features = ["derive"] }    # para Json<T>
serde_json = "1"
```

## Runtime asíncrono

Cada llamada a la base de datos en Memoire es una `async fn`, así que le haces `.await`. El runtime que necesitas depende del backend:

| Backend | Runtime necesario |
|---|---|
| `MemoryDb` | Cualquier ejecutor (Tokio, async-std, smol...) o ninguno con `memoire_orm::block_on` |
| `SqliteDb` | Cualquier ejecutor o ninguno con `memoire_orm::block_on` |
| `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb` | Tokio (su E/S de red y sus pools de conexiones se ejecutan sobre Tokio) |

Los backends en memoria y SQLite nunca esperan a la red: sus futures terminan en cuanto se sondean (poll). Por eso Memoire incluye un pequeño ayudante, `memoire_orm::block_on`, que ejecuta un future en el hilo actual. Resulta muy práctico en programas síncronos, herramientas de línea de comandos, pruebas y WebAssembly:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
struct Note {
    id: i64,
    text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;
        Note { id: 0, text: "hello".into() }.insert(&db).await?;
        println!("{} note(s)", Note::count(&db).await?);
        Ok(())
    })
}
```

> **Atención** No uses `block_on` con `PgPool`, `MySqlPool`, `RedisDb` ni `MongoDb`: necesitan un runtime de Tokio en ejecución. Usa `#[tokio::main]` (o `#[tokio::test]` en las pruebas).

## WebAssembly

El backend `memory` compila a `wasm32-unknown-unknown` sin ninguna configuración adicional:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

En el navegador puedes ejecutar los futures con `memoire_orm::block_on` o con `wasm-bindgen-futures`. Los demás backends (SQLite, PostgreSQL, MySQL, Redis y MongoDB) no están disponibles en `wasm32`. Consulta [Motor en memoria](memory) para más detalles.

## Comprueba que todo funciona

Crea un proyecto nuevo y ejecuta un programa mínimo:

```bash
cargo new hello-memoire
cd hello-memoire
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread
```

Sustituye `src/main.rs` por:

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "greetings")]
struct Greeting {
    id: i64,
    text: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Greeting>().await?;

    let mut hello = Greeting { id: 0, text: "Hello, Memoire!".into() };
    hello.insert(&db).await?;

    let found = Greeting::get(&db, hello.id).await?;
    println!("#{} says: {}", found.id, found.text);
    Ok(())
}
```

Después, `cargo run` debería imprimir `#1 says: Hello, Memoire!`.

## Requisitos

| Requisito | Valor |
|---|---|
| Versión mínima de Rust (MSRV) | 1.88 |
| Herramientas en tiempo de compilación | Un compilador de C cuando la feature `sqlite` está activada (SQLite se compila desde el código fuente) |
| Dependencias en tiempo de ejecución | Ninguna para memoria y SQLite; un servidor accesible para PostgreSQL, MySQL/MariaDB, Redis o MongoDB |
| Licencia | Propietaria (© Maxime Nathan Lestage, todos los derechos reservados): uso libre como dependencia, ver [LICENSE](https://maxlestage.github.io/Memoire/LICENSE.txt) |

## Próximos pasos

- [Inicio rápido](quick-start): construye una pequeña aplicación desde cero.
- [Modelos](models): describe tus tablas.
- Configuración de cada backend: [SQLite](sqlite), [PostgreSQL](postgres), [MySQL y MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).
