# Chuleta

Todas las operaciones habituales de Memoire en una sola página. Los ejemplos dan por hecho `use memoire_orm::prelude::*;`, un manejador de base de datos `db` y los modelos `User` (`id`, `email`, `name`, `age: Option<i32>`, `active: bool`, `visits: i32`) y `Post` (`id`, `user_id`, `title`, `views: i32`).

## Configuración

| Tarea | Código |
|---|---|
| Motor en memoria (feature por defecto) | `cargo add memoire-orm` |
| SQLite | `cargo add memoire-orm --features sqlite` |
| PostgreSQL / con TLS | `--features postgres` / `--features postgres-tls` (más Tokio) |
| MySQL, MariaDB / con TLS | `--features mysql` / `--features mysql-tls` (más Tokio) |
| Redis / con TLS | `--features redis` / `--features redis-tls` (más Tokio) |
| MongoDB | `--features mongodb` (más Tokio) |
| Tipos de columna adicionales | `--features chrono,uuid,json` (o `full`) |
| Importarlo todo | `use memoire_orm::prelude::*;` |
| Abrir en memoria | `let db = MemoryDb::new();` |
| Abrir SQLite en archivo / en memoria | `SqliteDb::open("app.db")?` / `SqliteDb::open_in_memory()?` |
| Conectarse a PostgreSQL | `PgPool::connect(url).await?` o `PgPool::connect_with(url, 10).await?` |
| Conectarse a MySQL / MariaDB | `MySqlPool::connect("mysql://user:pass@host:3306/app").await?` |
| Conectarse a Redis | `RedisDb::connect(url).await?` o `RedisDb::connect_with_prefix(url, "myapp").await?` |
| Conectarse a MongoDB | `MongoDb::connect("mongodb://host:27017/app").await?` o `MongoDb::connect_with_database(uri, "app").await?` |
| Ejecutar sin runtime (memoria, SQLite) | `memoire_orm::block_on(future)` |

Páginas de los backends: [Motor en memoria](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL y MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).

## Atributos de modelo

| Atributo | Ejemplo |
|---|---|
| Nombre de la tabla | `#[memoire(table = "users")]` sobre el struct |
| Clave primaria | `#[memoire(primary_key)]` (un campo llamado `id` es la clave por defecto) |
| Clave generada | `#[memoire(primary_key, auto_increment)]` (implícito para un `id` entero sin atributos) |
| Única / índice | `#[memoire(unique)]`, `#[memoire(index)]` |
| Nombre de la columna | `#[memoire(column = "e_mail")]` |
| Tipo SQL | `#[memoire(sql_type = "VARCHAR(120)")]` |
| Valor por defecto | `#[memoire(default = "0")]` |
| Clave foránea | `#[memoire(references = "users(id)", on_delete = "cascade")]` |
| No almacenado | `#[memoire(skip)]` (el tipo del campo debe implementar `Default`) |
| Columna que admite nulos | `age: Option<i32>` |
| Tipo de fila personalizado | `#[derive(FromRow)] struct Stats { user_id: i64, posts: i64 }` |

## Esquema

| Tarea | Código |
|---|---|
| Crear una tabla | `db.create_table::<User>().await?` |
| Crearla si no existe | `db.create_table_if_not_exists::<User>().await?` |
| Eliminar una tabla | `db.drop_table::<User>().await?` / `db.drop_table_if_exists::<User>().await?` |
| Ver el DDL | `memoire_orm::sql::create_table(User::SCHEMA, Dialect::Postgres, false)` |
| Ver el SQL por clave primaria | `User::SCHEMA.sql(Dialect::Sqlite).insert` (también `find`, `update`, `delete`, `upsert`) |

## Crear

| Tarea | Código |
|---|---|
| Insertar una fila (el id se escribe de vuelta) | `user.insert(&db).await?` (`user` debe ser `mut`) |
| Insertar muchas (devuelve el recuento) | `User::insert_many(&db, &users).await?` |
| Insertar o reemplazar por clave | `user.upsert(&db).await?` |

## Leer

| Tarea | Código |
|---|---|
| Por clave, opcional | `User::find(&db, 1).await?` → `Option<User>` |
| Por clave, obligatoria | `User::get(&db, 1).await?` → `User` o `Error::NotFound` |
| Todas las filas | `User::all(&db).await?` |
| Contar todas las filas | `User::count(&db).await?` |
| Refrescar un struct | `user.reload(&db).await?` |
| Empezar una consulta | `User::query()` |

