# Bloqueo optimista e identity map

Dos herramientas de Doctrine (y del `lock_version` de ActiveRecord) para aplicaciones donde varias personas o tareas escriben las mismas filas.

## Bloqueo optimista

Alice y Bob abren la misma cuenta a la vez. Alice guarda; después Bob guarda y sobrescribe sin aviso el cambio de Alice. El bloqueo optimista evita esa actualización perdida: marca un campo entero con `#[memoire(version)]`, y cada escritura comprueba que la fila sigue teniendo la versión que leíste.

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub balance: i64,
    #[memoire(version)]
    pub version: i32,
}

let mut alice = Account::get(&db, 1).await?;
let mut bob = Account::get(&db, 1).await?;

alice.balance -= 30;
alice.save(&db).await?;                   // versión 0 -> 1, escrita de vuelta en `alice`

bob.balance += 10;
match bob.save(&db).await {
    Err(e) if e.is_stale() => {
        bob.reload(&db).await?;           // leer la nueva versión y repetir el cambio
        bob.balance += 10;
        bob.save(&db).await?;
    }
    other => other?,
}
```

| Operación | Con `#[memoire(version)]` |
|---|---|
| `save`, `update`, `update_only` | `UPDATE ... SET ..., version = version + 1 WHERE id = ? AND version = ?`; `Error::StaleRow` si la versión cambió |
| `delete`, `force_delete` (y borrados lógicos) | solo si la versión no cambió, `Error::StaleRow` si no |
| `update_many()` | incrementa la versión de cada fila actualizada, para que las copias leídas antes queden obsoletas |
| `insert`, `upsert`, `insert_many` | escriben la versión tal cual (empieza en `0`) |

`save` escribe la nueva versión de vuelta en el struct. `update` y `update_only` lo toman prestado, así que no pueden: después de ellos, haz `reload` (o usa `save`). Una fila inexistente sigue dando `Error::NotFound`; `Error::is_stale()` distingue los dos casos. El campo de versión puede ser `i16`, `i32`, `i64` o un `Option` de ellos (una versión `NULL` pasa a 1); cualquier otro tipo se rechaza al compilar.

## Identity map

Un `IdentityMap` lee cada fila una sola vez y devuelve el mismo objeto compartido (`Arc<M>`) en cada búsqueda posterior de su clave, como `EntityManager::find` de Doctrine. Usa uno por unidad de trabajo: una petición, una tarea, un test.

```rust
use memoire_orm::identity::IdentityMap;

let map = IdentityMap::new();
let a = map.find::<User, _>(&db, 1).await?;           // una consulta
let b = map.find::<User, _>(&db, 1).await?;           // ninguna: el mismo Arc
let team = map.find_many::<User, _, _>(&db, [1, 2, 3]).await?; // una consulta, solo para 2 y 3
let rows = map.merge(User::query().all(&db).await?);  // una fila ya conocida sigue siendo el mismo objeto

map.forget::<User>(1);   // después de cambiarla o borrarla
map.clear();             // EntityManager::clear
```

El mapa es una caché que nunca caduca y no ve las escrituras hechas en otro sitio: que viva poco, y haz `remember` (o `forget`) de una fila después de escribirla.

## Próximos pasos

- [Transacciones](transactions): haz varias escrituras todo o nada.
- [Factories y seeders](factories)
