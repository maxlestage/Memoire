# Browser storage: a Pinia-style store and localStorage

Rust apps that run in the browser (WebAssembly, with Active, Leptos, Yew, Dioxus or plain `wasm-bindgen`) often need to keep things in `localStorage`: a cart, preferences, a draft, a list of to-dos. The `web` feature gives two tools for that:

- **`Store<S>`**: application state in the spirit of [Pinia](https://pinia.vuejs.org/). One store per id, read through getters, changed through actions, saved on every change, restored on the next visit, kept in step across tabs.
- **`LocalStorageDb`**: a complete Memoire database in `localStorage`. Models, queries, joins, transactions, unique indexes and full-text search work as on every other backend; each row is a readable JSON value.

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features web
cargo add serde --features derive
```

Both work on the browser's `LocalStorage` and `SessionStorage` (wasm32) and on `MemoryStorage`, which behaves the same way (strings, quota, tabs and their events) for tests, server-side rendering and native programs.

## A store, like Pinia

The state is a plain struct; getters are its methods; actions change it through `update`:

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
    Store::new("cart") // the same store wherever it is called
}

// Actions.
pub fn add(item: Item) {
    cart().update(|c| c.items.push(item));
}

pub fn apply_coupon(code: &str) {
    cart().update(|c| c.coupon = Some(code.to_owned()));
}
```

Every `update` changes the state, saves it to `localStorage` under `memoire:store:cart`, then calls the subscribers. On the next visit, `Store::new("cart")` starts from the saved state.

| Pinia | Memoire |
|---|---|
| `defineStore('cart', { state: () => ({ items: [] }) })` | a `#[derive(Clone, Default, Serialize, Deserialize)]` struct, and `Store::<Cart>::new("cart")` |
| `useCartStore()` anywhere | `Store::<Cart>::new("cart")` anywhere: the same store |
| getters | methods of the struct, read with `cart.get(Cart::total)` or `cart.get(\|c\| c.items.len())` |
| actions | functions calling `cart.update(\|c\| ...)`, which returns what the closure returns |
| `store.$state` | `cart.state()` (a copy) |
| `store.$patch({ coupon: 'X' })` | `cart.patch(json!({ "coupon": "X" }))?` (JSON merge; `null` removes a field) |
| `store.$reset()` | `cart.reset()` |
| `store.$subscribe((mutation, state) => ...)` | `let sub = cart.subscribe(\|state\| ...)` (dropping `sub` unsubscribes) |
| `store.$dispose()` | `cart.dispose()` |
| `pinia-plugin-persistedstate` | built in |

### Updating a user interface

`subscribe` takes listeners that are `Send + Sync`. The signals of UI frameworks usually are not, so use `subscribe_local`, which runs the listener on the thread that registered it (the only one in a browser). With [Active](https://active-c1883a966650.herokuapp.com/):

```rust
use active::prelude::*;

let count = use_state(cart().get(Cart::count));
let _sub = cart().subscribe_local(move |c| count.set(c.count()));
// `count` now follows the cart, including changes made in another tab.
```

### Saving, versions and other storages

- The saved form is `{"v": 0, "state": { ... }}`. `Store::builder("cart").key("shop:cart")` changes the key, `.storage(SessionStorage)` keeps the state for the tab only, and `.init(|| ...)` gives another initial state (`Store::builder_with` for a state without `Default`).
- When the struct changes, give a version and a migration so visitors keep what they had:

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

- Saving is best-effort, like Pinia's persistence plugins: if the storage refuses a write (full, disabled), the state still changes in memory and `cart.last_error()` says why; `cart.save()` retries and returns the error. A saved state that cannot be read (an old shape without migration) is reported the same way and the store starts from its initial state.
- `cart.reload()` reads the storage again; `cart.clear()` resets the state and removes the saved copy.

### Several tabs

When another tab of the site changes a store, the browser sends a `storage` event: this tab reads the new state and calls its subscribers. Nothing to set up.

## A database in localStorage

`LocalStorageDb` is the in-memory engine with every committed row change written to `localStorage`:

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

let db = LocalStorageDb::open("app")?; // the same database wherever it is opened
db.create_table_if_not_exists::<Todo>().await?;

Todo { id: 0, text: "Write the docs".into(), done: false }.insert(&db).await?;
let open: Vec<Todo> = Todo::query().filter(Todo::DONE.eq(false)).all(&db).await?;
let found = Todo::query().filter(Todo::TEXT.matches("docs")).count(&db).await?;
```

The futures are ready at once (there is no I/O to wait for), so `memoire_orm::block_on` runs them in synchronous browser code, and `wasm-bindgen-futures` works too.

What the storage holds, readable in the browser's developer tools (Application → Local Storage):

| Key | Value |
|---|---|
| `app:todos` | `{"columns":["id","text","done"],"pk":0,"seq":2,"ver":2}` |
| `app:todos:1` | `{"done":false,"id":1,"text":"Write the docs"}` |

- **Reloads and new visits:** tables come back the first time their model is used. Columns are matched by name: adding an `Option` field keeps the rows.
- **Several tabs:** before each statement, the tables it uses are read again if another tab changed them (their `ver` moved), so ids never collide; a transaction whose tables another tab wrote meanwhile fails with `Error::Conflict` at commit. `db.on_external_change(...)` (or `on_external_change_local` for UI signals) tells when to refresh what the page shows.
- **Quota:** `localStorage` holds about 5 MB per site. A write that does not fit fails with `Error::Storage("quota exceeded ...")` and changes nothing, neither in the storage nor in the database (a batch is all or nothing).
- `db.clear()` drops the tables of this database and removes only its keys; `db.close()` forgets the open database so the next `open` reads the storage again.

## Which one to use

| | `Store<S>` | `LocalStorageDb` |
|---|---|---|
| Holds | One value (a struct, a list...) | Tables of rows |
| Read with | Getters (`get`) | Queries, joins, aggregates, full-text search |
| Change with | Actions (`update`, `patch`, `reset`) | `insert`, `update`, `update_many`, transactions... |
| Constraints | None | Unique keys and indexes, also over several columns |
| Saved as | One JSON value per store | One JSON value per row |
| Good for | Preferences, a cart, a session, UI state | Notes, to-dos, offline data, caches of server data |

Both can share the same `localStorage`.

## Next steps

- [In-memory engine](memory): the engine under `LocalStorageDb`, and `MemoryDb::open` for files.
- [Change notifications](events): `Observed` also works on `LocalStorageDb`.
- [Query builder](queries): everything `LocalStorageDb` can run.
