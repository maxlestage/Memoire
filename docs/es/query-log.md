# Registro de consultas

Envuelve una conexión en `Logged` para registrar cada sentencia ejecutada a través de ella, sus clones y sus transacciones: el SQL, los parámetros, la duración y el número de filas. Es `DB::enableQueryLog()` / `DB::listen()` de Laravel y el logger SQL de Doctrine, en todas las bases de datos.

```rust
use memoire_orm::log::Logged;
use memoire_orm::prelude::*;

let db = Logged::new(SqliteDb::open("app.db")?); // o MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let users = User::query().filter(User::ACTIVE.eq(true)).all(&db).await?;

for q in db.queries() {
    println!("{q}");
}
// SELECT "users"."id", ... FROM "users" WHERE "users"."active" = ? -- [TRUE] (0.041 ms)
```

`Logged<D>` es a su vez una base de datos: pasa `&db` a los métodos de los modelos, a las consultas y a las transacciones como antes.

## Contenido de una entrada

| Campo | Contenido |
|---|---|
| `sql` | La sentencia, en el dialecto de la base (`BEGIN`, `COMMIT`, `ROLLBACK` para las transacciones) |
| `params` | Los parámetros enlazados, en orden |
| `duration` | Tiempo entre la llamada y el resultado (`None` en WebAssembly) |
| `rows` | Filas devueltas por una consulta, o afectadas por una escritura; `None` si falló |
| `error` | El mensaje de error, si la sentencia falló |
| `in_transaction` | Si se ejecutó dentro de una transacción |

El motor en memoria, Redis y MongoDB no ejecutan SQL: su registro muestra la sentencia equivalente en el dialecto de SQLite, igual de útil para ver qué hace un fragmento de código.

## Encontrar consultas N+1 en un test

```rust
let db = Logged::new(MemoryDb::new());
// ... datos de prueba ...
db.clear();

let users = User::all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

assert_eq!(db.query_count(), 2, "una consulta por nivel, sea cual sea el número de usuarios");
```

| Método | Efecto |
|---|---|
| `queries()` | Copia de las entradas, de la más antigua a la más reciente |
| `take_queries()` | Las entradas, vaciando el registro |
| `query_count()` | Número de entradas |
| `clear()` | Vacía el registro |
| `set_recording(false)` / `set_recording(true)` | Deja de guardar / vuelve a guardar las entradas |
| `listen(\|q\| ...)` | Llama a una función con cada sentencia, cuando se ejecuta |
| `Logged::with_limit(db, n)` | Guarda solo las `n` últimas entradas |

## Mostrar cada sentencia

En un programa que se ejecuta mucho tiempo, no guardes nada y muestra las sentencias:

```rust
let db = Logged::with_limit(PgPool::connect(&url).await?, 0);
db.listen(|q| {
    if q.duration.is_some_and(|d| d.as_millis() > 100) {
        eprintln!("consulta lenta: {q}");
    }
});
```

Los oyentes se ejecutan en la tarea que lanzó la sentencia, justo después: mantenlos rápidos.

> **Consejo** El registro convierte cada sentencia en texto y copia sus parámetros, lo que cuesta un poco en cada llamada. Úsalo en desarrollo y en los tests, o con un oyente que filtre lo que guarda.

## Próximos pasos

- [Notificaciones de cambios](events): enterarse de las escrituras, con las filas escritas.
- [SQL sin procesar](raw-sql): ejecutar tus propias sentencias.
