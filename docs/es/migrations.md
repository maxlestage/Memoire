# Migraciones

Las migraciones son cambios numerados y ordenados en el esquema de tu base de datos: "1: crear la tabla users", "2: añadir una columna bio", etc. Memoire registra cuáles se han ejecutado ya, de modo que todas las bases de datos (tu portátil, tus pruebas, producción) llegan al mismo esquema aplicando solo lo que falta.

## Tu primera migración

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub user_id: i64,
    pub title: String,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<User>())
        .add(Migration::new(2, "create_posts").create_table::<Post>())
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("app.db")?;

    let applied = migrations().run(&db).await?;
    println!("applied migrations: {applied:?}");

    // ... el resto de tu programa ...
    Ok(())
}
```

La primera ejecución imprime `applied migrations: [1, 2]`. Todas las ejecuciones posteriores imprimen `applied migrations: []`, porque ambas ya están registradas. Por tanto, ejecutar el migrador al arrancar es seguro y es la configuración más habitual.

Las piezas básicas:

- `Migration::new(version, name)` crea una migración. La versión es un número único (`i64`) y determina el orden; el nombre es una etiqueta legible para las personas.
- `Migrator::new().add(...)` reúne las migraciones. El orden de las llamadas a `add` no importa: las migraciones siempre se ejecutan por versión ascendente.
- `migrator.run(&db)` aplica las migraciones pendientes y devuelve las versiones que ha aplicado.

## Pasos de una migración

Una migración es una lista de pasos, que se ejecutan en el orden en que los añades:

| Paso | Qué hace | En memoria, Redis, MongoDB |
|---|---|---|
| `.create_table::<M>()` | `CREATE TABLE` (y sus índices) para el modelo `M`; en MongoDB, una colección y sus índices | Compatible |
| `.drop_table("name")` | `DROP TABLE name` | Compatible |
| `.sql("...")` | Ejecuta un script SQL en cualquier base de datos SQL (SQLite, PostgreSQL, MySQL) | **Falla** con `Error::Unsupported` |
| `.sql_for(Dialect::Postgres, "...")` | Ejecuta un script SQL solo en esa base de datos (`Dialect::Sqlite`, `Dialect::Postgres` o `Dialect::MySql`); en las demás se omite | Se omite |

Una migración puede mezclar varios pasos:

```rust
let release_2 = Migration::new(3, "comments_and_cleanup")
    .create_table::<Comment>()
    .drop_table("legacy_comments")
    .sql_for(Dialect::Postgres, "CREATE INDEX idx_posts_title ON posts (title)")
    .sql_for(Dialect::Sqlite, "CREATE INDEX idx_posts_title ON posts (title)")
    .sql_for(Dialect::MySql, "CREATE INDEX idx_posts_title ON posts (title(100))");
```

La versión de MySQL solo indexa los 100 primeros caracteres, porque MySQL no puede indexar una columna `TEXT` completa: `sql_for` es la forma de gestionar este tipo de diferencias.

Un script `sql` o `sql_for` puede contener varias sentencias separadas por punto y coma.

## Cambiar una tabla más adelante

Para cambiar una tabla existente, añade una **nueva** migración con el SQL del cambio. Aquí añadimos una columna `bio`. `sql_for` te permite escribir la sentencia para cada base de datos que admites:

```rust
let add_bio = Migration::new(4, "add_user_bio")
    .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
    .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT")
    .sql_for(Dialect::MySql, "ALTER TABLE users ADD COLUMN bio TEXT");
```

Después añade el campo al struct (`pub bio: Option<String>`), para que el modelo coincida con el nuevo esquema.

> **Atención** Nunca edites una migración que ya se haya ejecutado en algún sitio: las bases de datos que la aplicaron no volverán a ejecutarla, así que tu cambio nunca les llegaría. Añade siempre una nueva migración con una versión mayor.

### Congelar la forma de las tablas antiguas

`create_table::<User>()` crea la tabla a partir del struct **tal como está en tu código actual**. Después de añadir `bio` a `User`, en una base de datos recién creada la migración 1 crearía `users` con una columna `bio`, y la migración 4 fallaría a continuación porque la columna ya existe.

La solución es conservar una copia congelada del struct tal como era cuando se escribió la migración 1, y usar esa copia en la migración antigua:

```rust
/// `users` tal como era en la migración 1. No cambies nunca este módulo.
mod v1 {
    use memoire_orm::prelude::*;

    #[derive(Model)]
    #[memoire(table = "users")]
    pub struct User {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        #[memoire(unique)]
        pub email: String,
    }
}

