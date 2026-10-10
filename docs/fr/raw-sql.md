# SQL brut

Le constructeur de requêtes couvre les besoins du quotidien, mais il arrive que vous vouliez écrire le SQL vous-même : une fonctionnalité propre à une base de données, un rapport complexe, une auto-jointure, une commande de maintenance. Cette page montre comment exécuter du SQL brut en toute sécurité avec SQLite et PostgreSQL, et comment décoder ses résultats.

## Deux méthodes

Chaque backend SQL (et chaque transaction) dispose de deux méthodes, fournies par le trait `Executor` :

| Méthode | Pour | Renvoie |
|---|---|---|
| `db.raw::<T>(sql, &params)` | Les instructions qui renvoient des lignes (`SELECT`, `... RETURNING`) | `Vec<T>` |
| `db.raw_execute(sql, &params)` | Les instructions qui ne renvoient pas de lignes (`INSERT`, `UPDATE`, `DELETE`, DDL) | Le nombre de lignes affectées (`u64`) |

```rust
use memoire_orm::prelude::*;

#[tokio::main]
async fn main() -> memoire_orm::Result<()> {
    let db = SqliteDb::open_in_memory()?;
    db.raw_execute("CREATE TABLE cities (name TEXT NOT NULL, population INTEGER)", &[]).await?;

    let inserted = db
        .raw_execute(
            "INSERT INTO cities (name, population) VALUES (?, ?), (?, ?)",
            &params!["Paris", 2_100_000, "Lyon", 520_000],
        )
        .await?;
    assert_eq!(inserted, 2);

    let big: Vec<(String, i64)> = db
        .raw("SELECT name, population FROM cities WHERE population > ? ORDER BY name", &params![1_000_000])
        .await?;
    assert_eq!(big, vec![("Paris".to_string(), 2_100_000)]);
    Ok(())
}
```

## Les paramètres et `params!`

Ne collez jamais de valeurs dans le texte SQL. Écrivez des placeholders et passez les valeurs séparément : la base de données les reçoit en tant que données, elles ne peuvent donc jamais changer le sens de l'instruction (pas d'injection SQL, pas de problèmes de guillemets).

`params![...]` construit la liste des valeurs à partir de valeurs Rust ordinaires. Il accepte tout ce que Memoire sait stocker : nombres, `&str`, `String`, `bool`, `Option<T>`, slices d'octets, ainsi que les types `chrono`/`uuid`/`json` lorsque ces features sont activées. Utilisez `&[]` quand il n'y a aucun paramètre.

```rust
let values: Vec<Value> = params![42, "alice", Some(3.5), None::<i32>, true];
```

La syntaxe des placeholders dépend de la base de données :

| Base de données | Placeholders | Exemple |
|---|---|---|
| SQLite | `?` (ou numérotés `?1`, `?2`) | `WHERE age > ? AND name = ?` |
| PostgreSQL | `$1`, `$2`, ... | `WHERE age > $1 AND name = $2` |
| MySQL / MariaDB | `?` | Voir [MySQL & MariaDB](mysql) |

> **Attention** Les placeholders ne peuvent représenter que des **valeurs**, jamais des noms de tables ou de colonnes. Si un nom de table ou de colonne provient d'une saisie utilisateur, vérifiez-le par rapport à une liste fixe de noms autorisés avant de l'insérer dans le texte SQL.

## Décoder les résultats

Les lignes sont décodées **par position** : la première colonne du résultat va dans le premier élément ou champ, et ainsi de suite. Choisissez le type qui correspond à la forme de votre résultat :

| Type | À utiliser pour |
|---|---|
| `i64`, `String`, `f64`, `bool`, ... | Une seule colonne |
| `Option<T>` | Une seule colonne qui peut être `NULL` |
| Un tuple comme `(i64, String, Option<f64>)` | Plusieurs colonnes (jusqu'à 8) |
| Un modèle comme `User` | Toutes les colonnes du modèle, dans l'ordre de déclaration |
| Une struct `#[derive(FromRow)]` | Des champs nommés pour un résultat personnalisé |
| `Value` | N'importe quelle valeur unique, quel que soit son type (`Value::I64`, `Value::Text`, `Value::Null`, ...) |

Les exemples ci-dessous utilisent ce modèle :

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "employees")]
pub struct Employee {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    pub manager_id: Option<i64>,
    pub salary: i64,
}

#[derive(FromRow, Debug)]
pub struct TeamSize {
    pub manager: String,
    pub reports: i64,
}
```

### Scalaires et tuples

```rust
let total: Vec<i64> = db.raw("SELECT COUNT(*) FROM employees", &[]).await?;

let names: Vec<(i64, String)> = db
    .raw("SELECT id, name FROM employees WHERE salary >= ? ORDER BY id", &params![50_000])
    .await?;
```

### Modèles complets

Pour décoder dans un modèle, sélectionnez ses colonnes dans l'ordre de déclaration. La liste de colonnes pré-générée du modèle (voir [Modèles](models)) vous évite de les taper :

```rust
let columns = Employee::SCHEMA.sql(Dialect::Sqlite).columns;
// "employees"."id", "employees"."name", "employees"."manager_id", "employees"."salary"