## Filtros

| SQL | Código |
|---|---|
| `=`, `<>` | `User::NAME.eq("Ada")`, `User::NAME.ne("Ada")` |
| `<`, `<=`, `>`, `>=` | `User::AGE.lt(18)`, `.le(18)`, `.gt(18)`, `.ge(18)` |
| `AND`, `OR`, `NOT` | `a & b`, `a \| b`, `!a` |
| Varios filtros (AND) | `.filter(a).filter(b)` |
| OR con los filtros anteriores | `.or_filter(c)` |
| `IN`, `NOT IN` | `User::ID.is_in([1, 2, 3])`, `User::ID.not_in(ids)` |
| `BETWEEN` (inclusivo) | `User::AGE.between(18, 30)`, `.not_between(18, 30)` |
| `IS NULL`, `IS NOT NULL` | `User::AGE.is_null()`, `User::AGE.is_not_null()` |
| Columna = columna | `Post::USER_ID.eq_col(User::ID)` (también `ne_col`, `lt_col`, ...) |
| AND / OR de una lista | `Expr::all(conditions)`, `Expr::any(conditions)` |
| Aritmética | `User::VISITS.mul(2).gt(Expr::val(10))` (también `add`, `sub`, `div`) |

## Búsqueda de texto

| Coincidencia | Código |
|---|---|
| Patrón (`%`, `_`) | `User::EMAIL.like("%@example.com")`, `.not_like(...)` |
| Patrón, sin distinguir mayúsculas | `User::EMAIL.ilike("%@EXAMPLE.com")` |
| Contiene (escapado) | `User::NAME.contains("ad")`, `User::NAME.icontains("AD")` |
| Empieza / termina por (escapado) | `User::NAME.starts_with("A")`, `User::EMAIL.ends_with(".org")` |

## Ordenación y paginación

| Tarea | Código |
|---|---|
| Ascendente / descendente | `.order_by(User::NAME)`, `.order_by(User::NAME.asc())`, `.order_by(User::AGE.desc())` |
| Posición de los NULL | `.order_by(User::AGE.desc().nulls_last())`, `.nulls_first()` |
| Límite / desplazamiento | `.limit(20)`, `.offset(40)` |
| Página (empieza en 1) | `.page(3, 20)` |
| Página con total | `.paginate(&db, 3, 20).await?` → `Page { items, page, per_page, total }` |
| Filas distintas | `.distinct()` |

## Ejecutar una consulta

| Resultado | Código |
|---|---|
| Todas las filas | `.all(&db).await?` → `Vec<T>` |
| Primera fila | `.first(&db).await?` → `Option<T>` |
| Exactamente una | `.one(&db).await?` → `T` o `Error::NotFound` |
| Recuento | `.count(&db).await?` → `u64` |
| ¿Hay alguna coincidencia? | `.exists(&db).await?` → `bool` |

## Agregados y proyecciones

| Tarea | Código |
|---|---|
| Suma | `User::query().sum(User::VISITS, &db).await?` → `Option<i64>` (`f64` para flotantes) |
| Media | `.avg(User::AGE, &db).await?` → `Option<f64>` |
| Mínimo / máximo | `.min(User::AGE, &db).await?`, `.max(User::NAME, &db).await?` |
| Una columna | `User::query().columns(User::EMAIL).all(&db).await?` → `Vec<String>` |
| Varias columnas | `.columns((User::ID, User::NAME))` → `Vec<(i64, String)>` (hasta 8) |
| Recuento por grupo | `Post::query().group_by(Post::USER_ID).select_exprs([Post::USER_ID.expr(), count_all()])` → filas `(i64, i64)` |
| Filtrar grupos | `.having(count_all().gt(Expr::val(1_i64)))` |
| Expresiones de agregación | `count_all()`, `col.count()`, `col.count_distinct()`, `col.sum()`, `col.avg()`, `col.min()`, `col.max()` |

## Joins

