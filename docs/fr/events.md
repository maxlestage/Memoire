# Notifications de changements

Enveloppez une base dans `Observed` pour être prévenu de chaque écriture faite à travers elle : rafraîchir un cache, envoyer des mises à jour en direct à une page web, tenir un journal d'audit ou lancer un travail en arrière-plan. Cela fonctionne de la même façon sur toutes les bases.

```rust
use memoire_orm::prelude::*;

let db = Observed::new(SqliteDb::open("app.db")?); // ou MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let _subscription = db.subscribe(|change| {
    println!("{:?} sur {} : {} ligne(s), clé {:?}", change.kind, change.table, change.rows, change.key);
});

let mut alice = User { id: 0, name: "Alice".into() };
alice.insert(&db).await?; // Insert sur users : 1 ligne(s), clé Some(I64(1))
alice.delete(&db).await?; // Delete sur users : 1 ligne(s), clé Some(I64(1))
```

`Observed<D>` est lui-même une base : passez `&db` aux méthodes des modèles, aux requêtes et aux transactions comme avant. Ses clones partagent les écouteurs.

## Ce que contient un changement

| Champ | Contenu |
|---|---|
| `table` | La table écrite (vide pour du SQL brut) |
| `kind` | `Insert`, `Update`, `Upsert`, `Delete`, `CreateTable`, `DropTable` ou `Sql` |
| `rows` | Nombre de lignes touchées |
| `key` | La clé primaire, pour `insert`, `update`, `upsert`, `delete` et `delete_by_key` ; `None` pour les instructions sur plusieurs lignes (`insert_many`, `update_many`, `update_only`, `delete_many`) |
| `row` | Toutes les colonnes telles qu'écrites, dans l'ordre du schéma, pour `insert`, `update` et `upsert` (avec la clé générée) |

Les lectures, et les écritures qui ne changent rien (un `update_many` qui ne trouve aucune ligne, la suppression d'une clé absente), ne sont pas annoncées.

## Transactions

Les changements faits dans une transaction sont annoncés au moment du commit, dans l'ordre, et jamais si elle est annulée ou abandonnée :

```rust
let tx = db.begin().await?;
alice.insert(&tx).await?; // rien pour l'instant
tx.commit().await?;       // maintenant : Insert sur users
```

## Se désabonner, canaux

`subscribe` renvoie une `Subscription` : l'écouteur reste enregistré jusqu'à ce qu'elle soit abandonnée (drop) ou qu'on appelle `unsubscribe()`. Les écouteurs s'exécutent sur la tâche qui a écrit, juste après l'écriture : gardez-les rapides. Pour un travail lent, recevez les changements sur un canal et traitez-les ailleurs :

```rust
let (subscription, changes) = db.channel();
std::thread::spawn(move || {
    for change in changes {
        // envoyer un e-mail, mettre à jour un index de recherche...
    }
});
```

`db.listener_count()` indique combien d'écouteurs sont enregistrés.

## Limites

- Seules les écritures faites **à travers la base observée** (ses clones et ses transactions) sont vues. Un autre programme qui écrit dans la même base ne l'est pas, ni le code qui utilise directement la base enveloppée (`db.inner()`).
- Un changement est annoncé une fois l'écriture réussie ; pour une transaction, une fois le commit réussi.
- Le SQL brut (`raw_execute`, scripts de migration) est annoncé comme `ChangeKind::Sql`, sans nom de table.

## Étapes suivantes

- [Transactions](transactions) : commit, annulation et nouvelles tentatives.
- [Moteur en mémoire](memory) : une base qui peut vivre dans un fichier.
- [Aide-mémoire](cheatsheet) : tout sur une page.
