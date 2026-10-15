# Memoire, Diesel, SeaORM y Nitrite

Diesel y SeaORM son los dos ORM más usados en Rust, y Nitrite una base NoSQL embebida muy extendida. Esta página los compara con Memoire función por función, para elegir la herramienta adecuada o pasar de uno a otro. Intenta ser justa: cada biblioteca va por delante en algo.

## En una frase cada uno

- **Diesel**: un constructor de consultas síncrono que comprueba cada consulta en compilación contra un `schema.rs` generado desde tu base (lo asíncrono pasa por el crate aparte `diesel-async`). PostgreSQL, MySQL, SQLite.
- **SeaORM**: un ORM asíncrono construido sobre SQLx, con entidades generadas por una CLI, `ActiveModel` para los cambios y muchas utilidades de relaciones. PostgreSQL, MySQL, SQLite.
- **Memoire**: un ORM asíncrono con un solo `#[derive(Model)]` sobre tu propio struct, el SQL de las operaciones por clave primaria generado durante la compilación, y la misma API en SQL y NoSQL: PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB y un motor en memoria en Rust puro que funciona en WebAssembly.

## Función por función

| | Diesel | SeaORM | Memoire |
|---|---|---|---|
| Asíncrono | vía `diesel-async` | sí | sí, en todas las bases |
| Definición de modelos | esquema `table!` + derives en el struct | módulo de entidad generado (`Model`, `ActiveModel`, `Relation`...) | un `#[derive(Model)]` en tu struct |
| Filtros comprobados por tipos | sí, contra el esquema | en parte (enums de columnas, valores comprobados en ejecución) | sí: `User::AGE.gt("18")` no compila |
| SQL del CRUD por clave primaria | generado a partir de los tipos de consulta, sentencias preparadas en caché | construido en ejecución | **generado en compilación** para cada dialecto |
| PostgreSQL / MySQL / SQLite | sí | sí | sí |
| Redis, MongoDB | no | no | sí, misma API |
| Motor en memoria, WebAssembly | no | conexión simulada para pruebas | sí, un motor real con índices y transacciones, que puede guardarse en un archivo (cifrado) |
| Joins | sí, tipados | sí | sí, decodificados en tuplas |
| Relaciones sin N+1 | `belonging_to` + `grouped_by` | `find_with_related`, `load_one`, `load_many`, `load_many_to_many` | `belonging_to`, `load_children`, `load_parent`, `load_via` |
| Declaración de relaciones | `#[diesel(belongs_to)]` + `joinable!` | enum `Relation` + impls `Related` | ninguna: se lee de `references = "table(col)"` |
| Actualización parcial | struct `AsChangeset` | `ActiveModel` con `Set(...)` | `update_only(&db, (User::NAME, User::AGE))` |
| Índices sobre varias columnas | en las migraciones | en las migraciones | `#[memoire(unique(a, b), index(c, d))]`, aplicados en todas las bases |
| Búsqueda de texto completo | PostgreSQL, con el crate `diesel_full_text_search` | expresiones personalizadas | `Column::matches`, la misma regla en las seis bases |
| Notificaciones de cambios | no | hooks `ActiveModelBehavior` por entidad | `Observed::subscribe`: cada escritura, anunciada al confirmar |
| Actualización / borrado masivo con filtros | sí | sí | sí, con `increment`, `set_null`... |
| Inserción múltiple | sí, devuelve las filas en PG/SQLite | sí | sí, por lotes bajo el límite de parámetros de cada base |
| Upsert | `on_conflict` | `on_conflict` | `upsert` (por clave primaria) |
| Paginación | `limit`/`offset` a mano | `paginate`, paginación por cursor | `paginate` (recuento y página en paralelo), `cursor_paginate` |
| Agregados, `GROUP BY` | sí | sí | sí |
| Subconsultas, self-joins (alias) | sí | sí | todavía no: SQL sin procesar |
| Claves primarias compuestas | sí | sí | todavía no: clave artificial + `unique` |
| Resultados en streaming | `load_iter` | `stream` | `chunks(n)`: bloques acotados, por clave primaria |
| Marcas de tiempo, borrado lógico | a mano | a mano (`ActiveModelBehavior`) | `#[memoire(timestamps, soft_deletes)]` |
| Hooks de ciclo de vida | no | `ActiveModelBehavior` (`before_save`...) | `#[memoire(hooks)]` + `impl Hooks` (puede rechazar una escritura) |
| Buscar o crear | a mano | a mano | `first_or_create`, `update_or_create`, `save` |
| Registro de consultas | `set_instrumentation` | logs de `sqlx` | `Logged`: SQL, parámetros, duración, filas, en todas las bases |
| Migraciones | CLI, archivos SQL `up`/`down` | migraciones en Rust con `up`/`down` | Rust o SQL, solo hacia delante, en transacción |
| Errores normalizados | `DatabaseErrorKind` (unicidad, clave foránea...) | `SqlErr` (unicidad, clave foránea) | `UniqueViolation`, `NotFound`, `Constraint`, `Conflict`... en todas las bases, NoSQL incluido |
| Generación de código desde una base existente | sí (`diesel print-schema`) | sí (`sea-orm-cli generate entity`) | no |

## Pasar de Diesel a Memoire

