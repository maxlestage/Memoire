# Almacenamiento del navegador: un store al estilo Pinia y localStorage

Las aplicaciones Rust que se ejecutan en el navegador (WebAssembly, con Active, Leptos, Yew, Dioxus o simplemente `wasm-bindgen`) a menudo necesitan guardar cosas en `localStorage`: un carrito, preferencias, un borrador, una lista de tareas. La feature `web` aporta dos herramientas para ello:

- **`Store<S>`**: estado de la aplicación al estilo de [Pinia](https://pinia.vuejs.org/). Un store por identificador, leído con getters, modificado con acciones, guardado en cada cambio, restaurado en la siguiente visita y sincronizado entre pestañas.
- **`LocalStorageDb`**: una base Memoire completa en `localStorage`. Modelos, consultas, joins, transacciones, índices únicos y búsqueda de texto completo funcionan como en las demás bases; cada fila es un valor JSON legible.

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features web
cargo add serde --features derive
```

Ambos funcionan sobre `LocalStorage` y `SessionStorage` del navegador (wasm32) y sobre `MemoryStorage`, que se comporta igual (cadenas, cuota, pestañas y sus eventos) para pruebas, renderizado en el servidor y programas nativos.

## Un store, como Pinia

El estado es un struct simple; los getters son sus métodos; las acciones lo modifican con `update`:

```rust
use memoire_orm::web::Store;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Cart {
    pub items: Vec<Item>,
    pub coupon: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub price: f64,
    pub quantity: u32,
}

impl Cart {
    // Getters.
    pub fn count(&self) -> u32 {
        self.items.iter().map(|i| i.quantity).sum()
    }
    pub fn total(&self) -> f64 {
        self.items.iter().map(|i| i.price * f64::from(i.quantity)).sum()
    }
}

pub fn cart() -> Store<Cart> {
    Store::new("cart") // el mismo store dondequiera que se llame
}

// Acciones.
pub fn add(item: Item) {
    cart().update(|c| c.items.push(item));
}

pub fn apply_coupon(code: &str) {
    cart().update(|c| c.coupon = Some(code.to_owned()));
}
```

Cada `update` modifica el estado, lo guarda en `localStorage` con la clave `memoire:store:cart` y luego llama a los suscriptores. En la siguiente visita, `Store::new("cart")` parte del estado guardado.

| Pinia | Memoire |
|---|---|
| `defineStore('cart', { state: () => ({ items: [] }) })` | un struct `#[derive(Clone, Default, Serialize, Deserialize)]`, y `Store::<Cart>::new("cart")` |
| `useCartStore()` en cualquier parte | `Store::<Cart>::new("cart")` en cualquier parte: el mismo store |
| getters | métodos del struct, leídos con `cart.get(Cart::total)` o `cart.get(\|c\| c.items.len())` |
| actions | funciones que llaman a `cart.update(\|c\| ...)`, que devuelve lo que devuelve la closure |
| `store.$state` | `cart.state()` (una copia) |
| `store.$patch({ coupon: 'X' })` | `cart.patch(json!({ "coupon": "X" }))?` (fusión JSON; `null` quita un campo) |
| `store.$reset()` | `cart.reset()` |
| `store.$subscribe((mutation, state) => ...)` | `let sub = cart.subscribe(\|state\| ...)` (descartar `sub` cancela la suscripción) |
| `store.$dispose()` | `cart.dispose()` |
| `pinia-plugin-persistedstate` | integrado |

### Actualizar una interfaz

`subscribe` recibe oyentes `Send + Sync`. Las señales de los frameworks de interfaz normalmente no lo son: usa `subscribe_local`, que ejecuta el oyente en el hilo que lo registró (el único en un navegador). Con [Active](https://maxlestage.github.io/Active/):

```rust
use active::prelude::*;

let count = use_state(cart().get(Cart::count));
let _sub = cart().subscribe_local(move |c| count.set(c.count()));
// `count` sigue ahora al carrito, incluidos los cambios hechos en otra pestaña.
```

### Guardado, versiones y otros almacenamientos

- La forma guardada es `{"v": 0, "state": { ... }}`. `Store::builder("cart").key("shop:cart")` cambia la clave, `.storage(SessionStorage)` conserva el estado solo para la pestaña, y `.init(|| ...)` da otro estado inicial (`Store::builder_with` para un estado sin `Default`).
- Cuando el struct cambia, indica una versión y una migración para que los visitantes conserven lo que tenían:

  ```rust
  let prefs: Store<Prefs> = Store::builder("prefs")
      .version(2, |from, mut old| {
          if from < 2 {
              old["theme"] = if old["dark"] == true { "dark".into() } else { "light".into() };
          }
          old
      })
      .build();
  ```

- El guardado es de mejor esfuerzo, como los plugins de persistencia de Pinia: si el almacenamiento rechaza una escritura (lleno, desactivado), el estado cambia igualmente en memoria y `cart.last_error()` explica el motivo; `cart.save()` lo reintenta y devuelve el error. Un estado guardado ilegible (una forma antigua sin migración) se informa igual y el store parte de su estado inicial.
- `cart.reload()` vuelve a leer el almacenamiento; `cart.clear()` restablece el estado y elimina la copia guardada.

### Varias pestañas

Cuando otra pestaña del sitio modifica un store, el navegador envía un evento `storage`: esta pestaña lee el nuevo estado y llama a sus suscriptores. No hay nada que configurar.

## Una base de datos en localStorage

`LocalStorageDb` es el motor en memoria, con cada cambio de fila confirmado escrito en `localStorage`:

```rust
use memoire_orm::prelude::*;
use memoire_orm::web::LocalStorageDb;

#[derive(Model, Debug, Clone)]
#[memoire(table = "todos")]
pub struct Todo {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(fulltext)]
    pub text: String,
    pub done: bool,
}

let db = LocalStorageDb::open("app")?; // la misma base dondequiera que se abra
db.create_table_if_not_exists::<Todo>().await?;

Todo { id: 0, text: "Escribir la documentación".into(), done: false }.insert(&db).await?;
let open: Vec<Todo> = Todo::query().filter(Todo::DONE.eq(false)).all(&db).await?;
let found = Todo::query().filter(Todo::TEXT.matches("documentación")).count(&db).await?;
```

Los futures están listos de inmediato (no hay ninguna E/S que esperar): `memoire_orm::block_on` los ejecuta en código síncrono del navegador, y `wasm-bindgen-futures` también funciona.

Lo que contiene el almacenamiento, legible en las herramientas de desarrollo del navegador (Application → Local Storage):

| Clave | Valor |
|---|---|
| `app:todos` | `{"columns":["id","text","done"],"pk":0,"seq":2,"ver":2}` |
| `app:todos:1` | `{"done":false,"id":1,"text":"Escribir la documentación"}` |

- **Recargas y nuevas visitas:** las tablas vuelven la primera vez que se usa su modelo. Las columnas se emparejan por nombre: añadir un campo `Option` conserva las filas.
- **Varias pestañas:** antes de cada sentencia, las tablas que usa se vuelven a leer si otra pestaña las modificó (su `ver` cambió), así que los identificadores nunca chocan; una transacción cuyas tablas escribió otra pestaña mientras tanto falla con `Error::Conflict` al confirmar. `db.on_external_change(...)` (u `on_external_change_local` para señales de interfaz) indica cuándo refrescar lo que muestra la página.
- **Cuota:** `localStorage` admite unos 5 MB por sitio. Una escritura que no cabe falla con `Error::Storage("quota exceeded ...")` y no cambia nada, ni en el almacenamiento ni en la base (un lote es todo o nada).
- `db.clear()` elimina las tablas de esta base y solo sus claves; `db.close()` olvida la base abierta para que el siguiente `open` vuelva a leer el almacenamiento.

## Cuál usar

| | `Store<S>` | `LocalStorageDb` |
|---|---|---|
| Contiene | Un valor (un struct, una lista...) | Tablas de filas |
| Se lee con | Getters (`get`) | Consultas, joins, agregados, búsqueda de texto completo |
| Se modifica con | Acciones (`update`, `patch`, `reset`) | `insert`, `update`, `update_many`, transacciones... |
| Restricciones | Ninguna | Claves únicas e índices, también sobre varias columnas |
| Se guarda como | Un valor JSON por store | Un valor JSON por fila |
| Ideal para | Preferencias, carrito, sesión, estado de interfaz | Notas, tareas, datos sin conexión, cachés de datos del servidor |

Ambos pueden compartir el mismo `localStorage`.

## Próximos pasos

- [Motor en memoria](memory): el motor bajo `LocalStorageDb`, y `MemoryDb::open` para archivos.
- [Notificaciones de cambios](events): `Observed` también funciona sobre `LocalStorageDb`.
- [Constructor de consultas](queries): todo lo que `LocalStorageDb` puede ejecutar.
