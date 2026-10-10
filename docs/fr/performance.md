# Pourquoi c'est rapide

Memoire est conçu pour que la couche ORM coûte le moins possible par rapport à la base de données elle-même. Cette page explique d'où vient la vitesse, puis donne des conseils pratiques pour que votre propre application reste rapide.

## D'où vient la vitesse

### Du SQL généré à la compilation

Pour les opérations que vous exécutez le plus souvent (`find`, `get`, `insert`, `update`, `delete`, `upsert`, `reload`), `#[derive(Model)]` écrit le SQL pendant la compilation de votre code, pour PostgreSQL, SQLite et MySQL. À l'exécution, ces instructions sont de simples constantes de type chaîne : aucun SQL n'est construit, formaté ni alloué. Vous pouvez les voir dans `User::SCHEMA.sql(Dialect::Postgres)` (voir [Modèles](models)).

Les requêtes construites avec le constructeur de requêtes sont générées à l'exécution dans un unique buffer pré-dimensionné, en une seule passe sur un petit arbre syntaxique. Générer un `SELECT` typique avec un filtre, un tri et une limite prend environ une demi-microseconde.

### Aucune copie de vos paramètres

Chaque champ est converti une seule fois en `Value`. Ensuite, le générateur SQL et les drivers ne font qu'*emprunter* ces valeurs : elles ne sont plus recopiées sur le chemin vers la base de données. Sur SQLite, les valeurs texte et binaires sont transmises à SQLite par référence.

### Des lignes décodées par position, des valeurs déplacées

Quand les lignes reviennent, Memoire les décode **par position** en une seule passe : la colonne 0 va dans le premier champ, la colonne 1 dans le deuxième, et ainsi de suite. Il n'y a ni recherche par nom de colonne ni table de correspondance intermédiaire. Chaque valeur est *déplacée* (move) dans votre struct plutôt que clonée.

### Des requêtes préparées, mises en cache

Chaque instruction est préparée une seule fois par connexion, puis réutilisée :

| Backend | Cache d'instructions |
|---|---|
| SQLite | Jusqu'à 256 instructions par connexion |
| PostgreSQL | Toutes les instructions, par connexion, avec remise à zéro après 1 024 instructions distinctes |
| MySQL / MariaDB | Jusqu'à 256 instructions par connexion |

Comme le builder produit le même texte SQL pour des requêtes de même forme (les valeurs sont toujours des paramètres), une requête exécutée dans une boucle n'est analysée et planifiée qu'une seule fois.

### Des insertions en masse en peu d'instructions

`insert_many` transforme une slice de lignes en instructions `INSERT ... VALUES (...), (...), ...` multi-lignes. Chaque instruction transporte autant de lignes que la base de données le permet : le nombre de paramètres liés reste sous la limite (32 766 sur SQLite, 65 535 sur PostgreSQL et MySQL), avec au plus 10 000 lignes par instruction. Sur Redis, un lot correspond à un seul script atomique, et sur MongoDB à un seul appel `insertMany`. Insérer 1 000 lignes dans une transaction de cette façon est environ 4 fois plus rapide qu'une boucle d'insertions ligne par ligne écrite à la main avec `rusqlite` (voir [Benchmarks](benchmarks)).

### Un moteur en mémoire conçu sur mesure

`MemoryDb` n'analyse pas du tout de SQL : il exécute directement les structures de requête.

- Index de hachage sur la clé primaire et les colonnes `unique` et `index`, qui ne stockent que des positions de lignes : les recherches se font en temps constant et les clés ne sont jamais copiées.
- Les filtres sont vectorisés : découpés en leurs termes `AND`, chacun appliqué par une boucle spécialisée pour sa colonne et sa constante. Les colonnes numériques sont lues dans des copies typées contiguës, à environ 1 ns par ligne. Les autres conditions (`OR`, `LIKE`, fonctions…) sont compilées une fois par requête.
- Les jointures sur égalité de colonnes s'exécutent sous forme de hash joins.
- `order_by` + `limit` sélectionne les premières lignes sur des clés entières compactes, sans tout trier ; `count` compte sans construire de lignes, et les résultats sont décodés directement depuis les lignes stockées.
- Les transactions utilisent des snapshots en copy-on-write : en démarrer une ne coûte presque rien, et une table n'est copiée que lorsque la transaction y écrit pour la première fois.

### Redis : les recherches par clé d'abord

`RedisDb` ne lit que les lignes dont il a besoin quand vous recherchez par clé primaire (`find`, ou un filtre avec `eq`/`is_in` sur la clé) et compte avec `HLEN`. Chaque écriture, même un lot de milliers de lignes, est appliquée par un seul script Lua atomique. Les autres requêtes lisent la table entière et s'exécutent sur le moteur en mémoire. Voir [Redis](redis).

### MongoDB : le serveur se charge du filtrage

`MongoDb` traduit les filtres, le tri, `limit` et `offset` en un seul `find` natif, compte avec `countDocuments`, et exécute `update_many`/`delete_many` sous forme de commandes uniques côté serveur. Les jointures ne récupèrent que les documents dont les clés correspondent. Voir [MongoDB](mongodb).

