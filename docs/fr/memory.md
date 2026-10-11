# Moteur en mémoire

`MemoryDb` est un moteur de base de données écrit en Rust pur qui garde toutes les tables en mémoire. Il n'a besoin ni de fichier, ni de serveur, ni de runtime async, il exécute les mêmes requêtes que les autres backends, et il fonctionne même en WebAssembly. Il est activé par défaut (feature `memory`).

## Quand l'utiliser

| Idéal pour | Peu adapté à |
|---|---|
| Les tests unitaires et d'intégration : rapides, isolés, rien à nettoyer | Les données qui doivent survivre à un redémarrage (rien n'est écrit sur disque) |
| Les prototypes et les démos | Les données plus volumineuses que la mémoire disponible |
| Les caches et les données temporaires propres à un processus | Le partage de données entre plusieurs processus |
| Les applications web compilées en WebAssembly | Le SQL brut (il n'y a pas de parseur SQL) |

## Premiers pas

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub slug: String,
    pub text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;

        let mut note = Note { id: 0, slug: "hello".into(), text: "Hello!".into() };
        note.insert(&db).await?;

        let found = Note::query().filter(Note::SLUG.eq("hello")).one(&db).await?;
        println!("{}", found.text);
        Ok(())
    })
}
```

Aucun runtime n'est nécessaire : le moteur n'attend jamais d'E/S, donc `memoire_orm::block_on` exécute directement ses futures. Il fonctionne tout aussi bien dans Tokio ou dans n'importe quel autre exécuteur.

### Partager une base de données

Cloner un `MemoryDb` ne coûte presque rien, et chaque clone voit les mêmes données. Donnez un clone à chaque thread ou tâche :

```rust
let db = MemoryDb::new();
let for_worker = db.clone(); // mêmes tables, mêmes lignes
```

Les données vivent tant qu'il existe au moins un clone. La base peut être utilisée sans risque depuis plusieurs threads à la fois : les lectures s'exécutent en parallèle et les écritures sont sérialisées.

### Inspecter la base de données

| Méthode | Renvoie |
|---|---|
| `db.table_names()` | Les noms de toutes les tables, triés |
| `db.row_count("notes")` | Le nombre de lignes d'une table (`Error::NoSuchTable` si elle n'existe pas) |
| `format!("{db:?}")` | Chaque table avec son nombre de lignes, pratique quand un test échoue |

```rust
println!("{:?}", db.table_names());   // ["notes"]
println!("{}", db.row_count("notes")?); // 1
println!("{db:?}");                    // MemoryDb { tables: [("notes", 1)] }
```

## Comment ça marche

Le moteur ne génère ni n'analyse de SQL. Il interprète directement les structures de requête de Memoire, avec des algorithmes choisis pour leur rapidité :

| Opération | Stratégie |
|---|---|
| `find`, `get`, `update`, `delete` par clé | Index de hachage sur la clé primaire : temps constant |
| Filtres comme `eq` ou `is_in` sur la clé primaire, une colonne `unique` ou `index` | Recherche dans l'index de hachage au lieu d'un parcours complet |
| Autres filtres | Une passe vectorisée : la condition est découpée en ses termes `AND`, et chacun filtre les lignes avec une boucle spécialisée pour sa colonne et sa constante ; les colonnes numériques sont lues dans une copie typée contiguë (environ 1 ns par ligne). Les autres conditions (`OR`, `LIKE`, fonctions…) sont compilées une fois par requête |
| Jointures sur `a.column = b.column` | Hash join (ou recherche directe quand la colonne jointe est une clé, une colonne `unique` ou `index`) |
| Autres conditions de jointure | Boucles imbriquées |
| `order_by` + `limit` | Sélection partielle des premières lignes sur des clés entières compactes, au lieu de tout trier |
| `count` | Compte les lignes qui correspondent sans les construire |
| `group_by`, `distinct` | Tables de hachage |
| Transactions | Snapshots en copy-on-write |

Le backend [Redis](redis) évalue les requêtes avec ce même moteur, et [MongoDB](mongodb) l'utilise pour terminer les jointures, les `GROUP BY` et les agrégats : ils partagent donc sa sémantique.

> **Remarque** Les colonnes `#[memoire(index)]` ont ici aussi un index de hachage, comme sur les bases de données SQL et MongoDB. Les index ne stockent que des positions de lignes : les clés ne sont jamais copiées.

