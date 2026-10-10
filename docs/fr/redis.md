# Redis

`RedisDb` stocke vos modèles dans Redis, le magasin de données en mémoire. Vous utilisez les mêmes modèles, requêtes, transactions et migrations qu'avec n'importe quel autre backend : Memoire conserve chaque table dans quelques clés Redis, écrit chaque modification de façon atomique et évalue les requêtes avec son moteur en mémoire. C'est un bon choix pour les caches, les sessions, l'état partagé d'une application et les jeux de données de petite ou moyenne taille.

## Quand l'utiliser

| Idéal pour | À réfléchir pour |
|---|---|
| Sessions, tokens, paniers, feature flags, caches | Les grandes tables interrogées sur des colonnes quelconques (chacune de ces requêtes lit toute la table) |
| Les données partagées par plusieurs processus ou serveurs | Les données qui doivent survivre à la perte du serveur Redis, sauf si la persistance de Redis est configurée |
| Les jeux de données petits ou moyens qui doivent se lire rapidement par clé | Redis Cluster (voir « Limites ») |
| Les applications qui utilisent déjà Redis et veulent un accès typé | Le reporting et l'analytique |

## Activer le backend

```bash
cargo add memoire-orm --features redis
cargo add tokio --features macros,rt-multi-thread
```

Pour les URL `rediss://` (TLS), utilisez plutôt la feature `redis-tls` (elle inclut `redis`). Le client est la crate `redis` ; TLS utilise `rustls`. `RedisDb` a besoin du runtime **Tokio**.

> **Astuce** Pour essayer en local : `docker run -d -p 6379:6379 redis:7`.

## Se connecter

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "sessions")]
pub struct Session {
    #[memoire(primary_key)]
    pub token: String,
    pub user_id: i64,
    pub expires_at: i64,
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = RedisDb::connect("redis://127.0.0.1:6379").await?;
    db.create_table_if_not_exists::<Session>().await?;

    let mut session = Session { token: "4f9c2a".into(), user_id: 42, expires_at: 1_790_000_000 };
    session.insert(&db).await?;

    let found = Session::get(&db, "4f9c2a").await?;
    println!("token belongs to user {}", found.user_id);
    Ok(())
}
```

| Méthode | Ce qu'elle fait |
|---|---|
| `RedisDb::connect(url)` | Se connecte et range toutes les clés sous le préfixe `memoire` |
| `RedisDb::connect_with_prefix(url, "myapp")` | Idem, avec votre propre préfixe, pour que plusieurs applications (ou exécutions de tests) puissent partager un même serveur Redis |
| `db.connection()` | Le `redis::aio::ConnectionManager` sous-jacent (porte de sortie) |

Les URL ont la forme `redis://host:6379`, `redis://:password@host:6379/2` (base de données 2) ou `rediss://...` pour TLS. Le handle garde une connexion multiplexée unique qui se reconnecte automatiquement ; le cloner ne coûte presque rien.

### TLS et Heroku Key-Value Store

Avec la feature `redis-tls`, les URL `rediss://` sont chiffrées et le certificat du serveur est vérifié. Certains services hébergés, dont Heroku Key-Value Store, utilisent des certificats auto-signés : ajoutez `#insecure` à l'URL pour conserver le chiffrement tout en sautant la vérification du certificat.

```rust
async fn connect_on_heroku() -> Result<RedisDb, Box<dyn std::error::Error>> {
    // Heroku définit REDIS_URL (une URL rediss://) quand vous ajoutez Heroku Key-Value Store.
    let url = std::env::var("REDIS_URL")?;
    let url = if url.starts_with("rediss://") && !url.contains('#') { format!("{url}#insecure") } else { url };
    Ok(RedisDb::connect_with_prefix(&url, "myapp").await?)
}
```

## Comment vos données sont stockées

Chaque table utilise une poignée de clés. Leurs noms partagent la partie `{table}`, un *hash tag* Redis :

| Clé | Type Redis | Contenu |
|---|---|---|
| `memoire:tables` | set | Les noms des tables existantes |
| `memoire:{users}:rows` | hash | Clé primaire → la ligne sous forme d'objet JSON |
| `memoire:{users}:u:email` | hash | Une par colonne `unique` : valeur → clé primaire |
| `memoire:{users}:seq` | string | Le dernier id généré (`auto_increment`) |
| `memoire:{users}:ver` | string | Un numéro de version, incrémenté à chaque écriture |
| `memoire:{users}:meta` | hash | La liste des colonnes et les clés de la table |

Les lignes sont du JSON lisible : vous pouvez donc les inspecter avec `redis-cli`, et d'autres programmes peuvent les lire :

```text
> HGET memoire:{users}:rows 1
{"id":1,"email":"alice@example.com","name":"Alice","age":31,"joined":"2026-10-10T14:30:00.123456789Z"}
```

Les valeurs sont stockées sous forme de nombres, de chaînes et de booléens JSON. `NULL` devient `null` ; les octets sont des chaînes hexadécimales ; les UUID et les dates sont du texte (les horodatages conservent une précision à la nanoseconde) ; les colonnes JSON sont imbriquées telles quelles.

## Comment les opérations s'exécutent

