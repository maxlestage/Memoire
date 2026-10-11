# MySQL & MariaDB

`MySqlPool` connecte Memoire à MySQL ou MariaDB au moyen d'un pool de connexions réutilisables. Vos modèles et vos requêtes fonctionnent exactement comme sur les autres backends, et Memoire configure les tables pour que les comparaisons, les tris et `LIKE` se comportent eux aussi de la même façon.

## Activer le backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mysql
cargo add tokio --features macros,rt-multi-thread
```

Pour les connexions chiffrées (la plupart des bases hébergées), utilisez plutôt la feature `mysql-tls` (elle inclut `mysql`) :

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mysql-tls
```

Le client est `mysql_async`, une implémentation en Rust pur du protocole MySQL ; TLS utilise `rustls`. Aucune bibliothèque C ni OpenSSL n'est nécessaire.

| Prérequis | Valeur |
|---|---|
| Runtime async | **Tokio** (`#[tokio::main]`, `#[tokio::test]`) |
| MySQL | 8.0 et plus récent (testé sur 8.4) |
| MariaDB | 10.6 et plus récent (testé sur 10.11 et 11) |

> **Astuce** Pour essayer en local, démarrez un serveur avec Docker : `docker run -d -p 3306:3306 -e MARIADB_ROOT_PASSWORD=root -e MARIADB_DATABASE=app -e MARIADB_USER=app -e MARIADB_PASSWORD=secret mariadb:11` (ou l'image `mysql:8.4`, avec `MYSQL_` au lieu de `MARIADB_` dans le nom des variables).

## Se connecter

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "products")]
pub struct Product {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MySqlPool::connect("mysql://app:secret@localhost:3306/app").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);

    db.close().await // facultatif : ferme proprement les connexions
}
```

| Méthode | Ce qu'elle fait |
|---|---|
| `MySqlPool::connect(url)` | Se connecte avec une URL `mysql://user:password@host:port/database`. Une connexion est ouverte immédiatement : une adresse ou un mot de passe erroné échoue donc tout de suite |
| `MySqlPool::connect_with(builder)` | Se connecte avec un `mysql_async::OptsBuilder`, pour les options que l'URL ne permet pas d'exprimer (réglages TLS, options de socket...) |
| `db.pool()` | Le `mysql_async::Pool` sous-jacent (porte de sortie) |
| `db.close().await` | Ferme toutes les connexions du pool (pour un arrêt propre) |

Cloner un `MySqlPool` ne coûte presque rien : les clones partagent le même pool. Les connexions sont ouvertes à la demande, puis réutilisées. Par défaut, le pool garde entre 10 et 100 connexions (les valeurs par défaut de `mysql_async`) ; définissez d'autres bornes avec les paramètres d'URL `pool_min` et `pool_max`, par exemple `mysql://app:secret@localhost:3306/app?pool_min=1&pool_max=5`, ou avec `OptsBuilder::pool_opts`. `pool_min` ne doit pas dépasser `pool_max` : un `pool_max` inférieur à 10 nécessite donc aussi un `pool_min`. Sur une petite offre hébergée avec une limite de connexions basse, définissez les deux.

## Connexions chiffrées (TLS)

Avec la feature `mysql-tls`, ajoutez des paramètres TLS à l'URL :

| Paramètre d'URL | Effet |
|---|---|
| `require_ssl=true` | Chiffre la connexion et vérifie le certificat du serveur et le nom d'hôte |
| `verify_ca=false` | Accepte un certificat qui n'est pas signé par une autorité de confiance (auto-signé) |
| `verify_identity=false` | Ne vérifie pas que le certificat correspond au nom d'hôte |

```rust
async fn connect_securely() -> memoire_orm::Result<MySqlPool> {
    MySqlPool::connect("mysql://app:secret@db.example.com:3306/app?require_ssl=true").await
}
```

Pour un contrôle total, construisez vous-même les options avec `mysql_async` (0.37) dans vos dépendances :

```rust
use mysql_async::{Opts, OptsBuilder, SslOpts};

async fn connect_with_options(url: &str) -> Result<MySqlPool, Box<dyn std::error::Error>> {
    let builder = OptsBuilder::from_opts(Opts::from_url(url)?).ssl_opts(SslOpts::default());
    Ok(MySqlPool::connect_with(builder).await?)
}
```

## Ce que Memoire configure pour vous

