# Aide-mémoire

Toutes les opérations courantes de Memoire sur une seule page. Les exemples supposent `use memoire_orm::prelude::*;`, un handle de base de données `db`, et les modèles `User` (`id`, `email`, `name`, `age: Option<i32>`, `active: bool`, `visits: i32`) et `Post` (`id`, `user_id`, `title`, `views: i32`).

## Mise en place

| Tâche | Code |
|---|---|
| Moteur en mémoire (feature par défaut) | `cargo add memoire-orm` |
| SQLite | `cargo add memoire-orm --features sqlite` |
| PostgreSQL / avec TLS | `--features postgres` / `--features postgres-tls` (plus Tokio) |
| MySQL, MariaDB / avec TLS | `--features mysql` / `--features mysql-tls` (plus Tokio) |
| Redis / avec TLS | `--features redis` / `--features redis-tls` (plus Tokio) |
| MongoDB | `--features mongodb` (plus Tokio) |
| Types de colonnes supplémentaires | `--features chrono,uuid,json` (ou `full`) |
| Tout importer | `use memoire_orm::prelude::*;` |
| Ouvrir en mémoire | `let db = MemoryDb::new();` |
| Ouvrir un fichier SQLite / en mémoire | `SqliteDb::open("app.db")?` / `SqliteDb::open_in_memory()?` |
| Se connecter à PostgreSQL | `PgPool::connect(url).await?` ou `PgPool::connect_with(url, 10).await?` |
| Se connecter à MySQL / MariaDB | `MySqlPool::connect("mysql://user:pass@host:3306/app").await?` |
| Se connecter à Redis | `RedisDb::connect(url).await?` ou `RedisDb::connect_with_prefix(url, "myapp").await?` |
| Se connecter à MongoDB | `MongoDb::connect("mongodb://host:27017/app").await?` ou `MongoDb::connect_with_database(uri, "app").await?` |
| Exécuter sans runtime (mémoire, SQLite) | `memoire_orm::block_on(future)` |

Pages des backends : [Moteur en mémoire](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).

## Attributs de modèle

| Attribut | Exemple |
|---|---|
| Nom de la table | `#[memoire(table = "users")]` sur la struct |
| Clé primaire | `#[memoire(primary_key)]` (un champ nommé `id` est la clé par défaut) |
| Clé générée | `#[memoire(primary_key, auto_increment)]` (implicite pour un `id` entier sans attribut) |
| Unique / index | `#[memoire(unique)]`, `#[memoire(index)]` |
| Nom de colonne | `#[memoire(column = "e_mail")]` |
| Type SQL | `#[memoire(sql_type = "VARCHAR(120)")]` |
| Valeur par défaut | `#[memoire(default = "0")]` |
| Clé étrangère | `#[memoire(references = "users(id)", on_delete = "cascade")]` |
| Non stocké | `#[memoire(skip)]` (le type du champ doit implémenter `Default`) |
| Colonne nullable | `age: Option<i32>` |
| Type de ligne personnalisé | `#[derive(FromRow)] struct Stats { user_id: i64, posts: i64 }` |

## Schéma

| Tâche | Code |
|---|---|
| Créer une table | `db.create_table::<User>().await?` |
| Créer si absente | `db.create_table_if_not_exists::<User>().await?` |
| Supprimer une table | `db.drop_table::<User>().await?` / `db.drop_table_if_exists::<User>().await?` |
| Voir le DDL | `memoire_orm::sql::create_table(User::SCHEMA, Dialect::Postgres, false)` |
| Voir le SQL des opérations par clé primaire | `User::SCHEMA.sql(Dialect::Sqlite).insert` (aussi `find`, `update`, `delete`, `upsert`) |

## Créer

| Tâche | Code |
|---|---|
| Insérer une ligne (id réécrit dans la struct) | `user.insert(&db).await?` (`user` doit être `mut`) |
| Insérer plusieurs lignes (renvoie le nombre) | `User::insert_many(&db, &users).await?` |
| Insérer ou remplacer par clé | `user.upsert(&db).await?` |

## Lire

| Tâche | Code |
|---|---|
| Par clé, facultatif | `User::find(&db, 1).await?` → `Option<User>` |
| Par clé, obligatoire | `User::get(&db, 1).await?` → `User` ou `Error::NotFound` |
| Toutes les lignes | `User::all(&db).await?` |
| Compter toutes les lignes | `User::count(&db).await?` |
| Rafraîchir une struct | `user.reload(&db).await?` |
| Démarrer une requête | `User::query()` |