| Diesel | Memoire |
|---|---|
| `users::table.find(1).first::<User>(&mut conn)` | `User::get(&db, 1).await?` |
| `users::table.filter(users::age.gt(18)).load::<User>(&mut conn)` | `User::query().filter(User::AGE.gt(18)).all(&db).await?` |
| `diesel::insert_into(users::table).values(&new_user).execute(&mut conn)` | `user.insert(&db).await?` (el id se escribe en el struct) |
| `diesel::update(&user).set(&changes).execute(&mut conn)` | `user.update_only(&db, (User::NAME, User::AGE)).await?` |
| `Post::belonging_to(&users).load::<Post>(&mut conn)?.grouped_by(&users)` | `users.load_children::<Post>(&db).await?` |
| `conn.transaction(\|conn\| ...)` | `let tx = db.begin().await?; ...; tx.commit().await?` |

## Pasar de SeaORM a Memoire

| SeaORM | Memoire |
|---|---|
| `user::Entity::find_by_id(1).one(&db)` | `User::find(&db, 1).await?` |
| `user::Entity::find().filter(user::Column::Age.gt(18)).all(&db)` | `User::query().filter(User::AGE.gt(18)).all(&db).await?` |
| `active_model.insert(&db)` | `user.insert(&db).await?` |
| `let mut am: user::ActiveModel = u.into(); am.name = Set(..); am.update(&db)` | `u.name = ..; u.update_only(&db, User::NAME).await?` |
| `user.find_related(post::Entity).all(&db)` | `Post::belonging_to(&user).all(&db).await?` |
| `users.load_many(post::Entity, &db)` | `users.load_children::<Post>(&db).await?` |
| `posts.load_many_to_many(tag::Entity, post_tag::Entity, &db)` | `posts.load_via::<PostTag, Tag>(&db).await?` |
| `user::Entity::find().paginate(&db, 20).fetch_page(1)` | `User::query().paginate(&db, 2, 20).await?` (las páginas empiezan en 1) |

## Memoire y Nitrite (NoSQL)

Nitrite es una base de documentos embebida, para Java, Kotlin, Flutter y Rust (el crate `nitrite` llegó a la 1.0). No es un ORM delante de otras bases sino una base dentro de tu programa, como el motor en memoria de Memoire: esa es la comparación que tiene sentido.

| | Nitrite (Rust) | Memoire |
|---|---|---|
| Datos | Documentos sin esquema (`doc!{...}`), anidados, arrays | Structs tipados; datos anidados en columnas `Json<T>` |
| API | Síncrona | Asíncrona |
| Filtros | `field("age").gt(18)`: cualquier valor, comprobado en ejecución | `User::AGE.gt(18)`: comprobado en compilación |
| Guardado en un archivo | Sí (crate adaptador Fjall) | Sí: `MemoryDb::open(path)` |
| Cifrado | AES-GCM | AES-256-GCM con una clave Argon2id (feature `encryption`) |
| Índices | Único, no único, compuesto, texto completo; espacial en un crate aparte | Único, no único, sobre varias columnas, texto completo |
| Eventos de cambio | `collection.subscribe(...)` | `Observed::subscribe`, en todas las bases |
| Transacciones | Sí | Sí (instantáneas, conflictos detectados) |
| Joins, agregados, `GROUP BY` | `join` y `project` sobre un cursor, sin agregados | Sí |
| El mismo código en servidores | No | PostgreSQL, MySQL, SQLite, Redis, MongoDB |
| WebAssembly | No documentado | Sí |
| Licencia | Apache-2.0 | Propietaria, de uso libre |

Medido en la misma máquina con 10.000 documentos, los mismos índices y resultados idénticos (Nitrite 1.0.1 con su almacenamiento en memoria, frente a `MemoryDb`; un bucle de medición, no criterion):

| Operación | Nitrite | Memoire |
|---|---:|---:|
| Insertar 10.000 documentos | 189 ms | **2,96 ms** |
| Buscar por un email único | 11,2 µs | **0,41 µs** |
| Filtrar por un campo indexado (10 filas) | 30,7 µs | **1,70 µs** |
| Rango + filtro + orden + `LIMIT 20` (sin índice) | 10,2 ms | **27 µs** |
| Contar con un filtro (sin índice) | 9,0 ms | **7,7 µs** |

Las dos últimas filas filtran campos sin índice: Nitrite lee entonces documentos dinámicos uno a uno, mientras que Memoire recorre columnas tipadas y contiguas. Un índice en esos campos reduciría la diferencia del lado de Nitrite.

## Cuál elegir

- **Diesel** si quieres las garantías de compilación más fuertes contra el esquema real y la cobertura SQL más madura (subconsultas, claves compuestas, tipos SQL personalizados), y el código síncrono te sirve.
- **SeaORM** si quieres asincronía con un gran ecosistema alrededor (generación de código desde una base existente, GraphQL, herramientas de administración) y su estilo de entidades te convence.
- **Nitrite** si tus datos no tienen una forma fija (documentos anidados, arrays, campos que varían de un documento a otro), si necesitas consultas espaciales o una licencia de código abierto.
- **Memoire** si quieres asincronía con el mínimo de código repetitivo (tu propio struct, un derive), el mismo código en SQL, NoSQL y en el navegador, pruebas que corren en el motor en memoria sin servidor de base de datos, y errores normalizados.

## Próximos pasos

- [Uniones y relaciones](relations): `belonging_to`, `load_children`, `load_parent`, `load_via`.
- [Crear, leer, actualizar, eliminar](crud): `find_many`, `update_only`.
- [Benchmarks](benchmarks): las cifras medidas.