## Sémantique SQL

Le moteur suit les règles du SQL : vos tests sur `MemoryDb` se comportent donc comme la production sur les autres backends :

- **Logique à trois valeurs :** les comparaisons avec `NULL` ne sont ni vraies ni fausses ; utilisez `is_null()`.
- **Tri des NULL comme dans PostgreSQL :** les NULL arrivent en dernier dans l'ordre croissant et en premier dans l'ordre décroissant (`nulls_first()`/`nulls_last()` permettent de changer ce comportement).
- **`LIKE` sensible à la casse**, comme SQLite (tel que configuré par Memoire) et PostgreSQL ; `ilike` et `icontains` ignorent la casse, y compris pour les lettres accentuées.
- **Les agrégats** ignorent les valeurs `NULL` ; le `SUM` d'entiers est un `i64`, `AVG` est un `f64`.
- **Contraintes :** les clés primaires, les colonnes `unique` et `NOT NULL` (champs qui ne sont pas des `Option`) sont appliquées, avec les mêmes erreurs que sur les bases de données SQL (`Error::UniqueViolation`, `Error::Constraint`).
- **Écritures atomiques :** un `update_many` multi-lignes, un lot d'`insert_many` ou un `delete_many` réussit entièrement ou ne modifie rien.

Les différences avec les bases de données SQL à garder en tête :

| Fonctionnalité | Moteur en mémoire |
|---|---|
| Clés étrangères (`references`) | Acceptées mais **non appliquées** ; `on_delete = "cascade"` n'a aucun effet |
| `default = "..."` | Ignoré (de toute façon, les insertions de Memoire fournissent toujours toutes les valeurs) |
| SQL brut (`raw`, `raw_execute`) | `Error::Unsupported` |
| Migrations | Uniquement les étapes `create_table` et `drop_table` ; `sql` échoue, `sql_for` est ignorée |
| Auto-jointures (une table deux fois dans une même requête) | `Error::Unsupported` |
| Persistance | `MemoryDb::new()` : aucune, les données disparaissent avec le dernier clone. `MemoryDb::open(path)` : enregistrées dans un fichier (plus bas) |

## Enregistrer les données dans un fichier

`MemoryDb::open(path)` garde tout ce qui rend le moteur rapide (données en mémoire, index, transactions par instantanés) et écrit en plus chaque modification validée dans un fichier, relu à la prochaine ouverture :

```rust
let db = MemoryDb::open("app.memoire")?;
db.create_table_if_not_exists::<Note>().await?;
```

- **Ce qui est écrit :** un petit enregistrement par ligne modifiée, ajouté quand l'instruction ou la transaction est validée. Une transaction annulée n'écrit rien.
- **Crashs :** chaque enregistrement porte une somme de contrôle. Si le programme s'arrête au milieu d'une écriture, l'enregistrement incomplet est écarté au prochain `open` ; tout ce qui précède est conservé.
- **Durabilité :** par défaut, une écriture rend la main dès que le système d'exploitation a les données : elle survit à un crash du programme, pas toujours à une coupure de courant. `MemoryDb::open_with(path, StorageOptions::new().sync(true))` attend aussi le disque après chaque écriture (bien plus lent : environ 0,15 ms par écriture sur la machine de test, contre 2 à 3 µs).
- **Taille :** le fichier grossit à chaque modification. `db.compact()` le réécrit avec seulement les lignes actuelles ; cela se fait aussi tout seul quand le fichier contient bien plus de modifications que de lignes. Rouvrir un fichier de 20 000 lignes prend environ 40 ms.
- **Modèles :** les tables reviennent la première fois que leur modèle est utilisé. Les colonnes sont associées par nom : ajouter un champ `Option` conserve les lignes (la nouvelle colonne vaut `NULL`), retirer un champ supprime cette colonne. Un nouveau champ qui n'est pas un `Option` demande une migration des données.
- **Un seul processus à la fois :** deux programmes ne doivent pas ouvrir le même fichier en même temps.

