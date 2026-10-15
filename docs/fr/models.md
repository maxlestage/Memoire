# Modèles

Un modèle est une struct Rust qui décrit une table de la base de données. Cette page couvre tout ce que comprend `#[derive(Model)]` : noms de tables, clés primaires, attributs de colonnes, types Rust pris en charge, éléments générés pour vous par la macro, ainsi que `#[derive(FromRow)]` pour définir vos propres lignes de résultat.

## Votre premier modèle

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    #[memoire(index)]
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
}
```

Chaque champ devient une colonne du même nom. Voici ce que cela produit sur SQLite :

```sql
CREATE TABLE "users" (
    "id" INTEGER PRIMARY KEY,
    "email" TEXT NOT NULL UNIQUE,
    "name" TEXT NOT NULL,
    "age" INTEGER,
    "active" INTEGER NOT NULL
);
CREATE INDEX "idx_users_name" ON "users" ("name");
```

Remarquez que `age` n'a pas de `NOT NULL` : c'est une `Option<i32>`, elle accepte donc `NULL`. Toutes les autres colonnes sont obligatoires.

### Ce que génère le derive

`#[derive(Model)]` écrit tout ceci pour vous à la compilation :

| Élément généré | Exemple | Utilisé pour |
|---|---|---|
| Une implémentation du trait `Model` | `User::query()`, `user.insert(&db)`, `User::find(&db, 1)` | Toutes les opérations sur la base de données |
| Une implémentation de `FromRow` | (utilisée en interne) | Reconvertir une ligne de la base de données en `User` |
| Une constante de colonne typée par champ | `User::ID`, `User::EMAIL`, `User::AGE` | Filtres, tris et projections typés |
| Une description statique de la table avec du SQL pré-généré | `User::SCHEMA` | Créer les tables et exécuter les opérations par clé primaire sans construire de SQL à l'exécution |

### Contraintes

- La struct doit avoir des **champs nommés** (`struct User { ... }`). Les tuple structs, les unit structs et les enums ne peuvent pas être des modèles.
- La struct **ne peut pas être générique** (`struct Wrapper<T>` est refusée).
- Le type de chaque champ stocké doit être pris en charge (voir le tableau des types plus bas).
- Le modèle doit être `Send + Sync + 'static`, ce qui est toujours le cas d'une struct ordinaire qui possède ses données.

> **Remarque** `#[derive(Model)]` implémente déjà `FromRow`. N'ajoutez pas `#[derive(FromRow)]` sur la même struct, sinon les deux implémentations entreront en conflit.

## Le nom de la table

Définissez le nom de la table avec l'attribut de struct `table` :

```rust
#[derive(Model)]
#[memoire(table = "blog_posts")]
pub struct BlogPost {
    pub id: i64,
    pub title: String,
}
```

Sans cet attribut, le nom de la table est le nom de la struct en `snake_case`, **sans** mise au pluriel :

| Struct | Nom de table par défaut |
|---|---|
| `User` | `user` |
| `BlogPost` | `blog_post` |
| `HTTPRequest` | `http_request` |

> **Astuce** Écrire systématiquement `#[memoire(table = "...")]` rend votre schéma explicite et vous permet de renommer la struct plus tard sans toucher à la base de données.

## Utiliser Memoire à travers une autre crate

