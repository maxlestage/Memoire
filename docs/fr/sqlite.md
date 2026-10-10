# SQLite

`SqliteDb` stocke vos données dans un seul fichier grâce à SQLite, la base de données la plus déployée au monde. Il n'y a aucun serveur à installer ni à faire tourner : SQLite est compilé dans votre programme. C'est un excellent choix pour les applications de bureau, les outils en ligne de commande, les services web de petite et moyenne taille, et les tests qui ont besoin d'une vraie base de données SQL.

## Activer le backend

```bash
cargo add memoire-orm --features sqlite
```

La feature `sqlite` compile SQLite depuis les sources (via la crate `rusqlite`) : il vous faut donc un compilateur C au moment du build (GCC, Clang ou MSVC, en général déjà installé). Rien n'est à installer sur les machines qui exécutent votre programme.

## Ouvrir une base de données

| Constructeur | À utiliser pour |
|---|---|
| `SqliteDb::open("app.db")` | Un fichier de base de données, créé s'il n'existe pas |
| `SqliteDb::open_in_memory()` | Une base privée et temporaire, qui disparaît quand elle est droppée (tests) |
| `SqliteDb::from_connection(conn)` | Une `rusqlite::Connection` existante, que vous avez configurée vous-même |

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "bookmarks")]
pub struct Bookmark {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub url: String,
    pub title: String,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("bookmarks.db")?;
    db.create_table_if_not_exists::<Bookmark>().await?;

