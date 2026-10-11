# Notificaciones de cambios

Envuelve una base en `Observed` para enterarte de cada escritura hecha a través de ella: refrescar una caché, enviar actualizaciones en vivo a una página web, llevar un registro de auditoría o lanzar un trabajo en segundo plano. Funciona igual en todas las bases.

```rust
use memoire_orm::prelude::*;

let db = Observed::new(SqliteDb::open("app.db")?); // o MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let _subscription = db.subscribe(|change| {
    println!("{:?} en {}: {} fila(s), clave {:?}", change.kind, change.table, change.rows, change.key);
});

let mut alice = User { id: 0, name: "Alice".into() };
alice.insert(&db).await?; // Insert en users: 1 fila(s), clave Some(I64(1))
alice.delete(&db).await?; // Delete en users: 1 fila(s), clave Some(I64(1))
```

`Observed<D>` es en sí mismo una base: pasa `&db` a los métodos de los modelos, a las consultas y a las transacciones como antes. Sus clones comparten los oyentes.

## Qué contiene un cambio

| Campo | Contenido |
|---|---|
| `table` | La tabla escrita (vacía para SQL sin procesar) |
| `kind` | `Insert`, `Update`, `Upsert`, `Delete`, `CreateTable`, `DropTable` o `Sql` |
| `rows` | Número de filas afectadas |
| `key` | La clave primaria, para `insert`, `update`, `upsert`, `delete` y `delete_by_key`; `None` para las sentencias sobre varias filas (`insert_many`, `update_many`, `update_only`, `delete_many`) |
| `row` | Todas las columnas tal como se escribieron, en el orden del esquema, para `insert`, `update` y `upsert` (con la clave generada) |

Las lecturas, y las escrituras que no cambian nada (un `update_many` que no encuentra filas, borrar una clave inexistente), no se anuncian.

## Transacciones

Los cambios hechos en una transacción se anuncian al confirmarla, en orden, y nunca si se revierte o se descarta:

```rust
let tx = db.begin().await?;
alice.insert(&tx).await?; // nada todavía
tx.commit().await?;       // ahora: Insert en users
```

## Cancelar la suscripción, canales

`subscribe` devuelve una `Subscription`: el oyente sigue registrado hasta que se descarta (drop) o se llama a `unsubscribe()`. Los oyentes se ejecutan en la tarea que escribió, justo después de la escritura: mantenlos rápidos. Para un trabajo lento, recibe los cambios en un canal y procésalos en otro lugar:

```rust
let (subscription, changes) = db.channel();
std::thread::spawn(move || {
    for change in changes {
        // enviar un correo, actualizar un índice de búsqueda...
    }
});
```

`db.listener_count()` indica cuántos oyentes hay registrados.

## Límites

- Solo se ven las escrituras hechas **a través de la base observada** (sus clones y sus transacciones). Otro programa que escriba en la misma base no se ve, ni el código que usa directamente la base envuelta (`db.inner()`).
- Un cambio se anuncia cuando la escritura ha tenido éxito; para una transacción, cuando la confirmación ha tenido éxito.
- El SQL sin procesar (`raw_execute`, scripts de migración) se anuncia como `ChangeKind::Sql`, sin nombre de tabla.

## Próximos pasos

- [Transacciones](transactions): confirmar, revertir y reintentar.
- [Motor en memoria](memory): una base que puede vivir en un archivo.
- [Chuleta](cheatsheet): todo en una página.
