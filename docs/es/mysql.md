# MySQL & MariaDB

`MySqlPool` conecta Memoire a MySQL o MariaDB mediante un pool de conexiones reutilizables. Tus modelos y consultas funcionan exactamente igual que en los demás backends, y Memoire configura las tablas para que las comparaciones, la ordenación y `LIKE` también se comporten de la misma forma.

## Activar el backend

```bash
cargo add memoire-orm --features mysql
cargo add tokio --features macros,rt-multi-thread
```

Para las conexiones cifradas (la mayoría de las bases de datos alojadas), usa en su lugar la feature `mysql-tls` (que incluye `mysql`):

```bash
cargo add memoire-orm --features mysql-tls
```

El cliente es `mysql_async`, una implementación del protocolo de MySQL en Rust puro; TLS usa `rustls`. No hace falta ninguna biblioteca de C ni OpenSSL.

| Requisito | Valor |
|---|---|
| Runtime asíncrono | **Tokio** (`#[tokio::main]`, `#[tokio::test]`) |
| MySQL | 8.0 o posterior (probado con 8.4) |
| MariaDB | 10.6 o posterior (probado con 10.11 y 11) |

> **Consejo** Para probarlo en local, arranca un servidor con Docker: `docker run -d -p 3306:3306 -e MARIADB_ROOT_PASSWORD=root -e MARIADB_DATABASE=app -e MARIADB_USER=app -e MARIADB_PASSWORD=secret mariadb:11` (o la imagen `mysql:8.4`, con `MYSQL_` en lugar de `MARIADB_` en los nombres de las variables).

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
    let db = MySqlPool::connect("mysql://app:secret@localhost:3306/app").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);

    db.close().await // opcional: cierra las conexiones de forma ordenada
}
```

| Método | Qué hace |
|---|---|
| `MySqlPool::connect(url)` | Se conecta con una URL `mysql://user:password@host:port/database`. Se abre una conexión de inmediato, así que una dirección o una contraseña incorrectas fallan al instante |
| `MySqlPool::connect_with(builder)` | Se conecta con un `mysql_async::OptsBuilder`, para las opciones que la URL no puede expresar (ajustes de TLS, opciones del socket...) |
| `db.pool()` | El `mysql_async::Pool` subyacente (vía de escape) |
| `db.close().await` | Cierra todas las conexiones del pool (para un apagado limpio) |

Clonar un `MySqlPool` es barato: los clones comparten el mismo pool. Las conexiones se abren bajo demanda y se reutilizan. Por defecto, el pool mantiene entre 10 y 100 conexiones (los valores por defecto de `mysql_async`); define otros límites con los parámetros de URL `pool_min` y `pool_max`, por ejemplo `mysql://app:secret@localhost:3306/app?pool_min=1&pool_max=5`, o con `OptsBuilder::pool_opts`. `pool_min` no debe superar a `pool_max`, así que un `pool_max` inferior a 10 también necesita un `pool_min`; en un plan alojado pequeño con un límite de conexiones bajo, define los dos.

## Conexiones cifradas (TLS)

Con la feature `mysql-tls`, añade parámetros de TLS a la URL:

| Parámetro de URL | Efecto |
|---|---|
| `require_ssl=true` | Cifra la conexión y verifica el certificado del servidor y el nombre del host |
| `verify_ca=false` | Acepta un certificado que no está firmado por una autoridad de confianza (autofirmado) |
| `verify_identity=false` | No comprueba que el certificado corresponda al nombre del host |

```rust
async fn connect_securely() -> memoire_orm::Result<MySqlPool> {
    MySqlPool::connect("mysql://app:secret@db.example.com:3306/app?require_ssl=true").await
}
```

Para tener un control total, construye tú mismo las opciones con `mysql_async` (0.37) en tus dependencias:

```rust
use mysql_async::{Opts, OptsBuilder, SslOpts};

async fn connect_with_options(url: &str) -> Result<MySqlPool, Box<dyn std::error::Error>> {
    let builder = OptsBuilder::from_opts(Opts::from_url(url)?).ssl_opts(SslOpts::default());
    Ok(MySqlPool::connect_with(builder).await?)
}
```

## Lo que Memoire configura por ti