    let mut rust = Bookmark { id: 0, url: "https://www.rust-lang.org".into(), title: "Rust".into() };
    rust.insert(&db).await?;
    println!("{} bookmark(s) saved", Bookmark::count(&db).await?);
    Ok(())
}
```

L'ouverture est synchrone (pas de `.await`) et renvoie un `memoire_orm::Result`.

`from_connection` sert aux configurations particulières, comme l'ouverture d'un fichier en lecture seule ou l'utilisation d'une extension de chiffrement. Il faut alors `rusqlite` dans vos propres dépendances, avec la même version majeure que Memoire (`rusqlite = "0.40"`) :

```rust
fn read_only(path: &str) -> memoire_orm::Result<SqliteDb> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(Error::database)?;
    SqliteDb::from_connection(conn)
}
```

## Ce que Memoire configure pour vous

| Réglage | `open` (fichier) | `open_in_memory` | `from_connection` | Pourquoi |
|---|---|---|---|---|
| `journal_mode = WAL` | oui | | | Les lecteurs ne bloquent pas l'écrivain et inversement ; écritures plus rapides |
| `synchronous = NORMAL` | oui | | | Sûr avec WAL et bien plus rapide que la valeur par défaut |
| `busy_timeout = 5000` | oui | | | Attendre jusqu'à 5 secondes qu'un verrou se libère au lieu d'échouer immédiatement |
| `foreign_keys = ON` | oui | oui | oui | Appliquer les contraintes `references` (SQLite les désactive par défaut) |
| `case_sensitive_like = ON` | oui | oui | oui | Faire en sorte que `LIKE` se comporte comme sur PostgreSQL et le moteur en mémoire |
| Cache de requêtes préparées | 256 | 256 | 256 | Chaque instruction SQL distincte est compilée une fois puis réutilisée |

> **Remarque** Avec WAL, SQLite garde deux fichiers auxiliaires à côté de votre base de données tant qu'elle est ouverte (`app.db-wal` et `app.db-shm`). C'est normal ; copiez les trois fichiers si vous sauvegardez une base en cours d'utilisation, ou utilisez l'API de sauvegarde de SQLite via `with_connection`.

## Connexions et concurrence

Un `SqliteDb` détient **une seule** connexion. Cloner un `SqliteDb` ne coûte presque rien et partage cette connexion : vous pouvez donc distribuer des clones à chaque tâche ou thread. Les instructions s'y exécutent une à la fois, ce qui est la façon la plus performante d'écrire avec SQLite.

Si vous avez de nombreux lecteurs concurrents sur une base fichier, vous pouvez ouvrir plusieurs `SqliteDb` sur le même fichier : grâce à WAL, ils lisent en parallèle pendant que l'un d'eux écrit.

## Comportement async

SQLite tourne dans votre processus et n'attend jamais le réseau : les futures de `SqliteDb` se terminent donc dès qu'elles s'exécutent. Elles fonctionnent avec n'importe quel runtime async, ou sans runtime via `memoire_orm::block_on` :

```rust
fn count_bookmarks() -> memoire_orm::Result<u64> {
    let db = SqliteDb::open("bookmarks.db")?;
    memoire_orm::block_on(Bookmark::count(&db))
}
```

Dans Tokio, une requête s'exécute directement sur le thread worker qui l'attend. La plupart des requêtes prennent quelques microsecondes, ce n'est donc pas un problème. Pour une requête lourde occasionnelle (un gros rapport, un `VACUUM`), déplacez-la sur un thread bloquant pour que les autres tâches continuent de tourner :

```rust
async fn heavy_report(db: &SqliteDb) -> memoire_orm::Result<u64> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || memoire_orm::block_on(Bookmark::count(&db)))
        .await
        .expect("the report task panicked")
}
```

## Comment les valeurs sont stockées

SQLite ne dispose que de quelques types de stockage : Memoire convertit donc les types Rust plus riches :

| Type Rust | Stocké en | Exemple |
|---|---|---|
| `bool` | `INTEGER` | `0` ou `1` |
| Entiers (`i8` à `i64`, `u8` à `u32`) | `INTEGER` | `42` |
| `f32`, `f64` | `REAL` | `3.5` |
| `String` | `TEXT` | `'hello'` |
| `Vec<u8>` | `BLOB` | octets bruts |
| `uuid::Uuid` | `TEXT` | `'67e55044-10b1-426f-9247-bb680e5fe0c8'` |
| `chrono::NaiveDateTime` | `TEXT` | `'2026-10-10 14:30:00.250'` |
| `chrono::DateTime<Utc>` | `TEXT` (RFC 3339) | `'2026-10-10T14:30:00.250Z'` |
| `chrono::NaiveDate` | `TEXT` | `'2026-10-10'` |
| `chrono::NaiveTime` | `TEXT` | `'14:30:00'` |
| `serde_json::Value`, `Json<T>` | `TEXT` | `'{"theme":"dark"}'` |

La lecture effectue automatiquement la conversion inverse : vos structs reçoivent donc toujours les bons types Rust.

> **Remarque** Les dates et heures sont stockées sous forme de texte ISO 8601 à largeur fixe (précision à la nanoseconde, suffixe `Z` pour `DateTime<Utc>`) : l'ordre du texte correspond donc à l'ordre chronologique. `gt`, `between` et `order_by` fonctionnent sur toutes les colonnes de date et d'heure.

### Clés auto-incrémentées

Une clé `auto_increment` devient `INTEGER PRIMARY KEY`, le row id intégré de SQLite. Les nouvelles lignes reçoivent l'id le plus élevé existant plus un : l'id de la dernière ligne insérée peut donc être réutilisé si cette ligne est supprimée. Si les ids ne doivent jamais être réutilisés, conservez les lignes supprimées (par exemple avec un indicateur `active`) au lieu de les supprimer réellement.

## Transactions

`db.begin()` réserve la connexion à la transaction jusqu'à ce qu'elle soit validée, annulée ou droppée. Les transactions s'exécutent donc une à la fois et n'entrent jamais en conflit.

> **Attention** Tant qu'une transaction est ouverte, une requête lancée sur le handle `SqliteDb` lui-même (`&db` au lieu de `&tx`) attend la fin de la transaction. Dans la même tâche, c'est un deadlock. Dans une transaction, passez toujours `&tx`.

Les transactions sont aussi la clé d'écritures en masse rapides sur SQLite. En dehors d'une transaction, chaque écriture est une transaction à part entière, avec son propre travail de verrouillage et de journalisation ; à l'intérieur d'une transaction, ce coût n'est payé qu'une fois, au commit :

```rust
async fn import(db: &SqliteDb, bookmarks: &[Bookmark]) -> memoire_orm::Result<u64> {
    let tx = db.begin().await?;
    let n = Bookmark::insert_many(&tx, bookmarks).await?;
    tx.commit().await?;
    Ok(n)
}
```

## Erreurs

Les erreurs SQLite sont traduites dans les variantes d'erreur communes de Memoire :

| Situation SQLite | Erreur Memoire |
|---|---|
| `UNIQUE constraint failed: users.email` (ou une clé primaire en double) | `Error::UniqueViolation { table: "users", column: "email" }` |
| Échec d'une contrainte `NOT NULL`, `FOREIGN KEY` ou `CHECK` | `Error::Constraint(message)` |
| `no such table: users` | `Error::NoSuchTable("users")` |
| `table users already exists` | `Error::TableExists("users")` |
| Tout le reste | `Error::Database(...)` avec l'erreur `rusqlite` d'origine |

## Bon à savoir

- **`LIKE` est sensible à la casse** (Memoire active `case_sensitive_like`). `ilike` et `icontains` ignorent la casse uniquement pour les lettres ASCII : `icontains("é")` ne trouve pas `"É"`.
- **Tri des NULL :** SQLite place les NULL en premier dans l'ordre croissant, contrairement à PostgreSQL et au moteur en mémoire. Utilisez `nulls_first()`/`nulls_last()` pour des résultats portables.
- **Le SQL brut** utilise les placeholders `?`. Voir [SQL brut](raw-sql).
- **Accès direct :** `db.with_connection(|conn| ...)` vous donne la `rusqlite::Connection` pour tout ce que Memoire ne couvre pas (sauvegardes, pragmas, fonctions personnalisées). Voir [SQL brut](raw-sql).

## Étapes suivantes

- [PostgreSQL](postgres) : quand vous avez besoin d'un serveur de base de données.
- [Transactions](transactions) : comment les transactions SQLite se comparent à celles des autres backends.
- [Pourquoi c'est rapide](performance) : conseils pour les écritures en masse.