`#[derive(Model)]` écrit du code qui fait référence à `::memoire_orm`. Un framework qui réexporte Memoire (comme [Paquets](https://paquets-90dfc06b6fea.herokuapp.com/), où il s'appelle `paquets::orm`) indique à la dérivation où le trouver avec l'attribut de struct `crate` : votre projet n'a alors pas besoin de sa propre dépendance `memoire-orm`.

```rust
#[derive(paquets::orm::Model)]
#[memoire(crate = "paquets::orm", table = "notes")]
pub struct Note {
    pub id: i64,
    pub text: String,
}
```

## Clés primaires

Chaque modèle a besoin d'exactement une clé primaire. Elle identifie une ligne, et c'est elle qu'utilisent `find`, `get`, `update`, `delete` et `upsert`.

Il y a deux façons de la choisir :

1. Marquer un champ avec `#[memoire(primary_key)]`.
2. Ou simplement nommer un champ `id` : en l'absence de tout `primary_key` explicite, un champ appelé `id` devient la clé primaire.

### Clés auto-incrémentées

Avec `auto_increment`, c'est la base de données qui génère la clé quand vous insérez une ligne. La valeur présente dans votre struct est ignorée à l'insertion, et la valeur générée y est réécrite :

```rust
#[derive(Model, Debug)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub text: String,
}

async fn demo(db: &MemoryDb) -> memoire_orm::Result<()> {
    let mut note = Note { id: 0, text: "hi".into() };
    note.insert(db).await?;
    assert_eq!(note.id, 1); // générée par la base de données
    Ok(())
}
```

Les clés auto-incrémentées doivent être de type `i16`, `i32` ou `i64`.

### La règle implicite du champ `id`

Voici exactement comment Memoire décide, pour éviter toute surprise :

| Déclaration du champ | Clé primaire ? | Auto-incrément ? |
|---|---|---|
| `id: i64` (aucun attribut dans toute la struct) | oui (implicite) | **oui** (implicite, car c'est un `i16`/`i32`/`i64` nommé `id`) |
| `id: String` ou `id: uuid::Uuid` (aucun attribut) | oui (implicite) | non |
| `#[memoire(primary_key, auto_increment)] id: i64` | oui | oui |
| `#[memoire(primary_key)] id: i64` | oui | **non** : vous avez choisi la clé explicitement, ajoutez donc `auto_increment` si vous voulez des ids générés |
| `#[memoire(auto_increment = false)] id: i64` | oui (implicite) | non |
| `#[memoire(primary_key)] slug: String` | oui | non |

> **Attention** `#[memoire(primary_key)] id: i64` seul n'est *pas* auto-incrémenté. Si vous insérez plusieurs lignes avec `id: 0`, la deuxième insertion échoue sur une violation d'unicité. Ajoutez `auto_increment`, ou retirez l'attribut et laissez jouer la règle implicite.

### Clés naturelles

Une clé n'est pas forcément un nombre. Ici, le slug est la clé, et c'est vous qui le fournissez :

```rust
#[derive(Model, Debug, Clone)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}

async fn natural(db: &MemoryDb) -> memoire_orm::Result<()> {
    let mut rust = Tag { slug: "rust".into(), label: "Rust".into() };
    rust.insert(db).await?;
    let found = Tag::get(db, "rust").await?; // recherche par la clé de type chaîne
    assert_eq!(found.label, "Rust");
    Ok(())
}
```

## Attributs de colonnes

Les attributs se placent dans `#[memoire(...)]` au-dessus d'un champ. Vous pouvez en combiner plusieurs, séparés par des virgules.

| Attribut | Exemple | Effet |
|---|---|---|
| `primary_key` | `#[memoire(primary_key)]` | Ce champ est la clé primaire (alias : `id`) |
| `auto_increment` | `#[memoire(primary_key, auto_increment)]` | La base de données génère la clé (alias : `auto` ; `auto_increment = false` désactive la règle implicite) |
| `unique` | `#[memoire(unique)]` | Ajoute une contrainte `UNIQUE` ; les doublons échouent avec `Error::UniqueViolation` |
| `index` | `#[memoire(index)]` | Crée un index nommé `idx_<table>_<column>` |
| `fulltext` | `#[memoire(fulltext)]` | Indexe les mots d'une colonne de texte pour la recherche plein texte (`matches`) |
| `column` | `#[memoire(column = "e_mail")]` | Stocke le champ sous un autre nom de colonne (alias : `rename`) |
| `sql_type` | `#[memoire(sql_type = "VARCHAR(120)")]` | Remplace le type SQL utilisé dans `CREATE TABLE` |
| `default` | `#[memoire(default = "0")]` | Ajoute `DEFAULT <expression SQL>` à la colonne |
| `references` | `#[memoire(references = "users(id)")]` | Ajoute une clé étrangère ; `"users.id"` est accepté aussi |
| `on_delete` | `#[memoire(references = "users(id)", on_delete = "cascade")]` | Action à effectuer quand la ligne référencée est supprimée |
| `on_update` | `#[memoire(references = "users(id)", on_update = "cascade")]` | Action à effectuer quand la clé référencée change |
| `skip` | `#[memoire(skip)]` | Le champ n'est pas stocké du tout |
| `created_at`, `updated_at` | `#[memoire(created_at)]` | Rempli avec l'heure courante à l'insertion / à chaque écriture (voir [Horodatage, suppression douce et hooks](lifecycle) ; `#[memoire(timestamps)]` sur la structure utilise les champs de ces noms) |
| `soft_delete` | `#[memoire(soft_delete)]` | Colonne de suppression douce : `delete` la remplit et les requêtes ignorent les lignes marquées (`#[memoire(soft_deletes)]` sur la structure utilise `deleted_at`) |

Voici un modèle qui en utilise la plupart :

```rust
#[derive(Model, Debug, Clone)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub author_id: i64,
    #[memoire(sql_type = "VARCHAR(200)")]
    pub title: String,
    #[memoire(column = "body_markdown")]
    pub body: String,
    #[memoire(default = "0")]
    pub views: i32,
    #[memoire(skip)]
    pub rendered_html: Option<String>,
}
```

Sur PostgreSQL, cela crée :

```sql
CREATE TABLE "posts" (
    "id" BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    "author_id" BIGINT NOT NULL,
    "title" VARCHAR(200) NOT NULL,
    "body_markdown" TEXT NOT NULL,
    "views" INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY ("author_id") REFERENCES "users" ("id") ON DELETE CASCADE
);
CREATE INDEX "idx_posts_author_id" ON "posts" ("author_id");
```

### `unique` et `index`

- `unique` fait refuser par la base de données une deuxième ligne ayant la même valeur. Memoire le signale sous la forme `Error::UniqueViolation { table, column }` sur tous les backends. Une colonne `unique` est automatiquement indexée par la base de données.
- `index` accélère les filtres, les jointures et les tris sur cette colonne. Utilisez-le sur les colonnes sur lesquelles vous recherchez souvent, comme les clés étrangères. Les bases de données SQL, MongoDB et le moteur en mémoire créent un vrai index ; Redis l'ignore.

### Index sur plusieurs colonnes

`unique(...)` et `index(...)` placés sur la **struct** couvrent plusieurs champs à la fois. Répétez-les autant que nécessaire :

```rust
#[derive(Model, Debug, Clone)]
#[memoire(table = "memberships", unique(user_id, team_id), index(team_id, role))]
pub struct Membership {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub team_id: i64,
    pub role: String,
}
```

- `unique(user_id, team_id)` refuse une deuxième ligne avec la même *paire* ; chaque colonne seule peut se répéter. L'erreur est `Error::UniqueViolation { column: "user_id, team_id", .. }` sur toutes les bases. Comme en SQL, une ligne qui a un `NULL` dans l'une des colonnes n'est pas contrainte.
- `index(team_id, role)` accélère les filtres qui fixent toutes les colonnes de l'index, comme `Membership::TEAM_ID.eq(3) & Membership::ROLE.eq("admin")`.

Les index s'appellent `uq_<table>_<a>_<b>` et `idx_<table>_<a>_<b>` (raccourcis avec un hash au-delà de 63 caractères). Toutes les bases font respecter les index uniques : les bases SQL avec un `UNIQUE INDEX`, MongoDB avec un index composé unique, Redis avec une clé par combinaison, et le moteur en mémoire avec un index de hachage qu'il utilise aussi pour les recherches.

### `fulltext`

`#[memoire(fulltext)]` sur une colonne de texte la prépare à la [recherche plein texte](queries) avec `matches`. La recherche fonctionne sans ; l'attribut la rend rapide là où la base le permet : un index GIN sur PostgreSQL, et un index de mots dans le moteur en mémoire (`MemoryDb::new()` et `MemoryDb::open()`). Les autres bases examinent les lignes une à une.

### `column` (renommage)

Le champ Rust et la colonne peuvent avoir des noms différents. La constante typée garde le nom du *champ* (`Post::BODY`), mais le SQL utilise le nom de la colonne (`"body_markdown"`). C'est aussi la façon d'utiliser un mot-clé Rust comme nom de colonne :

```rust
#[derive(Model)]
#[memoire(table = "items")]
pub struct Item {
    pub id: i64,
    pub r#type: String,                 // colonne "type", constante Item::TYPE
    #[memoire(column = "item_kind")]
    pub kind: String,                   // colonne "item_kind", constante Item::KIND
}
```

### `sql_type`

`sql_type` remplace le type SQL écrit dans `CREATE TABLE`. Il ne change pas la façon dont les valeurs sont converties en Rust : choisissez donc un type compatible avec le champ. Le même texte est utilisé pour toutes les bases de données SQL, préférez donc des types portables comme `VARCHAR(120)`. Redis et MongoDB, qui n'ont pas de `CREATE TABLE`, l'ignorent.

### `default`

`default` est recopié tel quel dans la définition de la colonne : écrivez donc une expression SQL, par exemple `"0"`, `"'draft'"` (avec des guillemets SQL pour du texte) ou `"CURRENT_TIMESTAMP"`.

> **Remarque** Le `insert` de Memoire envoie toujours tous les champs, la valeur par défaut n'est donc jamais utilisée par `insert`. Elle compte pour les lignes insérées en SQL brut ou par d'autres programmes, et pour remplir les lignes existantes quand vous ajoutez une colonne dans une migration. Le moteur en mémoire, Redis et MongoDB l'ignorent.

### `references`, `on_delete` et `on_update`

`references = "table(column)"` (ou `"table.column"`) déclare une clé étrangère. `on_delete` et `on_update` acceptent `cascade`, `restrict`, `no_action`, `set_null` et `set_default` (la casse n'a pas d'importance). Ils ne sont autorisés qu'avec `references`.

| Backend | Clés étrangères appliquées ? |
|---|---|
| PostgreSQL | Oui |
| SQLite | Oui (Memoire active `PRAGMA foreign_keys`) |
| MySQL / MariaDB | Oui |
| Moteur en mémoire, Redis, MongoDB | Non : la contrainte est acceptée mais pas vérifiée, et `cascade` n'a aucun effet |

Créez d'abord la table référencée (`users` avant `posts`). Pour interroger des tables liées, consultez [Jointures et relations](relations).

### `skip`

Un champ ignoré n'est pas une colonne : il n'est ni stocké ni sélectionné, et reçoit `Default::default()` quand une ligne est chargée. Son type doit donc implémenter `Default`. Utilisez-le pour des caches ou des valeurs calculées. Un champ ignoré ne peut porter aucun autre attribut de colonne.

## Types Rust pris en charge

Enveloppez n'importe quel type dans `Option<T>` pour rendre la colonne nullable.

| Type Rust | Feature | PostgreSQL | SQLite | MySQL |
|---|---|---|---|---|
| `bool` | | `BOOLEAN` | `INTEGER` (0 ou 1) | `BOOLEAN` |
| `i8`, `i16`, `u8` | | `SMALLINT` | `INTEGER` | `SMALLINT` |
| `i32`, `u16` | | `INTEGER` | `INTEGER` | `INT` |
| `i64`, `u32` | | `BIGINT` | `INTEGER` | `BIGINT` |
| `f32` | | `REAL` | `REAL` | `FLOAT` |
| `f64` | | `DOUBLE PRECISION` | `REAL` | `DOUBLE` |
| `String` | | `TEXT` | `TEXT` | `TEXT`, ou `VARCHAR(255)` si indexée |
| `Vec<u8>` | | `BYTEA` | `BLOB` | `BLOB`, ou `VARBINARY(255)` si indexée |
| `uuid::Uuid` | `uuid` | `UUID` | `TEXT` | `CHAR(36)` |
| `chrono::NaiveDateTime` | `chrono` | `TIMESTAMP` | `TEXT` | `DATETIME(6)` |
| `chrono::DateTime<Utc>` | `chrono` | `TIMESTAMPTZ` | `TEXT` | `DATETIME(6)` |
| `chrono::NaiveDate` | `chrono` | `DATE` | `TEXT` | `DATE` |
| `chrono::NaiveTime` | `chrono` | `TIME` | `TEXT` | `TIME(6)` |
| `serde_json::Value` | `json` | `JSONB` | `TEXT` | `JSON` |
| `Json<T>` (n'importe quel type `serde`) | `json` | `JSONB` | `TEXT` | `JSON` |

« Indexée » signifie que la colonne est une clé primaire ou une colonne `unique`, `index` ou `references` (MySQL ne sait pas indexer une colonne `TEXT` de longueur illimitée).

Les clés auto-incrémentées ont un DDL particulier : `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` sur PostgreSQL, `INTEGER PRIMARY KEY` sur SQLite et `BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY` sur MySQL (pour une clé `i64`).

Redis et MongoDB n'ont pas de types de colonnes : Redis stocke chaque ligne sous forme d'objet JSON et MongoDB sous forme de document BSON. Leurs pages décrivent comment chaque type Rust est stocké ([Redis](redis), [MongoDB](mongodb)) ; tous les types du tableau ci-dessus sont pris en charge par les six backends.

> **Remarque** `u64`, `usize`, `i128` et `char` ne sont pas pris en charge : il n'existe pas de type SQL portable pour eux. Utilisez `i64` (ou `String` pour `char`). Pourquoi `i64` ? Parce que c'est le plus grand entier que toutes les bases de données prises en charge stockent nativement.

### Colonnes JSON

`Json<T>` stocke n'importe quel type qui implémente `serde::Serialize` et `serde::Deserialize` :

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: String,
    pub notifications: bool,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "profiles")]
pub struct Profile {
    pub id: i64,
    pub settings: Json<Settings>,
    pub extra: Option<serde_json::Value>,
}
```

`Json<T>` se déréférence en `T`, donc `profile.settings.theme` fonctionne directement, et vous en construisez un avec `Json(Settings { ... })`.

### Colonnes enum avec `#[derive(DbEnum)]`

Une enum de variantes sans données devient un type de colonne avec un seul derive, un cast d'attribut dans l'esprit des casts d'enum d'Eloquent. Chaque variante est stockée en texte, en snake_case sauf indication contraire :

```rust
#[derive(DbEnum, Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Draft,                          // "draft"
    #[memoire(rename = "live")]
    Published,                      // "live"
    InReview,                       // "in_review"
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "articles")]
pub struct Article {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub status: Status,
    pub previous: Option<Status>,
}

let live = Article::query().filter(Article::STATUS.eq(Status::Published)).all(&db).await?;
```

- `#[memoire(rename_all = "...")]` sur l'enum choisit la casse : `lowercase`, `UPPERCASE`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `PascalCase` ou `camelCase`.
- `#[memoire(integer)]` stocke plutôt le discriminant dans une colonne `INTEGER` (`enum Level { Low = 1, High = 10 }`).
- Un texte ou un nombre inconnu échoue au décodage avec une erreur qui liste les variantes attendues.

### Types de colonnes personnalisés

Vous pouvez stocker vos propres types, par exemple une enum, en implémentant quatre petits traits de conversion. Ici, une enum `Status` est stockée sous forme de texte :

```rust
use memoire_orm::prelude::*;
use memoire_orm::{ColumnType, FromValue, IntoValue, SqlType, ToValue};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Draft,
    Published,
}

impl SqlType for Status {
    type Base = Status; // le type utilisé dans les comparaisons typées
    const KIND: ColumnType = ColumnType::Text; // stocké en TEXT
}

impl ToValue for Status {
    fn to_value(&self) -> Value {
        let text = match self {
            Status::Draft => "draft",
            Status::Published => "published",
        };
        Value::Text(text.to_owned())
    }
}

impl IntoValue for Status {
    fn into_value(self) -> Value {
        self.to_value()
    }
}

impl FromValue for Status {
    fn from_value(value: Value) -> memoire_orm::Result<Self> {
        match value.as_str() {
            Some("draft") => Ok(Status::Draft),
            Some("published") => Ok(Status::Published),
            _ => Err(Error::conversion(format!("unknown status: {value}"))),
        }
    }
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "articles")]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub status: Status,
}

async fn published(db: &MemoryDb) -> memoire_orm::Result<Vec<Article>> {
    Article::query().filter(Article::STATUS.eq(Status::Published)).all(db).await
}
```

La comparaison `Article::STATUS.eq(Status::Published)` reste vérifiée par le typage : passer une chaîne à cet endroit ne compilerait pas.

## Constantes de colonnes typées

Pour chaque champ stocké, le derive ajoute une constante associée qui porte le nom du champ en `UPPER_SNAKE_CASE` :

| Champ | Constante | Type |
|---|---|---|
| `id: i64` | `User::ID` | `Column<User, i64>` |
| `email: String` | `User::EMAIL` | `Column<User, String>` |
| `age: Option<i32>` | `User::AGE` | `Column<User, Option<i32>>` |
| `r#type: String` | `Item::TYPE` | `Column<Item, String>` |

Une `Column<M, T>` connaît sa table, son nom et son type Rust, ce qui permet au compilateur de vérifier la façon dont vous l'utilisez :

- les comparaisons n'acceptent que des valeurs du type de la colonne (`User::AGE.gt(18)`, et non `User::AGE.gt("18")`) ;
- les helpers de texte comme `contains` n'existent que sur les colonnes de texte ;
- `update_many().set(...)` n'accepte que les colonnes du modèle en cours de modification ;
- `set_null` n'accepte que les colonnes `Option` ;
- une constante mal orthographiée (`User::EMIAL`) provoque une erreur de compilation.

`User::EMAIL.name()` renvoie le nom de la colonne (`"email"`). La liste complète de ce que vous pouvez faire avec les colonnes se trouve dans [Constructeur de requêtes](queries).

## Le schéma de la table et le SQL généré à la compilation

Chaque modèle expose `Model::SCHEMA`, une `&'static TableSchema` construite à la compilation. Elle contient le nom de la table, les colonnes et, pour chaque dialecte SQL, les requêtes toutes prêtes utilisées pour les opérations par clé primaire :

```rust
use memoire_orm::prelude::*;

fn inspect() {
    let schema = User::SCHEMA;
    println!("table: {}", schema.name);
    println!("primary key: {}", schema.pk().name);
    for column in schema.columns {
        println!("- {} (nullable: {})", column.name, column.nullable);
    }

    let pg = schema.sql(Dialect::Postgres);
    println!("{}", pg.insert);
    println!("{}", pg.find);
}
```

```text
table: users
primary key: id
- id (nullable: false)
- email (nullable: false)
- name (nullable: false)
- age (nullable: true)
- active (nullable: false)
INSERT INTO "users" ("email", "name", "age", "active") VALUES ($1, $2, $3, $4) RETURNING "id"
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active" FROM "users" WHERE "users"."id" = $1
```

`TableSchema::sql(dialect)` renvoie un `StaticSql` à sept champs : `columns`, `select`, `find`, `insert`, `update`, `delete` et `upsert`. Comme ces chaînes sont des constantes intégrées à votre binaire, `User::find`, `user.insert`, `user.update`, `user.delete` et `user.upsert` ne construisent jamais de SQL à l'exécution.

Pour voir les requêtes `CREATE TABLE` d'un dialecte, utilisez `memoire_orm::sql::create_table` :

```rust
use memoire_orm::prelude::*;

fn print_ddl() {
    for statement in memoire_orm::sql::create_table(User::SCHEMA, Dialect::Postgres, false) {
        println!("{statement};");
    }
}
```

Le dernier argument ajoute `IF NOT EXISTS` lorsqu'il vaut `true`.

## Lignes de résultat personnalisées avec `#[derive(FromRow)]`

Il arrive qu'une requête renvoie autre chose qu'un modèle complet : un rapport, quelques colonnes, le résultat d'une requête SQL brute. `#[derive(FromRow)]` transforme n'importe quelle struct à champs nommés en type de ligne :

```rust
use memoire_orm::prelude::*;

#[derive(FromRow, Debug)]
pub struct AuthorStats {
    pub author_id: i64,
    pub posts: i64,
    pub total_views: Option<i64>,
}

async fn stats(db: &MemoryDb) -> memoire_orm::Result<Vec<AuthorStats>> {
    Post::query()
        .group_by(Post::AUTHOR_ID)
        .select_exprs([Post::AUTHOR_ID.expr(), count_all(), Post::VIEWS.sum()])
        .all(db)
        .await
}
```

Les colonnes sont décodées **par position**, dans l'ordre de déclaration des champs : la première valeur sélectionnée va dans le premier champ, et ainsi de suite. Les noms ne sont pas comparés, ce qui garde le décodage rapide mais implique que l'ordre corresponde à votre `SELECT`. `#[memoire(skip)]` fonctionne ici aussi.

`FromRow` est aussi implémenté pour les scalaires (`i64`, `String`, ...), les `Option` de scalaires, `Value` et les tuples jusqu'à 8 éléments : pour de petits résultats, un tuple suffit donc souvent, par exemple `Vec<(i64, String)>`.

## Limites

| Non pris en charge | Que faire à la place |
|---|---|
| Clés primaires composites (deux champs `primary_key`) | Ajoutez une clé de substitution `id` et `#[memoire(unique(a, b))]` sur la struct |
| Structs génériques, tuple structs, enums comme modèles | Utilisez une struct simple à champs nommés |
| Champs `u64`, `usize`, `i128` | Utilisez `i64` |
| Comparaison automatique des schémas | Écrivez des [migrations](migrations) |

## Étapes suivantes

- [Constructeur de requêtes](queries) : filtres, tris, pagination et agrégats.
- [Créer, lire, modifier, supprimer](crud) : toutes les opérations sur un modèle.
- [Jointures et relations](relations) : combinez plusieurs modèles.