### Chiffrement

Avec la feature `encryption`, le fichier peut être chiffré avec un mot de passe :

```rust
use memoire_orm::StorageOptions;

let db = MemoryDb::open_with("secrets.memoire", StorageOptions::new().password(password))?;
```

Chaque enregistrement est chiffré en AES-256-GCM, avec une clé dérivée du mot de passe par Argon2id et un sel aléatoire conservé dans le fichier. Ouvrir le fichier avec un autre mot de passe échoue avec « wrong password », et rien n'y est lisible sans le mot de passe. Perdre le mot de passe, c'est perdre les données.

## Transactions

Les transactions sont des snapshots. `db.begin()` donne à la transaction sa propre vue de la base de données ; les tables sont copiées à la demande, uniquement lorsque la transaction y écrit pour la première fois. Les modifications deviennent visibles pour les autres au moment du `commit`, toutes en même temps.

Si une autre écriture a modifié une table que la transaction a elle aussi modifiée, `commit` échoue avec `Error::Conflict` et rien n'est appliqué : vous pouvez donc réessayer sans risque. Les transactions en lecture seule n'entrent jamais en conflit. Consultez [Transactions](transactions) pour un exemple de boucle de nouvelle tentative.

```rust
let tx = db.begin().await?;
Note { id: 0, slug: "draft".into(), text: "...".into() }.insert(&tx).await?;
assert_eq!(Note::count(&tx).await?, 2); // la transaction voit sa propre insertion
assert_eq!(Note::count(&db).await?, 1); // les autres, pas encore
tx.commit().await?;
assert_eq!(Note::count(&db).await?, 2);
```

## Tester avec `MemoryDb`

Écrivez le code de votre application en vous appuyant sur le trait `Executor` (ou `Database`), puis testez-le avec un `MemoryDb` tout neuf pour chaque test : chaque test obtient sa propre base de données isolée en quelques microsecondes.

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

/// Code de l'application : fonctionne avec n'importe quelle base de données.
pub async fn register<E: Executor>(db: &E, email: &str) -> memoire_orm::Result<User> {
    let mut user = User { id: 0, email: email.to_lowercase() };
    user.insert(db).await?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> memoire_orm::Result<MemoryDb> {
        let db = MemoryDb::new();
        db.create_table::<User>().await?;
        Ok(db)
    }

    #[test]
    fn emails_are_unique_ignoring_case() {
        memoire_orm::block_on(async {
            let db = test_db().await?;
            register(&db, "ada@example.com").await?;
            let err = register(&db, "ADA@example.com").await.unwrap_err();
            assert!(err.is_unique_violation());
            Ok::<(), Error>(())
        })
        .unwrap();
    }
}
```

> **Astuce** De simples fonctions `#[test]` avec `block_on` suffisent pour le moteur en mémoire ; `#[tokio::test]` fonctionne aussi si vous préférez.

## WebAssembly

Le moteur en mémoire compile vers `wasm32-unknown-unknown` sans aucune configuration supplémentaire, ce qui permet de faire tourner un vrai moteur de requêtes dans le navigateur :

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

Dans le navigateur, pilotez les futures avec `memoire_orm::block_on` (le moteur n'attend jamais réellement, donc il ne bloque pas) ou avec `wasm_bindgen_futures::spawn_local`. Seule la feature `memory` fonctionne sur `wasm32` : laissez `sqlite` et `postgres` désactivées pour cette cible.

> **Remarque** Sur `wasm32-unknown-unknown`, il n'y a pas d'horloge système : la colonne `applied_at` écrite par les [migrations](migrations) vaut donc `0`.

## Étapes suivantes

- [SQLite](sqlite) : la même API, avec des données enregistrées dans un fichier.
- [Transactions](transactions) : snapshots, conflits et nouvelles tentatives en détail.
- [Pourquoi c'est rapide](performance) : ce qui rend le moteur rapide.
