# Stockage du navigateur : un store façon Pinia et localStorage

Les applications Rust qui tournent dans le navigateur (WebAssembly, avec Active, Leptos, Yew, Dioxus ou simplement `wasm-bindgen`) ont souvent besoin de garder des choses dans `localStorage` : un panier, des préférences, un brouillon, une liste de tâches. La feature `web` apporte deux outils pour cela :

- **`Store<S>`** : un état d'application dans l'esprit de [Pinia](https://pinia.vuejs.org/). Un store par identifiant, lu par des getters, modifié par des actions, enregistré à chaque changement, restauré à la visite suivante, synchronisé entre les onglets.
- **`LocalStorageDb`** : une base Memoire complète dans `localStorage`. Modèles, requêtes, jointures, transactions, index uniques et recherche plein texte fonctionnent comme sur les autres bases ; chaque ligne est une valeur JSON lisible.

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features web
cargo add serde --features derive
```

Les deux fonctionnent sur `LocalStorage` et `SessionStorage` du navigateur (wasm32) et sur `MemoryStorage`, qui se comporte de la même façon (chaînes, quota, onglets et leurs événements) pour les tests, le rendu côté serveur et les programmes natifs.

## Un store, comme Pinia

L'état est une simple struct ; les getters sont ses méthodes ; les actions la modifient avec `update` :

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
    Store::new("cart") // le même store partout où on l'appelle
}

// Actions.
pub fn add(item: Item) {
    cart().update(|c| c.items.push(item));
}

pub fn apply_coupon(code: &str) {
    cart().update(|c| c.coupon = Some(code.to_owned()));
}
```

Chaque `update` modifie l'état, l'enregistre dans `localStorage` sous `memoire:store:cart`, puis appelle les abonnés. À la visite suivante, `Store::new("cart")` repart de l'état enregistré.

| Pinia | Memoire |
|---|---|
| `defineStore('cart', { state: () => ({ items: [] }) })` | une struct `#[derive(Clone, Default, Serialize, Deserialize)]`, et `Store::<Cart>::new("cart")` |
| `useCartStore()` n'importe où | `Store::<Cart>::new("cart")` n'importe où : le même store |
| getters | les méthodes de la struct, lues avec `cart.get(Cart::total)` ou `cart.get(\|c\| c.items.len())` |
| actions | des fonctions qui appellent `cart.update(\|c\| ...)`, qui renvoie ce que renvoie la closure |
| `store.$state` | `cart.state()` (une copie) |
| `store.$patch({ coupon: 'X' })` | `cart.patch(json!({ "coupon": "X" }))?` (fusion JSON ; `null` retire un champ) |
| `store.$reset()` | `cart.reset()` |
| `store.$subscribe((mutation, state) => ...)` | `let sub = cart.subscribe(\|state\| ...)` (abandonner `sub` désabonne) |
| `store.$dispose()` | `cart.dispose()` |
| `pinia-plugin-persistedstate` | intégré |

### Mettre à jour une interface

`subscribe` prend des écouteurs `Send + Sync`. Les signaux des frameworks d'interface ne le sont généralement pas : utilisez `subscribe_local`, qui exécute l'écouteur sur le thread qui l'a enregistré (le seul dans un navigateur). Avec [Active](https://active-c1883a966650.herokuapp.com/) :

```rust
use active::prelude::*;

let count = use_state(cart().get(Cart::count));
let _sub = cart().subscribe_local(move |c| count.set(c.count()));
// `count` suit désormais le panier, y compris les changements faits dans un autre onglet.
```

### Enregistrement, versions et autres stockages

- La forme enregistrée est `{"v": 0, "state": { ... }}`. `Store::builder("cart").key("shop:cart")` change la clé, `.storage(SessionStorage)` garde l'état pour l'onglet seulement, et `.init(|| ...)` donne un autre état initial (`Store::builder_with` pour un état sans `Default`).
- Quand la struct change, donnez une version et une migration pour que les visiteurs gardent ce qu'ils avaient :

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

- L'enregistrement est au mieux, comme les plugins de persistance de Pinia : si le stockage refuse une écriture (plein, désactivé), l'état change quand même en mémoire et `cart.last_error()` dit pourquoi ; `cart.save()` réessaie et renvoie l'erreur. Un état enregistré illisible (une ancienne forme sans migration) est signalé de la même façon et le store repart de son état initial.
- `cart.reload()` relit le stockage ; `cart.clear()` remet l'état initial et supprime la copie enregistrée.

### Plusieurs onglets

Quand un autre onglet du site modifie un store, le navigateur envoie un événement `storage` : cet onglet lit le nouvel état et appelle ses abonnés. Rien à configurer.

## Une base de données dans localStorage

`LocalStorageDb` est le moteur en mémoire, avec chaque modification de ligne validée écrite dans `localStorage` :

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

let db = LocalStorageDb::open("app")?; // la même base partout où on l'ouvre
db.create_table_if_not_exists::<Todo>().await?;

Todo { id: 0, text: "Écrire la doc".into(), done: false }.insert(&db).await?;
let open: Vec<Todo> = Todo::query().filter(Todo::DONE.eq(false)).all(&db).await?;
let found = Todo::query().filter(Todo::TEXT.matches("doc")).count(&db).await?;
```

Les futures sont prêtes immédiatement (il n'y a aucune entrée-sortie à attendre) : `memoire_orm::block_on` les exécute dans du code synchrone du navigateur, et `wasm-bindgen-futures` fonctionne aussi.

Ce que contient le stockage, lisible dans les outils de développement du navigateur (Application → Local Storage) :

| Clé | Valeur |
|---|---|
| `app:todos` | `{"columns":["id","text","done"],"pk":0,"seq":2,"ver":2}` |
| `app:todos:1` | `{"done":false,"id":1,"text":"Écrire la doc"}` |

- **Rechargements et nouvelles visites :** les tables reviennent la première fois que leur modèle est utilisé. Les colonnes sont associées par nom : ajouter un champ `Option` conserve les lignes.
- **Plusieurs onglets :** avant chaque instruction, les tables qu'elle utilise sont relues si un autre onglet les a modifiées (leur `ver` a bougé), donc les identifiants ne se chevauchent jamais ; une transaction dont un autre onglet a écrit les tables entre-temps échoue avec `Error::Conflict` au commit. `db.on_external_change(...)` (ou `on_external_change_local` pour les signaux d'interface) indique quand rafraîchir ce que la page affiche.
- **Quota :** `localStorage` contient environ 5 Mo par site. Une écriture qui ne tient pas échoue avec `Error::Storage("quota exceeded ...")` et ne change rien, ni dans le stockage ni dans la base (un lot est tout ou rien).
- `db.clear()` supprime les tables de cette base et seulement ses clés ; `db.close()` oublie la base ouverte, pour que le prochain `open` relise le stockage.

## Lequel utiliser

| | `Store<S>` | `LocalStorageDb` |
|---|---|---|
| Contient | Une valeur (une struct, une liste...) | Des tables de lignes |
| Lu avec | Des getters (`get`) | Des requêtes, jointures, agrégats, recherche plein texte |
| Modifié avec | Des actions (`update`, `patch`, `reset`) | `insert`, `update`, `update_many`, transactions... |
| Contraintes | Aucune | Clés uniques et index, y compris sur plusieurs colonnes |
| Enregistré comme | Une valeur JSON par store | Une valeur JSON par ligne |
| Idéal pour | Préférences, panier, session, état d'interface | Notes, tâches, données hors ligne, caches de données serveur |

Les deux peuvent partager le même `localStorage`.

## Étapes suivantes

- [Moteur en mémoire](memory) : le moteur sous `LocalStorageDb`, et `MemoryDb::open` pour les fichiers.
- [Notifications de changements](events) : `Observed` fonctionne aussi sur `LocalStorageDb`.
- [Constructeur de requêtes](queries) : tout ce que `LocalStorageDb` sait exécuter.
