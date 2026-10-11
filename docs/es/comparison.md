# Memoire, Diesel y SeaORM

Diesel y SeaORM son los dos ORM más usados en Rust. Esta página los compara con Memoire función por función, para elegir la herramienta adecuada o pasar de uno a otro. Intenta ser justa: cada biblioteca va por delante en algo.

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
| Motor en memoria, WebAssembly | no | conexión simulada para pruebas | sí, un motor real con índices y transacciones |
| Joins | sí, tipados | sí | sí, decodificados en tuplas |
| Relaciones sin N+1 | `belonging_to` + `grouped_by` | `find_with_related`, `load_one`, `load_many`, `load_many_to_many` | `belonging_to`, `load_children`, `load_parent`, `load_via` |
| Declaración de relaciones | `#[diesel(belongs_to)]` + `joinable!` | enum `Relation` + impls `Related` | ninguna: se lee de `references = "table(col)"` |
| Actualización parcial | struct `AsChangeset` | `ActiveModel` con `Set(...)` | `update_only(&db, (User::NAME, User::AGE))` |
| Actualización / borrado masivo con filtros | sí | sí | sí, con `increment`, `set_null`... |
| Inserción múltiple | sí, devuelve las filas en PG/SQLite | sí | sí, por lotes bajo el límite de parámetros de cada base |
| Upsert | `on_conflict` | `on_conflict` | `upsert` (por clave primaria) |
| Paginación | `limit`/`offset` a mano | `paginate`, paginación por cursor | `paginate` (recuento y página en paralelo) |
| Agregados, `GROUP BY` | sí | sí | sí |
| Subconsultas, self-joins (alias) | sí | sí | todavía no: SQL sin procesar |
| Claves primarias compuestas | sí | sí | todavía no: clave artificial + `unique` |
| Resultados en streaming | `load_iter` | `stream` | todavía no |
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

## Cuál elegir

- **Diesel** si quieres las garantías de compilación más fuertes contra el esquema real y la cobertura SQL más madura (subconsultas, claves compuestas, tipos SQL personalizados), y el código síncrono te sirve.
- **SeaORM** si quieres asincronía con un gran ecosistema alrededor (generación de código desde una base existente, GraphQL, herramientas de administración) y su estilo de entidades te convence.
- **Memoire** si quieres asincronía con el mínimo de código repetitivo (tu propio struct, un derive), el mismo código en SQL, NoSQL y en el navegador, pruebas que corren en el motor en memoria sin servidor de base de datos, y errores normalizados.

## Próximos pasos

- [Uniones y relaciones](relations): `belonging_to`, `load_children`, `load_parent`, `load_via`.
- [Crear, leer, actualizar, eliminar](crud): `find_many`, `update_only`.
- [Benchmarks](benchmarks): las cifras medidas.
