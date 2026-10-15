# Créer, lire, modifier, supprimer

Cette page présente toutes les façons d'écrire et de lire des lignes avec un modèle : insérer une ou plusieurs lignes, charger par clé, modifier une ligne ou plusieurs, faire un upsert, supprimer, et gérer les erreurs que ces opérations peuvent renvoyer.

## Les exemples de cette page

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
    pub visits: i32,
}

impl User {
    pub fn new(email: &str, name: &str) -> Self {
        User { id: 0, email: email.into(), name: name.into(), age: None, active: true, visits: 0 }
    }
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}
```

Chaque méthode prend `&db`, qui peut être n'importe quel handle de base de données (`MemoryDb`, `SqliteDb`, `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb`) ou une [transaction](transactions) ouverte.

## Créer

### Insérer une ligne

```rust
let mut alice = User::new("alice@example.com", "Alice");
alice.insert(&db).await?;
println!("Alice got id {}", alice.id); // 1
```

`insert` emprunte la ligne de façon mutable car, pour une clé `auto_increment`, il réécrit la valeur générée dans la struct. L'`id` que vous définissez avant l'insertion (ici `0`) est ignoré.

Pour un modèle dont la clé n'est pas auto-incrémentée, c'est la clé que vous définissez qui est stockée :

```rust
let mut rust = Tag { slug: "rust".into(), label: "Rust".into() };
rust.insert(&db).await?;
```

Insérer une deuxième ligne avec la même clé primaire ou la même valeur dans une colonne `unique` échoue avec `Error::UniqueViolation` (voir « Gérer les erreurs » plus bas).

### Insérer plusieurs lignes d'un coup

`insert_many` insère une slice entière avec le moins de requêtes possible et renvoie le nombre de lignes insérées :

```rust
let users: Vec<User> = (1..=1000)
    .map(|i| User::new(&format!("user{i}@example.com"), &format!("User {i}")))
    .collect();