| Ajuste | Por qué |
|---|---|
| Tablas creadas con `DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin` | Unicode completo (emojis incluidos) y comparaciones que distinguen mayúsculas y minúsculas y ordenan por bytes, como en los demás backends |
| `CLIENT_FOUND_ROWS` | `update` cuenta las filas que *coinciden*, aunque no cambie ningún valor, así que guardar una fila sin cambios no se notifica como `Error::NotFound` |
| 256 sentencias preparadas en caché por conexión | Cada sentencia distinta se analiza una sola vez por conexión |

Debido a la collation binaria, `User::NAME.eq("alice")` no coincide con `"Alice"`, y `like`, `contains`, `starts_with` y `ends_with` distinguen mayúsculas y minúsculas, exactamente igual que en PostgreSQL. Usa `ilike` o `icontains` para no distinguirlas (usan `LOWER()`, que gestiona las letras acentuadas).

> **Nota** Esto se aplica a las tablas creadas por Memoire (`create_table`, migraciones). Las tablas que hayas creado tú conservan su propia collation, que en MySQL a menudo no distingue mayúsculas y minúsculas.

## Tipos

| Tipo de Rust | Tipo de MySQL |
|---|---|
| `bool` | `BOOLEAN` (almacenado como `TINYINT(1)`: 0 o 1) |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INT` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `FLOAT` |
| `f64` | `DOUBLE` |
| `String` | `TEXT`, o `VARCHAR(255)` para las claves primarias y las columnas `unique`, `index` y de clave foránea |
| `Vec<u8>` | `BLOB`, o `VARBINARY(255)` para las columnas usadas como clave |
| `uuid::Uuid` | `CHAR(36)` |
| `chrono::NaiveDateTime` | `DATETIME(6)` |
| `chrono::DateTime<Utc>` | `DATETIME(6)`, con la hora UTC |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME(6)` |
| `serde_json::Value`, `Json<T>` | `JSON` |
| Clave `auto_increment` | `BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY` (para `i64`) |

Lo que debes saber:

- **Las columnas de texto usadas como clave admiten como máximo 255 caracteres**, porque MySQL no puede indexar un `TEXT` sin límite de longitud. Usa `#[memoire(sql_type = "VARCHAR(500)")]` para valores únicos más largos.
- **Las marcas de tiempo conservan los microsegundos.** Si comparas un valor con lo que lees de vuelta, trúncalo antes: `Utc::now().trunc_subsecs(6)` (de `chrono::SubsecRound`).
- **Los valores `DECIMAL`** (de tus propias tablas o de SQL sin procesar) se leen como texto exacto: decodifícalos en `String`, en `f64`, o en `i64` para los números enteros.
- **Los ids generados** provienen de `AUTO_INCREMENT` y se leen de vuelta con `LAST_INSERT_ID()`.

## Transacciones

`db.begin()` toma una conexión del pool para la transacción. Las demás consultas siguen ejecutándose en otras conexiones y solo ven los datos confirmados. El nivel de aislamiento es el predeterminado del servidor (`REPEATABLE READ` en InnoDB).

- Una transacción destruida se revierte cuando su conexión vuelve al pool.
- Los interbloqueos y los tiempos de espera de bloqueo agotados se notifican como `Error::Conflict`: reintenta la transacción completa, como se muestra en [Transacciones](transactions).

> **Atención** MySQL y MariaDB hacen commit automáticamente antes y después de cada cambio de esquema (`CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`...). Un cambio de esquema dentro de una transacción no se puede revertir, y además confirma los cambios hechos antes de él. Mantén los cambios de esquema fuera de las transacciones de tu aplicación.

Esto es importante para las [migraciones](migrations): si una migración falla después de crear una tabla, esa tabla se queda. Limita cada migración a un único cambio de esquema, para que un fallo sea fácil de limpiar a mano antes de volver a ejecutar la migración.

## Upsert

`upsert` busca las filas solo por la clave primaria, como en todos los demás backends. Si la fila de la que haces upsert tiene una clave primaria nueva pero un valor `unique` (por ejemplo, un email) que ya pertenece a otra fila, falla con `Error::UniqueViolation` y la otra fila queda intacta.

