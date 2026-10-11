# PostgreSQL

`PgPool` connecte Memoire à un serveur PostgreSQL au moyen d'un pool de connexions réutilisables. C'est le backend de prédilection pour les services web : plusieurs instances de l'application peuvent partager une même base de données, de nombreuses requêtes peuvent s'exécuter en même temps, et les offres hébergées (Heroku Postgres, Amazon RDS, Supabase, Neon...) sont faciles à trouver.

## Activer le backend

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread
```

Pour les bases de données hébergées, qui exigent des connexions chiffrées, utilisez plutôt la feature `postgres-tls` (elle inclut `postgres`) :

```bash
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls
```

Le client est écrit en Rust pur : `tokio-postgres` pour le protocole, `deadpool` pour le pool et, avec `postgres-tls`, `rustls` pour le chiffrement. Aucune bibliothèque C ni OpenSSL n'est nécessaire.

> **Remarque** `PgPool` a besoin du runtime **Tokio** : utilisez `#[tokio::main]` dans votre programme et `#[tokio::test]` dans vos tests. `memoire_orm::block_on` ne fonctionne pas avec PostgreSQL.

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
    let db = PgPool::connect("postgres://app:secret@localhost:5432/shop").await?;
    db.create_table_if_not_exists::<Product>().await?;

    let mut tea = Product { id: 0, sku: "TEA-01".into(), name: "Green tea".into(), price_cents: 450 };
    tea.insert(&db).await?;
    println!("product #{} created", tea.id);
    Ok(())
}
```

La chaîne de connexion peut être une URL, `postgres://user:password@host:port/database?option=value`, ou au format `key=value` : `host=localhost user=app password=secret dbname=shop`.

| Constructeur | Taille du pool | Remarques |
|---|---|---|
| `PgPool::connect(url)` | Jusqu'à 4 × le nombre de cœurs CPU | Ouvre immédiatement une connexion : une URL ou un mot de passe erroné échoue donc tout de suite |
| `PgPool::connect_with(url, max_size)` | Jusqu'à `max_size` | Idem, avec votre propre limite |
| `PgPool::connect_with_rustls(url, max_size, config)` | Jusqu'à `max_size` | TLS avec votre propre configuration `rustls` (feature `postgres-tls`) |
| `PgPool::from_pool(pool)` | Ce que vous avez configuré | Enveloppe un `deadpool_postgres::Pool` que vous avez construit vous-même |

Cloner un `PgPool` ne coûte presque rien : les clones partagent le même pool. Créez-le une seule fois au démarrage et faites circuler des clones (ou des références).

## Choisir la taille du pool

Chaque connexion est un processus sur le serveur de base de données : plus n'est donc pas toujours mieux.

