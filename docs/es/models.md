# Modelos

Un modelo es un struct de Rust que describe una tabla de la base de datos. Esta página cubre todo lo que entiende `#[derive(Model)]`: nombres de tabla, claves primarias, atributos de columna, tipos de Rust compatibles, los elementos que la macro genera por ti y `#[derive(FromRow)]` para filas de resultado personalizadas.

## Tu primer modelo

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
    pub active: bool,
}
```

Cada campo se convierte en una columna con el mismo nombre. Esto es lo que produce en SQLite:

```sql
CREATE TABLE "users" (
    "id" INTEGER PRIMARY KEY,
    "email" TEXT NOT NULL UNIQUE,
    "name" TEXT NOT NULL,
    "age" INTEGER,
    "active" INTEGER NOT NULL
);
CREATE INDEX "idx_users_name" ON "users" ("name");
```

Fíjate en que `age` no tiene `NOT NULL`: es un `Option<i32>`, así que acepta `NULL`. Todas las demás columnas son obligatorias.

### Qué genera el derive

`#[derive(Model)]` escribe todo esto por ti en tiempo de compilación:

| Elemento generado | Ejemplo | Para qué sirve |
|---|---|---|
| Una implementación del trait `Model` | `User::query()`, `user.insert(&db)`, `User::find(&db, 1)` | Todas las operaciones de base de datos |
| Una implementación de `FromRow` | (uso interno) | Convertir una fila de la base de datos de nuevo en un `User` |
| Una constante de columna tipada por campo | `User::ID`, `User::EMAIL`, `User::AGE` | Filtros, ordenación y proyecciones con tipado seguro |
| Una descripción estática de la tabla con el SQL ya generado | `User::SCHEMA` | Crear tablas y ejecutar operaciones por clave primaria sin construir SQL en tiempo de ejecución |

### Requisitos

- El struct debe tener **campos con nombre** (`struct User { ... }`). Los tuple structs, los unit structs y los enums no pueden ser modelos.
- El struct **no puede ser genérico** (`struct Wrapper<T>` se rechaza).
- El tipo de cada campo almacenado debe ser compatible (consulta la tabla de tipos más abajo).
- El modelo debe ser `Send + Sync + 'static`, algo que los structs normales con datos en propiedad siempre cumplen.

> **Nota** `#[derive(Model)]` ya implementa `FromRow`. No añadas `#[derive(FromRow)]` al mismo struct o las dos implementaciones entrarán en conflicto.

## El nombre de la tabla

Define el nombre de la tabla con el atributo de struct `table`:

```rust
#[derive(Model)]
#[memoire(table = "blog_posts")]
pub struct BlogPost {
    pub id: i64,
    pub title: String,
}
```

Sin él, el nombre de la tabla es el nombre del struct en `snake_case`, **sin** pluralizar:

| Struct | Nombre de tabla por defecto |
|---|---|
| `User` | `user` |
| `BlogPost` | `blog_post` |
| `HTTPRequest` | `http_request` |

> **Consejo** Escribir siempre `#[memoire(table = "...")]` hace explícito tu esquema y te permite renombrar el struct más adelante sin tocar la base de datos.

## Usar Memoire a través de otra crate

