# Installation

Cette page explique comment ajouter Memoire ORM à un projet Rust, quelles features Cargo activer pour votre base de données et de quel runtime async vous avez besoin.

## Ajouter la crate

La crate s'appelle `memoire-orm` sur crates.io, et vous l'importez en Rust sous le nom `memoire_orm`.

Le plus rapide est d'utiliser `cargo add`. Choisissez la ligne qui correspond à la base de données que vous voulez utiliser :

```bash
# Moteur en mémoire uniquement (la feature par défaut)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire

# SQLite
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features sqlite

# PostgreSQL (nécessite le runtime Tokio)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres
cargo add tokio --features macros,rt-multi-thread

# PostgreSQL via TLS (bases hébergées comme Heroku Postgres)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features postgres-tls

# MySQL / MariaDB (ajoutez "mysql-tls" pour TLS)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mysql

# Redis (ajoutez "redis-tls" pour les URL rediss://)
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features redis

# MongoDB
cargo add memoire-orm --git https://github.com/maxlestage/Memoire --features mongodb
```

Tous les backends serveur (PostgreSQL, MySQL/MariaDB, Redis, MongoDB) ont aussi besoin de Tokio : `cargo add tokio --features macros,rt-multi-thread`.

Vous pouvez aussi modifier `Cargo.toml` à la main :

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

> **Remarque** Tokio n'est *obligatoire* que pour les backends serveur. Les exemples de cette documentation utilisent `#[tokio::main]` parce que c'est la configuration la plus courante, mais les backends mémoire et SQLite fonctionnent sans (voir « Runtime async » plus bas).

## Features

| Feature | Activée par défaut | Ce qu'elle apporte |
|---|---|---|
| `memory` | oui | `MemoryDb`, le moteur en mémoire en Rust pur (fonctionne aussi sur `wasm32`) |
| `sqlite` | non | `SqliteDb`, une base SQLite embarquée (SQLite est compilé avec la crate) |
| `postgres` | non | `PgPool`, un pool de connexions PostgreSQL (nécessite Tokio) |
| `postgres-tls` | non | Connexions PostgreSQL chiffrées avec `rustls` (`sslmode=require`, `verify-full`...), indispensables pour Heroku Postgres et la plupart des bases hébergées. Implique `postgres` |
| `mysql` | non | `MySqlPool`, un pool de connexions MySQL et MariaDB (nécessite Tokio) |
| `mysql-tls` | non | Connexions MySQL chiffrées avec `rustls` (`?require_ssl=true`). Implique `mysql` |
| `redis` | non | `RedisDb`, des tables stockées dans Redis (nécessite Tokio ; inclut `memory`) |
| `redis-tls` | non | URL `rediss://` avec `rustls` ; ajoutez `#insecure` pour les certificats auto-signés. Implique `redis` |
| `mongodb` | non | `MongoDb`, des collections MongoDB via le driver officiel (nécessite Tokio ; inclut `memory`) |
| `encryption` | non | Fichiers `MemoryDb` chiffrés (AES-256-GCM, clé dérivée d'un mot de passe avec Argon2id). Implique `memory` |
| `chrono` | non | Colonnes de date et d'heure : `NaiveDateTime`, `DateTime<Utc>`, `NaiveDate`, `NaiveTime` |
| `uuid` | non | Colonnes `uuid::Uuid` |
| `json` | non | Colonnes JSON : `serde_json::Value` et `Json<T>` pour n'importe quel type `serde` |
| `full` | non | Tout ce qui précède |

Chaque backend est décrit sur sa propre page : [Moteur en mémoire](memory), [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis) et [MongoDB](mongodb).

Les features s'additionnent, vous pouvez donc les combiner librement :

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["postgres", "chrono", "uuid", "json"] }
```

Si vous n'avez pas du tout besoin du moteur en mémoire, désactivez les features par défaut :

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", default-features = false, features = ["sqlite"] }
```

> **Astuce** Gardez `memory` activée même si la production utilise une autre base de données : vos tests unitaires seront rapides et sans aucune configuration. Vous pouvez ne l'activer que pour les tests grâce à `[dev-dependencies]`.

### Crates supplémentaires pour les types de colonnes

