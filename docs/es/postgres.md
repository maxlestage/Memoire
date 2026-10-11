# PostgreSQL

`PgPool` conecta Memoire a un servidor PostgreSQL mediante un pool de conexiones reutilizables. Es el backend preferido para los servicios web: varias instancias de la aplicación pueden compartir una misma base de datos, muchas peticiones pueden ejecutarse al mismo tiempo y es fácil encontrar ofertas alojadas (Heroku Postgres, Amazon RDS, Supabase, Neon...).

## Activar el backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread
```

Para las bases de datos alojadas, que exigen conexiones cifradas, usa en su lugar la feature `postgres-tls` (que incluye `postgres`):

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls
```

El cliente es Rust puro: `tokio-postgres` para el protocolo, `deadpool` para el pool y, con `postgres-tls`, `rustls` para el cifrado. No hace falta ninguna biblioteca de C ni OpenSSL.

> **Nota** `PgPool` necesita el runtime **Tokio**: usa `#[tokio::main]` en tu programa y `#[tokio::test]` en las pruebas. `memoire_orm::block_on` no funciona con PostgreSQL.

## Conectarse

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "products")]
pub struct Product {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = PgPool::connect("postgres://app:secret@localhost:5432/shop").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);
    Ok(())
}
```

La cadena de conexión puede ser una URL, `postgres://user:password@host:port/database?option=value`, o el formato `key=value`: `host=localhost user=app password=secret dbname=shop`.

| Constructor | Tamaño del pool | Notas |
|---|---|---|
| `PgPool::connect(url)` | Hasta 4 × el número de núcleos de CPU | Abre una conexión de inmediato, así que una URL o una contraseña incorrectas fallan al instante |
| `PgPool::connect_with(url, max_size)` | Hasta `max_size` | Igual, con tu propio límite |
| `PgPool::connect_with_rustls(url, max_size, config)` | Hasta `max_size` | TLS con tu propia configuración de `rustls` (feature `postgres-tls`) |
| `PgPool::from_pool(pool)` | Lo que hayas configurado | Envuelve un `deadpool_postgres::Pool` que hayas construido tú |

Clonar un `PgPool` es barato: los clones comparten el mismo pool. Créalo una sola vez al arrancar y pasa clones (o referencias) de un lado a otro.

## Elegir el tamaño del pool

Cada conexión es un proceso en el servidor de base de datos, así que más no siempre es mejor:

- El valor por defecto (4 × los núcleos de CPU de la máquina de la aplicación) es adecuado para una única instancia de la aplicación que habla con su propio servidor.
- Los planes alojados limitan el número de conexiones, a veces a solo unas pocas decenas. Reparte ese límite entre las instancias de tu aplicación y reserva unas cuantas conexiones para las herramientas de mantenimiento: `PgPool::connect_with(&url, 10)`.
- Cuando todas las conexiones están ocupadas, las nuevas consultas esperan hasta que una quede libre.

Las conexiones se abren bajo demanda, hasta el máximo, y se reutilizan.

## Conexiones cifradas (TLS)

Con la feature `postgres-tls`, `connect` y `connect_with` respetan el parámetro `sslmode` de la cadena de conexión, igual que el cliente oficial `psql`:

| `sslmode` | Comportamiento |
|---|---|
| `disable` | Texto plano, nunca cifrado |
| `prefer` (por defecto) | Usa TLS si el servidor lo admite; si no, texto plano |
| `require` | Siempre cifrado; el certificado del servidor **no** se verifica |
| `verify-ca`, `verify-full` | Siempre cifrado, y el certificado del servidor se verifica con los certificados raíz de Mozilla |

```rust
async fn connect_securely() -> memoire_orm::Result<PgPool> {
    PgPool::connect("postgres://app:secret@db.example.com/shop?sslmode=verify-full").await
}
```

Sin la feature `postgres-tls`, las conexiones van en texto plano, y una cadena de conexión con `sslmode=require`, `verify-ca` o `verify-full` falla con `Error::Unsupported`, que te indica que actives la feature.

> **Consejo** Usa `verify-full` cuando el certificado de tu proveedor esté firmado por una autoridad pública: te protege frente a servidores impostores. Muchas bases de datos alojadas (entre ellas Heroku Postgres) usan certificados que no ha firmado ninguna autoridad pública; para ellas usa `require`, que sigue cifrando todo el tráfico.

### Certificados personalizados

Si tu servidor usa un certificado firmado por tu propia autoridad, construye un `rustls::ClientConfig` que confíe en ella y pásalo a `connect_with_rustls`. Necesitas `rustls` (0.23) en tus dependencias:

```toml
[dependencies]
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
```

```rust
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

async fn connect_with_my_ca(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from_pem_file("my-ca.pem")?)?;

    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(PgPool::connect_with_rustls(url, 10, config).await?)
}
```

Los certificados de cliente funcionan de la misma forma, con `with_client_auth_cert` en lugar de `with_no_client_auth`.

### Heroku Postgres

Heroku pone la cadena de conexión de tu base de datos en la variable de entorno `DATABASE_URL`, y Heroku Postgres solo acepta conexiones cifradas. Con la feature `postgres-tls`, conectarse es cuestión de una línea:

```rust
async fn heroku() -> Result<PgPool, Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")?;
    // `prefer` (el valor por defecto) ya usa TLS; `sslmode=require` lo hace obligatorio.
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    Ok(PgPool::connect_with(&url, 10).await?)
}
```

Consulta [Desplegar en Heroku desde el móvil](deploy-heroku-mobile) para ver un despliegue completo.

### Construir tu propio pool

Para tener un control total sobre el pool (tiempos de espera, reciclado, otra pila TLS), construye un `deadpool_postgres::Pool` y envuélvelo con `from_pool`. Para ello necesitas `deadpool-postgres` (0.14) y `tokio-postgres` (0.7) en tus dependencias:

