# Journal des requêtes

Enveloppez une connexion dans `Logged` pour enregistrer chaque instruction exécutée par elle, ses clones et ses transactions : le SQL, les paramètres, la durée et le nombre de lignes. C'est `DB::enableQueryLog()` / `DB::listen()` de Laravel et le logger SQL de Doctrine, sur toutes les bases.

```rust
use memoire_orm::log::Logged;
use memoire_orm::prelude::*;

let db = Logged::new(SqliteDb::open("app.db")?); // ou MemoryDb, PgPool, MySqlPool, RedisDb, MongoDb

let users = User::query().filter(User::ACTIVE.eq(true)).all(&db).await?;

for q in db.queries() {
    println!("{q}");
}
// SELECT "users"."id", ... FROM "users" WHERE "users"."active" = ? -- [TRUE] (0.041 ms)
```

`Logged<D>` est lui-même une base de données : passez `&db` aux méthodes des modèles, aux requêtes et aux transactions comme avant.

## Contenu d'une entrée

| Champ | Contenu |
|---|---|
| `sql` | L'instruction, dans le dialecte de la base (`BEGIN`, `COMMIT`, `ROLLBACK` pour les transactions) |
| `params` | Les paramètres liés, dans l'ordre |
| `duration` | Temps entre l'appel et le résultat (`None` en WebAssembly) |
| `rows` | Lignes renvoyées par une requête, ou modifiées par une écriture ; `None` en cas d'échec |
| `error` | Le message d'erreur, si l'instruction a échoué |
| `in_transaction` | Exécutée dans une transaction ou non |

Le moteur en mémoire, Redis et MongoDB n'exécutent pas de SQL : leur journal montre l'instruction équivalente dans le dialecte SQLite, tout aussi utile pour voir ce que fait un morceau de code.

## Repérer les requêtes N+1 dans un test

```rust
let db = Logged::new(MemoryDb::new());
// ... données de test ...
db.clear();

let users = User::all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

assert_eq!(db.query_count(), 2, "une requête par niveau, quel que soit le nombre d'utilisateurs");
```

| Méthode | Effet |
|---|---|
| `queries()` | Copie des entrées, de la plus ancienne à la plus récente |
| `take_queries()` | Les entrées, en vidant le journal |
| `query_count()` | Nombre d'entrées |
| `clear()` | Vide le journal |
| `set_recording(false)` / `set_recording(true)` | Arrête / reprend la conservation des entrées |
| `listen(\|q\| ...)` | Appelle une fonction avec chaque instruction, au moment où elle s'exécute |
| `Logged::with_limit(db, n)` | Ne garde que les `n` dernières entrées |

## Afficher chaque instruction

Pour un programme qui tourne longtemps, ne gardez rien et affichez plutôt :

```rust
let db = Logged::with_limit(PgPool::connect(&url).await?, 0);
db.listen(|q| {
    if q.duration.is_some_and(|d| d.as_millis() > 100) {
        eprintln!("requête lente : {q}");
    }
});
```

Les écouteurs s'exécutent sur la tâche qui a lancé l'instruction, juste après elle : gardez-les rapides.

> **Astuce** La journalisation met chaque instruction en texte et copie ses paramètres, ce qui coûte un peu à chaque appel. Utilisez-la en développement et dans les tests, ou avec un écouteur qui filtre ce qu'il garde.

## Étapes suivantes

- [Notifications de changements](events) : être prévenu des écritures, avec les lignes écrites.
- [SQL brut](raw-sql) : exécuter vos propres instructions.