let sql = format!("SELECT {columns} FROM employees WHERE salary > ? ORDER BY salary DESC");
let well_paid: Vec<Employee> = db.raw(&sql, &params![60_000]).await?;
```

### Structs personnalisées

`#[derive(FromRow)]` donne des noms lisibles à un résultat personnalisé. Ici, une auto-jointure, que le constructeur de requêtes ne sait pas exprimer, compte les subordonnés de chaque responsable :

```rust
let teams: Vec<TeamSize> = db
    .raw(
        "SELECT m.name, COUNT(e.id)
         FROM employees m
         JOIN employees e ON e.manager_id = m.id
         GROUP BY m.id, m.name
         ORDER BY m.name",
        &[],
    )
    .await?;
for team in &teams {
    println!("{} manages {} people", team.manager, team.reports);
}
```

### `NULL` et erreurs de type

- Lire un `NULL` dans un type qui n'est pas une `Option` échoue avec `Error::UnexpectedNull` : utilisez `Option<T>` pour les colonnes nullables et pour les résultats de `LEFT JOIN`.
- Une valeur qui ne peut pas être convertie (par exemple du texte en `i64`) échoue avec `Error::Decode`, qui indique la position de la colonne (`#0`, `#1`, ...).
- Les tailles d'entiers sont souples : une colonne `INTEGER` peut être lue comme `i32` ou `i64` tant que la valeur tient dans le type.

## Le SQL brut dans les transactions

Une transaction est elle aussi un `Executor` : `raw` et `raw_execute` fonctionnent donc à l'intérieur et en font partie :

```rust
let tx = db.begin().await?;
tx.raw_execute("UPDATE employees SET salary = salary + ? WHERE manager_id IS NULL", &params![1_000])
    .await?;
Employee::update_many().increment(Employee::SALARY, 500).execute(&tx).await?;
tx.commit().await?;
```

## Remarques sur PostgreSQL

Le même code fonctionne sur PostgreSQL avec des placeholders `$n` :

```rust
async fn pg_example(db: &PgPool) -> memoire_orm::Result<()> {
    let rows: Vec<(String, i64)> = db
        .raw("SELECT name, salary FROM employees WHERE salary > $1 ORDER BY name", &params![50_000])
        .await?;
    println!("{rows:?}");
    Ok(())
}
```

Quelques règles de typage utiles à connaître :

- Les paramètres entiers s'adaptent à la colonne : `params![7]` peut être comparé à un `SMALLINT`, un `INTEGER` ou un `BIGINT` (avec une vérification de plage).
- `COUNT(*)` est un `BIGINT` : lisez-le comme un `i64`. Le `SUM` d'une colonne `BIGINT` est un `NUMERIC`.
- Les valeurs `NUMERIC` sont décodées sous forme de texte sans perte de précision : lisez-les comme `String` (exact) ou `f64`. Vous pouvez aussi faire un cast en SQL : `SUM(salary)::bigint`.
- Quand PostgreSQL ne peut pas deviner le type d'un paramètre, ajoutez un cast : `SELECT $1::text`.
- Un type que Memoire ne connaît pas (par exemple `INTERVAL`) provoque une erreur : convertissez-le en texte dans le SQL (`duration::text`).

## Non disponible sur le moteur en mémoire

`MemoryDb` ne comprend pas le SQL : `raw` et `raw_execute` renvoient `Error::Unsupported`. Le code qui doit aussi tourner sur le moteur en mémoire (tests, WebAssembly) doit s'en tenir au constructeur de requêtes.

## Portes de sortie

Quand vous avez besoin d'une fonctionnalité du driver sous-jacent lui-même, les deux backends SQL vous permettent d'y accéder.

### La connexion SQLite

`SqliteDb::with_connection` exécute une closure avec la connexion `rusqlite`, par exemple pour lire un pragma ou lancer des commandes de maintenance :

```rust
async fn maintenance(db: &SqliteDb) -> memoire_orm::Result<()> {
    let mode: String = db
        .with_connection(|conn| conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)))
        .await
        .map_err(Error::database)?;
    println!("journal mode: {mode}");

    db.with_connection(|conn| conn.execute_batch("VACUUM")).await.map_err(Error::database)?;
    Ok(())
}
```

`Error::database` enveloppe n'importe quelle erreur du driver dans `Error::Database`. N'appelez pas de méthodes de `db` depuis la closure : la connexion y est déjà utilisée.

### Le pool PostgreSQL

`PgPool::pool()` renvoie le `deadpool_postgres::Pool` sous-jacent, pour la supervision ou pour des fonctionnalités de `tokio-postgres` comme `LISTEN`/`NOTIFY` ou `COPY` :

```rust
async fn pool_tools(db: &PgPool) -> memoire_orm::Result<()> {
    let status = db.pool().status();
    println!("{} connection(s) open, {} idle, max {}", status.size, status.available, status.max_size);

    let client = db.pool().get().await.map_err(|e| Error::Pool(e.to_string()))?;
    client.batch_execute("SET application_name = 'my-app'").await.map_err(Error::database)?;
    Ok(())
}
```

## Étapes suivantes

- [Constructeur de requêtes](queries) : la plupart des requêtes n'ont pas besoin de SQL brut.
- [SQLite](sqlite) et [PostgreSQL](postgres) : détails des backends et correspondance des types.
- [Migrations](migrations) : exécutez des scripts SQL sous forme de modifications de schéma versionnées.
