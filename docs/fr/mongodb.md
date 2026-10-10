# MongoDB (NoSQL)

`MongoDb` stocke vos modèles sous forme de documents MongoDB, une collection par table, via le driver MongoDB officiel. Les filtres, le tri et la pagination sont traduits en requêtes MongoDB natives et exécutés sur le serveur ; le reste du constructeur de requêtes (jointures, `GROUP BY`, agrégats) est complété par Memoire, si bien que toute l'API continue de fonctionner.

## Activer le backend

```bash
cargo add memoire-orm --features mongodb
cargo add tokio --features macros,rt-multi-thread
```

Le client est le driver officiel `mongodb` (3.x), qui chiffre les connexions avec `rustls` quand l'URI demande TLS (ce que fait toujours MongoDB Atlas). `MongoDb` a besoin du runtime **Tokio**.

## Se connecter

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
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MongoDb::connect("mongodb://localhost:27017/app").await?;
    db.create_table_if_not_exists::<User>().await?;

    let mut alice = User { id: 0, email: "alice@example.com".into(), name: "Alice".into(), age: None };
    alice.insert(&db).await?;

    let adults = User::query().filter(User::AGE.ge(18)).order_by(User::NAME.asc()).all(&db).await?;
    println!("{} adult(s)", adults.len());
    Ok(())
}
```

| Méthode | Ce qu'elle fait |
|---|---|
| `MongoDb::connect(uri)` | Se connecte avec une URI `mongodb://` ou `mongodb+srv://` qui **nomme la base de données** (`.../app`) ; sinon, échoue avec `Error::Pool` |
| `MongoDb::connect_with_database(uri, "app")` | Se connecte au serveur de `uri` et utilise la base de données `app` |
| `db.database()` | La `mongodb::Database` du driver, pour les fonctionnalités natives de MongoDB (porte de sortie) |
| `db.supports_transactions()` | `true` sur un replica set ou un cluster shardé (voir « Transactions ») |

Le handle partage le pool de connexions du driver, et le cloner ne coûte presque rien. Pour MongoDB Atlas, utilisez l'URI `mongodb+srv://user:password@cluster0.example.mongodb.net/app` affichée dans le tableau de bord Atlas.

### Un serveur local avec transactions

Les transactions nécessitent un replica set, mais un replica set à un seul nœud suffit pour le développement :

```bash
docker run -d --name mongo -p 27017:27017 mongo:7 --replSet rs0
docker exec mongo mongosh --eval 'rs.initiate({_id:"rs0",members:[{_id:0,host:"127.0.0.1:27017"}]})'
```

Connectez-vous ensuite avec `mongodb://127.0.0.1:27017/app?directConnection=true`.

## Comment vos données sont stockées

| Memoire | MongoDB |
|---|---|
| Table | Collection |
| Ligne | Document ; la clé primaire est stockée dans `_id` |
| `NULL` | Le champ est absent |
| `#[memoire(unique)]` | Index unique nommé d'après la colonne, qui ignore les champs absents (plusieurs `NULL` sont autorisés, comme en SQL) |
| `#[memoire(index)]` | Index classique nommé d'après la colonne |
| `auto_increment` | Un document compteur dans la collection `__memoire_counters` |
| `DateTime<Utc>`, `NaiveDateTime` | Date BSON (précision à la **milliseconde**) |
| `NaiveDate`, `NaiveTime` | Chaînes ISO 8601, qui se trient dans l'ordre chronologique |
| `uuid::Uuid` | Binaire BSON, sous-type 4 (UUID) |
| `Vec<u8>` | Binaire BSON |
| `serde_json::Value`, `Json<T>` | Documents et tableaux imbriqués |

Un utilisateur sans âge ressemble à ceci dans MongoDB :

```text
{ "_id": 1, "email": "alice@example.com", "name": "Alice" }
```

> **Attention** Les dates BSON conservent les millisecondes, pas les microsecondes ni les nanosecondes. Un `DateTime<Utc>` relu depuis MongoDB est tronqué à la milliseconde : tronquez les valeurs avant de les comparer (`Utc::now().trunc_subsecs(3)`, depuis `chrono::SubsecRound`).

## Comment les requêtes s'exécutent

| Requête | Où elle s'exécute |
|---|---|
| `find`, `get` par clé primaire | Serveur : `find` sur `_id` |
| `filter`, `order_by`, `limit`, `offset`, `columns` sur une seule collection | Serveur : un seul `find` avec filtre, tri, skip et limit |
| `count` | Serveur : `countDocuments` |
| `update_many` avec `set`, `set_null`, `increment`, `decrement` | Serveur : une seule mise à jour avec un pipeline d'agrégation |
| `delete_many` | Serveur : `deleteMany` |
| Jointures | Le filtre est envoyé au serveur ; les collections jointes sont récupérées avec `$in` sur les seules clés de jointure ; la jointure est faite par Memoire |
| `group_by`, `sum`, `avg`, `min`, `max`, `distinct` | Les documents correspondants sont récupérés avec le filtre appliqué sur le serveur ; l'agrégation est faite par Memoire |

