# Introduction

Memoire ORM est un ORM rapide, typé et asynchrone, écrit en Rust pur. Vous décrivez vos tables sous forme de structs Rust ordinaires, et Memoire vous fournit une API typée pour créer, interroger, modifier et supprimer des lignes, avec le même code sur six bases de données : un moteur en mémoire, SQLite, PostgreSQL, MySQL/MariaDB, Redis et MongoDB.

## Qu'est-ce qu'un ORM ?

Un ORM (Object-Relational Mapper, ou « mapping objet-relationnel ») est une bibliothèque qui fait la traduction entre les lignes d'une table de base de données et les valeurs de votre langage de programmation. Au lieu d'écrire des chaînes SQL à la main et de recopier vous-même chaque colonne dans une struct, vous écrivez :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?; // alice.id vaut maintenant 1

    let adults = User::query()
        .filter(User::AGE.ge(18))
        .order_by(User::NAME.asc())
        .all(&db)
        .await?;
    println!("{adults:?}");
    Ok(())
}
```

Memoire transforme `User::query().filter(User::AGE.ge(18))` en SQL adapté à votre base de données (ou en filtre MongoDB natif), lie la valeur `18` en toute sécurité sous forme de paramètre, exécute la requête et reconvertit chaque ligne renvoyée en `User`.

## Pourquoi Memoire ?

- **Des requêtes typées.** Chaque champ dispose d'une constante de colonne typée (`User::AGE`, `User::EMAIL`, ...). `User::AGE.gt(18)` compile ; `User::AGE.gt("18")` provoque une erreur de compilation. Une faute de frappe dans un nom de colonne aussi.
- **Une seule API, six bases de données.** Le même modèle et les mêmes requêtes fonctionnent sur le moteur en mémoire, SQLite, PostgreSQL, MySQL/MariaDB, Redis et MongoDB, et tous passent la même suite de tests. Changer de base de données tient en une ligne.
- **Rapide par conception.** Le SQL des opérations par clé primaire est généré pendant la compilation de votre code, les lignes sont décodées en une seule passe sans clonage, et les requêtes sont préparées une seule fois puis mises en cache. Voir [Pourquoi c'est rapide](performance).
- **Tout est inclus.** Jointures, agrégats, `GROUP BY`, pagination, upserts, insertions en masse, transactions, migrations et SQL brut.
- **Sûr.** La crate est en `#![forbid(unsafe_code)]`, et les valeurs sont toujours envoyées comme paramètres liés, jamais collées dans le texte SQL.
- **Fonctionne partout.** Le moteur en mémoire est écrit en Rust pur, n'a besoin d'aucun runtime et compile vers WebAssembly (`wasm32-unknown-unknown`).

## Aperçu des fonctionnalités