```rust
use std::time::Duration;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};

fn custom_pool(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager = Manager::from_config(
        config,
        tokio_postgres::NoTls,
        ManagerConfig { recycling_method: RecyclingMethod::Fast },
    );
    let pool = Pool::builder(manager)
        .max_size(8)
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(Duration::from_secs(5))) // falla en lugar de esperar para siempre
        .build()?;
    Ok(PgPool::from_pool(pool))
}
```

A diferencia de `connect`, `from_pool` no abre ninguna conexión de inmediato: los errores aparecen en la primera consulta.

## Sentencias preparadas

Cada sentencia que ejecuta Memoire se prepara una vez por conexión y se guarda en una caché, así que PostgreSQL solo la analiza y planifica una vez. Las operaciones por clave primaria reutilizan el SQL generado en tiempo de compilación, y las consultas del constructor con la misma forma producen el mismo texto SQL, de modo que comparten una entrada de la caché. Para proteger los procesos de larga duración, la caché de una conexión se vacía cuando supera las 1024 sentencias.

## Tipos

### Cómo se crean las columnas

| Tipo de Rust | Tipo de PostgreSQL |
|---|---|
| `bool` | `BOOLEAN` |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INTEGER` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `REAL` |
| `f64` | `DOUBLE PRECISION` |
| `String` | `TEXT` |
| `Vec<u8>` | `BYTEA` |
| `uuid::Uuid` | `UUID` |
| `chrono::NaiveDateTime` | `TIMESTAMP` |
| `chrono::DateTime<Utc>` | `TIMESTAMPTZ` |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME` |
| `serde_json::Value`, `Json<T>` | `JSONB` |
| Clave `auto_increment` | `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` (para `i64`) |

### Cómo se leen los valores

| Tipo de PostgreSQL | Se lee como |
|---|---|
| `BOOLEAN` | `bool` |
| `SMALLINT`, `INTEGER`, `BIGINT` | Cualquier tipo entero en el que quepa el valor |
| `REAL`, `DOUBLE PRECISION` | `f32`, `f64` |
| `NUMERIC` | `String` (exacto) o `f64` |
| `TEXT`, `VARCHAR`, `CHAR`, `NAME`, `citext`, tipos enum | `String` |
| `BYTEA` | `Vec<u8>` |
| `UUID` | `uuid::Uuid` con la feature `uuid`; si no, `String` |
| `TIMESTAMP`, `TIMESTAMPTZ`, `DATE`, `TIME` | Tipos de `chrono` (requiere la feature `chrono`) |
| `JSON`, `JSONB` | `serde_json::Value` o `Json<T>` con la feature `json`; si no, `String` |

`NUMERIC` (decimales exactos, habituales para importes de dinero) no tiene un tipo nativo en Rust, así que Memoire lo lee como texto sin ningún redondeo: decodifícalo en un `String` para conservar todos los dígitos, o en un `f64` para hacer cálculos. Escribir un `String` o un `f64` en una columna `NUMERIC` también funciona. Los demás tipos (`INTERVAL`, arrays, tipos geométricos...) no se admiten directamente: conviértelos a texto en [SQL sin procesar](raw-sql), por ejemplo `duration::text`.

> **Consejo** `TIMESTAMPTZ` almacena microsegundos, mientras que `chrono` conserva nanosegundos. Si comparas un valor con lo que lees de vuelta, trúncalo antes: `Utc::now().trunc_subsecs(6)` (de `chrono::SubsecRound`).

## Errores

Los códigos de error de PostgreSQL se traducen a las variantes comunes de Memoire:

| Error de PostgreSQL | Error de Memoire |
|---|---|
| `unique_violation` (23505) | `Error::UniqueViolation { table, column }` |
| `not_null_violation`, `foreign_key_violation`, `check_violation` | `Error::Constraint(message)` |
| `undefined_table` (42P01) | `Error::NoSuchTable(name)` |
| `duplicate_table` (42P07) | `Error::TableExists(name)` |
| `serialization_failure` (40001), `deadlock_detected` (40P01) | `Error::Conflict(table)`: reintenta la transacción |
| El pool no puede conectarse (host o contraseña incorrectos, fallo en el handshake TLS...) | `Error::Pool(message)` |
| Cualquier otra cosa | `Error::Database(...)` con el error original |

## Lo que debes saber

- **Las transacciones** retienen una conexión del pool hasta que terminan; una transacción destruida se revierte en segundo plano. Consulta [Transacciones](transactions).
- **`LIKE` distingue mayúsculas y minúsculas**, e `ilike`/`icontains` usan el `ILIKE` nativo de PostgreSQL.
- **Orden de los NULL:** los NULL van al final en orden ascendente y al principio en orden descendente.
- **El SQL sin procesar** usa los marcadores `$1`, `$2`, .... Consulta [SQL sin procesar](raw-sql).
- **Hacer upsert con ids explícitos** en una tabla `auto_increment` no hace avanzar la secuencia de identidad de PostgreSQL, así que un `insert` posterior puede chocar con ellos. Usa `insert` para las filas nuevas de los modelos con autoincremento.
- **Acceso directo:** `db.pool()` devuelve el `deadpool_postgres::Pool`, para monitorización (`db.pool().status()`) o para funcionalidades de `tokio-postgres` como `LISTEN`/`NOTIFY`.

## Próximos pasos

- [Desplegar en Heroku desde el móvil](deploy-heroku-mobile): ejecuta una aplicación Memoire con Heroku Postgres.
- [Transacciones](transactions): niveles de aislamiento y reintentos en PostgreSQL.
- [SQL sin procesar](raw-sql): consultas específicas de PostgreSQL.
