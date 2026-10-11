# MongoDB (NoSQL)

`MongoDb` guarda tus modelos como documentos de MongoDB, una colección por tabla, a través del driver oficial de MongoDB. Los filtros, la ordenación y la paginación se traducen en consultas nativas de MongoDB y se ejecutan en el servidor; el resto del constructor de consultas (joins, `GROUP BY`, agregados) lo completa Memoire, así que toda la API sigue funcionando.

## Activar el backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mongodb
cargo add tokio --features macros,rt-multi-thread
```

El cliente es el driver oficial `mongodb` (3.x), que cifra las conexiones con `rustls` cuando la URI pide TLS (MongoDB Atlas siempre lo hace). `MongoDb` necesita el runtime **Tokio**.

## Conectarse

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    #[memoire(index)]
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MongoDb::connect("mongodb://localhost:27017/app").await?;
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@example.com".into(), name: "Alice".into(), age: None };
    alice.insert(&db).await?;

    let adults = User::query().filter(User::AGE.ge(18)).order_by(User::NAME.asc()).all(&db).await?;
    println!("{} adult(s)", adults.len());
    Ok(())
}
```

| Método | Qué hace |
|---|---|
| `MongoDb::connect(uri)` | Se conecta con una URI `mongodb://` o `mongodb+srv://` que **indica la base de datos** (`.../app`); si no la indica, falla con `Error::Pool` |
| `MongoDb::connect_with_database(uri, "app")` | Se conecta al servidor de `uri` y usa la base de datos `app` |
| `db.database()` | La `mongodb::Database` del driver, para las funcionalidades nativas de MongoDB (vía de escape) |
| `db.supports_transactions()` | `true` en un replica set o en un clúster fragmentado (consulta "Transacciones") |

El manejador comparte el pool de conexiones del driver, y clonarlo es barato. Para MongoDB Atlas, usa la URI `mongodb+srv://user:password@cluster0.example.mongodb.net/app` que aparece en el panel de Atlas.

### Un servidor local con transacciones

Las transacciones necesitan un replica set, pero para el desarrollo basta con un replica set de un solo nodo:

```bash
docker run -d --name mongo -p 27017:27017 mongo:7 --replSet rs0
docker exec mongo mongosh --eval 'rs.initiate({_id:"rs0",members:[{_id:0,host:"127.0.0.1:27017"}]})'
```

Después conéctate con `mongodb://127.0.0.1:27017/app?directConnection=true`.

## Cómo se almacenan tus datos

| Memoire | MongoDB |
|---|---|
| Tabla | Colección |
| Fila | Documento; la clave primaria se guarda en `_id` |
| `NULL` | El campo no existe en el documento |
| `#[memoire(unique)]` | Índice único con el nombre de la columna, que ignora los campos ausentes (se permiten muchos `NULL`, como en SQL) |
| `#[memoire(index)]` | Índice normal con el nombre de la columna |
| `auto_increment` | Un documento contador en la colección `__memoire_counters` |
| `DateTime<Utc>`, `NaiveDateTime` | Fecha BSON (precisión de **milisegundos**) |
| `NaiveDate`, `NaiveTime` | Cadenas ISO 8601, que se ordenan cronológicamente |
| `uuid::Uuid` | Binario BSON, subtipo 4 (UUID) |
| `Vec<u8>` | Binario BSON |
| `serde_json::Value`, `Json<T>` | Documentos y arrays incrustados |

Así se ve en MongoDB un usuario sin edad:

```text
{ "_id": 1, "email": "alice@example.com", "name": "Alice" }
```

> **Atención** Las fechas BSON conservan milisegundos, no microsegundos ni nanosegundos. Un `DateTime<Utc>` leído de vuelta desde MongoDB queda truncado al milisegundo: trunca los valores antes de compararlos (`Utc::now().trunc_subsecs(3)`, de `chrono::SubsecRound`).

## Cómo se ejecutan las consultas

| Consulta | Dónde se ejecuta |
|---|---|
| `find`, `get` por clave primaria | Servidor: `find` sobre `_id` |
| `filter`, `order_by`, `limit`, `offset`, `columns` sobre una colección | Servidor: un único `find` con filtro, ordenación, skip y limit |
| `count` | Servidor: `countDocuments` |
| `update_many` con `set`, `set_null`, `increment`, `decrement` | Servidor: una sola actualización con un pipeline de agregación |
| `delete_many` | Servidor: `deleteMany` |
| Joins | El filtro se envía al servidor; las colecciones unidas se recuperan con `$in` solo sobre las claves del join; el join lo hace Memoire |
| `group_by`, `sum`, `avg`, `min`, `max`, `distinct` | Los documentos que coinciden se recuperan con el filtro aplicado en el servidor; la agregación la hace Memoire |

Los filtros se traducen de forma que conserven el significado que tiene `NULL` en SQL (lógica trivalente). La parte de un filtro que no tiene equivalente en MongoDB (aritmética como `User::AGE.add(1).gt(...)`, por ejemplo) la comprueba Memoire sobre los documentos que coinciden con el resto del filtro. Algunos ejemplos:

