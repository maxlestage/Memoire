# Démarrage rapide

Dans ce tutoriel de dix minutes, vous allez construire de A à Z un petit gestionnaire de tâches. Vous commencerez avec le moteur en mémoire, découvrirez une à une les opérations du quotidien, puis vous finirez par enregistrer vos tâches dans un fichier SQLite en changeant une seule ligne.

## Ce que vous allez construire

Un programme en ligne de commande qui :

1. crée une table `tasks`,
2. ajoute quelques tâches,
3. liste les tâches en cours, les plus importantes en premier,
4. modifie et supprime des tâches,
5. calcule quelques statistiques,
6. et, pour finir, conserve le tout dans un fichier `tasks.db`.

Il vous suffit de Rust 1.88 ou plus récent. Aucun serveur de base de données n'est nécessaire.

## Étape 1 : créer le projet

```bash
cargo new tasks
cd tasks
cargo add memoire-orm
cargo add tokio --features macros,rt-multi-thread
```

`memoire-orm` est fourni avec le moteur en mémoire activé par défaut. Tokio est le runtime async qui fera tourner notre `async fn main`.

## Étape 2 : décrire la table avec une struct

Ouvrez `src/main.rs` et commencez par le modèle. Un *modèle* est une struct Rust normale avec `#[derive(Model)]` : chaque champ devient une colonne.

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tasks")]
pub struct Task {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub priority: i32,
    pub done: bool,
    pub note: Option<String>,
}

impl Task {
    fn new(title: &str, priority: i32) -> Self {
        Task { id: 0, title: title.to_string(), priority, done: false, note: None }
    }
}
```

Ce que signifie chaque élément :

| Code | Signification |
|---|---|
| `use memoire_orm::prelude::*;` | Importe tout ce dont vous avez habituellement besoin (`Model`, `MemoryDb`, `Executor`, ...) |
| `#[derive(Model)]` | Génère le code d'accès à la base de données pour `Task` à la compilation |
| `#[memoire(table = "tasks")]` | Le nom de la table. Sans lui, la table s'appellerait `task` |
| `#[memoire(primary_key, auto_increment)]` | `id` identifie chaque ligne, et c'est la base de données qui choisit sa valeur |
| `Option<String>` | Une colonne nullable : `None` est stocké sous forme de `NULL` |

> **Astuce** `#[derive(Model)]` crée aussi une *constante de colonne typée* par champ : `Task::ID`, `Task::TITLE`, `Task::PRIORITY`, `Task::DONE` et `Task::NOTE`. Vous allez les utiliser dans un instant pour construire des requêtes.

## Étape 3 : ouvrir une base de données et créer la table

```rust
#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MemoryDb::new();
    db.create_table::<Task>().await?;

    // les étapes suivantes viennent ici

    Ok(())
}
```

`MemoryDb::new()` crée une base de données vide qui vit en mémoire. `create_table::<Task>()` crée la table `tasks` avec les colonnes de `Task`. Chaque opération sur la base est `async`, vous l'attendez donc avec `.await`, et renvoie un `memoire_orm::Result`, si bien que `?` fait remonter les erreurs.

## Étape 4 : insérer des lignes

```rust
let mut task = Task::new("Write the docs", 3);
task.insert(&db).await?;
println!("created task #{}", task.id);
```

Ce code affiche `created task #1`. Comme `id` est en `auto_increment`, la valeur que vous mettez dans la struct (`0`) est ignorée et la base de données en génère une. `insert` prend `&mut self` afin de pouvoir réécrire le nouvel id dans votre struct.

Ajoutons-en quelques autres :

```rust
let more = [("Fix the bike", 1), ("Call grandma", 2), ("Learn Memoire", 3)];
for (title, priority) in more {
    let mut task = Task::new(title, priority);
    task.insert(&db).await?;
    println!("created task #{}: {}", task.id, task.title);
}
```

```text
created task #2: Fix the bike
created task #3: Call grandma
created task #4: Learn Memoire
```

## Étape 5 : lire des lignes

Récupérez une ligne par sa clé primaire :

```rust
let task = Task::get(&db, 2).await?;
println!("task 2 is {:?}", task.title);

let missing = Task::find(&db, 42).await?;
println!("task 42: {missing:?}");
```

```text
task 2 is "Fix the bike"
task 42: None
```

`get` renvoie une erreur (`Error::NotFound`) quand la ligne n'existe pas, alors que `find` renvoie une `Option`. Utilisez celle qui se lit le mieux dans votre code.

Pour chercher avec des conditions, commencez une requête avec `Task::query()` et enchaînez ce dont vous avez besoin :