Les filtres sont traduits de façon à conserver le sens que le SQL donne à `NULL` (logique à trois valeurs). Une partie de filtre qui n'a pas d'équivalent MongoDB (de l'arithmétique comme `User::AGE.add(1).gt(...)`, par exemple) est vérifiée par Memoire sur les documents trouvés par le reste du filtre. Quelques exemples :

| Filtre Memoire | Filtre MongoDB |
|---|---|
| `User::AGE.ge(18)` | `{ age: { $gte: 18 } }` |
| `!User::AGE.gt(18)` | `{ age: { $lte: 18 } }` (les utilisateurs sans âge ne correspondent pas) |
| `User::AGE.ne(3)` | `{ age: { $nin: [null, 3] } }` |
| `User::AGE.is_null()` | `{ age: null }` |
| `User::NAME.contains("li")` | `{ name: { $regex: "^.*li.*$", $options: "s" } }` |
| `User::ID.is_in([1, 2])` | `{ _id: { $in: [1, 2] } }` |

> **Astuce** Ajoutez `#[memoire(index)]` aux champs sur lesquels vous filtrez et triez : MongoDB utilise ces index pour la partie de chaque requête exécutée côté serveur.

## Sémantique des requêtes

- **Les comparaisons et `LIKE` sont sensibles à la casse**, et les chaînes sont triées octet par octet. `ilike` et `icontains` ignorent la casse, y compris pour les lettres accentuées.
- **Le tri des NULL suit MongoDB :** les valeurs absentes arrivent en premier dans l'ordre croissant et en dernier dans l'ordre décroissant. Utilisez `nulls_first()`/`nulls_last()` pour choisir explicitement (un placement différent de celui de MongoDB est appliqué par Memoire après la récupération).
- **Les clés primaires, les colonnes `unique` et `NOT NULL`** (champs qui ne sont pas des `Option`) sont appliquées, avec les erreurs habituelles.
- **Les clés étrangères ne sont pas appliquées**, et `on_delete = "cascade"` n'a aucun effet.
- **Le SQL brut** renvoie `Error::Unsupported` : utilisez `db.database()` pour les commandes natives. Les auto-jointures (une collection deux fois dans une même requête) ne sont pas prises en charge non plus.

## Transactions

Sur un **replica set** ou un **cluster shardé** (MongoDB Atlas compris), `db.begin()` ouvre une vraie transaction multi-documents :

- les lectures dans la transaction voient ses propres écritures ;
- un conflit d'écriture avec une autre transaction est signalé comme `Error::Conflict` (par l'instruction en conflit ou par `commit`) : réessayez toute la transaction, comme le montre [Transactions](transactions) ;
- dropper la transaction l'interrompt ;
- la création et la suppression de collections se font en dehors de la transaction, comme les modifications de schéma sur MySQL.

Memoire enveloppe aussi ses propres écritures multi-documents, `insert_many` et `update_many`, dans une transaction : elles se font donc en tout ou rien, même quand vous n'en utilisez pas vous-même.

Sur un **serveur autonome**, MongoDB n'a pas de transactions :

| | Serveur autonome |
|---|---|
| `db.begin()` | Échoue avec `Error::Unsupported` |
| `db.supports_transactions()` | `false` |
| Migrations | Appliquées sans transaction |
| `insert_many` | Si un document échoue (par exemple un doublon), Memoire supprime au mieux les documents de cet appel déjà insérés |

## Migrations

`create_table` crée la collection et ses index ; `drop_table` supprime la collection et son compteur d'ids. Les étapes `sql` échouent avec `Error::Unsupported` et les étapes `sql_for` sont ignorées.

Comme les collections sont créées en dehors des transactions, une migration qui échoue après une étape `create_table` laisse cette collection derrière elle. Ne mettez qu'une seule modification de schéma par migration, et supprimez la collection à la main avant de relancer une migration corrigée.

## Erreurs

| Situation MongoDB | Erreur Memoire |
|---|---|
| Clé en double (E11000) | `Error::UniqueViolation { table, column }` |
| Conflit d'écriture, erreur de transaction transitoire | `Error::Conflict(table)` : réessayez la transaction |
| Collection absente | `Error::NoSuchTable(name)` (Memoire mémorise quelles collections existent, par handle) |
| La collection existe déjà | `Error::TableExists(name)` |
| URI sans base de données | `Error::Pool(message)` |
| Tout le reste | `Error::Database(...)` avec l'erreur d'origine du driver |

## Accès direct

`db.database()` renvoie la `mongodb::Database` du driver, pour les pipelines d'agrégation, la recherche plein texte, les change streams et tout ce que MongoDB propose d'autre. Ajoutez `mongodb` (3.x) à vos dépendances :

```rust
use mongodb::bson::{doc, Document};

async fn top_authors(db: &MongoDb) -> Result<Vec<Document>, mongodb::error::Error> {
    let posts = db.database().collection::<Document>("posts");
    let mut cursor = posts
        .aggregate([
            doc! { "$group": { "_id": "$user_id", "views": { "$sum": "$views" } } },
            doc! { "$sort": { "views": -1 } },
            doc! { "$limit": 5 },
        ])
        .await?;
    let mut top = Vec::new();
    while cursor.advance().await? {
        top.push(cursor.deserialize_current()?);
    }
    Ok(top)
}
```

N'oubliez pas que Memoire stocke la clé primaire dans `_id` et omet les champs `NULL` dans les documents.

## Étapes suivantes

- [Transactions](transactions) : conflits et nouvelles tentatives.
- [Constructeur de requêtes](queries) : tout ce que vous pouvez filtrer, trier et agréger.
- [Redis](redis) : l'autre backend NoSQL.
