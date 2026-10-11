# Memoire, Diesel, SeaORM et Nitrite

Diesel et SeaORM sont les deux ORM les plus utilisés en Rust, et Nitrite une base NoSQL embarquée répandue. Cette page les compare à Memoire fonctionnalité par fonctionnalité, pour choisir le bon outil ou passer de l'un à l'autre. Elle se veut honnête : chaque bibliothèque est en avance quelque part.

## En une phrase chacun

- **Diesel** : un constructeur de requêtes synchrone qui vérifie chaque requête à la compilation contre un `schema.rs` généré depuis votre base (l'asynchrone passe par la crate séparée `diesel-async`). PostgreSQL, MySQL, SQLite.
- **SeaORM** : un ORM asynchrone construit sur SQLx, avec des entités générées par une CLI, des `ActiveModel` pour les modifications et de nombreux outils de relations. PostgreSQL, MySQL, SQLite.
- **Memoire** : un ORM asynchrone avec un seul `#[derive(Model)]` sur votre propre struct, le SQL des opérations par clé primaire produit pendant la compilation, et la même API en SQL et en NoSQL : PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB et un moteur en mémoire en pur Rust qui tourne en WebAssembly.

## Fonctionnalité par fonctionnalité

| | Diesel | SeaORM | Memoire |
|---|---|---|---|
| Asynchrone | via `diesel-async` | oui | oui, sur toutes les bases |
| Définition des modèles | schéma `table!` + derives sur la struct | module d'entité généré (`Model`, `ActiveModel`, `Relation`...) | un `#[derive(Model)]` sur votre struct |
| Filtres vérifiés par le typage | oui, contre le schéma | en partie (enums de colonnes, valeurs vérifiées à l'exécution) | oui : `User::AGE.gt("18")` ne compile pas |
| SQL du CRUD par clé primaire | généré à partir des types de requête, requêtes préparées en cache | construit à l'exécution | **produit à la compilation** pour chaque dialecte |
| PostgreSQL / MySQL / SQLite | oui | oui | oui |
| Redis, MongoDB | non | non | oui, même API |
| Moteur en mémoire, WebAssembly | non | connexion factice pour les tests | oui, un vrai moteur avec index et transactions, enregistrable dans un fichier (chiffré) |
| Jointures | oui, typées | oui | oui, décodées en tuples |
| Relations sans N+1 | `belonging_to` + `grouped_by` | `find_with_related`, `load_one`, `load_many`, `load_many_to_many` | `belonging_to`, `load_children`, `load_parent`, `load_via` |
| Déclaration des relations | `#[diesel(belongs_to)]` + `joinable!` | enum `Relation` + impls `Related` | aucune : lue dans `references = "table(col)"` |
| Mise à jour partielle | struct `AsChangeset` | `ActiveModel` avec `Set(...)` | `update_only(&db, (User::NAME, User::AGE))` |
| Index sur plusieurs colonnes | dans les migrations | dans les migrations | `#[memoire(unique(a, b), index(c, d))]`, respectés par toutes les bases |
| Recherche plein texte | PostgreSQL, avec la crate `diesel_full_text_search` | expressions personnalisées | `Column::matches`, la même règle sur les six bases |
| Notifications de changements | non | hooks `ActiveModelBehavior` par entité | `Observed::subscribe` : chaque écriture, annoncée au commit |
| Mise à jour / suppression en masse filtrée | oui | oui | oui, avec `increment`, `set_null`... |
| Insertion multiple | oui, renvoie les lignes sur PG/SQLite | oui | oui, par lots sous la limite de paramètres de chaque base |
| Upsert | `on_conflict` | `on_conflict` | `upsert` (par clé primaire) |
| Pagination | `limit`/`offset` à la main | `paginate`, pagination par curseur | `paginate` (comptage et page en parallèle) |
| Agrégats, `GROUP BY` | oui | oui | oui |
| Sous-requêtes, auto-jointures (alias) | oui | oui | pas encore : SQL brut |
| Clés primaires composites | oui | oui | pas encore : clé artificielle + `unique` |
| Résultats en flux | `load_iter` | `stream` | pas encore |
| Migrations | CLI, fichiers SQL `up`/`down` | migrations Rust avec `up`/`down` | Rust ou SQL, vers l'avant uniquement, en transaction |
| Erreurs normalisées | `DatabaseErrorKind` (unicité, clé étrangère...) | `SqlErr` (unicité, clé étrangère) | `UniqueViolation`, `NotFound`, `Constraint`, `Conflict`... sur toutes les bases, NoSQL compris |
| Génération de code depuis une base existante | oui (`diesel print-schema`) | oui (`sea-orm-cli generate entity`) | non |

## Passer de Diesel à Memoire

| Diesel | Memoire |
|---|---|
| `users::table.find(1).first::<User>(&mut conn)` | `User::get(&db, 1).await?` |
| `users::table.filter(users::age.gt(18)).load::<User>(&mut conn)` | `User::query().filter(User::AGE.gt(18)).all(&db).await?` |
| `diesel::insert_into(users::table).values(&new_user).execute(&mut conn)` | `user.insert(&db).await?` (l'id est réécrit dans la struct) |
| `diesel::update(&user).set(&changes).execute(&mut conn)` | `user.update_only(&db, (User::NAME, User::AGE)).await?` |
| `Post::belonging_to(&users).load::<Post>(&mut conn)?.grouped_by(&users)` | `users.load_children::<Post>(&db).await?` |
| `conn.transaction(\|conn\| ...)` | `let tx = db.begin().await?; ...; tx.commit().await?` |

## Passer de SeaORM à Memoire

| SeaORM | Memoire |
|---|---|
| `user::Entity::find_by_id(1).one(&db)` | `User::find(&db, 1).await?` |
| `user::Entity::find().filter(user::Column::Age.gt(18)).all(&db)` | `User::query().filter(User::AGE.gt(18)).all(&db).await?` |
| `active_model.insert(&db)` | `user.insert(&db).await?` |
| `let mut am: user::ActiveModel = u.into(); am.name = Set(..); am.update(&db)` | `u.name = ..; u.update_only(&db, User::NAME).await?` |
| `user.find_related(post::Entity).all(&db)` | `Post::belonging_to(&user).all(&db).await?` |
| `users.load_many(post::Entity, &db)` | `users.load_children::<Post>(&db).await?` |
| `posts.load_many_to_many(tag::Entity, post_tag::Entity, &db)` | `posts.load_via::<PostTag, Tag>(&db).await?` |
| `user::Entity::find().paginate(&db, 20).fetch_page(1)` | `User::query().paginate(&db, 2, 20).await?` (les pages commencent à 1) |

## Memoire et Nitrite (NoSQL)

Nitrite est une base de documents embarquée, pour Java, Kotlin, Flutter et Rust (la crate `nitrite` est en 1.0). Ce n'est pas un ORM devant d'autres bases mais une base dans votre programme, comme le moteur en mémoire de Memoire : c'est la comparaison qui a du sens.

| | Nitrite (Rust) | Memoire |
|---|---|---|
| Données | Documents sans schéma (`doc!{...}`), imbriqués, tableaux | Structs typées ; données imbriquées dans des colonnes `Json<T>` |
| API | Synchrone | Asynchrone |
| Filtres | `field("age").gt(18)` : n'importe quelle valeur, vérifiée à l'exécution | `User::AGE.gt(18)` : vérifié à la compilation |
| Enregistré dans un fichier | Oui (crate d'adaptation Fjall) | Oui : `MemoryDb::open(path)` |
| Chiffrement | AES-GCM | AES-256-GCM avec une clé Argon2id (feature `encryption`) |
| Index | Unique, non unique, composé, plein texte ; spatial dans une crate séparée | Unique, non unique, sur plusieurs colonnes, plein texte |
| Événements de changement | `collection.subscribe(...)` | `Observed::subscribe`, sur toutes les bases |
| Transactions | Oui | Oui (instantanés, conflits détectés) |
| Jointures, agrégats, `GROUP BY` | `join` et `project` sur un curseur, pas d'agrégats | Oui |
| Le même code sur des serveurs | Non | PostgreSQL, MySQL, SQLite, Redis, MongoDB |
| WebAssembly | Non documenté | Oui |
| Licence | Apache-2.0 | Propriétaire, libre d'utilisation |

Mesuré sur la même machine avec 10 000 documents, les mêmes index et des résultats identiques (Nitrite 1.0.1 avec son stockage en mémoire, face à `MemoryDb` ; une boucle de mesure, pas criterion) :

| Opération | Nitrite | Memoire |
|---|---:|---:|
| Insérer 10 000 documents | 189 ms | **2,96 ms** |
| Trouver par un e-mail unique | 11,2 µs | **0,41 µs** |
| Filtrer sur un champ indexé (10 lignes) | 30,7 µs | **1,70 µs** |
| Intervalle + filtre + tri + `LIMIT 20` (sans index) | 10,2 ms | **27 µs** |
| Compter avec un filtre (sans index) | 9,0 ms | **7,7 µs** |

Les deux dernières lignes filtrent des champs sans index : Nitrite lit alors des documents dynamiques un par un, alors que Memoire parcourt des colonnes typées et contiguës. Un index sur ces champs réduirait l'écart côté Nitrite.

## Lequel choisir

- **Diesel** si vous voulez les garanties de compilation les plus fortes contre le schéma réel et la couverture SQL la plus mûre (sous-requêtes, clés composites, types SQL personnalisés), et que du code synchrone vous convient.
- **SeaORM** si vous voulez de l'asynchrone avec un large écosystème autour (génération de code depuis une base existante, GraphQL, outils d'administration) et que son style d'entités vous convient.
- **Nitrite** si vos données n'ont pas de forme fixe (documents imbriqués, tableaux, champs qui varient d'un document à l'autre), si vous avez besoin de requêtes spatiales, ou d'une licence open source.
- **Memoire** si vous voulez de l'asynchrone avec le moins de code répétitif possible (votre propre struct, un derive), le même code en SQL, en NoSQL et dans le navigateur, des tests qui tournent sur le moteur en mémoire sans serveur de base de données, et des erreurs normalisées.

## Étapes suivantes

- [Jointures et relations](relations) : `belonging_to`, `load_children`, `load_parent`, `load_via`.
- [Créer, lire, modifier, supprimer](crud) : `find_many`, `update_only`.
- [Benchmarks](benchmarks) : les chiffres mesurés.
