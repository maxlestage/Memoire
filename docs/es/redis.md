# Redis

`RedisDb` guarda tus modelos en Redis, el almacén de datos en memoria. Usas los mismos modelos, consultas, transacciones y migraciones que con cualquier otro backend: Memoire guarda cada tabla en unas pocas claves de Redis, escribe cada cambio de forma atómica y evalúa las consultas con su motor en memoria. Encaja bien para cachés, sesiones, estado compartido de la aplicación y conjuntos de datos pequeños y medianos.

## Cuándo usarlo

| Ideal para | Piénsalo dos veces para |
|---|---|
| Sesiones, tokens, carritos, feature flags, cachés | Tablas grandes consultadas por columnas arbitrarias (cada consulta de ese tipo lee la tabla completa) |
| Datos compartidos por varios procesos o servidores | Datos que deben sobrevivir a la pérdida del servidor Redis, salvo que configures la persistencia de Redis |
| Conjuntos de datos pequeños y medianos que deben leerse rápido por clave | Redis Cluster (consulta "Limitaciones") |
| Aplicaciones que ya usan Redis y quieren un acceso tipado | Informes y analítica |

## Activar el backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features redis
cargo add tokio --features macros,rt-multi-thread
```

Para las URL `rediss://` (TLS), usa en su lugar la feature `redis-tls` (que incluye `redis`). El cliente es el crate `redis`; TLS usa `rustls`. `RedisDb` necesita el runtime **Tokio**.

> **Consejo** Para probarlo en local: `docker run -d -p 6379:6379 redis:7`.

## Conectarse

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "sessions")]
pub struct Session {
    #[memoire(primary_key)]
    pub token: String,
    pub user_id: i64,
    pub expires_at: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = RedisDb::connect("redis://127.0.0.1:6379").await?;
    db.create_table_if_not_exists::<Session>().await?;

    let mut session = Session { token: "4f9c2a".into(), user_id: 42, expires_at: 1_790_000_000 };
    session.insert(&db).await?;

    let found = Session::get(&db, "4f9c2a").await?;
    println!("token belongs to user {}", found.user_id);
    Ok(())
}
```

| Método | Qué hace |
|---|---|
| `RedisDb::connect(url)` | Se conecta y guarda todas las claves bajo el prefijo `memoire` |
| `RedisDb::connect_with_prefix(url, "myapp")` | Igual, con tu propio prefijo, para que varias aplicaciones (o ejecuciones de pruebas) puedan compartir un mismo servidor Redis |
| `db.connection()` | El `redis::aio::ConnectionManager` subyacente (vía de escape) |

Las URL tienen la forma `redis://host:6379`, `redis://:password@host:6379/2` (base de datos 2) o `rediss://...` para TLS. El manejador mantiene una conexión multiplexada que se reconecta automáticamente; clonarlo es barato.

### TLS y Heroku Key-Value Store

Con la feature `redis-tls`, las URL `rediss://` van cifradas y se verifica el certificado del servidor. Algunos servicios alojados, entre ellos Heroku Key-Value Store, usan certificados autofirmados: añade `#insecure` al final de la URL para mantener el cifrado pero omitir la comprobación del certificado.

```rust
async fn connect_on_heroku() -> Result<RedisDb, Box<dyn std::error::Error>> {
    // Heroku define REDIS_URL (una URL rediss://) cuando añades Heroku Key-Value Store.
    let url = std::env::var("REDIS_URL")?;
    let url = if url.starts_with("rediss://") && !url.contains('#') { format!("{url}#insecure") } else { url };
    Ok(RedisDb::connect_with_prefix(&url, "myapp").await?)
}
```

## Cómo se almacenan tus datos

Cada tabla usa unas pocas claves. Sus nombres comparten la parte `{table}`, un *hash tag* de Redis:

| Clave | Tipo de Redis | Contenido |
|---|---|---|
| `memoire:tables` | set | Los nombres de las tablas existentes |
| `memoire:{users}:rows` | hash | Clave primaria → la fila como objeto JSON |
| `memoire:{users}:u:email` | hash | Una por cada columna `unique`: valor → clave primaria |
| `memoire:{users}:seq` | string | El último id generado (`auto_increment`) |
| `memoire:{users}:ver` | string | Un número de versión, que aumenta con cada escritura |
| `memoire:{users}:meta` | hash | La lista de columnas y las claves de la tabla |

Las filas son JSON legible, así que puedes inspeccionarlas con `redis-cli` y otros programas pueden leerlas:

```text
> HGET memoire:{users}:rows 1
{"id":1,"email":"alice@example.com","name":"Alice","age":31,"joined":"2026-10-10T14:30:00.123456789Z"}
```

Los valores se guardan como números, cadenas y booleanos JSON. `NULL` es `null`; los bytes son cadenas hexadecimales; los UUID y las fechas son texto (las marcas de tiempo conservan la precisión de nanosegundos); las columnas JSON se incrustan tal cual.

## Cómo se ejecutan las operaciones