Memoire ne réexporte pas `chrono`, `uuid` ni `serde_json`. Pour pouvoir *nommer* ces types dans vos structs, ajoutez aussi ces crates à votre propre `Cargo.toml` :

```toml
[dependencies]
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite", "chrono", "uuid", "json"] }
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }         # "v4" pour Uuid::new_v4()
serde = { version = "1", features = ["derive"] }    # pour Json<T>
serde_json = "1"
```

## Runtime async

Chaque appel à la base de données dans Memoire est une `async fn` : vous devez donc l'attendre avec `.await`. Le runtime nécessaire dépend du backend :

| Backend | Runtime nécessaire |
|---|---|
| `MemoryDb` | N'importe quel exécuteur (Tokio, async-std, smol...) ou aucun avec `memoire_orm::block_on` |
| `SqliteDb` | N'importe quel exécuteur ou aucun avec `memoire_orm::block_on` |
| `PgPool`, `MySqlPool`, `RedisDb`, `MongoDb` | Tokio (leurs E/S réseau et leurs pools de connexions tournent sur Tokio) |

Les backends mémoire et SQLite n'attendent jamais le réseau : leurs futures se terminent dès qu'elles sont pollées. C'est pourquoi Memoire fournit un petit utilitaire, `memoire_orm::block_on`, qui exécute une future sur le thread courant. C'est pratique dans les programmes synchrones, les outils en ligne de commande, les tests et WebAssembly :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
struct Note {
    id: i64,
    text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;
        Note { id: 0, text: "hello".into() }.insert(&db).await?;
        println!("{} note(s)", Note::count(&db).await?);
        Ok(())
    })
}
```

> **Attention** N'utilisez pas `block_on` avec `PgPool`, `MySqlPool`, `RedisDb` ou `MongoDb` : ils ont besoin d'un runtime Tokio en cours d'exécution. Utilisez `#[tokio::main]` (ou `#[tokio::test]` dans les tests).

## WebAssembly

Le backend `memory` compile vers `wasm32-unknown-unknown` sans aucune configuration supplémentaire :

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

Dans le navigateur, vous pouvez piloter les futures avec `memoire_orm::block_on` ou avec `wasm-bindgen-futures`. Les autres backends (SQLite, PostgreSQL, MySQL, Redis et MongoDB) ne sont pas disponibles sur `wasm32`. Pour plus de détails, consultez [Moteur en mémoire](memory).

## Vérifier que tout fonctionne

Créez un nouveau projet et lancez un tout petit programme :

```bash
cargo new hello-memoire
cd hello-memoire
cargo add memoire-orm --git https://github.com/maxlestage/Memoire
cargo add tokio --features macros,rt-multi-thread
```

Remplacez `src/main.rs` par :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug)]
#[memoire(table = "greetings")]
struct Greeting {
    id: i64,
    text: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Greeting>().await?;

    let mut hello = Greeting { id: 0, text: "Hello, Memoire!".into() };
    hello.insert(&db).await?;

    let found = Greeting::get(&db, hello.id).await?;
    println!("#{} says: {}", found.id, found.text);
    Ok(())
}
```

`cargo run` devrait alors afficher `#1 says: Hello, Memoire!`.

## Prérequis

| Prérequis | Valeur |
|---|---|
| Version minimale de Rust (MSRV) | 1.88 |
| Outils nécessaires à la compilation | Un compilateur C lorsque la feature `sqlite` est activée (SQLite est compilé depuis les sources) |
| Dépendances à l'exécution | Aucune pour la mémoire et SQLite ; un serveur accessible pour PostgreSQL, MySQL/MariaDB, Redis ou MongoDB |
| Licence | Propriétaire (© Maxime Nathan Lestage, tous droits réservés) : utilisation libre comme dépendance, voir [LICENSE](https://maxlestage.github.io/Memoire/LICENSE.txt) |

## Étapes suivantes

- [Démarrage rapide](quick-start) : construisez une petite application à partir de zéro.
- [Modèles](models) : décrivez vos tables.
- Configuration des backends : [SQLite](sqlite), [PostgreSQL](postgres), [MySQL & MariaDB](mysql), [Redis](redis), [MongoDB](mongodb).