```rust
let todo = Task::query()
    .filter(Task::DONE.eq(false))
    .order_by(Task::PRIORITY.desc())
    .order_by(Task::TITLE.asc())
    .all(&db)
    .await?;
for task in &todo {
    println!("[{}] {} (#{})", task.priority, task.title, task.id);
}
```

```text
[3] Learn Memoire (#4)
[3] Write the docs (#1)
[2] Call grandma (#3)
[1] Fix the bike (#2)
```

Lisez-la comme une phrase : « les tâches dont done vaut false, triées par priorité (la plus haute en premier) puis par titre, toutes ». Rien n'est envoyé à la base de données avant le `.all(&db)` final.

Les conditions se combinent avec `&` (et), `|` (ou) et `!` (non) :

```rust
let urgent = Task::query()
    .filter(Task::PRIORITY.ge(3) & Task::TITLE.contains("Memoire"))
    .first(&db)
    .await?;
println!("urgent: {:?}", urgent.map(|t| t.title));
```

Ce code affiche `urgent: Some("Learn Memoire")`. `first` renvoie la première ligne qui correspond, s'il y en a une.

> **Remarque** Les constantes de colonnes sont typées. `Task::PRIORITY.ge(3)` compile parce que `priority` est un `i32`, mais `Task::PRIORITY.ge("3")` est refusé par le compilateur. Les erreurs sont détectées avant même que votre programme ne s'exécute.

## Étape 6 : modifier des lignes

Pour modifier une ligne, changez la struct puis appelez `update` :

```rust
let mut bike = Task::get(&db, 2).await?;
bike.done = true;
bike.note = Some("new chain".to_string());
bike.update(&db).await?;
```

Pour modifier plusieurs lignes d'un coup sans les charger, utilisez `update_many` :

```rust
let changed = Task::update_many()
    .increment(Task::PRIORITY, 1)
    .filter(Task::DONE.eq(false) & Task::PRIORITY.lt(3))
    .execute(&db)
    .await?;
println!("bumped {changed} task(s)");
```

Ce code affiche `bumped 1 task(s)` : seule « Call grandma » était en cours avec une priorité inférieure à 3, elle passe donc de 2 à 3.

## Étape 7 : compter et calculer des statistiques

```rust
let total = Task::count(&db).await?;
let open = Task::query().filter(Task::DONE.eq(false)).count(&db).await?;
let average = Task::query().avg(Task::PRIORITY, &db).await?;
println!("{open} open task(s) out of {total}, average priority {:.2}", average.unwrap_or(0.0));
```

```text
3 open task(s) out of 4, average priority 2.50
```

`avg` renvoie une `Option<f64>`, car la moyenne de zéro ligne n'existe pas.

## Étape 8 : supprimer des lignes

```rust
let removed = Task::delete_many().filter(Task::DONE.eq(true)).execute(&db).await?;
println!("removed {removed} finished task(s)");
```

Ce code affiche `removed 1 finished task(s)`. Pour supprimer une seule ligne que vous avez déjà chargée, appelez plutôt `task.delete(&db).await?`.

> **Attention** `delete_many()` et `update_many()` sans aucun `filter` s'appliquent à **toutes les lignes** de la table. C'est parfois ce que vous voulez, donc Memoire l'autorise, mais vérifiez bien vos filtres.

## Étape 9 : jeter un œil au SQL

Curieux de savoir ce que Memoire envoie à la base de données ? N'importe quelle requête peut afficher son SQL :

```rust
let query = Task::query().filter(Task::DONE.eq(false)).order_by(Task::PRIORITY.desc()).limit(10);
println!("{}", query.to_sql(Dialect::Sqlite));
println!("{}", query.to_sql(Dialect::Postgres));
```

```text
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = ? ORDER BY "tasks"."priority" DESC LIMIT 10
SELECT "tasks"."id", "tasks"."title", "tasks"."priority", "tasks"."done", "tasks"."note" FROM "tasks" WHERE "tasks"."done" = $1 ORDER BY "tasks"."priority" DESC LIMIT 10
```

La valeur `false` ne figure pas dans le texte SQL : elle est envoyée séparément sous forme de paramètre (`?` ou `$1`). C'est ce qui vous protège des injections SQL.

## Étape 10 : enregistrer vos tâches dans un fichier SQLite

La base de données en mémoire disparaît quand le programme s'arrête. Conservons donc les tâches dans un fichier. Commencez par activer la feature `sqlite` :

```bash
cargo add memoire-orm --features sqlite
```

Déplacez maintenant la logique de l'application dans une fonction qui accepte *n'importe quelle* base de données. Le trait `Executor` est implémenté par tous les backends (et par les transactions) : la même fonction marche donc avec `MemoryDb`, `SqliteDb` et `PgPool`. Voici le programme complet :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tasks")]
pub struct Task {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub priority: i32,
    pub done: bool,
    pub note: Option<String>,
}

