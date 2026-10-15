# Marcas de tiempo, borrado lógico y hooks

Tres comportamientos que se activan modelo a modelo con un atributo, al estilo de Eloquent de Laravel (`$timestamps`, `SoftDeletes`, eventos de modelo) y de los callbacks de ciclo de vida de Doctrine (`prePersist`, `preUpdate`, `preRemove`...). Funcionan igual en todas las bases de datos, y un modelo que no los pide se comporta exactamente como antes.

```rust
use memoire_orm::lifecycle::Hooks;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "articles", timestamps, soft_deletes, hooks)]
pub struct Article {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub created_at: Option<i64>, // milisegundos desde la época Unix
    pub updated_at: Option<i64>,
    pub deleted_at: Option<i64>,
}
```

## Marcas de tiempo automáticas

`#[memoire(timestamps)]` en la estructura usa los campos llamados `created_at` y `updated_at`. Para otros nombres, marca los campos: `#[memoire(created_at)]` y `#[memoire(updated_at)]` (uno de cada).

| Operación | `created_at` | `updated_at` |
|---|---|---|
| `insert`, `save` de una fila nueva | se rellena si está vacío (`None`, `0` o `""`), y se escribe en la estructura | igual |
| `update` | sin cambios | ahora (en la base; la estructura prestada conserva su valor) |
| `save` de una fila existente | sin cambios | ahora, escrito en la estructura |
| `update_only(&db, columnas)` | sin cambios | ahora, añadido a las columnas |
| `touch(&mut self, &db)` | sin cambios | ahora, y nada más |
| `update_many()` | sin cambios | ahora, salvo que lo fijes con `set` |
| `insert_many` | se rellena si está vacío | se rellena si está vacío |
| `upsert` | se rellena si está vacío | ahora |
| `delete` lógico, `restore` | sin cambios | ahora |

```rust
let mut a = Article { id: 0, title: "Hola".into(), slug: String::new(),
                      created_at: None, updated_at: None, deleted_at: None };
a.insert(&db).await?;
assert!(a.created_at.is_some() && a.updated_at == a.created_at);

a.title = "Hola, mundo".into();
a.save(&db).await?; // updated_at avanza, en la base y en `a`
```

El valor depende del tipo del campo; cualquier otro tipo se rechaza al compilar:

| Tipo del campo | Valor guardado |
|---|---|
| `i64` | milisegundos desde la época Unix |
| `i32` | segundos desde la época Unix |
| `f64` | segundos desde la época Unix, con fracción |
| `String` | RFC 3339 en UTC, `2026-10-15T08:30:00.123Z` (se ordena como el tiempo) |
| `chrono::DateTime<Utc>`, `NaiveDateTime` (UTC), `NaiveDate` | ahora (feature `chrono`) |

Cada uno puede ser un `Option<...>`, la forma más clara de decir «todavía sin valor». En WebAssembly, activa la feature `web` para que la hora venga del reloj del navegador.

## Borrado lógico (soft delete)

Con `#[memoire(soft_deletes)]` (un campo nulable llamado `deleted_at`, o cualquier campo de tiempo `Option<...>` marcado `#[memoire(soft_delete)]`), borrar una fila solo rellena esa columna, y la fila desaparece de todas las consultas construidas con `Model::query()`, incluidas `find`, `get`, `count`, `paginate` y la carga de relaciones:

```rust
a.delete(&db).await?;                                     // UPDATE ... SET deleted_at = ahora
assert_eq!(Article::find(&db, a.id).await?, None);         // oculta
assert_eq!(Article::with_trashed().count(&db).await?, 1);  // sigue guardada
let papelera = Article::only_trashed().all(&db).await?;    // solo las filas borradas

let mut a = papelera[0].clone();
assert!(a.trashed());
a.restore(&db).await?;      // deleted_at vuelve a NULL
a.force_delete(&db).await?; // DELETE, definitivo
```

| Método | Efecto |
|---|---|
| `fila.delete(&db)` / `Model::delete_by_key(&db, clave)` | Borrado lógico; `false` si ya estaba borrada |
| `Model::delete_many().filter(...).execute(&db)` | Borra lógicamente las filas encontradas |
| `Model::delete_many().force()...` / `fila.force_delete(&db)` | `DELETE` real |
| `fila.restore(&db)` | Vacía `deleted_at` |
| `fila.trashed()` | `true` si `deleted_at` tiene valor |
| `Model::with_trashed()`, `query().with_trashed()` | Todas las filas |
| `Model::only_trashed()`, `query().only_trashed()` | Solo las filas borradas |
| `query().without_trashed()` | Vuelta al comportamiento por defecto |

El ámbito se aplica a toda la condición: `filter(a).or_filter(b)` pasa a ser `deleted_at IS NULL AND (a OR b)`, nunca una fila borrada que cumpla `b`. `update_many` y `reload` ven todas las filas, como una tabla unida.

> **Consejo** Una columna `unique` sigue contando las filas borradas lógicamente: un usuario borrado conserva su email. Restaura la fila, o bórrala con `force_delete`, antes de reutilizar el valor.

## Hooks de ciclo de vida

Añade `hooks` al atributo de la estructura e implementa `memoire_orm::lifecycle::Hooks`, redefiniendo solo los métodos que necesites:

```rust
impl Hooks for Article {
    fn before_save(&mut self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(Error::Validation("un artículo necesita un título".into()));
        }
        self.slug = self.title.to_lowercase().replace(' ', "-");
        Ok(())
    }

    fn after_delete(&self) {
        println!("artículo {} borrado", self.id);
    }
}
```

| Hook | Se ejecuta en | Doctrine / Eloquent |
|---|---|---|
| `before_save(&mut self)` | `insert`, `save` | `saving` |
| `before_insert(&mut self)` / `after_insert(&self)` | `insert`, `save` de una fila nueva | `prePersist` / `postPersist`, `creating` / `created` |
| `before_update(&self)` / `after_update(&self)` | `update`, `update_only`, `save` de una fila existente | `preUpdate` / `postUpdate`, `updating` / `updated` |
| `before_delete(&self)` / `after_delete(&self)` | `delete` (lógico también), `force_delete` | `preRemove` / `postRemove`, `deleting` / `deleted` |

Un hook `before_*` que devuelve un error cancela la escritura: no se envía nada a la base y el error vuelve a quien llamó. `Error::Validation(mensaje)` está pensado para eso (`err.is_validation()`).

Los hooks son síncronos y solo ven la fila. Las escrituras de muchas filas a la vez (`insert_many`, `upsert`, `update_many`, `delete_many`) no ejecutan ninguno, como las actualizaciones masivas de Eloquent. Para reaccionar a cada escritura con acceso a la base (registro de auditoría, caché), usa las [notificaciones de cambios](events).

## `save`

`save(&mut self, &db)` es la llamada única para «guardar esta fila», como `save()` de Eloquent: una fila cuya clave `auto_increment` aún vale `0` se inserta, cualquier otra se actualiza (y se inserta si ninguna fila tiene su clave). Ejecuta `before_save`, después los hooks de inserción o de actualización, y escribe las marcas de tiempo en la estructura.

## Próximos pasos

- [Crear, leer, actualizar, eliminar](crud): `first_or_create`, `update_or_create` y las demás escrituras.
- [Constructor de consultas](queries): lectura por bloques y paginación por cursor.
- [Registro de consultas](query-log): ver las sentencias que ejecutan estos métodos.