El `INSERT ... ON DUPLICATE KEY UPDATE` propio de MySQL se activa cuando choca **cualquier** clave única, así que actualizaría esa otra fila. Por eso Memoire solo lo usa en las tablas sin otras columnas `unique`, donde es una única sentencia atómica. Para las demás, `upsert` ejecuta un `INSERT` y, cuando ese insert choca en la clave primaria, un `UPDATE` por clave. Los dos pasos son seguros cuando varios programas hacen upsert al mismo tiempo, y también dentro de una transacción: una sentencia fallida no aborta una transacción de MySQL.

Hacer upsert de filas con ids explícitos en una tabla `auto_increment` es seguro en MySQL: el contador avanza más allá del id más alto.

## Errores

| Error de MySQL | Error de Memoire |
|---|---|
| Entrada duplicada (1062) | `Error::UniqueViolation { table, column }` (`column` es el nombre de la clave, que Memoire nombra según la columna) |
| La tabla no existe (1146), tabla desconocida (1051) | `Error::NoSuchTable(name)` |
| La tabla ya existe (1050) | `Error::TableExists(name)` |
| Violación de `NOT NULL`, de clave foránea o de `CHECK` | `Error::Constraint(message)` |
| Interbloqueo (1213), tiempo de espera de bloqueo agotado (1205) | `Error::Conflict(table)`: reintenta la transacción |
| No se puede conectar | `Error::Pool(message)` o `Error::Database(...)` |
| Cualquier otra cosa | `Error::Database(...)` con el error original |

## Lo que debes saber

- **Orden de los NULL:** igual que SQLite, MySQL coloca los NULL al principio en orden ascendente. `nulls_first()`/`nulls_last()` se emulan con una clave de ordenación `IS NULL`, así que funcionan en todas partes.
- **Las claves foráneas** (`references`, `on_delete = "cascade"`) se aplican (InnoDB).
- **El SQL sin procesar** usa marcadores `?`; los scripts de migración pueden contener varias sentencias. Consulta [SQL sin procesar](raw-sql).
- **Acceso directo:** `db.pool()` devuelve el `mysql_async::Pool`:

```rust
use mysql_async::prelude::Queryable;

async fn server_version(db: &MySqlPool) -> Result<Option<String>, mysql_async::Error> {
    let mut conn = db.pool().get_conn().await?;
    conn.query_first("SELECT VERSION()").await
}
```

## Generar SQL de MySQL para otro driver

Memoire también puede limitarse a *escribir* por ti el SQL de MySQL, sin `MySqlPool`: por ejemplo, para inspeccionar una consulta, preparar un archivo de esquema o ejecutar las sentencias con otra biblioteca cliente de MySQL. `Dialect::MySql` siempre está disponible, sean cuales sean las features que actives.

```rust
fn mysql_sql() {
    // Una consulta, con sus parámetros en el orden de los marcadores.
    let query = Product::query().filter(Product::PRICE_CENTS.lt(1_000)).order_by(Product::NAME.asc()).limit(20);
    let (sql, params) = query.to_sql_with_params(Dialect::MySql);
    println!("{sql}\n{params:?}");

    // Las sentencias que usan find / insert / update / delete / upsert.
    println!("{}", Product::SCHEMA.sql(Dialect::MySql).insert);

    // CREATE TABLE.
    for statement in memoire_orm::sql::create_table(Product::SCHEMA, Dialect::MySql, false) {
        println!("{statement};");
    }
}
```

```text
SELECT `products`.`id`, `products`.`sku`, `products`.`name`, `products`.`price_cents` FROM `products` WHERE `products`.`price_cents` < ? ORDER BY `products`.`name` ASC LIMIT 20
[I64(1000)]
INSERT INTO `products` (`sku`, `name`, `price_cents`) VALUES (?, ?, ?)
CREATE TABLE `products` (
    `id` BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    `sku` VARCHAR(255) NOT NULL UNIQUE,
    `name` TEXT NOT NULL,
    `price_cents` BIGINT NOT NULL
) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
```

`memoire_orm::sql::render` devuelve cualquier sentencia con sus parámetros (por ejemplo, el `INSERT` de un modelo), y las filas devueltas por otro driver se pueden decodificar en modelos poniendo sus valores en un `memoire_orm::ValueRow` y llamando a `FromRow::from_row`.

## Próximos pasos

- [Transacciones](transactions): reintentos y comportamiento de cada backend.
- [Migraciones](migrations): cambios de esquema en MySQL.
- [SQL sin procesar](raw-sql): consultas específicas de MySQL.