| Domaine | Ce que vous obtenez |
|---|---|
| Modèles | `#[derive(Model)]` avec clés primaires, auto-incrément, `unique`, index, valeurs par défaut, clés étrangères, champs renommés et ignorés |
| Requêtes | Filtres typés combinés avec `&`, `\|`, `!` ; helpers `LIKE` avec échappement automatique ; `IN`, `BETWEEN`, `IS NULL` |
| Résultats | `all`, `first`, `one`, `count`, `exists`, `paginate`, `sum`, `avg`, `min`, `max`, projections typées sur des colonnes |
| Écritures | `insert`, `insert_many` (regroupement multi-lignes), `update`, `upsert`, `delete`, `update_many` avec `increment`/`decrement`, `delete_many` |
| Relations | `inner_join` et `left_join`, qui renvoient des tuples comme `(User, Post)` et `(User, Option<Post>)` |
| Rapports | `group_by`, `having`, `count_all()`, agrégats décodés en tuples ou en structs `#[derive(FromRow)]` |
| Filets de sécurité | Transactions (annulées automatiquement lorsqu'elles sont droppées), migrations versionnées, erreurs normalisées |
| Portes de sortie | SQL brut avec `params![...]` sur les bases de données SQL, accès direct à chaque driver (connexion SQLite, pools PostgreSQL et MySQL, connexion Redis, base MongoDB) |

## Backends

| Backend | Type | Feature Cargo | Runtime async | SQL brut |
|---|---|---|---|---|
| `MemoryDb` | Moteur en mémoire, Rust pur | `memory` (par défaut) | N'importe lequel, ou aucun (`block_on`) | Non |
| `SqliteDb` | Base SQLite embarquée (fichier ou mémoire) | `sqlite` | N'importe lequel, ou aucun (`block_on`) | Oui |
| `PgPool` | Client PostgreSQL avec pool de connexions | `postgres` (`postgres-tls` pour TLS) | Tokio | Oui |
| `MySqlPool` | Client MySQL et MariaDB avec pool de connexions | `mysql` (`mysql-tls` pour TLS) | Tokio | Oui |
| `RedisDb` | Tables stockées dans Redis, requêtes évaluées par le moteur en mémoire | `redis` (`redis-tls` pour TLS) | Tokio | Non |
| `MongoDb` | Une collection par table, requêtes traduites en filtres MongoDB | `mongodb` | Tokio | Non |

Chaque backend a sa propre page : [Moteur en mémoire](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis) et [MongoDB](mongodb).

## Quel backend choisir ?

| Vous voulez... | Utilisez | Pourquoi |
|---|---|---|
| Apprendre Memoire, prototyper, écrire des tests unitaires rapides | `MemoryDb` | Aucune configuration, aucun fichier, extrêmement rapide, rien à nettoyer |
| Tourner dans le navigateur (WebAssembly) | `MemoryDb` | C'est le seul backend qui compile vers `wasm32` |
| Livrer une application de bureau, un outil en ligne de commande ou un petit serveur avec des données sur disque | `SqliteDb` | Un seul fichier, aucun serveur à installer, très rapide sur une seule machine |
| Faire tourner un service web avec plusieurs instances ou de nombreuses écritures concurrentes | `PgPool` | Un vrai serveur de base de données avec pool de connexions, accès réseau et excellente gestion de la concurrence |
| Utiliser une base MySQL ou MariaDB existante, ou un hébergeur qui propose MySQL | `MySqlPool` | Un serveur SQL complet avec pool de connexions, proposé par presque tous les hébergeurs |
| Stocker des sessions, des caches ou de petits jeux de données partagés, consultés surtout par clé | `RedisDb` | Recherches par clé très rapides, données partagées entre processus, typées et atomiques |
| Travailler avec une base MongoDB existante ou MongoDB Atlas | `MongoDb` | Des documents utilisables aussi depuis d'autres outils MongoDB, des filtres exécutés sur le serveur |

> **Astuce** Comme tous les backends partagent la même API, une configuration courante consiste à utiliser `SqliteDb`, `PgPool` ou un autre backend serveur en production et `MemoryDb` dans les tests unitaires. Écrivez vos fonctions en vous appuyant sur les traits `Executor` ou `Database`, puis passez-leur la base de données de votre choix.

## Prérequis et licence

- **Rust 1.88 ou plus récent** (la version minimale de Rust prise en charge, ou MSRV).
- **Licence :** propriétaire, © Maxime Nathan Lestage, tous droits réservés. Utilisation libre comme dépendance de vos propres programmes, y compris commerciaux ; le code source est publié pour pouvoir être audité et compilé. Voir [LICENSE](https://memoire-7f4525f1fdaf.herokuapp.com/static/LICENSE.txt).
- La feature `sqlite` compile SQLite depuis les sources : il faut donc un compilateur C au moment du build. Tous les autres backends sont écrits en Rust pur ; TLS utilise `rustls`, OpenSSL n'est donc jamais nécessaire.

## Étapes suivantes

- [Installation](installation) : ajoutez Memoire à votre projet et choisissez vos features.
- [Démarrage rapide](quick-start) : construisez une petite application pas à pas, en une dizaine de minutes.
- [Modèles](models) : découvrez tout ce que `#[derive(Model)]` sait faire.