/// El modelo actual, el que usa la aplicación.
#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub bio: Option<String>,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<v1::User>())
        .add(
            Migration::new(2, "add_user_bio")
                .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
                .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT")
                .sql_for(Dialect::MySql, "ALTER TABLE users ADD COLUMN bio TEXT"),
        )
}
```

Ahora una base de datos nueva y una antigua terminan con el mismo esquema. Como alternativa, puedes pegar en un paso `sql_for` la sentencia `CREATE TABLE` que produce `memoire_orm::sql::create_table`.

## Dónde registra Memoire las migraciones

Las migraciones aplicadas se guardan en una tabla llamada `_memoire_migrations`, que se crea automáticamente la primera vez que llamas a `run`, `pending` o `applied`:

| Columna | Tipo | Contenido |
|---|---|---|
| `version` | `BIGINT` (clave primaria) | La versión de la migración |
| `name` | `TEXT` | El nombre de la migración |
| `applied_at` | `BIGINT` | Cuándo se ejecutó, en milisegundos desde la época Unix |

Puedes inspeccionarla desde Rust:

```rust
async fn status<D: Database>(db: &D) -> memoire_orm::Result<()> {
    let migrator = migrations();

    for done in migrator.applied(db).await? {
        println!("applied: {} {} (at {} ms)", done.version, done.name, done.applied_at);
    }
    for todo in migrator.pending(db).await? {
        println!("pending: {} {}", todo.version(), todo.name());
    }
    Ok(())
}
```

`applied` devuelve filas `memoire_orm::migrate::AppliedMigration`; `pending` devuelve las `Migration` que aún no se han ejecutado, ordenadas por versión.

> **Nota** Solo se usa la versión para decidir si una migración se ha ejecutado. Renombrar una migración no hace que vuelva a ejecutarse.

## Cómo funciona `run` y qué ocurre si algo falla

`run` hace lo siguiente:

1. Comprueba que no haya dos migraciones con la misma versión (si las hay, falla antes de tocar la base de datos).
2. Lee `_memoire_migrations` para encontrar las migraciones pendientes.
3. Para cada migración pendiente, por orden ascendente de versión, abre una **transacción**, ejecuta todos los pasos, registra la migración en `_memoire_migrations` y hace commit. (En un servidor MongoDB independiente, que no tiene transacciones, los pasos se ejecutan sin ella).
4. Devuelve la lista de versiones aplicadas.

Si un paso falla:

- la transacción de esa migración se revierte: no se conserva ninguno de sus pasos ni ningún registro;
- las migraciones aplicadas antes siguen aplicadas (ya se confirmaron);
- no se intentan las migraciones siguientes;
- `run` devuelve `Error::Migration { version, name, source }`, donde `source` es el error subyacente.

```rust
match migrations().run(&db).await {
    Ok(applied) => println!("schema up to date (applied {applied:?})"),
    Err(Error::Migration { version, name, source }) => {
        eprintln!("migration {version} ({name}) failed: {source}");
    }
    Err(other) => return Err(other),
}
```

Corrige la migración y vuelve a ejecutarlo: continúa donde se detuvo.

Cuánto se deshace de una migración fallida depende del backend:

| Backend | Cambios de esquema de una migración fallida |
|---|---|
| SQLite, PostgreSQL, en memoria, Redis | Se revierten: no queda nada aplicado a medias |
| MySQL / MariaDB | **Se conservan**: MySQL confirma cada cambio de esquema inmediatamente |
| MongoDB | **Se conservan**: las colecciones se crean y se eliminan fuera de las transacciones |

> **Atención** En MySQL y MongoDB, una migración que falla después de un `create_table` (o de cualquier otro cambio de esquema) deja ese cambio aplicado, mientras que la migración no queda registrada como aplicada. Deshazlo a mano antes de volver a ejecutar la migración corregida. Poner un solo cambio de esquema por migración lo facilita.

## Migraciones sin SQL: en memoria, Redis y MongoDB

El motor en memoria, Redis y MongoDB no tienen SQL, así que solo entienden los pasos `create_table` y `drop_table`. Un paso `sql` hace que la migración falle con `Error::Unsupported`, y los pasos `sql_for` se omiten. Redis y MongoDB no tienen columnas fijas, así que añadir un campo a un modelo no requiere ninguna migración en ellos: las filas nuevas tienen el campo, y las filas escritas antes lo leen como vacío (`NULL`), así que declara los campos nuevos como `Option<T>`. (Esto no crea índices para un nuevo campo `unique` o `index`. El motor en memoria empieza vacío en cada ejecución, así que no tiene nada que migrar).

Para las pruebas unitarias sobre `MemoryDb`, lo más sencillo suele ser crear las tablas directamente a partir de tus modelos actuales en lugar de ejecutar el migrador:

```rust
async fn test_db() -> memoire_orm::Result<MemoryDb> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;
    db.create_table::<Post>().await?;
    Ok(db)
}
```

## Próximos pasos

- [Transacciones](transactions): cómo se comporta la transacción que envuelve cada migración.
- [SQL sin procesar](raw-sql): ejecuta SQL fuera de las migraciones.
- [Modelos](models): los atributos que dan forma al `CREATE TABLE` generado.