## Filtres

| SQL | Code |
|---|---|
| `=`, `<>` | `User::NAME.eq("Ada")`, `User::NAME.ne("Ada")` |
| `<`, `<=`, `>`, `>=` | `User::AGE.lt(18)`, `.le(18)`, `.gt(18)`, `.ge(18)` |
| `AND`, `OR`, `NOT` | `a & b`, `a \| b`, `!a` |
| Plusieurs filtres (AND) | `.filter(a).filter(b)` |
| OR avec les filtres précédents | `.or_filter(c)` |
| `IN`, `NOT IN` | `User::ID.is_in([1, 2, 3])`, `User::ID.not_in(ids)` |
| `BETWEEN` (bornes incluses) | `User::AGE.between(18, 30)`, `.not_between(18, 30)` |
| `IS NULL`, `IS NOT NULL` | `User::AGE.is_null()`, `User::AGE.is_not_null()` |
| Colonne = colonne | `Post::USER_ID.eq_col(User::ID)` (aussi `ne_col`, `lt_col`, ...) |
| AND / OR d'une liste | `Expr::all(conditions)`, `Expr::any(conditions)` |
| Arithmétique | `User::VISITS.mul(2).gt(Expr::val(10))` (aussi `add`, `sub`, `div`) |

## Recherche dans le texte

| Correspondance | Code |
|---|---|
| Motif (`%`, `_`) | `User::EMAIL.like("%@example.com")`, `.not_like(...)` |
| Motif, insensible à la casse | `User::EMAIL.ilike("%@EXAMPLE.com")` |
| Contient (échappé) | `User::NAME.contains("ad")`, `User::NAME.icontains("AD")` |
| Commence / se termine par (échappé) | `User::NAME.starts_with("A")`, `User::EMAIL.ends_with(".org")` |

## Tri et pagination

| Tâche | Code |
|---|---|
| Croissant / décroissant | `.order_by(User::NAME)`, `.order_by(User::NAME.asc())`, `.order_by(User::AGE.desc())` |
| Position des NULL | `.order_by(User::AGE.desc().nulls_last())`, `.nulls_first()` |
| Limite / offset | `.limit(20)`, `.offset(40)` |
| Page (à partir de 1) | `.page(3, 20)` |
| Page avec total | `.paginate(&db, 3, 20).await?` → `Page { items, page, per_page, total }` |
| Lignes distinctes | `.distinct()` |

## Exécuter une requête

| Résultat | Code |
|---|---|
| Toutes les lignes | `.all(&db).await?` → `Vec<T>` |
| Première ligne | `.first(&db).await?` → `Option<T>` |
| Exactement une | `.one(&db).await?` → `T` ou `Error::NotFound` |
| Nombre | `.count(&db).await?` → `u64` |
| Au moins une correspondance ? | `.exists(&db).await?` → `bool` |

## Agrégats et projections

