# FAQ

Les réponses aux questions les plus fréquentes sur Memoire ORM. Chaque réponse renvoie vers la page qui donne tous les détails.

## Quelles bases de données sont prises en charge ?

Six backends, qui partagent tous la même API et passent la même suite de tests :

| Backend | Feature | Page |
|---|---|---|
| `MemoryDb` | `memory` (par défaut) | [Moteur en mémoire](memory) |
| `SqliteDb` | `sqlite` | [SQLite](sqlite) |
| `PgPool` | `postgres` (`postgres-tls` pour TLS) | [PostgreSQL](postgres) |
| `MySqlPool` | `mysql` (`mysql-tls` pour TLS) | [MySQL & MariaDB](mysql) |
| `RedisDb` | `redis` (`redis-tls` pour TLS) | [Redis](redis) |
| `MongoDb` | `mongodb` | [MongoDB](mongodb) |

## De quel runtime async ai-je besoin ?

Les backends serveur (PostgreSQL, MySQL/MariaDB, Redis et MongoDB) ont besoin de **Tokio**. Le moteur en mémoire et SQLite n'attendent jamais le réseau : leurs futures fonctionnent donc avec n'importe quel exécuteur (Tokio, async-std, smol...) ou même sans aucun runtime. Voir [Installation](installation).

## Puis-je utiliser Memoire depuis du code synchrone ?

Oui, avec le moteur en mémoire et SQLite. Enveloppez les appels dans `memoire_orm::block_on`, qui exécute une future sur le thread courant :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "settings")]
pub struct Setting {
    #[memoire(primary_key)]
    pub key: String,
    pub value: String,
}

fn read_setting(db: &SqliteDb, key: &str) -> memoire_orm::Result<Option<Setting>> {
    memoire_orm::block_on(Setting::find(db, key))
}
```

Pour les backends serveur, utilisez un runtime Tokio (`#[tokio::main]`, ou `tokio::runtime::Runtime::block_on` dans du code synchrone).

## Pourquoi les ids sont-ils des `i64`, et pourquoi écrire `id: 0` ?

