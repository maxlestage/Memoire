# Migrations

Les migrations sont des modifications numérotées et ordonnées de votre schéma de base de données : « 1 : créer la table users », « 2 : ajouter une colonne bio », et ainsi de suite. Memoire enregistre celles qui ont déjà été exécutées, de sorte que chaque base de données (votre portable, vos tests, la production) atteint le même schéma en n'appliquant que ce qui lui manque.

## Votre première migration

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub user_id: i64,
    pub title: String,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<User>())
        .add(Migration::new(2, "create_posts").create_table::<Post>())
}

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open("app.db")?;

    let applied = migrations().run(&db).await?;
    println!("applied migrations: {applied:?}");

    // ... le reste de votre programme ...
    Ok(())
}
```

La première exécution affiche `applied migrations: [1, 2]`. Toutes les exécutions suivantes affichent `applied migrations: []`, car les deux migrations sont déjà enregistrées. Lancer le migrateur au démarrage est donc sans risque, et c'est la configuration la plus courante.

Les briques de base :

- `Migration::new(version, name)` crée une migration. La version est un nombre unique (`i64`) qui détermine l'ordre ; le nom est un libellé lisible par un humain.
- `Migrator::new().add(...)` rassemble les migrations. L'ordre des appels à `add` n'a pas d'importance : les migrations s'exécutent toujours par ordre croissant de version.
- `migrator.run(&db)` applique les migrations en attente et renvoie les versions qu'il a appliquées.

## Les étapes d'une migration

Une migration est une liste d'étapes, exécutées dans l'ordre où vous les ajoutez :

| Étape | Ce qu'elle fait | Moteur en mémoire |
|---|---|---|
| `.create_table::<M>()` | `CREATE TABLE` (et ses index) pour le modèle `M` | Pris en charge |
| `.drop_table("name")` | `DROP TABLE name` | Pris en charge |
| `.sql("...")` | Exécute un script SQL sur n'importe quelle base de données SQL | **Échoue** avec `Error::Unsupported` |
| `.sql_for(Dialect::Postgres, "...")` | Exécute un script SQL uniquement sur cette base de données ; ignorée ailleurs | Ignorée |

Une migration peut combiner plusieurs étapes :

```rust
let release_2 = Migration::new(3, "comments_and_cleanup")
    .create_table::<Comment>()
    .drop_table("legacy_comments")
    .sql_for(Dialect::Postgres, "CREATE INDEX idx_posts_title ON posts (title)")
    .sql_for(Dialect::Sqlite, "CREATE INDEX idx_posts_title ON posts (title)");
```

Un script `sql` ou `sql_for` peut contenir plusieurs instructions séparées par des points-virgules.

## Modifier une table par la suite

Pour modifier une table existante, ajoutez une **nouvelle** migration contenant le SQL de la modification. Ici, nous ajoutons une colonne `bio`. `sql_for` vous permet d'écrire l'instruction pour chaque base de données que vous prenez en charge :

```rust
let add_bio = Migration::new(4, "add_user_bio")
    .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
    .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT");
```

Ajoutez ensuite le champ à la struct (`pub bio: Option<String>`), pour que le modèle corresponde au nouveau schéma.

> **Attention** Ne modifiez jamais une migration qui a déjà été exécutée quelque part : les bases de données qui l'ont appliquée ne la rejoueront pas, et votre modification ne les atteindrait donc jamais. Ajoutez toujours une nouvelle migration avec une version plus élevée.

### Figer la forme des anciennes tables

`create_table::<User>()` crée la table à partir de la struct **telle qu'elle est dans votre code actuel**. Une fois `bio` ajouté à `User`, la migration 1 créerait `users` avec une colonne `bio` sur une base de données toute neuve, et la migration 4 échouerait alors parce que la colonne existe déjà.

La solution consiste à garder une copie figée de la struct telle qu'elle était quand la migration 1 a été écrite, et à utiliser cette copie dans l'ancienne migration :

```rust
/// `users` tel qu'il était dans la migration 1. Ne modifiez jamais ce module.
mod v1 {
    use memoire_orm::prelude::*;

    #[derive(Model)]
    #[memoire(table = "users")]
    pub struct User {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        #[memoire(unique)]
        pub email: String,
    }
}

/// Le modèle actuel, utilisé par l'application.
#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub bio: Option<String>,
}