| Opération | Ce qui se passe dans Redis |
|---|---|
| `insert`, `upsert`, `update`, `delete` | Un seul script Lua atomique pour la ligne et ses index `unique` |
| `insert_many` | Un seul script Lua atomique pour tout le lot |
| `find`, `get`, `reload` | Un seul `HGET` |
| `filter` sur la clé primaire avec `eq` ou `is_in` | Ne lit que ces lignes (`HGET`/`HMGET`) |
| `count()` sans filtre | `HLEN` |
| Toutes les autres requêtes (filtres sur d'autres colonnes, tris, jointures, agrégats...) | Lit la ou les tables concernées en entier, puis évalue la requête avec le [moteur en mémoire](memory) de Memoire |
| `update_many`, `delete_many` | Lit la table, calcule les modifications avec le moteur en mémoire, puis ne les écrit que si la table n'a pas changé entre-temps |

Chaque écriture est un seul script Lua qui vérifie d'abord ses conditions (la table existe, les valeurs `unique` sont libres, la ligne ou la table n'a pas changé) et n'écrit qu'ensuite. Une écriture se fait donc en tout ou rien, même quand elle touche de nombreuses lignes et de nombreux index.

Les écritures qui dépendent de données lues juste avant (`update`, `delete`, `update_many`...) sont optimistes : si un autre client a modifié les données entre-temps, Memoire réessaie automatiquement l'instruction, jusqu'à 64 fois, avant d'abandonner avec `Error::Conflict`.

> **Astuce** Les recherches par clé primaire sont le chemin rapide. Faites des clés par lesquelles vous cherchez, comme un token de session ou un nom d'utilisateur, la clé primaire plutôt qu'une colonne `unique` : un filtre sur une colonne `unique` lit toute la table.

## Sémantique des requêtes

Comme les requêtes sont évaluées par le moteur en mémoire, Redis se comporte exactement comme [`MemoryDb`](memory) :

- les comparaisons suivent la logique à trois valeurs du SQL (`NULL` ne satisfait jamais une comparaison) ;
- les NULL arrivent en dernier dans l'ordre croissant et en premier dans l'ordre décroissant ;
- `like`, `contains`... sont sensibles à la casse ; `ilike` et `icontains` ignorent la casse, y compris pour les lettres accentuées ;
- les clés primaires, les colonnes `unique` et `NOT NULL` (champs qui ne sont pas des `Option`) sont appliquées, avec les erreurs habituelles (`Error::UniqueViolation`, `Error::Constraint`) ;
- les clés étrangères ne sont pas appliquées et `on_delete = "cascade"` n'a aucun effet ;
- le SQL brut (`raw`, `raw_execute`) renvoie `Error::Unsupported`, tout comme les auto-jointures.

## Transactions

`db.begin()` démarre une transaction qui lit chaque table une seule fois, la première fois qu'elle l'utilise, puis travaille sur ce snapshot privé. Au `commit`, toutes les modifications sont écrites par un seul script Lua atomique. Si un autre client a modifié l'une des tables que la transaction a modifiées, le commit échoue avec `Error::Conflict` et rien n'est écrit : réessayez toute la transaction (voir [Transactions](transactions)).

- Les conflits sont détectés par table : deux transactions qui écrivent des lignes différentes de la même table entrent quand même en conflit.
- Une transaction qui ne fait que lire n'entre jamais en conflit.
- Dropper une transaction l'abandonne ; rien n'a été écrit.
- Créer ou supprimer des tables dans une transaction prend effet au commit : une [migration](migrations) qui échoue ne laisse donc rien derrière elle.

## Migrations

Les étapes `create_table` et `drop_table` fonctionnent, et chaque migration est appliquée de façon atomique. Les étapes `sql` échouent avec `Error::Unsupported` (il n'y a pas de SQL), et les étapes `sql_for` sont ignorées.

## Limites

- **Redis Cluster n'est pas pris en charge**, car une même écriture peut toucher des clés de plusieurs tables, qu'un cluster peut stocker sur des nœuds différents. Un serveur autonome, Redis Sentinel et les services managés à un seul shard fonctionnent.
- **Tout vit en mémoire.** Configurez la persistance de Redis (snapshots RDB ou AOF) si les données doivent survivre à un redémarrage.
- **Les grandes tables interrogées autrement que par clé** sont transférées entièrement vers votre application à chaque requête. Gardez ces tables petites, ou interrogez-les par clé primaire.
- **Pas d'expiration par ligne.** Les lignes n'expirent pas d'elles-mêmes ; supprimez les anciennes lignes avec `delete_many`, ou utilisez la connexion brute pour vos propres clés à expiration.

## Accès direct

`db.connection()` renvoie le gestionnaire de connexion `redis`, pour tout ce que Memoire ne couvre pas. Ajoutez `redis` (1.x, avec la feature `tokio-comp`) à vos dépendances pour envoyer des commandes :

```rust
async fn raw_row(db: &RedisDb, id: i64) -> redis::RedisResult<Option<String>> {
    let mut conn = db.connection();
    redis::cmd("HGET").arg("memoire:{users}:rows").arg(id).query_async(&mut conn).await
}
```

> **Attention** Ne modifiez pas directement les clés de Memoire : les index `unique`, les versions et les ids doivent rester cohérents avec les lignes. Lisez-les librement, mais écrivez via Memoire.

## Étapes suivantes

- [Moteur en mémoire](memory) : le moteur de requêtes sur lequel s'appuie Redis.
- [Transactions](transactions) : conflits et nouvelles tentatives.
- [MongoDB](mongodb) : un autre backend NoSQL, avec des requêtes exécutées par le serveur.