`i64` est le plus grand type entier que toutes les bases de données prises en charge stockent nativement (`BIGINT`, l'`INTEGER` de SQLite) : il ne déborde donc jamais en pratique et se mappe proprement partout. Les clés `i32` et `i16` fonctionnent aussi. Les entiers non signés sur 64 bits ne sont pas pris en charge, car la plupart des bases de données n'ont pas de type non signé 64 bits portable.

Pour une clé `auto_increment`, c'est la base de données qui choisit la valeur : le `0` que vous écrivez est une valeur provisoire que `insert` ignore, et l'id généré est réécrit dans votre struct. Voir [Modèles](models).

## Comment rendre une colonne nullable ?

Faites du champ une `Option` : `pub age: Option<i32>`. `None` est stocké sous forme de `NULL`, et la colonne est créée sans `NOT NULL`. Pour trouver les lignes où elle est vide, utilisez `User::AGE.is_null()` ; `eq(None)` ne compile pas, et c'est voulu, car en SQL `= NULL` ne correspond jamais à rien. Voir [Constructeur de requêtes](queries).

## `LIKE` est-il sensible à la casse ?

Oui, `like`, `contains`, `starts_with` et `ends_with` sont sensibles à la casse sur les six backends (Memoire configure en ce sens SQLite et les tables MySQL qu'il crée, pour que tous les backends se comportent de la même façon). Utilisez `ilike` ou `icontains` pour ignorer la casse. Sur SQLite, la recherche insensible à la casse ne concerne que les lettres ASCII ; les autres backends gèrent aussi les lettres accentuées. Voir [Constructeur de requêtes](queries).

## Pourquoi les valeurs NULL apparaissent-elles en premier sur une base de données et en dernier sur une autre ?

Les bases de données ne sont pas d'accord : PostgreSQL, Redis et le moteur en mémoire placent les NULL en dernier dans l'ordre croissant ; SQLite, MySQL et MongoDB les placent en premier. Dites ce que vous voulez et Memoire le produit partout : `order_by(User::AGE.asc().nulls_last())` ou `.nulls_first()`.

## Memoire prend-il en charge les clés primaires composites ?

Non : un modèle a exactement un champ de clé primaire. Pour les tables de liaison, ajoutez une clé de substitution `id` et, si vous voulez que la base de données refuse les paires en double, créez un index unique sur les deux colonnes avec une [migration](migrations) SQL (ou avec `db.database()` sur MongoDB).

## Comment stocker une enum ?

Deux possibilités :

1. **La simple :** stockez un champ `String` ou `i32` et faites la conversion vers et depuis votre enum dans votre propre code.
2. **La typée :** implémentez les traits `SqlType`, `ToValue`, `IntoValue` et `FromValue` pour votre enum, puis utilisez-la directement comme type de champ. Les filtres restent vérifiés par le typage : `Article::STATUS.eq(Status::Published)`.

La seconde possibilité est détaillée pas à pas dans [Modèles](models). Sur PostgreSQL, vous pouvez aussi utiliser un type enum natif avec un champ `String` et `#[memoire(sql_type = "mood")]`, après avoir créé le type dans une migration.

## Comment stocker du JSON ?

Activez la feature `json`. Utilisez `serde_json::Value` pour du JSON libre, ou `Json<T>` pour n'importe quel type qui implémente `serde::Serialize` et `serde::Deserialize`. Il est stocké en `JSONB` sur PostgreSQL, en `JSON` sur MySQL, sous forme de texte sur SQLite, en JSON imbriqué sur Redis et en documents imbriqués sur MongoDB. Voir [Modèles](models).

## Puis-je utiliser des UUID comme clés primaires ?

Oui. Activez la feature `uuid`, ajoutez `uuid` avec sa feature `v4` à vos dépendances, et générez la clé vous-même :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "documents")]
pub struct Document {
    #[memoire(primary_key)]
    pub id: uuid::Uuid,
    pub title: String,
}