`#[derive(Model)]` genera código que hace referencia a `::memoire_orm`. Un framework que reexporta Memoire (como [Paquets](https://github.com/maxlestage/Paquets), donde se llama `paquets::orm`) indica a la derivación dónde encontrarlo con el atributo de struct `crate`, así tu proyecto no necesita su propia dependencia `memoire-orm`:

```rust
#[derive(paquets::orm::Model)]
#[memoire(crate = "paquets::orm", table = "notes")]
pub struct Note {
    pub id: i64,
    pub text: String,
}
```

## Claves primarias

Cada modelo necesita exactamente una clave primaria. Identifica una fila y es lo que usan `find`, `get`, `update`, `delete` y `upsert`.

Hay dos formas de elegirla:

1. Marcar un campo con `#[memoire(primary_key)]`.
2. O simplemente llamar `id` a un campo: si no hay ningún `primary_key` explícito, un campo llamado `id` se convierte en la clave primaria.

### Claves autoincrementales

Con `auto_increment`, la base de datos genera la clave cuando insertas una fila. El valor de tu struct se ignora al insertar y el valor generado se escribe de vuelta en él:

```rust
#[derive(Model, Debug)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub text: String,
}

async fn demo(db: &MemoryDb) -> memoire_orm::Result<()> {
    let mut note = Note { id: 0, text: "hi".into() };
    note.insert(db).await?;
    assert_eq!(note.id, 1); // generado por la base de datos
    Ok(())
}
```

Las claves autoincrementales deben ser `i16`, `i32` o `i64`.

### La regla del `id` implícito

Así es exactamente como decide Memoire, para que no haya sorpresas:

| Declaración del campo | ¿Clave primaria? | ¿Autoincremento? |
|---|---|---|
| `id: i64` (sin ningún atributo en todo el struct) | sí (implícita) | **sí** (implícito, porque es un `i16`/`i32`/`i64` llamado `id`) |
| `id: String` o `id: uuid::Uuid` (sin atributo) | sí (implícita) | no |
| `#[memoire(primary_key, auto_increment)] id: i64` | sí | sí |
| `#[memoire(primary_key)] id: i64` | sí | **no**: elegiste la clave de forma explícita, así que añade `auto_increment` si quieres ids generados |
| `#[memoire(auto_increment = false)] id: i64` | sí (implícita) | no |
| `#[memoire(primary_key)] slug: String` | sí | no |

> **Atención** `#[memoire(primary_key)] id: i64` por sí solo *no* es autoincremental. Si insertas varias filas con `id: 0`, la segunda inserción falla por una violación de unicidad. Añade `auto_increment`, o quita el atributo y confía en la regla implícita.

### Claves naturales

Una clave no tiene por qué ser un número. Aquí la clave es el slug y lo proporcionas tú:

```rust
#[derive(Model, Debug, Clone)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}

async fn natural(db: &MemoryDb) -> memoire_orm::Result<()> {
    let mut rust = Tag { slug: "rust".into(), label: "Rust".into() };
    rust.insert(db).await?;
    let found = Tag::get(db, "rust").await?; // búsqueda por la clave de texto
    assert_eq!(found.label, "Rust");
    Ok(())
}
```

## Atributos de columna

Los atributos van en `#[memoire(...)]` encima de un campo. Puedes combinar varios, separados por comas.

| Atributo | Ejemplo | Efecto |
|---|---|---|
| `primary_key` | `#[memoire(primary_key)]` | Este campo es la clave primaria (alias: `id`) |
| `auto_increment` | `#[memoire(primary_key, auto_increment)]` | La base de datos genera la clave (alias: `auto`; `auto_increment = false` desactiva la regla implícita) |
| `unique` | `#[memoire(unique)]` | Añade una restricción `UNIQUE`; los duplicados fallan con `Error::UniqueViolation` |
| `index` | `#[memoire(index)]` | Crea un índice llamado `idx_<table>_<column>` |
| `column` | `#[memoire(column = "e_mail")]` | Guarda el campo con otro nombre de columna (alias: `rename`) |
| `sql_type` | `#[memoire(sql_type = "VARCHAR(120)")]` | Sustituye el tipo SQL usado en `CREATE TABLE` |
| `default` | `#[memoire(default = "0")]` | Añade `DEFAULT <SQL expression>` a la columna |
| `references` | `#[memoire(references = "users(id)")]` | Añade una clave foránea; también se acepta `"users.id"` |
| `on_delete` | `#[memoire(references = "users(id)", on_delete = "cascade")]` | Acción cuando se elimina la fila referenciada |
| `on_update` | `#[memoire(references = "users(id)", on_update = "cascade")]` | Acción cuando cambia la clave referenciada |
| `skip` | `#[memoire(skip)]` | El campo no se almacena en absoluto |

Este es un modelo que usa la mayoría de ellos:

```rust
#[derive(Model, Debug, Clone)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub author_id: i64,
    #[memoire(sql_type = "VARCHAR(200)")]
    pub title: String,
    #[memoire(column = "body_markdown")]
    pub body: String,
    #[memoire(default = "0")]
    pub views: i32,
    #[memoire(skip)]
    pub rendered_html: Option<String>,
}
```

En PostgreSQL esto crea:

```sql
CREATE TABLE "posts" (
    "id" BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    "author_id" BIGINT NOT NULL,
    "title" VARCHAR(200) NOT NULL,
    "body_markdown" TEXT NOT NULL,
    "views" INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY ("author_id") REFERENCES "users" ("id") ON DELETE CASCADE
);
CREATE INDEX "idx_posts_author_id" ON "posts" ("author_id");
```

### `unique` e `index`

- `unique` hace que la base de datos rechace una segunda fila con el mismo valor. Memoire lo notifica como `Error::UniqueViolation { table, column }` en todos los backends. La base de datos indexa automáticamente las columnas `unique`.
- `index` acelera los filtros, los joins y la ordenación sobre esa columna. Úsalo en las columnas por las que buscas a menudo, como las claves foráneas. Las bases de datos SQL y MongoDB crean un índice real; el motor en memoria y Redis lo ignoran.

### `column` (renombrar)

El campo de Rust y la columna pueden tener nombres distintos. La constante tipada conserva el nombre del *campo* (`Post::BODY`), pero el SQL usa el nombre de la columna (`"body_markdown"`). También es la forma de usar una palabra reservada de Rust como nombre de columna:

```rust
#[derive(Model)]
#[memoire(table = "items")]
pub struct Item {
    pub id: i64,
    pub r#type: String,                 // columna "type", constante Item::TYPE
    #[memoire(column = "item_kind")]
    pub kind: String,                   // columna "item_kind", constante Item::KIND
}
```

### `sql_type`

`sql_type` sustituye el tipo SQL escrito en `CREATE TABLE`. No cambia cómo se convierten los valores en Rust, así que elige un tipo compatible con el campo. Se usa el mismo texto para todas las bases de datos SQL, por lo que es preferible usar tipos portables como `VARCHAR(120)`. Redis y MongoDB, que no tienen `CREATE TABLE`, lo ignoran.

### `default`

`default` se copia tal cual en la definición de la columna, así que escribe una expresión SQL: `"0"`, `"'draft'"` (con comillas SQL para el texto) o `"CURRENT_TIMESTAMP"`.

> **Nota** El `insert` de Memoire siempre envía todos los campos, así que `insert` nunca usa el valor por defecto. Este importa para las filas insertadas con SQL sin procesar o por otros programas, y para rellenar las filas existentes cuando añades una columna en una migración. El motor en memoria, Redis y MongoDB lo ignoran.

### `references`, `on_delete` y `on_update`

`references = "table(column)"` (o `"table.column"`) declara una clave foránea. `on_delete` y `on_update` aceptan `cascade`, `restrict`, `no_action`, `set_null` y `set_default` (sin distinguir mayúsculas y minúsculas). Solo se permiten junto con `references`.

| Backend | ¿Se aplican las claves foráneas? |
|---|---|
| PostgreSQL | Sí |
| SQLite | Sí (Memoire activa `PRAGMA foreign_keys`) |
| MySQL / MariaDB | Sí |
| Motor en memoria, Redis, MongoDB | No: la restricción se acepta pero no se comprueba, y `cascade` no hace nada |

Crea primero la tabla referenciada (`users` antes que `posts`). Consulta [Uniones y relaciones](relations) para ver cómo consultar tablas relacionadas.

### `skip`

Un campo omitido no es una columna: no se almacena, no se selecciona y recibe `Default::default()` cuando se carga una fila. Por tanto, su tipo debe implementar `Default`. Úsalo para cachés o valores calculados. Un campo omitido no puede llevar otros atributos de columna.

## Tipos de Rust compatibles

Envuelve cualquier tipo en `Option<T>` para que la columna admita nulos.

| Tipo de Rust | Feature | PostgreSQL | SQLite | MySQL |
|---|---|---|---|---|
| `bool` | | `BOOLEAN` | `INTEGER` (0 o 1) | `BOOLEAN` |
| `i8`, `i16`, `u8` | | `SMALLINT` | `INTEGER` | `SMALLINT` |
| `i32`, `u16` | | `INTEGER` | `INTEGER` | `INT` |
| `i64`, `u32` | | `BIGINT` | `INTEGER` | `BIGINT` |
| `f32` | | `REAL` | `REAL` | `FLOAT` |
| `f64` | | `DOUBLE PRECISION` | `REAL` | `DOUBLE` |
| `String` | | `TEXT` | `TEXT` | `TEXT`, o `VARCHAR(255)` si es clave |
| `Vec<u8>` | | `BYTEA` | `BLOB` | `BLOB`, o `VARBINARY(255)` si es clave |
| `uuid::Uuid` | `uuid` | `UUID` | `TEXT` | `CHAR(36)` |
| `chrono::NaiveDateTime` | `chrono` | `TIMESTAMP` | `TEXT` | `DATETIME(6)` |
| `chrono::DateTime<Utc>` | `chrono` | `TIMESTAMPTZ` | `TEXT` | `DATETIME(6)` |
| `chrono::NaiveDate` | `chrono` | `DATE` | `TEXT` | `DATE` |
| `chrono::NaiveTime` | `chrono` | `TIME` | `TEXT` | `TIME(6)` |
| `serde_json::Value` | `json` | `JSONB` | `TEXT` | `JSON` |
| `Json<T>` (cualquier tipo `serde`) | `json` | `JSONB` | `TEXT` | `JSON` |

"Si es clave" significa que la columna es una clave primaria o una columna `unique`, `index` o `references` (MySQL no puede indexar una columna `TEXT` sin límite de longitud).

Las claves autoincrementales reciben un DDL especial: `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` en PostgreSQL, `INTEGER PRIMARY KEY` en SQLite y `BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY` en MySQL (para una clave `i64`).

Redis y MongoDB no tienen tipos de columna: Redis guarda cada fila como un objeto JSON y MongoDB como un documento BSON. Sus páginas describen cómo se almacena cada tipo de Rust ([Redis](redis), [MongoDB](mongodb)); todos los tipos de la tabla anterior son compatibles con los seis backends.

> **Nota** `u64`, `usize`, `i128` y `char` no son compatibles: no existe un tipo SQL portable para ellos. Usa `i64` (o `String` en lugar de `char`). ¿Por qué `i64`? Porque es el entero más grande que todas las bases de datos compatibles almacenan de forma nativa.

### Columnas JSON

`Json<T>` almacena cualquier tipo que implemente `serde::Serialize` y `serde::Deserialize`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: String,
    pub notifications: bool,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "profiles")]