| Filtro de Memoire | Filtro de MongoDB |
|---|---|
| `User::AGE.ge(18)` | `{ age: { $gte: 18 } }` |
| `!User::AGE.gt(18)` | `{ age: { $lte: 18 } }` (los usuarios sin edad no coinciden) |
| `User::AGE.ne(3)` | `{ age: { $nin: [null, 3] } }` |
| `User::AGE.is_null()` | `{ age: null }` |
| `User::NAME.contains("li")` | `{ name: { $regex: "^.*li.*$", $options: "s" } }` |
| `User::ID.is_in([1, 2])` | `{ _id: { $in: [1, 2] } }` |

> **Consejo** Añade `#[memoire(index)]` a los campos por los que filtras y ordenas: MongoDB usa esos índices en la parte de cada consulta que se ejecuta en el servidor.

## Semántica de las consultas

- **Las comparaciones y `LIKE` distinguen mayúsculas y minúsculas**, y las cadenas se ordenan por sus bytes. `ilike` e `icontains` no las distinguen, incluidas las letras acentuadas.
- **El orden de los NULL sigue a MongoDB:** los valores ausentes van al principio en orden ascendente y al final en orden descendente. Usa `nulls_first()`/`nulls_last()` para elegirlo explícitamente (Memoire aplica una posición distinta de la de MongoDB después de recuperar los documentos).
- **Las claves primarias, las columnas `unique` y `NOT NULL`** (campos que no son `Option`) se aplican, con los errores habituales.
- **Las claves foráneas no se aplican**, y `on_delete = "cascade"` no hace nada.
- **El SQL sin procesar** devuelve `Error::Unsupported`: usa `db.database()` para los comandos nativos. Los auto-joins (una colección dos veces en la misma consulta) tampoco son compatibles.

## Transacciones

En un **replica set** o en un **clúster fragmentado** (sharded cluster, MongoDB Atlas incluido), `db.begin()` abre una transacción multidocumento real:

- las lecturas dentro de la transacción ven sus propias escrituras;
- un conflicto de escritura con otra transacción se notifica como `Error::Conflict` (en la sentencia en conflicto o en el `commit`): reintenta la transacción completa, como se muestra en [Transacciones](transactions);
- destruir la transacción la aborta;
- la creación y la eliminación de colecciones se hacen fuera de la transacción, como los cambios de esquema en MySQL.

Memoire también envuelve en una transacción sus propias escrituras de varios documentos, `insert_many` y `update_many`, para que sean de todo o nada aunque tú no uses ninguna.

En un **servidor independiente** (standalone), MongoDB no tiene transacciones:

| | Servidor independiente |
|---|---|
| `db.begin()` | Falla con `Error::Unsupported` |
| `db.supports_transactions()` | `false` |
| Migraciones | Se aplican sin transacción |
| `insert_many` | Si un documento falla (por ejemplo, un duplicado), Memoire elimina en la medida de lo posible los documentos de esa llamada que ya se habían insertado |
| `update_many` | No es todo o nada: si un documento falla (por ejemplo, un conflicto en una columna `unique`), los documentos ya modificados siguen modificados |

## Migraciones

`create_table` crea la colección y sus índices; `drop_table` elimina la colección y su contador de ids. Los pasos `sql` fallan con `Error::Unsupported` y los pasos `sql_for` se omiten.

Como las colecciones se crean fuera de las transacciones, una migración que falla después de un paso `create_table` deja esa colección creada. Pon un solo cambio de esquema por migración, y elimina la colección a mano antes de volver a ejecutar la migración corregida.

## Errores

| Situación en MongoDB | Error de Memoire |
|---|---|
| Clave duplicada (E11000) | `Error::UniqueViolation { table, column }` |
| Conflicto de escritura, error transitorio de transacción | `Error::Conflict(table)`: reintenta la transacción |
| Colección inexistente | `Error::NoSuchTable(name)` (Memoire recuerda qué colecciones existen, por manejador) |
| La colección ya existe | `Error::TableExists(name)` |
| URI sin base de datos | `Error::Pool(message)` |
| Cualquier otra cosa | `Error::Database(...)` con el error original del driver |

## Acceso directo

`db.database()` devuelve la `mongodb::Database` del driver, para los pipelines de agregación, la búsqueda de texto, los change streams y todo lo demás que ofrece MongoDB. Añade `mongodb` (3.x) a tus dependencias:

```rust
use mongodb::bson::{doc, Document};

async fn top_authors(db: &MongoDb) -> Result<Vec<Document>, mongodb::error::Error> {
    let posts = db.database().collection::<Document>("posts");
    let mut cursor = posts
        .aggregate([
            doc! { "$group": { "_id": "$user_id", "views": { "$sum": "$views" } } },
            doc! { "$sort": { "views": -1 } },
            doc! { "$limit": 5 },
        ])
        .await?;
    let mut top = Vec::new();
    while cursor.advance().await? {
        top.push(cursor.deserialize_current()?);
    }
    Ok(top)
}
```

Recuerda que Memoire guarda la clave primaria en `_id` y omite de los documentos los campos `NULL`.

## Próximos pasos

- [Transacciones](transactions): conflictos y reintentos.
- [Constructor de consultas](queries): todo lo que puedes filtrar, ordenar y agregar.
- [Redis](redis): el otro backend NoSQL.
