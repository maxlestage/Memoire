# Transacciones

Una transacción agrupa varios cambios para que se apliquen todos juntos o ninguno. Esta página muestra cómo abrir, confirmar (commit) y revertir (rollback) transacciones, cómo reintentar cuando dos transacciones chocan y cómo se comporta cada backend.

## ¿Por qué usar transacciones?

Mover dinero de una cuenta a otra requiere dos actualizaciones: restar de la primera cuenta y sumar a la segunda. Si el programa se cuelga, o la segunda actualización falla, entre ambas, el dinero desaparece. Dentro de una transacción, o se guardan las dos actualizaciones o no se guarda ninguna.

Las transacciones también te dan una vista coherente de los datos mientras trabajas y hacen que las escrituras masivas sean mucho más rápidas en SQLite.

## Uso básico

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub owner: String,
    pub balance: i64,
}

/// Mueve `amount` de una cuenta a otra.
/// Devuelve `false` (y no cambia nada) si la cuenta de origen no tiene suficiente dinero.
async fn transfer<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let tx = db.begin().await?;

    let source = Account::get(&tx, from).await?;
    if source.balance < amount {
        tx.rollback().await?;
        return Ok(false);
    }

    Account::update_many()
        .decrement(Account::BALANCE, amount)
        .filter(Account::ID.eq(from))
        .execute(&tx)
        .await?;
    Account::update_many()
        .increment(Account::BALANCE, amount)
        .filter(Account::ID.eq(to))
        .execute(&tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}
```

Los tres pasos:

1. `db.begin().await?` inicia una transacción y devuelve un manejador de transacción.
2. Pasa `&tx` en lugar de `&db` a todas las operaciones que forman parte de la transacción. Una transacción es un `Executor`, así que todos los métodos de modelo, las consultas y las llamadas de SQL sin procesar la aceptan.
3. `tx.commit().await?` lo guarda todo. `tx.rollback().await?` lo descarta todo.

`commit` y `rollback` consumen la transacción, así que el compilador te impide usarla después.

`begin` forma parte del trait `Database`, implementado por `MemoryDb`, `SqliteDb` y `PgPool`. Escribir `D: Database` hace que `transfer` funcione con los tres.

## Destruir una transacción la revierte

Si una transacción se destruye (drop) sin `commit`, se revierte. Esto simplifica la gestión de errores: cuando `?` sale antes de tiempo, la transacción queda fuera de alcance y sus cambios se descartan.

```rust
async fn create_two(db: &MemoryDb) -> memoire_orm::Result<()> {
    let tx = db.begin().await?;
    Account { id: 0, owner: "Ada".into(), balance: 10 }.insert(&tx).await?;
    Account::get(&tx, 999).await?; // falla con NotFound: `?` sale antes de tiempo...
    tx.commit().await              // ...así que esta línea nunca se ejecuta y Ada no se guarda
}
```

> **Consejo** Rara vez necesitas un `rollback` explícito: devolver un error con `?` es suficiente. Llama a `rollback` cuando decidas cancelar sin que haya un error, como en `transfer` más arriba.

## Funciones que funcionan con o sin transacción

Escribe las funciones auxiliares contra el trait `Executor`. Así aceptan un manejador de base de datos *o* una transacción, y es quien llama el que decide:

```rust
async fn open_account<E: Executor>(db: &E, owner: &str) -> memoire_orm::Result<Account> {
    let mut account = Account { id: 0, owner: owner.into(), balance: 0 };
    account.insert(db).await?;
    Ok(account)
}

async fn open_family_accounts(db: &MemoryDb) -> memoire_orm::Result<()> {
    open_account(db, "standalone").await?; // se ejecuta por su cuenta

    let tx = db.begin().await?;
    open_account(&tx, "parent").await?; // ambas forman parte de la misma transacción
    open_account(&tx, "child").await?;
    tx.commit().await
}
```

## Comportamiento en cada backend

| | Motor en memoria | SQLite | PostgreSQL |
|---|---|---|---|
| Cómo funciona | Instantánea privada con copia en escritura (copy-on-write) | Retiene la única conexión | Retiene una conexión del pool |
| El resto del código durante la transacción | Se ejecuta con normalidad y solo ve los datos confirmados | **Espera** hasta que termina la transacción | Se ejecuta en otras conexiones y solo ve los datos confirmados |
| Escritores en conflicto | Se detecta en el `commit`: `Error::Conflict` | Imposible: las transacciones se ejecutan de una en una | Espera a los bloqueos de fila; los fallos de serialización y los interbloqueos dan `Error::Conflict` |
| Destruida sin commit | Los cambios se descartan inmediatamente | `ROLLBACK` se ejecuta inmediatamente | `ROLLBACK` se envía en segundo plano (necesita Tokio) |

### Motor en memoria: instantáneas y conflictos

`MemoryDb::begin` toma una instantánea (snapshot) de la base de datos. La transacción lee y escribe en su propia copia (las tablas solo se copian la primera vez que se escriben), así que nunca ve los cambios concurrentes y nadie ve sus cambios antes del `commit`.

En el momento del commit, Memoire comprueba si otro escritor ha cambiado alguna tabla en la que esta transacción también ha escrito. Si es así, el commit falla con `Error::Conflict` y no se aplica nada: gana el primer escritor.

```rust
let t1 = db.begin().await?;
let t2 = db.begin().await?;
Account::update_many().increment(Account::BALANCE, 10).execute(&t1).await?;
Account::update_many().increment(Account::BALANCE, 20).execute(&t2).await?;