impl Task {
    fn new(title: &str, priority: i32) -> Self {
        Task { id: 0, title: title.to_string(), priority, done: false, note: None }
    }
}

/// Toute la logique de l'application. Elle fonctionne avec n'importe quelle base de données.
async fn run<E: Executor>(db: &E) -> memoire_orm::Result<()> {
    println!("tasks already stored: {}", Task::count(db).await?);

    for (title, priority) in [("Write the docs", 3), ("Fix the bike", 1), ("Call grandma", 2)] {
        let mut task = Task::new(title, priority);
        task.insert(db).await?;
    }

    Task::update_many()
        .set(Task::DONE, true)
        .filter(Task::TITLE.eq("Fix the bike"))
        .execute(db)
        .await?;

    let todo = Task::query()
        .filter(Task::DONE.eq(false))
        .order_by(Task::PRIORITY.desc())
        .order_by(Task::ID.asc())
        .all(db)
        .await?;
    for task in &todo {
        println!("[{}] {} (#{})", task.priority, task.title, task.id);
    }
    println!("tasks stored now: {}", Task::count(db).await?);
    Ok(())
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("tasks.db")?;
    db.create_table_if_not_exists::<Task>().await?;
    run(&db).await
}
```

Seul `main` mentionne SQLite. Deux détails ont changé par rapport à la version en mémoire :

- `SqliteDb::open("tasks.db")?` ouvre le fichier, en le créant si nécessaire.
- `create_table_if_not_exists` n'échoue pas quand la table existe déjà depuis une exécution précédente.

Lancez-le deux fois :

```text
$ cargo run
tasks already stored: 0
[3] Write the docs (#1)
[2] Call grandma (#3)
tasks stored now: 3

$ cargo run
tasks already stored: 3
[3] Write the docs (#1)
[3] Write the docs (#4)
[2] Call grandma (#3)
[2] Call grandma (#6)
tasks stored now: 6
```

La deuxième exécution a retrouvé les trois tâches enregistrées par la première : vos données vivent désormais dans `tasks.db`.

> **Astuce** Pour revenir au moteur en mémoire (dans les tests, par exemple), remplacez les deux lignes de `main` par `let db = MemoryDb::new(); db.create_table::<Task>().await?;`. Rien d'autre ne change.

### Utiliser une autre base de données

La même fonction `run` fonctionne avec tous les backends. Activez la feature, puis modifiez la ligne qui ouvre la base de données :

| Base de données | Feature | L'ouvrir dans `main` |
|---|---|---|
| En mémoire | `memory` (par défaut) | `let db = MemoryDb::new();` |
| SQLite | `sqlite` | `let db = SqliteDb::open("tasks.db")?;` |
| PostgreSQL | `postgres` | `let db = PgPool::connect("postgres://user:password@localhost/tasks").await?;` |
| MySQL / MariaDB | `mysql` | `let db = MySqlPool::connect("mysql://user:password@localhost:3306/tasks").await?;` |
| Redis | `redis` | `let db = RedisDb::connect("redis://127.0.0.1:6379").await?;` |
| MongoDB | `mongodb` | `let db = MongoDb::connect("mongodb://localhost:27017/tasks").await?;` |

Par exemple, avec MongoDB :

```rust
#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = MongoDb::connect("mongodb://localhost:27017/tasks").await?;
    db.create_table_if_not_exists::<Task>().await?;
    run(&db).await
}
```

Les backends serveur ont besoin d'un serveur en cours d'exécution ; la page de chaque backend explique comment en démarrer un en local.

## Ce que vous avez appris

| Tâche | Code |
|---|---|
| Décrire une table | `#[derive(Model)]` sur une struct |
| Créer la table | `db.create_table::<Task>().await?` |
| Insérer une ligne | `task.insert(&db).await?` |
| Lire par clé | `Task::get(&db, id)` ou `Task::find(&db, id)` |
| Rechercher | `Task::query().filter(...).order_by(...).all(&db)` |
| Modifier | `task.update(&db)` ou `Task::update_many()...execute(&db)` |
| Supprimer | `task.delete(&db)` ou `Task::delete_many()...execute(&db)` |
| Compter et agréger | `Task::count(&db)`, `.avg(Task::PRIORITY, &db)` |
| Inspecter le SQL | `query.to_sql(Dialect::Sqlite)` |

## Étapes suivantes

- [Modèles](models) : tous les attributs, types de colonnes et éléments générés.
- [Constructeur de requêtes](queries) : tous les filtres, tris, la pagination et les agrégats.
- [Transactions](transactions) : appliquez plusieurs modifications en mode tout ou rien.
- [Migrations](migrations) : faites évoluer votre schéma en toute sécurité au fil du temps.