let inserted = User::insert_many(&db, &users).await?;
assert_eq!(inserted, 1000);
```

Sur les bases de données SQL, cela envoie un `INSERT ... VALUES (...), (...), ...` multi-lignes par lot. La taille des lots est calculée pour rester sous la limite de paramètres liés de la base de données (32 766 pour SQLite, 65 535 pour PostgreSQL et MySQL), et un lot contient au plus 10 000 lignes : même les très grandes slices sont donc gérées automatiquement. Redis écrit chaque lot avec un seul script atomique, et MongoDB avec un seul appel `insertMany`.

Ce qu'il faut savoir sur `insert_many` :

- Les clés générées ne sont **pas** relues : les structs de votre slice gardent `id: 0`. Interrogez à nouveau les lignes si vous avez besoin de leurs ids, ou insérez-les une par une avec `insert`.
- Chaque lot est atomique : si une ligne échoue (par exemple un e-mail en double), rien de ce lot n'est stocké. (Sur un serveur MongoDB autonome, qui n'a pas de transactions, Memoire supprime au mieux les documents déjà insérés.)
- Quand la slice dépasse la taille d'un lot, les lots précédents restent stockés si un lot suivant échoue. Enveloppez l'appel dans une [transaction](transactions) pour que toute l'opération se fasse en mode tout ou rien. Une transaction rend aussi les insertions en masse beaucoup plus rapides sur SQLite.

## Lire

### Par clé primaire

```rust
let maybe: Option<User> = User::find(&db, 1).await?; // None s'il n'y a pas de ligne 1
let alice: User = User::get(&db, 1).await?;          // Error::NotFound s'il n'y a pas de ligne 1
let tag = Tag::get(&db, "rust").await?;              // les clés de type chaîne fonctionnent de la même façon
```

Utilisez `find` quand l'absence de ligne est une situation normale, et `get` quand c'est une erreur. La clé doit avoir le type de la clé primaire : `User::find(&db, "1")` ne compile pas.

### Plusieurs lignes par clé primaire

`find_many` récupère une liste de clés en une seule requête (`WHERE id IN (...)`), au lieu d'un `find` par clé. Les clés absentes sont ignorées et les lignes reviennent dans un ordre quelconque :

```rust
let some: Vec<User> = User::find_many(&db, [3, 1, 42]).await?; // utilisateurs 1 et 3
```

### Toutes les lignes, ou leur nombre

```rust
let everyone: Vec<User> = User::all(&db).await?;
let how_many: u64 = User::count(&db).await?;
```

Pour tout ce qui est plus précis (filtres, tri, pagination, quelques colonnes...), utilisez le [constructeur de requêtes](queries) : `User::query().filter(...).all(&db)`.

### Rafraîchir une ligne

`reload` relit une ligne depuis la base de données et remplace tous les champs de votre struct. C'est utile après des modifications faites avec `update_many` ou par quelqu'un d'autre :

```rust
let mut alice = User::get(&db, 1).await?;
User::update_many().increment(User::VISITS, 1).filter(User::ID.eq(alice.id)).execute(&db).await?;
alice.reload(&db).await?;
assert_eq!(alice.visits, 1);
```

`reload` échoue avec `Error::NotFound` si la ligne a été supprimée entre-temps.

## Modifier

### Modifier une ligne

Chargez la ligne, changez des champs, puis appelez `update` :

```rust
let mut alice = User::get(&db, 1).await?;
alice.name = "Alice Liddell".into();
alice.age = Some(32);
alice.update(&db).await?;
```

`update` écrit **toutes les colonnes** de la struct dans la ligne qui a la même clé primaire. Si aucune ligne n'a cette clé, il échoue avec `Error::NotFound` ; il ne crée jamais de ligne.

> **Remarque** Comme `update` écrit toutes les colonnes, deux programmes qui chargent la même ligne, modifient des champs différents et appellent tous deux `update` écraseront mutuellement leurs modifications (la dernière écriture l'emporte). Pour les compteurs et autres modifications concurrentes, utilisez `update_many` avec `increment`, ou une [transaction](transactions).

### Modifier seulement certaines colonnes

`update_only` écrit les colonnes que vous listez, et seulement elles, dans la ligne de même clé primaire. Les autres colonnes gardent la valeur stockée en base : une modification concurrente de celles-ci n'est pas écrasée, et moins de données transitent :

```rust
let mut alice = User::get(&db, 1).await?;
alice.name = "Alice Liddell".into();
alice.age = Some(32);
alice.update_only(&db, (User::NAME, User::AGE)).await?; // une seule colonne : `User::NAME`
```

Les colonnes doivent appartenir au modèle : `alice.update_only(&db, Post::TITLE)` ne compile pas. Comme `update`, elle échoue avec `Error::NotFound` si la ligne n'existe pas.

### Modifier plusieurs lignes

`update_many` modifie toutes les lignes qui correspondent à ses filtres, sans les charger, et renvoie le nombre de lignes modifiées :

```rust
let changed = User::update_many()
    .set(User::ACTIVE, false)
    .filter(User::AGE.lt(18))
    .execute(&db)
    .await?;
println!("{changed} user(s) deactivated");
```

Les affectations disponibles :

| Méthode | SQL | Exemple |
|---|---|---|
| `set(column, value)` | `column = value` | `.set(User::NAME, "Bob")` |
| `set_null(column)` | `column = NULL` | `.set_null(User::AGE)` (uniquement pour les colonnes `Option`) |
| `increment(column, n)` | `column = column + n` | `.increment(User::VISITS, 1)` |
| `decrement(column, n)` | `column = column - n` | `.decrement(User::VISITS, 1)` |
| `set_expr(column, expr)` | `column = <expression>` | `.set_expr(User::VISITS, User::VISITS.mul(2))` |

Affectations et filtres se combinent librement :

```rust
User::update_many()
    .set(User::ACTIVE, true)
    .increment(User::VISITS, 1)
    .set_null(User::AGE)
    .filter(User::EMAIL.ends_with("@example.com") & User::ACTIVE.eq(false))
    .execute(&db)
    .await?;
```

Comme les requêtes, une modification peut afficher son SQL :

```rust
let update = User::update_many()
    .set(User::ACTIVE, false)
    .increment(User::VISITS, 1)
    .filter(User::NAME.eq("Bob"));
