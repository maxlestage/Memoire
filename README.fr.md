<div align="center">

# Memoire ORM

**L'ORM Rust natif, ultra-rapide et typé.**

[![CI](https://github.com/maxlestage/Memoire/actions/workflows/ci.yml/badge.svg)](https://github.com/maxlestage/Memoire/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![Licence : MIT OU Apache-2.0](https://img.shields.io/badge/licence-MIT%20OU%20Apache--2.0-blue.svg)](#licence)

[English](README.md) · **Français** · [Español](README.es.md)

[![Déployer sur Heroku](https://www.herokucdn.com/deploy/button.svg)](https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire)

</div>

---

## Pourquoi Memoire ?

| | |
|---|---|
| ⚡ **SQL généré à la compilation** | `#[derive(Model)]` produit toutes les requêtes par clé primaire (`find`, `insert`, `update`, `delete`, `upsert`) pour PostgreSQL, SQLite et MySQL pendant la compilation. |
| 🛡️ **Requêtes typées** | `User::AGE.gt(18)` compile, `User::AGE.gt("18")` non. Combinez les conditions avec `&`, `\|` et `!`. |
| 🧬 **Mapping sans surcoût** | Les lignes sont décodées par position en une seule passe ; les valeurs sont déplacées, jamais copiées. |
| 🗄️ **SQL et NoSQL, une seule API** | PostgreSQL, MySQL/MariaDB, SQLite, Redis, MongoDB et un moteur en mémoire 100 % Rust (qui tourne même en WebAssembly !). |
| 🔋 **Tout est inclus** | Transactions, migrations, jointures, agrégats, `GROUP BY`, pagination, upserts, erreurs unifiées. |
| 🦀 **Sûr** | `#![forbid(unsafe_code)]`, la même suite de tests passe sur les six moteurs. |

## Aperçu

```toml
[dependencies]
memoire-orm = { version = "0.1", features = ["sqlite"] }   # ou "postgres", "mysql", "redis", "mongodb", "full"
tokio = { version = "1", features = ["full"] }
```

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
    let db = SqliteDb::open("app.db")?;          // ou MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
    alice.insert(&db).await?;                    // alice.id est maintenant renseigné

    let majeurs = User::query()
        .filter(User::AGE.ge(18) & User::NAME.contains("li"))
        .order_by(User::NAME.asc())
        .limit(20)
        .all(&db)
        .await?;
    println!("{majeurs:#?}");
    Ok(())
}
```

Moteurs pris en charge (une feature chacun) :

| Moteur | Feature | Connexion | Remarques |
|---|---|---|---|
| En mémoire | `memory` (par défaut) | `MemoryDb::new()` | 100 % Rust, tourne en WebAssembly, idéal pour les tests |
| SQLite | `sqlite` | `SqliteDb::open("app.db")?` | embarqué, rien à installer |
| PostgreSQL | `postgres` (+ `postgres-tls`) | `PgPool::connect(url).await?` | Heroku Postgres, Neon, Supabase… |
| MySQL / MariaDB | `mysql` (+ `mysql-tls`) | `MySqlPool::connect(url).await?` | MySQL 5.7/8+, MariaDB 10.3+ |
| Redis | `redis` (+ `redis-tls`) | `RedisDb::connect(url).await?` | NoSQL clé-valeur, lignes stockées en JSON |
| MongoDB | `mongodb` | `MongoDb::connect(url).await?` | NoSQL documents, vraies transactions sur replica set |

Lancer les exemples :

```bash
cargo run -p memoire-orm --example quickstart                    # moteur en mémoire
cargo run -p memoire-orm --example sqlite --features sqlite      # migrations + transactions
cargo run -p memoire-orm --example blog                          # jointures, chargement groupé, GROUP BY
DATABASE_URL=postgres://… cargo run -p memoire-orm --example postgres --features postgres
```

## Performances

Comparaison avec du code `rusqlite` écrit à la main (même pilote, requêtes préparées en cache :
le plancher que tout ORM SQLite peut atteindre). Intel Xeon 2,8 GHz, médianes criterion :

| Opération | Memoire | rusqlite brut |
|---|---:|---:|
| Recherche par clé primaire | 987 ns | 840 ns |
| Sélection + décodage de 1 000 lignes | 343 µs | 324 µs |
| Insertion d'une ligne | 8,88 µs | 8,21 µs |
| Insertion de 1 000 lignes dans une transaction | **1,80 ms** | 7,98 ms |
| Moteur en mémoire : recherche par clé parmi 10 000 lignes | 231 ns | — |

Détails : [`crates/memoire-orm/benches/RESULTS.md`](crates/memoire-orm/benches/RESULTS.md) ·
à reproduire avec `cargo bench -p memoire-orm --features memory,sqlite`.

## Documentation

Le guide complet se trouve dans [`docs/fr`](docs/fr) (aussi en [anglais](docs/en) et en [espagnol](docs/es)) et sur le site.

| Bien démarrer | Concepts clés | Bases de données | Pour aller plus loin |
|---|---|---|---|
| [Introduction](docs/fr/introduction.md) | [Modèles](docs/fr/models.md) | [En mémoire](docs/fr/memory.md) | [Pourquoi c'est rapide](docs/fr/performance.md) |
| [Installation](docs/fr/installation.md) | [Constructeur de requêtes](docs/fr/queries.md) | [SQLite](docs/fr/sqlite.md) | [Benchmarks](docs/fr/benchmarks.md) |
| [Démarrage rapide](docs/fr/quick-start.md) | [CRUD](docs/fr/crud.md) | [PostgreSQL](docs/fr/postgres.md) | [Déployer depuis son téléphone](docs/fr/deploy-heroku-mobile.md) |
| | [Jointures et relations](docs/fr/relations.md) | [MySQL et MariaDB](docs/fr/mysql.md) | [Publier sur crates.io](docs/fr/publish-crates-io.md) |
| | [Transactions](docs/fr/transactions.md) · [Migrations](docs/fr/migrations.md) · [SQL brut](docs/fr/raw-sql.md) | [Redis](docs/fr/redis.md) · [MongoDB (NoSQL)](docs/fr/mongodb.md) | [Aide-mémoire](docs/fr/cheatsheet.md) · [FAQ](docs/fr/faq.md) |

Référence de l'API : [docs.rs/memoire-orm](https://docs.rs/memoire-orm).

## Organisation du dépôt

```text
crates/memoire-orm/          l'ORM (publié sur crates.io)
crates/memoire-orm-derive/   #[derive(Model)] et #[derive(FromRow)]
docs/{en,fr,es}/             documentation (affichée par le site)
site/                        site de présentation — Yew + WebAssembly
server/                      petit serveur Rust qui sert le site sur Heroku
Dockerfile, heroku.yml, app.json   déploiement Heroku en un clic
.github/workflows/           CI, fusion automatique, déploiement Heroku, publication crates.io
```

## Le site web

Le site de présentation est écrit en Rust avec [Yew](https://yew.rs) et compilé en WebAssembly.
Il contient toute la documentation en trois langues et un bac à sable qui exécute le moteur
en mémoire directement dans votre navigateur.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd site && trunk serve --open
```

## Déployer depuis son téléphone 📱

Aucun ordinateur nécessaire :

1. Touchez le bouton **Deploy to Heroku** en haut de cette page, choisissez un nom d'application, puis **Deploy app**.
2. Ou ajoutez les secrets `HEROKU_API_KEY` et `HEROKU_APP_NAME` dans *Settings → Secrets and variables → Actions* :
   chaque push sur `master` est alors déployé automatiquement, et vous pouvez redéployer depuis l'application
   GitHub (*Actions → Deploy to Heroku → Run workflow*).

Guide pas à pas : [docs/fr/deploy-heroku-mobile.md](docs/fr/deploy-heroku-mobile.md).

## Automatisation

- **CI** (`ci.yml`) : formatage, clippy, la même suite de tests sur **les six moteurs** (mémoire, SQLite,
  PostgreSQL, MySQL/MariaDB, Redis, MongoDB), chaque feature isolément, compilation WebAssembly, compilation du site.
- **Fusion automatique** (`ci.yml`) : les pull requests vers `master` sont fusionnées automatiquement dès que
  la CI est verte (puis le déploiement Heroku est lancé).
- **Déploiement** (`deploy-heroku.yml`) : chaque push sur `master` déploie le site sur Heroku.
- **Publication** (`publish.yml`) : pousser un tag `v*` (ou publier une release GitHub) publie les crates sur
  crates.io — voir [Publier sur crates.io](docs/fr/publish-crates-io.md).

## Contribuer

Les contributions sont les bienvenues ! Consultez [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Distribué sous la licence [Apache 2.0](LICENSE-APACHE) ou la licence [MIT](LICENSE-MIT), au choix.
Sauf mention contraire explicite, toute contribution soumise pour inclusion dans ce projet sera
distribuée sous cette double licence, sans conditions supplémentaires.