pub struct Profile {
    pub id: i64,
    pub settings: Json<Settings>,
    pub extra: Option<serde_json::Value>,
}
```

`Json<T>` se desreferencia a `T`, así que `profile.settings.theme` funciona directamente, y creas uno con `Json(Settings { ... })`.

### Tipos de columna personalizados

Puedes almacenar tus propios tipos, como un enum, implementando cuatro pequeños traits de conversión. Aquí un enum `Status` se almacena como texto:

```rust
use memoire_orm::prelude::*;
use memoire_orm::{ColumnType, FromValue, IntoValue, SqlType, ToValue};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Draft,
    Published,
}

impl SqlType for Status {
    type Base = Status; // el tipo usado en las comparaciones tipadas
    const KIND: ColumnType = ColumnType::Text; // se almacena como TEXT
}

impl ToValue for Status {
    fn to_value(&self) -> Value {
        let text = match self {
            Status::Draft => "draft",
            Status::Published => "published",
        };
        Value::Text(text.to_owned())
    }
}

impl IntoValue for Status {
    fn into_value(self) -> Value {
        self.to_value()
    }
}

impl FromValue for Status {
    fn from_value(value: Value) -> memoire_orm::Result<Self> {
        match value.as_str() {
            Some("draft") => Ok(Status::Draft),
            Some("published") => Ok(Status::Published),
            _ => Err(Error::conversion(format!("unknown status: {value}"))),
        }
    }
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "articles")]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub status: Status,
}