- La valeur par défaut (4 × le nombre de cœurs CPU de la machine de l'application) convient à une instance unique de l'application qui dialogue avec son propre serveur.
- Les offres hébergées limitent le nombre de connexions, parfois à quelques dizaines seulement. Répartissez cette limite entre vos instances de l'application et gardez quelques connexions pour les outils de maintenance : `PgPool::connect_with(&url, 10)`.
- Quand toutes les connexions sont occupées, les nouvelles requêtes attendent qu'une connexion se libère.

Les connexions sont ouvertes à la demande, jusqu'au maximum, puis réutilisées.

## Connexions chiffrées (TLS)

Avec la feature `postgres-tls`, `connect` et `connect_with` respectent le paramètre `sslmode` de la chaîne de connexion, comme le client officiel `psql` :

| `sslmode` | Comportement |
|---|---|
| `disable` | En clair, jamais chiffré |
| `prefer` (par défaut) | Utilise TLS si le serveur le prend en charge, sinon en clair |
| `require` | Toujours chiffré ; le certificat du serveur n'est **pas** vérifié |
| `verify-ca`, `verify-full` | Toujours chiffré, et le certificat du serveur est vérifié à partir des certificats racines de Mozilla |

```rust
async fn connect_securely() -> memoire_orm::Result<PgPool> {
    PgPool::connect("postgres://app:secret@db.example.com/shop?sslmode=verify-full").await
}
```

Sans la feature `postgres-tls`, les connexions sont en clair, et une chaîne de connexion avec `sslmode=require`, `verify-ca` ou `verify-full` échoue avec `Error::Unsupported`, en vous indiquant d'activer la feature.

> **Astuce** Utilisez `verify-full` lorsque le certificat de votre fournisseur est signé par une autorité publique : cela vous protège contre les serveurs usurpateurs. Beaucoup de bases de données hébergées (dont Heroku Postgres) utilisent des certificats qui ne sont pas signés par une autorité publique ; utilisez `require` pour celles-ci, ce qui chiffre tout de même tout le trafic.

### Certificats personnalisés

Si votre serveur utilise un certificat signé par votre propre autorité, construisez une `rustls::ClientConfig` qui lui fait confiance et passez-la à `connect_with_rustls`. Il vous faut `rustls` (0.23) dans vos dépendances :

```toml
[dependencies]
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
```

```rust
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

async fn connect_with_my_ca(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from_pem_file("my-ca.pem")?)?;

    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(PgPool::connect_with_rustls(url, 10, config).await?)
}
```

Les certificats client fonctionnent de la même façon, avec `with_client_auth_cert` au lieu de `with_no_client_auth`.

### Heroku Postgres

Heroku place la chaîne de connexion de votre base de données dans la variable d'environnement `DATABASE_URL`, et Heroku Postgres n'accepte que des connexions chiffrées. Avec la feature `postgres-tls`, la connexion tient en une ligne :

```rust
async fn heroku() -> Result<PgPool, Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")?;
    // `prefer` (la valeur par défaut) utilise déjà TLS ; `sslmode=require` le rend obligatoire.
    let url = if url.contains('?') { url } else { format!("{url}?sslmode=require") };
    Ok(PgPool::connect_with(&url, 10).await?)
}
```

Consultez [Déployer sur Heroku depuis son téléphone](deploy-heroku-mobile) pour un déploiement complet.

### Construire votre propre pool

Pour un contrôle total sur le pool (timeouts, recyclage, autre pile TLS), construisez un `deadpool_postgres::Pool` et enveloppez-le avec `from_pool`. Il vous faut alors `deadpool-postgres` (0.14) et `tokio-postgres` (0.7) dans vos dépendances :

```rust
use std::time::Duration;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};

fn custom_pool(url: &str) -> Result<PgPool, Box<dyn std::error::Error>> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager = Manager::from_config(
        config,
        tokio_postgres::NoTls,
        ManagerConfig { recycling_method: RecyclingMethod::Fast },
    );
    let pool = Pool::builder(manager)
        .max_size(8)
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(Duration::from_secs(5))) // échouer plutôt que d'attendre indéfiniment
        .build()?;
    Ok(PgPool::from_pool(pool))
}
```

Contrairement à `connect`, `from_pool` n'ouvre pas de connexion immédiatement : les erreurs apparaissent à la première requête.

## Requêtes préparées

Chaque instruction exécutée par Memoire est préparée une fois par connexion et conservée dans un cache : PostgreSQL ne l'analyse et ne la planifie donc qu'une seule fois. Les opérations par clé primaire réutilisent le SQL généré à la compilation, et les requêtes du builder qui ont la même forme produisent le même texte SQL : elles partagent donc une entrée du cache. Pour protéger les processus qui tournent longtemps, le cache d'une connexion est vidé lorsqu'il dépasse 1 024 instructions.

## Types

### Comment les colonnes sont créées

| Type Rust | Type PostgreSQL |
|---|---|
| `bool` | `BOOLEAN` |
| `i8`, `i16`, `u8` | `SMALLINT` |
| `i32`, `u16` | `INTEGER` |
| `i64`, `u32` | `BIGINT` |
| `f32` | `REAL` |
| `f64` | `DOUBLE PRECISION` |
| `String` | `TEXT` |
| `Vec<u8>` | `BYTEA` |
| `uuid::Uuid` | `UUID` |
| `chrono::NaiveDateTime` | `TIMESTAMP` |
| `chrono::DateTime<Utc>` | `TIMESTAMPTZ` |
| `chrono::NaiveDate` | `DATE` |
| `chrono::NaiveTime` | `TIME` |
| `serde_json::Value`, `Json<T>` | `JSONB` |
| Clé `auto_increment` | `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` (pour `i64`) |

### Comment les valeurs sont lues

| Type PostgreSQL | Lu comme |
|---|---|
| `BOOLEAN` | `bool` |
| `SMALLINT`, `INTEGER`, `BIGINT` | N'importe quel type entier dans lequel la valeur tient |
| `REAL`, `DOUBLE PRECISION` | `f32`, `f64` |
| `NUMERIC` | `String` (exact) ou `f64` |
| `TEXT`, `VARCHAR`, `CHAR`, `NAME`, `citext`, types enum | `String` |
| `BYTEA` | `Vec<u8>` |
| `UUID` | `uuid::Uuid` avec la feature `uuid`, sinon `String` |
| `TIMESTAMP`, `TIMESTAMPTZ`, `DATE`, `TIME` | Types `chrono` (nécessite la feature `chrono`) |
| `JSON`, `JSONB` | `serde_json::Value` ou `Json<T>` avec la feature `json`, sinon `String` |

`NUMERIC` (décimaux exacts, courants pour les montants) n'a pas de type Rust natif : Memoire le lit donc sous forme de texte, sans aucun arrondi. Décodez-le dans une `String` pour garder tous les chiffres, ou dans un `f64` pour faire des calculs. Écrire une `String` ou un `f64` dans une colonne `NUMERIC` fonctionne aussi. Les autres types (`INTERVAL`, tableaux, types géométriques...) ne sont pas pris en charge directement : convertissez-les en texte en [SQL brut](raw-sql), par exemple `duration::text`.

> **Astuce** `TIMESTAMPTZ` stocke des microsecondes alors que `chrono` conserve des nanosecondes. Si vous comparez une valeur avec ce que vous relisez, tronquez-la d'abord : `Utc::now().trunc_subsecs(6)` (depuis `chrono::SubsecRound`).

## Erreurs

Les codes d'erreur PostgreSQL sont traduits dans les variantes communes de Memoire :

| Erreur PostgreSQL | Erreur Memoire |
|---|---|
| `unique_violation` (23505) | `Error::UniqueViolation { table, column }` |
| `not_null_violation`, `foreign_key_violation`, `check_violation` | `Error::Constraint(message)` |
| `undefined_table` (42P01) | `Error::NoSuchTable(name)` |
| `duplicate_table` (42P07) | `Error::TableExists(name)` |
| `serialization_failure` (40001), `deadlock_detected` (40P01) | `Error::Conflict(table)` : réessayez la transaction |
| Le pool ne parvient pas à se connecter (mauvais hôte, mot de passe, handshake TLS...) | `Error::Pool(message)` |
| Tout le reste | `Error::Database(...)` avec l'erreur d'origine |

## Bon à savoir

- **Les transactions** monopolisent une connexion du pool jusqu'à leur fin ; une transaction droppée est annulée en arrière-plan. Voir [Transactions](transactions).
- **`LIKE` est sensible à la casse**, et `ilike`/`icontains` utilisent l'`ILIKE` natif de PostgreSQL.
- **Tri des NULL :** les NULL arrivent en dernier dans l'ordre croissant et en premier dans l'ordre décroissant.
- **Le SQL brut** utilise les placeholders `$1`, `$2`, ... Voir [SQL brut](raw-sql).
- **Faire un upsert avec des ids explicites** dans une table `auto_increment` ne fait pas avancer la séquence d'identité de PostgreSQL : un `insert` ultérieur peut donc entrer en collision avec eux. Utilisez `insert` pour les nouvelles lignes des modèles auto-incrémentés.
- **Accès direct :** `db.pool()` renvoie le `deadpool_postgres::Pool`, pour la supervision (`db.pool().status()`) ou pour des fonctionnalités de `tokio-postgres` comme `LISTEN`/`NOTIFY`.

## Étapes suivantes

- [Déployer sur Heroku depuis son téléphone](deploy-heroku-mobile) : faites tourner une application Memoire avec Heroku Postgres.
- [Transactions](transactions) : niveaux d'isolation et nouvelles tentatives sur PostgreSQL.
- [SQL brut](raw-sql) : requêtes propres à PostgreSQL.