println!("{}", update.to_sql(Dialect::Postgres));
// UPDATE "users" SET "active" = $1, "visits" = ("users"."visits" + $2) WHERE "users"."name" = $3
```

> **Attention** Sans `filter`, `update_many` modifie **toutes les lignes** de la table. Au moins une affectation est requise : `update_many()` sans `set`, `increment`, ... échoue avec `Error::Unsupported`.

Une modification qui violerait une contrainte `unique` échoue dans son ensemble et laisse toutes les lignes intactes :

```rust
let err = User::update_many().set(User::EMAIL, "same@example.com").execute(&db).await.unwrap_err();
assert!(err.is_unique_violation());
```

## Upsert

`upsert` signifie « insérer, ou mettre à jour si la ligne existe déjà ». Il insère la ligne ou, si une ligne avec la même clé primaire existe, écrase toutes ses colonnes :

```rust
let tag = Tag { slug: "rust".into(), label: "Rust 🦀".into() };
tag.upsert(&db).await?; // crée "rust", ou met à jour son label

Tag { slug: "orm".into(), label: "ORM".into() }.upsert(&db).await?;
```

La correspondance se fait uniquement sur la clé primaire, et l'opération est sûre même quand deux programmes font un upsert sur la même clé au même moment (`INSERT ... ON CONFLICT (key) DO UPDATE` sur SQLite et PostgreSQL, `INSERT ... ON DUPLICATE KEY UPDATE` ou une insertion suivie d'une modification sur MySQL, un script Lua sur Redis, `replaceOne` avec `upsert` sur MongoDB). Si la ligne a une nouvelle clé mais une valeur `unique` qui appartient à une autre ligne, `upsert` échoue avec `Error::UniqueViolation` sur tous les backends.

> **Attention** `upsert` stocke toujours la clé présente dans votre struct, même pour une clé `auto_increment`. Un upsert de `User { id: 0, ... }` crée une ligne dont l'id vaut `0`. Utilisez `insert` pour les nouvelles lignes des modèles auto-incrémentés, et `upsert` pour les clés naturelles (slugs, codes, UUID) ou les lignes dont vous connaissez l'id. Sur PostgreSQL, notez aussi que faire un upsert de nouvelles lignes avec des ids explicites ne fait pas avancer la séquence des ids : un `insert` ultérieur peut donc entrer en collision avec elles.

## Trouver ou créer, modifier ou créer, enregistrer

Les raccourcis du quotidien d'Eloquent, sur n'importe quelle requête d'un modèle :

```rust
// La première ligne trouvée, ou cette nouvelle ligne, insérée (firstOrCreate d'Eloquent).
let alice = User::query()
    .filter(User::EMAIL.eq("alice@example.com"))
    .first_or_create(&db, || User::new("alice@example.com", "Alice"))
    .await?;

// Modifie la première ligne trouvée, ou insère une nouvelle ligne modifiée (updateOrCreate).
let bob = User::query()
    .filter(User::EMAIL.eq("bob@example.com"))
    .update_or_create(&db, || User::new("bob@example.com", "Bob"), |u| u.visits += 1)
    .await?;

// Insère une nouvelle ligne (clé auto-incrémentée encore à 0), met à jour une ligne existante.
let mut carol = User::new("carol@example.com", "Carol");
carol.save(&db).await?; // INSERT
carol.age = Some(28);
carol.save(&db).await?; // UPDATE
```

`first_or_create` gère les accès concurrents : si un autre programme insère la même ligne entre la recherche et l'insertion, l'insertion échoue sur la colonne `unique` et la ligne désormais stockée est renvoyée. `save` et `update_or_create` exécutent les [hooks et l'horodatage](lifecycle) du modèle.

## Supprimer

```rust
let alice = User::get(&db, 1).await?;
let deleted: bool = alice.delete(&db).await?; // true : la ligne existait