async fn create(db: &MemoryDb, title: &str) -> memoire_orm::Result<Document> {
    let mut doc = Document { id: uuid::Uuid::new_v4(), title: title.into() };
    doc.insert(db).await?;
    Ok(doc)
}
```

Les UUID sont stockés en `UUID` sur PostgreSQL, en `CHAR(36)` sur MySQL, sous forme de texte sur SQLite et Redis, et en binaire BSON (sous-type 4) sur MongoDB.

## Quel type utiliser pour les horodatages ?

Activez la feature `chrono`. Utilisez `chrono::DateTime<Utc>` pour des instants précis (stockés en `TIMESTAMPTZ` sur PostgreSQL), `NaiveDateTime` pour une date et une heure sans fuseau horaire, `NaiveDate` pour les dates et `NaiveTime` pour les heures de la journée. Deux détails :

- La précision dépend du backend : nanosecondes sur le moteur en mémoire, SQLite et Redis, microsecondes sur PostgreSQL et MySQL, millisecondes sur MongoDB. Si vous comparez une valeur avec ce que vous relisez, tronquez-la d'abord, par exemple avec `Utc::now().trunc_subsecs(3)` (depuis `chrono::SubsecRound`) pour du code qui doit fonctionner partout.
- Les comparaisons et les tris fonctionnent sur tous les backends, y compris SQLite et Redis, où les horodatages sont stockés sous forme de texte à largeur fixe.

## Dois-je configurer un pool de connexions ?

Non. `PgPool` et `MySqlPool` *sont* des pools de connexions : créez-les une seule fois et partagez-en des clones (`PgPool::connect_with(url, size)` définit la taille du pool PostgreSQL). `MongoDb` utilise le pool propre au driver, `RedisDb` une connexion multiplexée unique qui se reconnecte automatiquement, `SqliteDb` une connexion partagée unique, et `MemoryDb` n'en a pas besoin.

## Comment tester du code qui utilise Memoire ?

Écrivez vos fonctions en vous appuyant sur le trait `Executor` (ou `Database`) et donnez-leur un `MemoryDb` tout neuf dans chaque test. Il se crée en quelques microsecondes, ne demande aucun nettoyage et applique les mêmes clés et contraintes d'unicité que les bases de données SQL. Pour les comportements qui dépendent de votre base de données de production (suppressions en cascade via les clés étrangères, SQL brut, transactions sur MongoDB), ajoutez quelques tests d'intégration sur cette base de données, ou avec `SqliteDb::open_in_memory()` pour les fonctionnalités SQL. Voir [Moteur en mémoire](memory).

## Est-ce que ça fonctionne en WebAssembly ?

Oui, le moteur en mémoire compile vers `wasm32-unknown-unknown` sans configuration supplémentaire, et exécute ses futures avec `memoire_orm::block_on` ou `wasm-bindgen-futures`. Les autres backends n'y sont pas disponibles. Voir [Moteur en mémoire](memory).

## Comment voir le SQL qu'exécute Memoire ?

Appelez `to_sql(Dialect::Postgres)` (ou `Dialect::Sqlite`, `Dialect::MySql`) sur n'importe quel builder de requête, de modification ou de suppression ; `to_sql_with_params` renvoie en plus les valeurs liées. Les instructions utilisées par `find`, `insert`, `update`, `delete` et `upsert` se trouvent dans `User::SCHEMA.sql(dialect)`. Redis et MongoDB n'utilisent pas le SQL ; leurs pages expliquent ce qu'ils font à la place. Voir [Constructeur de requêtes](queries).

## Memoire crée-t-il ou met-il à jour mes tables automatiquement ?

`db.create_table::<User>()` crée une table à partir d'un modèle, mais Memoire ne modifie jamais de lui-même une table existante. Pour faire évoluer un schéma, écrivez des [migrations](migrations) : elles sont versionnées, s'exécutent dans des transactions et sont enregistrées, de sorte que chacune ne s'exécute qu'une seule fois.

## Memoire utilise-t-il du code `unsafe` ?

Non. La crate est compilée avec `#![forbid(unsafe_code)]` : elle ne peut donc contenir aucun bloc `unsafe`. Toutes les valeurs sont envoyées aux bases de données sous forme de paramètres liés, jamais collées dans le texte SQL.

## Comment contribuer ?

Les contributions de toutes tailles sont les bienvenues : signalements de bugs, corrections de documentation, traductions et nouvelles fonctionnalités.

1. Forkez `https://maxlestage.github.io/Memoire/` et clonez-le.
2. Lancez les tests : `cargo test --workspace --features memory,sqlite`. Pour inclure les backends serveur, activez toutes les features et faites pointer `MEMOIRE_PG_URL`, `MEMOIRE_MYSQL_URL`, `MEMOIRE_REDIS_URL` et `MEMOIRE_MONGO_URL` vers des serveurs de test (MongoDB en replica set).
3. Avant d'ouvrir une pull request, lancez `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` et les tests.
4. Ajoutez les tests de comportement dans `crates/memoire-orm/tests/common/mod.rs`, pour qu'ils s'exécutent sur tous les backends.
5. La documentation se trouve dans `docs/en`, `docs/fr` et `docs/es` : mettez à jour les trois, ou ouvrez une issue pour que quelqu'un se charge de la traduction.

Les pull requests vers `master` sont mergées automatiquement dès que la CI est au vert ; ouvrez la vôtre en brouillon (draft) ou ajoutez le label `no-auto-merge` si vous souhaitez d'abord une relecture. Le fichier `CONTRIBUTING.md` du dépôt donne tous les détails.

## Étapes suivantes

- [Démarrage rapide](quick-start) : construisez une petite application en dix minutes.
- [Aide-mémoire](cheatsheet) : toutes les opérations sur une seule page.
- [Modèles](models) : tout sur `#[derive(Model)]`.