### Léger par construction

Les builders sont de fines surcouches typées : les types de colonnes et le type du résultat n'existent qu'à la compilation et ne coûtent rien à l'exécution. Il n'y a ni réflexion ni découverte du schéma à l'exécution.

## Conseils pour des applications rapides

### Insérer en masse

```rust
// Lent : une instruction (et un aller-retour) par ligne.
for user in &new_users {
    let mut user = user.clone();
    user.insert(&db).await?;
}

// Rapide : quelques instructions multi-lignes.
User::insert_many(&db, &new_users).await?;
```

Utilisez `insert` quand vous avez besoin de récupérer chaque id généré ; utilisez `insert_many` pour tout le reste.

### Regrouper les écritures dans une transaction

Sur SQLite en particulier, chaque écriture faite en dehors d'une transaction est une transaction à part entière. Regrouper de nombreuses écritures dans une seule transaction est souvent dix fois plus rapide :

```rust
let tx = db.begin().await?;
User::insert_many(&tx, &new_users).await?;
User::update_many().set(User::ACTIVE, true).filter(User::AGE.ge(18)).execute(&tx).await?;
tx.commit().await?;
```

### Modifier et supprimer dans la base de données

Ne chargez pas des lignes uniquement pour les modifier une par une. Laissez la base de données le faire en une seule instruction :

```rust
// Une seule instruction, quel que soit le nombre de lignes concernées.
User::update_many().increment(User::VISITS, 1).filter(User::ACTIVE.eq(true)).execute(&db).await?;
User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
```

### Ne sélectionner que ce dont vous avez besoin

Charger des lignes complètes implique de transférer et d'allouer toutes les colonnes. Quand vous n'en avez besoin que de quelques-unes, utilisez `columns` :

```rust
let emails: Vec<String> = User::query().columns(User::EMAIL).filter(User::ACTIVE.eq(true)).all(&db).await?;
```

### Poser des questions précises

| Au lieu de | Utilisez | Pourquoi |
|---|---|---|
| `query.all(&db).await?.len()` | `query.count(&db)` | Compte dans la base de données et ne transfère qu'un nombre |
| `query.count(&db).await? > 0` | `query.exists(&db)` | S'arrête à la première ligne correspondante |
| `query.all(&db).await?.into_iter().next()` | `query.first(&db)` | Ajoute `LIMIT 1` |
| Charger des lignes pour additionner une colonne | `query.sum(Column, &db)` | Agrège dans la base de données |

### Indexer les colonnes sur lesquelles vous cherchez

Ajoutez `#[memoire(index)]` aux colonnes sur lesquelles vous filtrez, joignez ou triez souvent, en particulier les clés étrangères. Les colonnes `unique` sont indexées automatiquement. MongoDB crée les mêmes index sur ses collections. Le moteur en mémoire indexe lui aussi la clé primaire et les colonnes `unique` et `index` (un filtre d'égalité ou une jointure sur elles devient une recherche, pas un parcours) ; Redis retrouve directement les lignes par clé primaire, donc `index` n'y a pas d'effet.

### Éviter les requêtes N+1

Charger des lignes liées avec une requête par parent est le problème de performance le plus courant dans le code qui utilise un ORM. Chargez tous les enfants avec une seule requête `is_in` et regroupez-les en mémoire, comme le montre [Jointures et relations](relations).

### Paginer par clé, pas par offset

`offset(10_000)` oblige la base de données à parcourir 10 000 lignes avant de renvoyer quoi que ce soit. Pour les longues listes et le défilement infini, mémorisez le dernier id affiché et repartez de là :

```rust
async fn next_page<E: Executor>(db: &E, after_id: i64) -> memoire_orm::Result<Vec<User>> {
    User::query()
        .filter(User::ID.gt(after_id))
        .order_by(User::ID.asc())
        .limit(50)
        .all(db)
        .await
}
```

Cette approche reste rapide quelle que soit la page. Réservez `paginate` (qui exécute aussi un `COUNT`) aux écrans qui ont vraiment besoin de numéros de page.

### Réutiliser le handle de base de données

Ouvrez la base de données une seule fois au démarrage et partagez-la : cloner n'importe quel handle (`MemoryDb`, `SqliteDb`, `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb`) ne coûte presque rien, et les clones partagent les mêmes connexions. Ouvrir un nouveau pool à chaque requête jette les connexions et les caches d'instructions. Dimensionnez le pool PostgreSQL selon ce que votre serveur autorise (voir [PostgreSQL](postgres)).

### Mesurer en mode release

Les builds de debug du code Rust sont bien plus lents. Faites vos benchmarks et vos déploiements avec `cargo build --release`. Pour comprendre une requête lente, affichez son SQL avec `to_sql` et exécutez-la avec `EXPLAIN` via le [SQL brut](raw-sql).

## Étapes suivantes

- [Benchmarks](benchmarks) : les chiffres mesurés et comment les reproduire.
- [Jointures et relations](relations) : charger efficacement les lignes liées.
- [Transactions](transactions) : regrouper les écritures.