fn migrations() -> Migrator {
    Migrator::new()
        .add(Migration::new(1, "create_users").create_table::<v1::User>())
        .add(
            Migration::new(2, "add_user_bio")
                .sql_for(Dialect::Sqlite, "ALTER TABLE users ADD COLUMN bio TEXT")
                .sql_for(Dialect::Postgres, "ALTER TABLE users ADD COLUMN bio TEXT"),
        )
}
```

Désormais, une nouvelle base de données et une ancienne aboutissent toutes deux au même schéma. Vous pouvez aussi coller l'instruction `CREATE TABLE` produite par `memoire_orm::sql::create_table` dans une étape `sql_for`.

## Où Memoire enregistre les migrations

Les migrations appliquées sont stockées dans une table appelée `_memoire_migrations`, créée automatiquement la première fois que vous appelez `run`, `pending` ou `applied` :

| Colonne | Type | Contenu |
|---|---|---|
| `version` | `BIGINT` (clé primaire) | La version de la migration |
| `name` | `TEXT` | Le nom de la migration |
| `applied_at` | `BIGINT` | Le moment de son exécution, en millisecondes depuis l'epoch Unix |

Vous pouvez l'inspecter depuis Rust :

```rust
async fn status<D: Database>(db: &D) -> memoire_orm::Result<()> {
    let migrator = migrations();

    for done in migrator.applied(db).await? {
        println!("applied: {} {} (at {} ms)", done.version, done.name, done.applied_at);
    }
    for todo in migrator.pending(db).await? {
        println!("pending: {} {}", todo.version(), todo.name());
    }
    Ok(())
}
```

`applied` renvoie des lignes `memoire_orm::migrate::AppliedMigration` ; `pending` renvoie les `Migration` qui n'ont pas encore été exécutées, triées par version.

> **Remarque** Seule la version sert à déterminer si une migration a été exécutée. Renommer une migration ne la fait pas s'exécuter à nouveau.

## Comment fonctionne `run` et ce qui se passe en cas d'échec

`run` procède ainsi :

1. Il vérifie que deux migrations ne partagent pas la même version (sinon il échoue avant de toucher à la base de données).
2. Il lit `_memoire_migrations` pour trouver les migrations en attente.
3. Pour chaque migration en attente, par ordre croissant de version, il ouvre une **transaction**, exécute toutes les étapes, enregistre la migration dans `_memoire_migrations` et valide (commit).
4. Il renvoie la liste des versions appliquées.

Si une étape échoue :

- la transaction de cette migration est annulée : aucune de ses étapes ni aucun enregistrement n'est conservé ;
- les migrations appliquées avant elle restent appliquées (elles ont été validées) ;
- les migrations suivantes ne sont pas tentées ;
- `run` renvoie `Error::Migration { version, name, source }`, où `source` est l'erreur sous-jacente.

```rust
match migrations().run(&db).await {
    Ok(applied) => println!("schema up to date (applied {applied:?})"),
    Err(Error::Migration { version, name, source }) => {
        eprintln!("migration {version} ({name}) failed: {source}");
    }
    Err(other) => return Err(other),
}
```

Corrigez la migration et relancez : l'exécution reprend là où elle s'était arrêtée.

> **Astuce** SQLite et PostgreSQL savent annuler les modifications de schéma (`CREATE TABLE`, `ALTER TABLE`...) à l'intérieur d'une transaction : une migration qui échoue ne laisse donc derrière elle aucun schéma à moitié appliqué.

## Les migrations et le moteur en mémoire

Le moteur en mémoire n'a pas de SQL : il ne comprend donc que les étapes `create_table` et `drop_table`. Une étape `sql` fait échouer la migration avec `Error::Unsupported`, et les étapes `sql_for` sont ignorées.

Pour les tests unitaires sur `MemoryDb`, l'approche la plus simple consiste généralement à créer les tables directement à partir de vos modèles actuels au lieu de lancer le migrateur :

```rust
async fn test_db() -> memoire_orm::Result<MemoryDb> {
    let db = MemoryDb::new();
    db.create_table::<User>().await?;
    db.create_table::<Post>().await?;
    Ok(db)
}
```

## Étapes suivantes

- [Transactions](transactions) : comment se comporte la transaction qui entoure chaque migration.
- [SQL brut](raw-sql) : exécutez du SQL en dehors des migrations.
- [Modèles](models) : les attributs qui façonnent le `CREATE TABLE` généré.