t1.commit().await?;                       // gana el primer escritor
let err = t2.commit().await.unwrap_err(); // el segundo es rechazado
assert!(err.is_conflict());
```

Conviene saber que:

- Los conflictos se detectan **por tabla**, no por fila: dos transacciones que escriben filas distintas de la misma tabla también entran en conflicto.
- Una transacción que solo lee nunca entra en conflicto.
- Cuenta cualquier escritura en la tabla, incluidas las hechas directamente sobre `db` fuera de una transacción.

### Reintentar en caso de conflicto

Un conflicto no es un bug: significa "alguien ha cambiado estos datos mientras trabajabas; vuelve a empezar con datos frescos". Como una transacción fallida no aplica nada, reintentar siempre es seguro. Envuelve toda la transacción en un bucle:

```rust
async fn transfer_with_retry<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match transfer(db, from, to, amount).await {
            Err(err) if err.is_conflict() && attempts < 5 => continue, // vuelve a intentarlo con datos frescos
            result => return result,
        }
    }
}
```

Reintenta la función *completa*, incluidas las lecturas: el nuevo intento debe iniciar una nueva transacción y ver los datos más recientes. El mismo bucle funciona en PostgreSQL, donde los fallos de serialización y los interbloqueos también se notifican como `Error::Conflict`.

### SQLite: una transacción cada vez

Un `SqliteDb` usa una única conexión. Una transacción se queda con esa conexión hasta que se confirma, se revierte o se destruye, así que las transacciones de SQLite nunca entran en conflicto: simplemente se ejecutan una detrás de otra.

> **Atención** Mientras una transacción de SQLite está abierta, retiene la única conexión. Una consulta lanzada sobre el propio manejador `SqliteDb` (`&db` en lugar de `&tx`) espera hasta que termine la transacción, así que dentro de la misma tarea espera para siempre: un interbloqueo (deadlock). Dentro de una transacción, pasa siempre `&tx`:
>
> ```rust,ignore
> let tx = db.begin().await?;
> Account::count(&db).await?; // nunca termina: usa `&tx` aquí
> ```

Las demás tareas o hilos que usan el mismo `SqliteDb` simplemente esperan a que termine la transacción, así que mantén las transacciones cortas. Para inserciones masivas, envolver miles de inserciones en una sola transacción es muchísimo más rápido que insertarlas una a una.

### PostgreSQL: conexiones del pool

`PgPool::begin` toma una conexión del pool y la conserva hasta que termina la transacción. Las demás consultas siguen ejecutándose en otras conexiones y solo ven los datos confirmados.

- El nivel de aislamiento es el predeterminado de PostgreSQL, `READ COMMITTED`. Para subirlo, ejecuta `SET TRANSACTION` como primera sentencia de la transacción:

  ```rust
  async fn strict(db: &PgPool) -> memoire_orm::Result<()> {
      let tx = db.begin().await?;
      tx.raw_execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", &[]).await?;
      // ... lecturas y escrituras ...
      tx.commit().await
  }
  ```

  Con `REPEATABLE READ` y `SERIALIZABLE`, PostgreSQL puede abortar una transacción que choca con otra. Memoire lo notifica como `Error::Conflict`, así que el bucle de reintento de arriba se aplica sin cambios. Los interbloqueos se notifican de la misma forma.
- Una transacción destruida se revierte en segundo plano en el runtime de Tokio. Sin runtime, en su lugar se cierra la conexión, lo que también cancela la transacción.
- Usar `&db` en lugar de `&tx` dentro de una transacción no bloquea, pero se ejecuta en otra conexión: no ve los cambios sin confirmar de la transacción y no forma parte de ella.

## Limitaciones

- **Sin transacciones anidadas.** Una transacción no puede iniciar otra transacción (no hay API de savepoints). En su lugar, pasa el `&tx` existente a las funciones auxiliares.
- Se devuelve `Error::TransactionFinished` si se usa una transacción después de haber terminado, algo que la API normalmente hace imposible.

## Próximos pasos

- [Migraciones](migrations): cada migración se ejecuta en su propia transacción.
- [Motor en memoria](memory), [SQLite](sqlite) y [PostgreSQL](postgres): detalles de cada backend.
- [Por qué es rápido](performance): transacciones y escrituras masivas.