| Operación | Qué ocurre en Redis |
|---|---|
| `insert`, `upsert`, `update`, `delete` | Un script Lua atómico para la fila y sus índices `unique` |
| `insert_many` | Un script Lua atómico para todo el lote |
| `find`, `get`, `reload` | Un `HGET` |
| `filter` sobre la clave primaria con `eq` o `is_in` | Lee solo esas filas (`HGET`/`HMGET`) |
| `count()` sin filtro | `HLEN` |
| Cualquier otra consulta (filtros sobre otras columnas, ordenación, joins, agregados...) | Lee completas la tabla o las tablas implicadas y luego evalúa la consulta con el [motor en memoria](memory) de Memoire |
| `update_many`, `delete_many` | Lee la tabla, calcula los cambios con el motor en memoria y luego los escribe solo si la tabla no ha cambiado entretanto |

Cada escritura es un único script Lua que primero comprueba sus condiciones (la tabla existe, los valores `unique` están libres, la fila o la tabla no han cambiado) y solo entonces escribe. Por tanto, una escritura es de todo o nada, aunque afecte a muchas filas e índices.

Las escrituras que dependen de datos leídos justo antes (`update`, `delete`, `update_many`...) son optimistas: si otro cliente ha cambiado los datos entretanto, Memoire reintenta la sentencia automáticamente, hasta 64 veces, antes de rendirse con `Error::Conflict`.

> **Consejo** Las búsquedas por clave primaria son el camino rápido. Diseña las claves por las que buscas, como un token de sesión o un nombre de usuario, como clave primaria en lugar de como columna `unique`: un filtro sobre una columna `unique` lee la tabla completa.

## Semántica de las consultas

Como las consultas las evalúa el motor en memoria, Redis se comporta exactamente igual que [`MemoryDb`](memory):

- las comparaciones siguen la lógica trivalente de SQL (`NULL` nunca coincide en una comparación);
- los NULL van al final en orden ascendente y al principio en orden descendente;
- `like`, `contains`... distinguen mayúsculas y minúsculas; `ilike` e `icontains` no las distinguen, incluidas las letras acentuadas;
- se aplican las claves primarias, las columnas `unique` y `NOT NULL` (campos que no son `Option`), con los errores habituales (`Error::UniqueViolation`, `Error::Constraint`);
- las claves foráneas no se aplican y `on_delete = "cascade"` no hace nada;
- el SQL sin procesar (`raw`, `raw_execute`) devuelve `Error::Unsupported`, y los auto-joins también.

## Transacciones

`db.begin()` inicia una transacción que lee cada tabla una sola vez, la primera vez que la usa, y después trabaja sobre esa instantánea privada. En el `commit`, todos los cambios se escriben con un único script Lua atómico. Si otro cliente ha modificado alguna de las tablas que la transacción ha escrito, el commit falla con `Error::Conflict` y no se escribe nada: reintenta la transacción completa (consulta [Transacciones](transactions)).

- Los conflictos se detectan por tabla: dos transacciones que escriben filas distintas de la misma tabla también entran en conflicto.
- Una transacción que solo lee nunca entra en conflicto.
- Destruir una transacción la descarta; no se ha escrito nada.
- Crear o eliminar tablas dentro de una transacción tiene efecto en el commit, así que una [migración](migrations) fallida no deja nada atrás.

## Migraciones

Los pasos `create_table` y `drop_table` funcionan, y cada migración se aplica de forma atómica. Los pasos `sql` fallan con `Error::Unsupported` (no hay SQL), y los pasos `sql_for` se omiten.

## Limitaciones

- **Redis Cluster no es compatible**, porque una escritura puede afectar a claves de varias tablas, que un clúster puede guardar en nodos distintos. Funcionan un servidor independiente, Redis Sentinel y los servicios gestionados de un solo shard.
- **Todo vive en memoria.** Configura la persistencia de Redis (instantáneas RDB o AOF) si los datos deben sobrevivir a un reinicio.
- **Las tablas grandes con consultas que no son por clave** transfieren la tabla completa a tu aplicación en cada consulta. Mantén esas tablas pequeñas, o consúltalas por clave primaria.
- **Sin caducidad por fila.** Las filas no caducan por sí solas; elimina las filas antiguas con `delete_many`, o usa la conexión directa para tus propias claves con caducidad.

## Acceso directo

`db.connection()` devuelve el gestor de conexiones de `redis`, para todo lo que Memoire no cubre. Añade `redis` (1.x, con la feature `tokio-comp`) a tus dependencias para enviar comandos:

```rust
async fn raw_row(db: &RedisDb, id: i64) -> redis::RedisResult<Option<String>> {
    let mut conn = db.connection();
    redis::cmd("HGET").arg("memoire:{users}:rows").arg(id).query_async(&mut conn).await
}
```

> **Atención** No modifiques directamente las claves de Memoire: los índices `unique`, las versiones y los ids deben mantenerse coherentes con las filas. Léelas libremente y escribe a través de Memoire.

## Próximos pasos

- [Motor en memoria](memory): el motor de consultas en el que se apoya Redis.
- [Transacciones](transactions): conflictos y reintentos.
- [MongoDB](mongodb): otro backend NoSQL, con las consultas ejecutadas por el servidor.