let deleted_again = User::delete_by_key(&db, 1).await?; // false : déjà supprimée
```

Les deux renvoient `true` si une ligne a été supprimée et `false` s'il n'y avait rien à supprimer ; une ligne absente n'est pas une erreur.

Pour supprimer toutes les lignes qui correspondent à un filtre, utilisez `delete_many`, qui renvoie le nombre de lignes supprimées :

```rust
let removed = User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
println!("removed {removed} inactive user(s)");
```

> **Attention** `User::delete_many().execute(&db)` sans filtre supprime **toutes les lignes** de la table.

Sur un modèle avec [suppression douce](lifecycle), `delete`, `delete_by_key` et `delete_many` ne font que remplir `deleted_at` ; `force_delete()` et `delete_many().force()` suppriment vraiment les lignes.

Si d'autres tables référencent les lignes supprimées avec `on_delete = "cascade"`, la base de données supprime aussi ces lignes-là (sur SQLite, PostgreSQL et MySQL ; le moteur en mémoire, Redis et MongoDB n'appliquent pas les clés étrangères).

## Gérer les erreurs

Chaque opération renvoie un `memoire_orm::Result<T>`, alias de `Result<T, memoire_orm::Error>`. Les erreurs sont normalisées : la même erreur donne la même variante sur tous les backends, vous pouvez donc faire un `match` dessus sans savoir quelle base de données vous utilisez.

| Erreur | Quand elle se produit |
|---|---|
| `Error::NotFound` | `get`, `one`, `update` ou `reload` n'ont trouvé aucune ligne |
| `Error::UniqueViolation { table, column }` | Une clé primaire ou une valeur `unique` existe déjà |
| `Error::Constraint(message)` | Une autre contrainte a échoué : `NOT NULL`, clé étrangère, `CHECK` |
| `Error::UnexpectedNull { column }` | Un `NULL` a été lu dans un champ qui n'est pas une `Option` |
| `Error::Decode { column, message }` | Une valeur n'a pas pu être convertie dans le type du champ |
| `Error::Conversion(message)` | Une valeur n'a pas pu être convertie (en dehors d'une colonne précise) |
| `Error::NoSuchTable(name)` | La table n'existe pas |
| `Error::TableExists(name)` | `create_table` sur une table qui existe déjà |
| `Error::Unsupported(message)` | Le backend ne sait pas faire cela (par exemple du SQL brut sur Redis, ou une transaction sur un serveur MongoDB autonome) |
| `Error::Conflict(table)` | Une transaction est entrée en collision avec une transaction concurrente ; on peut réessayer sans risque |
| `Error::Validation(message)` | Un [hook](lifecycle) (ou votre propre code) a refusé la ligne avant son écriture |
| `Error::TransactionFinished` | La transaction a déjà été validée (commit) ou annulée (rollback) |
| `Error::Migration { version, name, source }` | Une migration a échoué ; `source` contient la cause |
| `Error::Pool(message)` | Le pool PostgreSQL n'a pas pu fournir de connexion |
| `Error::Database(error)` | Toute autre erreur provenant du driver de la base de données |

Quatre helpers couvrent les vérifications les plus courantes : `is_not_found()`, `is_unique_violation()`, `is_conflict()` et `is_validation()`.

```rust
let mut duplicate = User::new("alice@example.com", "Another Alice");
match duplicate.insert(&db).await {
    Ok(()) => println!("created #{}", duplicate.id),
    Err(Error::UniqueViolation { column, .. }) => println!("this {column} is already taken"),
    Err(other) => return Err(other),
}
```

```text
this email is already taken
```

> **Astuce** `Error` est marquée `#[non_exhaustive]` : quand vous faites un `match` dessus, gardez une branche fourre-tout (`Err(other) => ...`) comme ci-dessus. Toutes les erreurs implémentent `std::error::Error` et `Display`, elles fonctionnent donc aussi avec `anyhow`, `thiserror` et `Box<dyn Error>`.

## Étapes suivantes

- [Horodatage, suppression douce et hooks](lifecycle) : `created_at`, `updated_at`, `deleted_at` et du code exécuté autour de chaque écriture.
- [Constructeur de requêtes](queries) : trouvez exactement les lignes dont vous avez besoin.
- [Transactions](transactions) : regroupez plusieurs écritures en une seule modification tout ou rien.
- [Jointures et relations](relations) : travaillez avec des tables liées.