async fn published(db: &MemoryDb) -> memoire_orm::Result<Vec<Article>> {
    Article::query().filter(Article::STATUS.eq(Status::Published)).all(db).await
}
```

La comparación `Article::STATUS.eq(Status::Published)` sigue estando comprobada por el sistema de tipos: pasar ahí una cadena no compilaría.

## Constantes de columna tipadas

Por cada campo almacenado, el derive añade una constante asociada con el nombre del campo en `UPPER_SNAKE_CASE`:

| Campo | Constante | Tipo |
|---|---|---|
| `id: i64` | `User::ID` | `Column<User, i64>` |
| `email: String` | `User::EMAIL` | `Column<User, String>` |
| `age: Option<i32>` | `User::AGE` | `Column<User, Option<i32>>` |
| `r#type: String` | `Item::TYPE` | `Column<Item, String>` |

Un `Column<M, T>` conoce su tabla, su nombre y su tipo de Rust, así que el compilador comprueba cómo lo usas:

- las comparaciones solo aceptan valores del tipo de la columna (`User::AGE.gt(18)`, no `User::AGE.gt("18")`);
- los ayudantes de texto como `contains` solo existen en las columnas de texto;
- `update_many().set(...)` solo acepta columnas del modelo que se está actualizando;
- `set_null` solo acepta columnas `Option`;
- una constante mal escrita (`User::EMIAL`) es un error de compilación.

