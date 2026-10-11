<div align="center">

# Memoire ORM

**L'ORM Rust natif, ultra-rapide et typé.**

[![crates.io](https://img.shields.io/crates/v/memoire-orm.svg)](https://crates.io/crates/memoire-orm)
[![docs.rs](https://img.shields.io/docsrs/memoire-orm)](https://docs.rs/memoire-orm)
[![Licence : propriétaire](https://img.shields.io/badge/license-proprietary-2a2a2e.svg)](#licence)

[English](README.md) · **Français** · [Español](README.es.md)

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
memoire-orm = { git = "https://github.com/maxlestage/Memoire", features = ["sqlite"] }   # ou "postgres", "mysql", "redis", "mongodb", "full"
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
| MySQL / MariaDB | `mysql` (+ `mysql-tls`) | `MySqlPool::connect(url).await?` | MySQL 8+, MariaDB 10.6+ |
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

Le moteur en mémoire (qui évalue aussi les requêtes des backends Redis et MongoDB) filtre avec des
boucles spécialisées pour chaque type de colonne, sur des copies contiguës des colonnes numériques,
et ses index ne copient jamais les clés. Intel Xeon 2,8 GHz, médianes criterion, table de 10 000 lignes :

| Moteur en mémoire | Avant | Maintenant |
|---|---:|---:|
| Égalité sur une colonne `#[memoire(index)]` | 140,9 µs | **1,1 µs** (×123) |
| `COUNT(*) WHERE` | 106,2 µs | **11,1 µs** (×9,6) |
| Filtre + `ORDER BY` + `LIMIT 20` | 231,2 µs | **36,3 µs** (×6,4) |
| `update_many` avec un filtre | 177,0 µs | **35,8 µs** (×4,9) |
| `insert_many`, 1 000 lignes | 422,9 µs | **246,0 µs** (×1,7) |

Sur SQLite, comparaison avec du code `rusqlite` écrit à la main (même pilote, requêtes préparées en
cache : le plancher que tout ORM SQLite peut atteindre) :

| Opération | Memoire | rusqlite brut |
|---|---:|---:|
| Recherche par clé primaire | 961 ns | 746 ns |
| Sélection + décodage de 1 000 lignes | 315 µs | 308 µs |
| Insertion d'une ligne | 6,26 µs | 5,92 µs |
| Insertion de 1 000 lignes dans une transaction | **1,95 ms** | 6,64 ms |

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
site/                        site de présentation, écrit avec active (serveur + îlots WebAssembly, PWA)
Dockerfile, heroku.yml, app.json   déploiement Heroku en un clic
.github/workflows/           CI, fusion automatique, déploiement Heroku et GitHub Pages, publication crates.io
```

## Le site web

Le site de présentation est écrit en Rust avec [Active](https://maxlestage.github.io/Active/) :
un petit serveur Rust rend chaque page (balises SEO complètes, en anglais, français et espagnol),
et seules les parties interactives (le bac à sable, les onglets de code, la recherche dans la
doc) se réveillent dans le navigateur en WebAssembly. Le bac à sable exécute le vrai moteur en
mémoire dans votre navigateur. Le site est une PWA : installez-le sur l'écran d'accueil de votre
téléphone ; une fois ouvert, il fonctionne même hors ligne, bac à sable compris. Le même serveur
exporte tout le site en fichiers statiques pour GitHub Pages.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # la version de site/Cargo.lock
cd site
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm
cargo run                                          # http://localhost:8080
BASE_PATH=/Memoire cargo run -- export dist-static  # fichiers statiques, comme sur GitHub Pages
```

## Déployer depuis son téléphone 📱

Aucun ordinateur nécessaire :

1. Ouvrez `https://heroku.com/deploy?template=https://github.com/maxlestage/Memoire` sur votre téléphone, choisissez un nom d'application, puis **Deploy app**.
2. Ou ajoutez les secrets `HEROKU_API_KEY` et `HEROKU_APP_NAME` dans *Settings → Secrets and variables → Actions* :
   chaque push sur `master` est alors déployé automatiquement, et vous pouvez redéployer depuis l'application
   GitHub (*Actions → Deploy to Heroku → Run workflow*).
3. Ou publiez le site **gratuitement sur GitHub Pages** : *Settings → Pages → Source : GitHub Actions*, puis
   *Actions → Deploy to GitHub Pages → Run workflow*. Il est alors en ligne sur `https://<votre-compte>.github.io/Memoire/`.

Guide pas à pas : [docs/fr/deploy-heroku-mobile.md](docs/fr/deploy-heroku-mobile.md).

## Automatisation

- **CI** (`ci.yml`) : formatage, clippy, la même suite de tests sur **les six moteurs** (mémoire, SQLite,
  PostgreSQL, MySQL/MariaDB, Redis, MongoDB), chaque feature isolément, compilation WebAssembly, compilation du site.
- **Fusion automatique** (`ci.yml`) : les pull requests vers `master` sont fusionnées automatiquement dès que
  la CI est verte (puis les déploiements Heroku et GitHub Pages sont lancés).
- **Déploiement** (`deploy-heroku.yml`) : chaque push sur `master` déploie le site sur Heroku.
- **GitHub Pages** (`pages.yml`) : chaque push sur `master` qui modifie le site ou la documentation le publie sur GitHub Pages.
- **Publication** (`publish.yml`) : pousser un tag `v*` (ou publier une release GitHub) publie les crates sur
  crates.io — voir [Publier sur crates.io](docs/fr/publish-crates-io.md).

## Contribuer

Les contributions sont les bienvenues ! Consultez [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Memoire ORM est un logiciel propriétaire : © 2026 Maxime Nathan Lestage, tous
droits réservés. Ce n'est pas un logiciel open source. Le code source est
publié sur crates.io et GitHub pour pouvoir être lu, audité et compilé, et
vous pouvez l'utiliser librement comme dépendance de vos propres programmes,
y compris commerciaux. Le modifier ou redistribuer son code source demande
l'accord écrit de l'auteur. Conditions complètes : [LICENSE](LICENSE).

Les contributions (code, documentation, idées) sont les bienvenues et
rejoignent le projet aux mêmes conditions : en soumettre une, c'est en céder
les droits à l'auteur, comme l'indique la licence.

## Crédits

Conçu et développé par Maxime Nathan Lestage,
au sein d'une même famille de projets Rust : [Active](https://maxlestage.github.io/Active/), [Vitesse](https://maxlestage.github.io/Vitesse/), [Securite](https://maxlestage.github.io/Securite/), [Memoire](https://maxlestage.github.io/Memoire/), [Paquets](https://maxlestage.github.io/Paquets/) et [FerroSwift](https://maxlestage.github.io/FerroSwift/).