| Tâche | Code |
|---|---|
| Somme | `User::query().sum(User::VISITS, &db).await?` → `Option<i64>` (`f64` pour les flottants) |
| Moyenne | `.avg(User::AGE, &db).await?` → `Option<f64>` |
| Min / max | `.min(User::AGE, &db).await?`, `.max(User::NAME, &db).await?` |
| Une colonne | `User::query().columns(User::EMAIL).all(&db).await?` → `Vec<String>` |
| Plusieurs colonnes | `.columns((User::ID, User::NAME))` → `Vec<(i64, String)>` (jusqu'à 8) |
| Nombre par groupe | `Post::query().group_by(Post::USER_ID).select_exprs([Post::USER_ID.expr(), count_all()])` → lignes `(i64, i64)` |
| Filtrer les groupes | `.having(count_all().gt(Expr::val(1_i64)))` |
| Expressions d'agrégation | `count_all()`, `col.count()`, `col.count_distinct()`, `col.sum()`, `col.avg()`, `col.min()`, `col.max()` |

## Jointures

| Tâche | Code | Lignes |
|---|---|---|
| Jointure interne | `Post::query().inner_join::<User>(User::ID.eq_col(Post::USER_ID))` | `(Post, User)` |
| Jointure externe gauche | `User::query().left_join::<Post>(Post::USER_ID.eq_col(User::ID))` | `(User, Option<Post>)` |
| Jointure pour filtrer uniquement | `User::query().join::<Post>(JoinKind::Inner, cond).distinct()` | `User` |
| Colonnes de plusieurs tables | `.inner_join::<User>(cond).columns((User::NAME, Post::TITLE))` | `(String, String)` |
| Charger les enfants en une requête | `Post::query().filter(Post::USER_ID.is_in(user_ids))` | `Post` |

## Modifier et supprimer

| Tâche | Code |
|---|---|
| Enregistrer une ligne (toutes les colonnes) | `user.update(&db).await?` (`Error::NotFound` si absente) |
| Modifier plusieurs lignes | `User::update_many().set(User::ACTIVE, false).filter(...).execute(&db).await?` → `u64` |
| Mettre à NULL | `.set_null(User::AGE)` |
| Incrémenter / décrémenter | `.increment(User::VISITS, 1)`, `.decrement(User::VISITS, 1)` |
| Affecter une expression | `.set_expr(User::VISITS, User::VISITS.mul(2))` |
| Supprimer une ligne | `user.delete(&db).await?` → `bool` |
| Supprimer par clé | `User::delete_by_key(&db, 1).await?` → `bool` |
| Supprimer plusieurs lignes | `User::delete_many().filter(...).execute(&db).await?` → `u64` |

## Transactions

| Tâche | Code |
|---|---|
| Démarrer | `let tx = db.begin().await?;` |
| L'utiliser | passez `&tx` au lieu de `&db` |
| Valider / annuler | `tx.commit().await?` / `tx.rollback().await?` |
| Annulation automatique | droppez `tx` (par exemple en sortant prématurément avec `?`) |
| Réessayer en cas de conflit | `Err(e) if e.is_conflict() => continue` dans une boucle autour de toute la transaction |
| Code générique | `async fn f<E: Executor>(db: &E)` (handle ou transaction), `async fn g<D: Database>(db: &D)` (peut appeler `begin`) |

## Migrations

| Tâche | Code |
|---|---|
| Définir | `Migration::new(1, "create_users").create_table::<User>()` |
| Autres étapes | `.drop_table("old")`, `.sql("...")`, `.sql_for(Dialect::Postgres, "...")` |
| Exécuter celles en attente | `Migrator::new().add(m1).add(m2).run(&db).await?` → versions appliquées |
| Inspecter | `migrator.applied(&db).await?`, `migrator.pending(&db).await?` |

## SQL brut et accès aux drivers

| Tâche | Code |
|---|---|
| Requête | `db.raw::<(i64, String)>("SELECT id, name FROM users WHERE age > ?", &params![18]).await?` |
| Exécution | `db.raw_execute("DELETE FROM users WHERE active = ?", &params![false]).await?` → `u64` |
| Sans paramètres | `db.raw::<i64>("SELECT COUNT(*) FROM users", &[]).await?` |
| Disponible sur | SQLite, PostgreSQL, MySQL/MariaDB (en mémoire, Redis et MongoDB renvoient `Error::Unsupported`) |
| Placeholders | SQLite et MySQL `?`, PostgreSQL `$1`, `$2`, ... |
| Connexion SQLite | `db.with_connection(\|conn\| ...).await` |
| Pool PostgreSQL / MySQL | `db.pool()` |
| Connexion Redis | `db.connection()` |
| Base de données MongoDB | `db.database()` |
| Transactions MongoDB disponibles ? | `db.supports_transactions()` |

## Erreurs

| Vérification | Code |
|---|---|
| Ligne absente | `err.is_not_found()` / `Error::NotFound` |
| Clé ou valeur unique en double | `err.is_unique_violation()` / `Error::UniqueViolation { table, column }` |
| Conflit de transaction | `err.is_conflict()` / `Error::Conflict(table)` |
| Autre contrainte | `Error::Constraint(message)` |
| NULL dans un champ qui n'est pas une `Option` | `Error::UnexpectedNull { column }` |
| Non pris en charge par le backend | `Error::Unsupported(message)` |

## Inspecter le SQL

| Tâche | Code |
|---|---|
| SQL d'une requête | `User::query().filter(...).to_sql(Dialect::Postgres)` |
| SQL et paramètres | `.to_sql_with_params(Dialect::Sqlite)` → `(String, Vec<Value>)` |
| SQL d'une modification / suppression | `User::update_many().set(...).to_sql(Dialect::MySql)`, `User::delete_many().to_sql(...)` |

## Étapes suivantes

- [Constructeur de requêtes](queries) : les détails derrière chaque ligne.
- [FAQ](faq) : les réponses aux questions fréquentes.
- [Démarrage rapide](quick-start) : voyez tout cela en action.