`User::EMAIL.name()` devuelve el nombre de la columna (`"email"`). La lista completa de lo que puedes hacer con las columnas está en [Constructor de consultas](queries).

## El esquema de la tabla y el SQL generado en compilación

Cada modelo expone `Model::SCHEMA`, un `&'static TableSchema` construido en tiempo de compilación. Contiene el nombre de la tabla, las columnas y, para cada dialecto SQL, las sentencias ya preparadas que se usan en las operaciones por clave primaria:

```rust
use memoire_orm::prelude::*;

fn inspect() {
    let schema = User::SCHEMA;
    println!("table: {}", schema.name);
    println!("primary key: {}", schema.pk().name);
    for column in schema.columns {
        println!("- {} (nullable: {})", column.name, column.nullable);
    }

    let pg = schema.sql(Dialect::Postgres);
    println!("{}", pg.insert);
    println!("{}", pg.find);
}
```

```text
table: users
primary key: id
- id (nullable: false)
- email (nullable: false)
- name (nullable: false)
- age (nullable: true)
- active (nullable: false)
INSERT INTO "users" ("email", "name", "age", "active") VALUES ($1, $2, $3, $4) RETURNING "id"
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active" FROM "users" WHERE "users"."id" = $1
```

`TableSchema::sql(dialect)` devuelve un `StaticSql` con siete campos: `columns`, `select`, `find`, `insert`, `update`, `delete` y `upsert`. Como estas cadenas son constantes incrustadas en tu binario, `User::find`, `user.insert`, `user.update`, `user.delete` y `user.upsert` nunca construyen SQL en tiempo de ejecución.

Para ver las sentencias `CREATE TABLE` de un dialecto, usa `memoire_orm::sql::create_table`:

```rust
use memoire_orm::prelude::*;

fn print_ddl() {
    for statement in memoire_orm::sql::create_table(User::SCHEMA, Dialect::Postgres, false) {
        println!("{statement};");
    }
}
```

El último argumento añade `IF NOT EXISTS` cuando vale `true`.

## Filas de resultado personalizadas con `#[derive(FromRow)]`

A veces una consulta devuelve algo que no es un modelo completo: un informe, unas pocas columnas, el resultado de SQL sin procesar. `#[derive(FromRow)]` convierte cualquier struct con campos con nombre en un tipo de fila:

```rust
use memoire_orm::prelude::*;

#[derive(FromRow, Debug)]
pub struct AuthorStats {
    pub author_id: i64,
    pub posts: i64,
    pub total_views: Option<i64>,
}

async fn stats(db: &MemoryDb) -> memoire_orm::Result<Vec<AuthorStats>> {
    Post::query()
        .group_by(Post::AUTHOR_ID)
        .select_exprs([Post::AUTHOR_ID.expr(), count_all(), Post::VIEWS.sum()])
        .all(db)
        .await
}
```

Las columnas se decodifican **por posición**, en el orden en que se declaran los campos: el primer valor seleccionado va al primer campo, y así sucesivamente. Los nombres no se comparan, lo que mantiene la decodificación rápida pero implica que el orden debe coincidir con tu `SELECT`. `#[memoire(skip)]` también funciona aquí.

`FromRow` también está implementado para los escalares (`i64`, `String`, ...), los `Option` de escalares, `Value` y las tuplas de hasta 8 elementos, así que para resultados pequeños a menudo basta con una tupla: `Vec<(i64, String)>`.

## Limitaciones

| No compatible | Qué hacer en su lugar |
|---|---|
| Claves primarias compuestas (dos campos `primary_key`) | Añade una clave sustituta `id` y una columna `unique`, o crea una restricción compuesta con una [migración](migrations) SQL |
| Structs genéricos, tuple structs o enums como modelos | Usa un struct normal con campos con nombre |
| Campos `u64`, `usize`, `i128` | Usa `i64` |
| Comparación automática de esquemas (schema diffing) | Escribe [migraciones](migrations) |

## Próximos pasos

- [Constructor de consultas](queries): filtros, ordenación, paginación y agregados.
- [Crear, leer, actualizar, eliminar](crud): todas las operaciones sobre un modelo.
- [Uniones y relaciones](relations): combina varios modelos.