| Réglage | Pourquoi |
|---|---|
| Tables créées avec `DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin` | Unicode complet (emoji compris), et des comparaisons sensibles à la casse et ordonnées octet par octet, comme sur les autres backends |
| `CLIENT_FOUND_ROWS` | `update` compte les lignes qui *correspondent*, même si aucune valeur n'a changé : enregistrer une ligne inchangée n'est donc pas signalé comme `Error::NotFound` |
| 256 requêtes préparées en cache par connexion | Chaque instruction distincte n'est analysée qu'une fois par connexion |

À cause de la collation binaire, `User::NAME.eq("alice")` ne trouve pas `"Alice"`, et `like`, `contains`, `starts_with` et `ends_with` sont sensibles à la casse, exactement comme sur PostgreSQL. Utilisez `ilike` ou `icontains` pour ignorer la casse (ils utilisent `LOWER()`, qui gère les lettres accentuées).

> **Remarque** Cela s'applique aux tables créées par Memoire (`create_table`, migrations). Les tables que vous avez créées vous-même gardent leur propre collation, souvent insensible à la casse dans MySQL.

## Types

| Type Rust | Type MySQL |
|---|---|
| `bool` | `BOOLEAN` (stocké en `TINYINT(1)` : 0 ou 1) |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INT` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `FLOAT` |
| `f64` | `DOUBLE` |
| `String` | `TEXT`, ou `VARCHAR(255)` pour les clés primaires et les colonnes `unique`, `index` ou de clé étrangère |
| `Vec<u8>` | `BLOB`, ou `VARBINARY(255)` pour les colonnes indexées |
| `uuid::Uuid` | `CHAR(36)` |
| `chrono::NaiveDateTime` | `DATETIME(6)` |
| `chrono::DateTime<Utc>` | `DATETIME(6)`, qui contient l'heure UTC |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME(6)` |
| `serde_json::Value`, `Json<T>` | `JSON` |
| Clé `auto_increment` | `BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY` (pour `i64`) |

Bon à savoir :

- **Les colonnes de texte indexées contiennent au plus 255 caractères**, car MySQL ne sait pas indexer un `TEXT` de longueur illimitée. Utilisez `#[memoire(sql_type = "VARCHAR(500)")]` pour des valeurs uniques plus longues.
- **Les horodatages conservent les microsecondes.** Si vous comparez une valeur avec ce que vous relisez, tronquez-la d'abord : `Utc::now().trunc_subsecs(6)` (depuis `chrono::SubsecRound`).
- **Les valeurs `DECIMAL`** (de vos propres tables ou du SQL brut) sont lues sous forme de texte exact : décodez-les en `String`, en `f64`, ou en `i64` pour les nombres entiers.
- **Les ids générés** proviennent d'`AUTO_INCREMENT` et sont relus avec `LAST_INSERT_ID()`.

## Transactions

`db.begin()` prend une connexion dans le pool pour la transaction. Les autres requêtes continuent de s'exécuter sur d'autres connexions et ne voient que les données validées. Le niveau d'isolation est celui par défaut du serveur (`REPEATABLE READ` pour InnoDB).

- Une transaction droppée est annulée quand sa connexion retourne dans le pool.
- Les deadlocks et les dépassements du délai d'attente de verrou sont signalés comme `Error::Conflict` : réessayez toute la transaction, comme le montre [Transactions](transactions).

> **Attention** MySQL et MariaDB valident automatiquement avant et après chaque modification de schéma (`CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`...). Une modification de schéma dans une transaction ne peut pas être annulée, et elle valide aussi les modifications faites avant elle. Gardez les modifications de schéma en dehors des transactions de votre application.

C'est important pour les [migrations](migrations) : si une migration échoue après avoir créé une table, cette table reste en place. Limitez chaque migration à une seule modification de schéma, pour qu'un échec soit facile à nettoyer à la main avant de relancer la migration.

## Upsert

`upsert` fait correspondre les lignes uniquement sur la clé primaire, comme sur tous les autres backends. Si la ligne que vous passez à `upsert` a une nouvelle clé primaire mais une valeur `unique` (un e-mail, par exemple) qui appartient déjà à une autre ligne, l'opération échoue avec `Error::UniqueViolation` et l'autre ligne reste intacte.