| Tarea | Código | Filas |
|---|---|---|
| Inner join | `Post::query().inner_join::<User>(User::ID.eq_col(Post::USER_ID))` | `(Post, User)` |
| Left join | `User::query().left_join::<Post>(Post::USER_ID.eq_col(User::ID))` | `(User, Option<Post>)` |
| Join solo para filtrar | `User::query().join::<Post>(JoinKind::Inner, cond).distinct()` | `User` |
| Columnas de varias tablas | `.inner_join::<User>(cond).columns((User::NAME, Post::TITLE))` | `(String, String)` |
| Cargar los hijos en una sola consulta | `Post::query().filter(Post::USER_ID.is_in(user_ids))` | `Post` |

## Actualizar y eliminar

| Tarea | Código |
|---|---|
| Guardar una fila (todas las columnas) | `user.update(&db).await?` (`Error::NotFound` si no existe) |
| Actualizar muchas | `User::update_many().set(User::ACTIVE, false).filter(...).execute(&db).await?` → `u64` |
| Asignar NULL | `.set_null(User::AGE)` |
| Incrementar / decrementar | `.increment(User::VISITS, 1)`, `.decrement(User::VISITS, 1)` |
| Asignar a partir de una expresión | `.set_expr(User::VISITS, User::VISITS.mul(2))` |
| Eliminar una fila | `user.delete(&db).await?` → `bool` |
| Eliminar por clave | `User::delete_by_key(&db, 1).await?` → `bool` |
| Eliminar muchas | `User::delete_many().filter(...).execute(&db).await?` → `u64` |

## Transacciones

| Tarea | Código |
|---|---|
| Iniciar | `let tx = db.begin().await?;` |
| Usarla | pasa `&tx` en lugar de `&db` |
| Confirmar / revertir | `tx.commit().await?` / `tx.rollback().await?` |
| Rollback automático | destruye `tx` (por ejemplo, saliendo antes de tiempo con `?`) |
| Reintentar en caso de conflicto | `Err(e) if e.is_conflict() => continue` en un bucle que envuelva toda la transacción |
| Código genérico | `async fn f<E: Executor>(db: &E)` (manejador o transacción), `async fn g<D: Database>(db: &D)` (puede hacer `begin`) |

## Migraciones

| Tarea | Código |
|---|---|
| Definir | `Migration::new(1, "create_users").create_table::<User>()` |
| Otros pasos | `.drop_table("old")`, `.sql("...")`, `.sql_for(Dialect::Postgres, "...")` |
| Ejecutar las pendientes | `Migrator::new().add(m1).add(m2).run(&db).await?` → versiones aplicadas |
| Inspeccionar | `migrator.applied(&db).await?`, `migrator.pending(&db).await?` |

## SQL sin procesar y acceso al driver

| Tarea | Código |
|---|---|
| Consulta | `db.raw::<(i64, String)>("SELECT id, name FROM users WHERE age > ?", &params![18]).await?` |
| Ejecución | `db.raw_execute("DELETE FROM users WHERE active = ?", &params![false]).await?` → `u64` |
| Sin parámetros | `db.raw::<i64>("SELECT COUNT(*) FROM users", &[]).await?` |
| Marcadores | SQLite `?`, PostgreSQL `$1`, `$2`, ... |
| Conexión SQLite | `db.with_connection(\|conn\| ...).await` |
| Pool de PostgreSQL | `db.pool()` |

## Errores

| Comprobación | Código |
|---|---|
| Fila inexistente | `err.is_not_found()` / `Error::NotFound` |
| Clave o valor único duplicado | `err.is_unique_violation()` / `Error::UniqueViolation { table, column }` |
| Conflicto de transacción | `err.is_conflict()` / `Error::Conflict(table)` |
| Otra restricción | `Error::Constraint(message)` |
| NULL en un campo que no es `Option` | `Error::UnexpectedNull { column }` |
| No compatible con el backend | `Error::Unsupported(message)` |

## Inspeccionar el SQL

| Tarea | Código |
|---|---|
| SQL de una consulta | `User::query().filter(...).to_sql(Dialect::Postgres)` |
| SQL y parámetros | `.to_sql_with_params(Dialect::Sqlite)` → `(String, Vec<Value>)` |
| SQL de update / delete | `User::update_many().set(...).to_sql(Dialect::MySql)`, `User::delete_many().to_sql(...)` |

## Próximos pasos

- [Constructor de consultas](queries): los detalles que hay detrás de cada línea.
- [Preguntas frecuentes](faq): respuestas a las preguntas habituales.
- [Inicio rápido](quick-start): míralo todo en acción.