Le `INSERT ... ON DUPLICATE KEY UPDATE` propre à MySQL se déclenche quand **n'importe quelle** clé unique entre en collision : il modifierait donc cette autre ligne à la place. Memoire ne l'utilise donc que pour les tables sans autre colonne `unique`, où c'est une seule instruction atomique. Pour les autres, `upsert` exécute un `INSERT` puis, quand cette insertion entre en collision sur la clé primaire, un `UPDATE` par clé. Les deux étapes sont sûres lorsque plusieurs programmes font un upsert en même temps, et aussi dans une transaction : une instruction qui échoue n'interrompt pas une transaction MySQL.

Faire un upsert de lignes avec des ids explicites dans une table `auto_increment` ne pose pas de problème sur MySQL : le compteur passe au-delà de l'id le plus élevé.

## Erreurs

| Erreur MySQL | Erreur Memoire |
|---|---|
| Duplicate entry (1062) | `Error::UniqueViolation { table, column }` (`column` est le nom de la clé, que Memoire nomme d'après la colonne) |
| Table doesn't exist (1146), unknown table (1051) | `Error::NoSuchTable(name)` |
| Table already exists (1050) | `Error::TableExists(name)` |
| Violation de `NOT NULL`, de clé étrangère ou de `CHECK` | `Error::Constraint(message)` |
| Deadlock (1213), lock wait timeout (1205) | `Error::Conflict(table)` : réessayez la transaction |
| Connexion impossible | `Error::Pool(message)` ou `Error::Database(...)` |
| Tout le reste | `Error::Database(...)` avec l'erreur d'origine |

## Bon à savoir

- **Tri des NULL :** comme SQLite, MySQL place les NULL en premier dans l'ordre croissant. `nulls_first()`/`nulls_last()` sont émulés avec une clé de tri `IS NULL` : ils fonctionnent donc partout.
- **Les clés étrangères** (`references`, `on_delete = "cascade"`) sont appliquées (InnoDB).
- **Le SQL brut** utilise les placeholders `?` ; les scripts de migration peuvent contenir plusieurs instructions. Voir [SQL brut](raw-sql).
- **Accès direct :** `db.pool()` renvoie le `mysql_async::Pool` :

```rust
use mysql_async::prelude::Queryable;

async fn server_version(db: &MySqlPool) -> Result<Option<String>, mysql_async::Error> {
    let mut conn = db.pool().get_conn().await?;
    conn.query_first("SELECT VERSION()").await
}
```

## Générer du SQL MySQL pour un autre driver

Memoire peut aussi se contenter d'*écrire* du SQL MySQL pour vous, sans `MySqlPool` : par exemple pour inspecter une requête, préparer un fichier de schéma ou exécuter des instructions avec une autre bibliothèque cliente MySQL. `Dialect::MySql` est toujours disponible, quelles que soient les features activées.

```rust
fn mysql_sql() {
    // Une requête, avec ses paramètres dans l'ordre des placeholders.
    let query = Product::query().filter(Product::PRICE_CENTS.lt(1_000)).order_by(Product::NAME.asc()).limit(20);
    let (sql, params) = query.to_sql_with_params(Dialect::MySql);
    println!("{sql}\n{params:?}");

    // Les instructions utilisées par find / insert / update / delete / upsert.
    println!("{}", Product::SCHEMA.sql(Dialect::MySql).insert);

    // CREATE TABLE.
    for statement in memoire_orm::sql::create_table(Product::SCHEMA, Dialect::MySql, false) {
        println!("{statement};");
    }
}
```

```text
SELECT `products`.`id`, `products`.`sku`, `products`.`name`, `products`.`price_cents` FROM `products` WHERE `products`.`price_cents` < ? ORDER BY `products`.`name` ASC LIMIT 20
[I64(1000)]
INSERT INTO `products` (`sku`, `name`, `price_cents`) VALUES (?, ?, ?)
CREATE TABLE `products` (
    `id` BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    `sku` VARCHAR(255) NOT NULL UNIQUE,
    `name` TEXT NOT NULL,
    `price_cents` BIGINT NOT NULL
) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
```

`memoire_orm::sql::render` renvoie n'importe quelle instruction avec ses paramètres (par exemple l'`INSERT` d'un modèle), et les lignes renvoyées par un autre driver peuvent être décodées en modèles : placez leurs valeurs dans un `memoire_orm::ValueRow` puis appelez `FromRow::from_row`.

## Étapes suivantes

- [Transactions](transactions) : nouvelles tentatives et comportement selon le backend.
- [Migrations](migrations) : les modifications de schéma sur MySQL.
- [SQL brut](raw-sql) : requêtes propres à MySQL.
