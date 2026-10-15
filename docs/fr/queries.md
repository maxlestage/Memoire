# Constructeur de requêtes

Le constructeur de requêtes vous permet de décrire un `SELECT` en Rust à l'aide de colonnes typées. Cette page passe en revue les filtres, les tris, la pagination, les méthodes de récupération, les agrégats, les projections et les rapports `GROUP BY`, et montre le SQL que Memoire génère pour chaque base de données.

## Les exemples de cette page

Tous les exemples utilisent ces deux modèles et ces données :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
    pub score: f64,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", index)]
    pub user_id: i64,
    pub title: String,
    pub views: i32,
}
```

| id | name | age | active | score |
|---|---|---|---|---|
| 1 | Alice | 31 | true | 1.5 |
| 2 | Bob | 17 | true | 2.5 |
| 3 | Carol | NULL | false | 3.0 |
| 4 | Dave | 45 | true | 4.0 |
| 5 | Eve | 22 | false | 0.5 |

Posts : Alice (utilisateur 1) a écrit "Hello" (10 vues) et "Rust" (30 vues), Bob (utilisateur 2) a écrit "Bob's" (5 vues) et Dave (utilisateur 4) a écrit "Dave" (0 vue).

## Construire une requête

Toute requête commence par `Model::query()`, qui signifie « toutes les lignes de cette table ». Chaque méthode renvoie une nouvelle requête, plus précise, et rien ne touche la base de données tant que vous n'appelez pas une méthode de récupération comme `.all(&db)` :

```rust
let adults = User::query()
    .filter(User::AGE.ge(18))
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
// Alice, Dave, Eve
```

Les requêtes sont de simples valeurs : vous pouvez les stocker, les passer d'une fonction à l'autre et les `clone()` pour réutiliser une base commune :

```rust
let base = User::query().filter(User::ACTIVE.eq(true));
let young = base.clone().filter(User::AGE.lt(30)).all(&db).await?; // Bob
let total = base.count(&db).await?; // 3
```

## Filtrer

### Comparaisons

| Méthode | SQL | Exemple |
|---|---|---|
| `eq(v)` | `column = v` | `User::NAME.eq("Bob")` |
| `ne(v)` | `column <> v` | `User::NAME.ne("Bob")` |
| `lt(v)` | `column < v` | `User::AGE.lt(18)` |
| `le(v)` | `column <= v` | `User::AGE.le(18)` |
| `gt(v)` | `column > v` | `User::SCORE.gt(2.0)` |
| `ge(v)` | `column >= v` | `User::AGE.ge(18)` |

La valeur doit correspondre au type Rust de la colonne. Pour les colonnes de texte, vous pouvez passer `&str`, `&String` ou `String`.

### Combiner des conditions

Utilisez `&` pour AND, `|` pour OR et `!` pour NOT. Les parenthèses fonctionnent comme en Rust classique :

```rust
let q = User::query().filter(User::AGE.ge(18) & (User::NAME.eq("Alice") | User::NAME.eq("Eve")));
// Alice, Eve

let inactive = User::query().filter(!User::ACTIVE.eq(true)).all(&db).await?;
// Carol, Eve
```

Appeler `filter` plusieurs fois combine les conditions avec AND : ces deux requêtes sont donc identiques :

```rust
let a = User::query().filter(User::ACTIVE.eq(true)).filter(User::AGE.ge(18)).all(&db).await?;
let b = User::query().filter(User::ACTIVE.eq(true) & User::AGE.ge(18)).all(&db).await?;
assert_eq!(a, b);
```

`or_filter` combine avec OR : il signifie « tout ce qui a été filtré jusqu'ici, **ou** ceci » :

```rust
let q = User::query()
    .filter(User::ACTIVE.eq(true))
    .filter(User::AGE.ge(40))
    .or_filter(User::NAME.eq("Eve"));
// WHERE (("users"."active" = ? AND "users"."age" >= ?) OR "users"."name" = ?)
// Dave, Eve
```

> **Astuce** Quand une condition mélange AND et OR, l'écrire dans un seul `filter(...)` avec des parenthèses explicites est généralement le plus lisible.

### Listes et intervalles

```rust
let some = User::query().filter(User::ID.is_in([1, 3, 5])).all(&db).await?;
// Alice, Carol, Eve

let wanted = vec!["Bob".to_string(), "Eve".to_string()];
let others = User::query().filter(User::NAME.not_in(&wanted)).all(&db).await?;
// Alice, Carol, Dave

let twenties = User::query().filter(User::AGE.between(20, 29)).all(&db).await?;
// Eve
```

| Méthode | SQL | Remarques |
|---|---|---|
| `is_in(values)` | `column IN (...)` | Accepte les tableaux, les vecteurs, les slices et les itérateurs. Une liste vide ne correspond à rien |
| `not_in(values)` | `column NOT IN (...)` | Une liste vide correspond à tout |
| `between(low, high)` | `column BETWEEN low AND high` | Les deux bornes sont incluses |
| `not_between(low, high)` | `column NOT BETWEEN low AND high` | |

### Valeurs NULL

`NULL` signifie « inconnu », et les comparaisons SQL avec une valeur inconnue ne sont ni vraies ni fausses. Utilisez les méthodes dédiées :

```rust
let unknown = User::query().filter(User::AGE.is_null()).all(&db).await?; // Carol
let known = User::query().filter(User::AGE.is_not_null()).count(&db).await?; // 4
```

> **Attention** `!User::AGE.ge(18)` ne renvoie que Bob, et **pas** Carol : son âge est `NULL`, donc « age >= 18 » est inconnu, et sa négation aussi. Si vous la voulez également, dites-le explicitement : `!User::AGE.ge(18) | User::AGE.is_null()`. Vous ne pouvez pas écrire `User::AGE.eq(None)` ; utilisez `is_null()` à la place.

### Recherche dans le texte

| Méthode | Correspond à | Casse |
|---|---|---|
| `like(pattern)` | Motif SQL : `%` représente n'importe quel texte, `_` un seul caractère | Sensible |
| `not_like(pattern)` | L'inverse de `like` | Sensible |
| `ilike(pattern)` | Comme `like` | Insensible |
| `contains(text)` | `text` n'importe où | Sensible |
| `icontains(text)` | `text` n'importe où | Insensible |
| `starts_with(text)` | Les valeurs qui commencent par `text` | Sensible |
| `ends_with(text)` | Les valeurs qui se terminent par `text` | Sensible |

```rust
let bob = User::query().filter(User::EMAIL.like("_ob@%")).all(&db).await?;
let carol = User::query().filter(User::NAME.icontains("AR")).all(&db).await?;
let dave = User::query().filter(User::NAME.starts_with("Da")).all(&db).await?;
let all = User::query().filter(User::EMAIL.ends_with("@example.com")).count(&db).await?; // 5
```

`contains`, `icontains`, `starts_with` et `ends_with` **échappent** `%`, `_` et `\` dans le texte que vous passez, de sorte que la saisie de l'utilisateur est recherchée littéralement. Chercher `"50%"` recherche vraiment les caractères `50%` :

```rust
let q = User::query().filter(User::NAME.contains("50%"));
// SQL :    ... WHERE "users"."name" LIKE $1 ESCAPE '\'
// valeur : "%50\%%"
```

> **Astuce** Pour les champs de recherche et toute autre saisie utilisateur, préférez `contains`, `icontains`, `starts_with` et `ends_with` à `like` : ils vous protègent des jokers accidentels.

Dans un motif `like` écrit à la main, `\` échappe le caractère suivant sur tous les backends (Memoire émet toujours `ESCAPE '\'`). Pour mélanger vos propres jokers avec une saisie utilisateur, échappez la saisie avec `memoire_orm::escape_like` :

```rust
let input = "50%";
let q = User::query().filter(User::NAME.like(format!("{}%", memoire_orm::escape_like(input))));
// trouve les noms qui commencent par les quatre caractères `50%`
```

La sensibilité à la casse est la même sur tous les backends gérés par Memoire :

| Backend | `like`, `contains`, ... | `ilike`, `icontains` |
|---|---|---|
| Moteur en mémoire | Sensible à la casse | Insensible à la casse (Unicode) |
| SQLite | Sensible à la casse (Memoire active `case_sensitive_like`) | Insensible à la casse pour les lettres ASCII uniquement (`LOWER()`) |
| PostgreSQL | Sensible à la casse | Insensible à la casse (`ILIKE` natif) |
| MySQL / MariaDB | Sensible à la casse (Memoire crée les tables avec `utf8mb4_bin`) | Insensible à la casse (`LOWER()`) |
| Redis | Sensible à la casse (même moteur que le moteur en mémoire) | Insensible à la casse (Unicode) |
| MongoDB | Sensible à la casse (expression régulière ancrée) | Insensible à la casse (expression régulière avec l'option `i`, Unicode) |

> **Remarque** Sur SQLite, `icontains("émi")` ne trouve pas `"Émile"`, car le `LOWER()` de SQLite ne modifie que les lettres ASCII. Tous les autres backends gèrent les lettres accentuées.

### Recherche plein texte

`matches` trouve les lignes dont le texte contient **tous les mots** d'une recherche, en mots entiers, sans tenir compte de la casse. Les mots sont des suites de lettres et de chiffres ; tout le reste (espaces, ponctuation, `_`, `@`...) les sépare.

```rust
let posts = Post::query()
    .filter(Post::BODY.matches("rust orm"))
    .order_by(Post::ID)
    .all(&db)
    .await?;
```

| Texte | `matches("rust orm")` |
|---|---|
| « An ORM for Rust! » | oui |
| « rust-orm » | oui |
| « Rusty ORM » | non (« Rusty » est un autre mot) |
| « Rust » | non (« orm » manque) |
| `NULL` | non |

Une recherche sans aucun mot (`""`, `"?!"`) ne trouve rien. `matches` se combine avec toutes les autres conditions, `!` compris : `!Post::BODY.matches("brouillon")` garde les textes sans ce mot.

La règle est la même sur toutes les bases ; seule la vitesse change :

| Base | Comment | Avec `#[memoire(fulltext)]` |
|---|---|---|
| Moteur en mémoire | Mots comparés en Rust | Index de mots : seules les lignes qui contiennent le mot le plus rare sont examinées |
| SQLite | Une fonction `memoire_match` enregistrée sur la connexion | Les lignes sont examinées une à une |
| PostgreSQL | `to_tsvector('simple', ...) @@ plainto_tsquery('simple', ...)` | Index GIN sur la même expression |
| MySQL / MariaDB | Un `REGEXP` par mot | Les lignes sont examinées une à une |
| Redis | Même moteur qu'en mémoire | Les lignes sont chargées puis examinées |
| MongoDB | Un `$regex` par mot, exécuté par le serveur | Les documents sont examinés un à un |

> **Remarque** Il n'y a ni racinisation ni mots vides : `matches("courir")` ne trouve pas « court ». Sur PostgreSQL, les lettres hors ASCII demandent une base avec une locale UTF-8 (le réglage par défaut presque partout, y compris l'image Docker officielle).

### Comparer deux colonnes

Chaque comparaison a une jumelle en `_col` qui compare à une autre colonne au lieu d'une valeur : `eq_col`, `ne_col`, `lt_col`, `le_col`, `gt_col`, `ge_col`. Les deux colonnes doivent avoir le même type Rust. Elles servent surtout dans les conditions de jointure, comme `Post::USER_ID.eq_col(User::ID)` (voir [Jointures et relations](relations)).

### Construire des filtres dynamiquement

Comme une requête n'est qu'une valeur, vous pouvez ajouter des filtres sous condition, par exemple à partir de paramètres de recherche facultatifs :

```rust
let min_age: Option<i32> = Some(18);
let search: Option<&str> = Some("a");

let mut query = User::query();
if let Some(min) = min_age {
    query = query.filter(User::AGE.ge(min));
}
if let Some(text) = search {
    query = query.filter(User::NAME.icontains(text));
}
let users = query.order_by(User::NAME.asc()).all(&db).await?;
// Alice, Dave
```

`Expr::all` et `Expr::any` relient une liste de conditions par AND ou par OR :

```rust
let names_wanted = ["Alice", "Dave"];
let any = Expr::any(names_wanted.iter().map(|n| User::NAME.eq(*n)));
let found = User::query().filter(any).all(&db).await?; // Alice, Dave

let conditions = vec![User::ACTIVE.eq(true), User::SCORE.gt(1.0)];
let found = User::query().filter(Expr::all(conditions)).all(&db).await?; // Alice, Bob, Dave
```

`Expr::all` d'une liste vide est toujours vrai, et `Expr::any` d'une liste vide est toujours faux.

### Requêtes conditionnelles et portées réutilisables

`when`, `when_some` et `apply` gardent la chaîne d'appels au lieu de réaffecter une variable, comme `when()` et les portées locales (local scopes) de Laravel :

```rust
fn active(q: Select<User>) -> Select<User> {
    q.filter(User::ACTIVE.eq(true)) // une « portée » réutilisable
}

let users = User::query()
    .apply(active)
    .when_some(min_age, |q, min| q.filter(User::AGE.ge(min)))
    .when_some(search, |q, text| q.filter(User::NAME.icontains(text)))
    .when(newest_first, |q| q.order_by(User::ID.desc()))
    .all(&db)
    .await?;
```

| Méthode | Effet |
|---|---|
| `apply(portée)` | `portée(requête)` : n'importe quelle `fn(Select<T>) -> Select<T>` |
| `when(condition, \|q\| ...)` | Applique la closure seulement si `condition` est vraie |
| `when_some(option, \|q, valeur\| ...)` | Applique la closure avec la valeur, seulement s'il y en a une |

`Select` est `memoire_orm::Select`.

## Trier

```rust
let by_age = User::query()
    .order_by(User::AGE.desc())
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
```

- `order_by(User::NAME)` trie par ordre croissant ; `.asc()` et `.desc()` rendent le sens explicite.
- Appelez `order_by` plusieurs fois pour départager les ex æquo : le premier appel donne la clé de tri principale.
- Sans `order_by`, la base de données renvoie les lignes dans un ordre quelconque. Triez toujours quand l'ordre compte (en particulier pour la pagination).

### Où vont les NULL ?

Les bases de données ne sont pas d'accord sur la place des valeurs `NULL` lors d'un tri :

| Backend | `ASC` | `DESC` |
|---|---|---|
| Moteur en mémoire | NULL en dernier | NULL en premier |
| PostgreSQL | NULL en dernier | NULL en premier |
| SQLite | NULL en premier | NULL en dernier |
| MySQL / MariaDB | NULL en premier | NULL en dernier |
| Redis | NULL en dernier | NULL en premier |
| MongoDB | NULL en premier | NULL en dernier |

Rendez-le explicite avec `nulls_first()` ou `nulls_last()` pour obtenir le même résultat partout :

```rust
let q = User::query().order_by(User::AGE.asc().nulls_first()).all(&db).await?;
// Carol, Bob, Eve, Alice, Dave sur tous les backends
```

## Limiter et paginer

| Méthode | Effet |
|---|---|
| `limit(n)` | Au plus `n` lignes |
| `offset(n)` | Saute les `n` premières lignes |
| `page(page, per_page)` | Raccourci pour `limit(per_page).offset((page - 1) * per_page)` ; les pages commencent à 1 |

```rust
let rows = User::query().order_by(User::ID).limit(2).offset(2).all(&db).await?; // Carol, Dave
let same = User::query().order_by(User::ID).page(2, 2).all(&db).await?;         // Carol, Dave
```

Pour obtenir une page ainsi que les informations dont un paginateur a besoin, utilisez `paginate`. Elle exécute deux requêtes (un `COUNT` et la page elle-même) et renvoie une `Page` :

```rust
let page = User::query().order_by(User::ID).paginate(&db, 2, 2).await?;
println!(
    "page {} of {}: {} users (total {})",
    page.page,
    page.total_pages(),
    page.items.len(),
    page.total,
);
// page 2 of 3: 2 users (total 5)
```

| Élément de `Page` | Signification |
|---|---|
| `items` | Les lignes de cette page |
| `page` | Le numéro de la page (à partir de 1 ; `0` est traité comme `1`) |
| `per_page` | La taille de la page |
| `total` | Nombre de lignes correspondantes, toutes pages confondues |
| `total_pages()` | Nombre de pages |
| `has_next()` / `has_prev()` | Indique s'il existe une page après / avant celle-ci |

### Pagination par curseur

Les grands offsets deviennent lents, car la base parcourt quand même chaque ligne sautée. `cursor_paginate` pagine plutôt par clé primaire (`WHERE id > <curseur> ORDER BY id LIMIT n`), comme `cursorPaginate` de Laravel : chaque page coûte autant que la première, et les lignes insérées entre-temps ne décalent pas les pages. Idéal pour le « défilement infini » et les API :

```rust
let first = User::query().filter(User::ACTIVE.eq(true)).cursor_paginate(&db, 20, None).await?;
// first.items : jusqu'à 20 utilisateurs ; first.next_cursor : Some(clé du dernier), ou None à la fin
if first.has_more() {
    let next = User::query().filter(User::ACTIVE.eq(true)).cursor_paginate(&db, 20, first.next_cursor).await?;
}
```

Le curseur est une `Value` (la clé primaire de la dernière ligne) : envoyez-la au client, puis reconvertissez-la en `Value` (`Value::I64(id)`) à la requête suivante. Les `ORDER BY` et `OFFSET` de la requête sont remplacés par l'ordre des clés.

### Lire par paquets

Pour traiter une grande table sans la charger d'un coup (un export, une migration de données), lisez-la par paquets, par clé primaire, comme `chunkById` de Laravel ou `toIterable()` de Doctrine :

```rust
let mut chunks = User::query().filter(User::ACTIVE.eq(true)).chunks(500);
while let Some(users) = chunks.next(&db).await? {
    for user in users {
        // au plus 500 utilisateurs en mémoire à la fois
    }
}

// Ou avec une closure ; renvoie le nombre de lignes lues.
let n = User::query().chunks(500).for_each(&db, |users| { /* ... */ Ok(()) }).await?;
```

Chaque paquet est une requête qui commence après la dernière clé du précédent : modifier ou supprimer des lignes pendant la lecture n'en saute ni n'en répète aucune.

## Exécuter la requête

| Méthode | Renvoie | Remarques |
|---|---|---|
| `all(&db)` | `Vec<T>` | Toutes les lignes correspondantes |
| `first(&db)` | `Option<T>` | La première ligne ; ajoute `LIMIT 1` |
| `one(&db)` | `T` | La première ligne, ou `Error::NotFound` |
| `count(&db)` | `u64` | Nombre de lignes correspondantes ; ignore `order_by`, `limit` et `offset` |
| `exists(&db)` | `bool` | Indique si au moins une ligne correspond |
| `paginate(&db, page, per_page)` | `Page<T>` | Voir plus haut |

```rust
let oldest: Option<User> = User::query().order_by(User::AGE.desc().nulls_last()).first(&db).await?;
let bob: User = User::query().filter(User::NAME.eq("Bob")).one(&db).await?;
let active: u64 = User::query().filter(User::ACTIVE.eq(true)).count(&db).await?;   // 3
let any_teen: bool = User::query().filter(User::AGE.lt(18)).exists(&db).await?;    // true
```

`&db` peut être n'importe quel handle de base de données ou une [transaction](transactions).

## Agrégats

Les agrégats calculent une seule valeur sur l'ensemble des lignes correspondantes :

```rust
let total_age: Option<i64> = User::query().sum(User::AGE, &db).await?;      // Some(115)
let average_age: Option<f64> = User::query().avg(User::AGE, &db).await?;    // Some(28.75)
let youngest: Option<i32> = User::query().min(User::AGE, &db).await?;       // Some(17)
let first_name: Option<String> = User::query().min(User::NAME, &db).await?; // Some("Alice")
let best: Option<f64> = User::query()
    .filter(User::ACTIVE.eq(true))
    .max(User::SCORE, &db)
    .await?; // Some(4.0)
```

| Méthode | S'applique à | Type du résultat |
|---|---|---|
| `sum(column, &db)` | Colonnes numériques | `Option<i64>` pour les entiers, `Option<f64>` pour les flottants |
| `avg(column, &db)` | Colonnes numériques | `Option<f64>` |
| `min(column, &db)` | N'importe quelle colonne | `Option<T>` |
| `max(column, &db)` | N'importe quelle colonne | `Option<T>` |

Le résultat vaut `None` quand aucune ligne ne correspond, ou quand toutes les valeurs sont `NULL`. Les valeurs `NULL` sont ignorées par tous les agrégats (l'âge moyen ci-dessus est calculé sur les quatre âges connus). Les filtres et les jointures s'appliquent comme d'habitude.

## Ne sélectionner que certaines colonnes

`columns` remplace les « lignes complètes » par les colonnes que vous indiquez. Le type du résultat suit automatiquement :

```rust
let emails: Vec<String> = User::query().columns(User::EMAIL).all(&db).await?;

let pairs: Vec<(i64, String)> = User::query()
    .columns((User::ID, User::NAME))
    .order_by(User::ID)
    .limit(2)
    .all(&db)
    .await?;
// [(1, "Alice"), (2, "Bob")]

let ages: Vec<Option<i32>> = User::query().columns(User::AGE).all(&db).await?;
```

Passez une colonne ou un tuple de 8 colonnes au maximum. Les colonnes nullables reviennent sous forme d'`Option<T>`. Ajoutez `distinct()` pour supprimer les lignes en double :

```rust
let states: Vec<bool> = User::query().columns(User::ACTIVE).distinct().order_by(User::ACTIVE).all(&db).await?;
// [false, true]
```

> **Astuce** Ne sélectionner que les colonnes dont vous avez besoin envoie moins de données et alloue moins de chaînes. C'est un gain facile sur les tables larges.

## Rapports GROUP BY

Pour calculer des agrégats par groupe, combinez `group_by` avec `select_exprs`. `select_exprs` prend une liste d'*expressions* : transformez une colonne en expression avec `.expr()`, et utilisez les helpers d'agrégation ci-dessous. Le type de ligne s'écrit sur la variable (ou avec un turbofish) et il est décodé par position :

```rust
let per_user: Vec<(i64, i64)> = Post::query()
    .group_by(Post::USER_ID)
    .select_exprs([Post::USER_ID.expr(), count_all()])
    .order_by(Post::USER_ID.asc())
    .all(&db)
    .await?;
// [(1, 2), (2, 1), (4, 1)] : l'utilisateur 1 a écrit 2 posts, les utilisateurs 2 et 4 un chacun
```

| Expression | SQL |
|---|---|
| `count_all()` | `COUNT(*)` |
| `Column::count()` | `COUNT(column)` : compte les valeurs non NULL |
| `Column::count_distinct()` | `COUNT(DISTINCT column)` |
| `Column::sum()` | `SUM(column)` (colonnes numériques) |
| `Column::avg()` | `AVG(column)` (colonnes numériques) |
| `Column::min()` / `Column::max()` | `MIN(column)` / `MAX(column)` |

`having` filtre les groupes après l'agrégation, et vous pouvez trier selon un agrégat :

```rust
let busy: Vec<(i64, i64, Option<i64>)> = Post::query()
    .group_by(Post::USER_ID)
    .having(count_all().ge(Expr::val(2_i64)))
    .select_exprs([Post::USER_ID.expr(), count_all(), Post::VIEWS.sum()])
    .all(&db)
    .await?;
// [(1, 2, Some(40))]

let ranking: Vec<(i64, Option<i64>)> = Post::query()
    .group_by(Post::USER_ID)
    .select_exprs([Post::USER_ID.expr(), Post::VIEWS.sum()])
    .order_by(Post::VIEWS.sum().desc())
    .limit(2)
    .all(&db)
    .await?;
// [(1, Some(40)), (2, Some(5))]
```

Sans `group_by`, les agrégats portent sur toute la table. `one` est pratique pour obtenir une seule ligne de synthèse :

```rust
let (count, distinct_ages, avg_score): (i64, i64, Option<f64>) = User::query()
    .select_exprs([count_all(), User::AGE.count_distinct(), User::SCORE.avg()])
    .one(&db)
    .await?;
// (5, 4, Some(2.3))
```

Memoire normalise les types des résultats pour qu'ils se décodent de la même façon partout : `COUNT` est un `i64`, le `SUM` d'entiers est un `i64`, le `SUM` de flottants et tous les `AVG` sont des `f64`. Utilisez `Option<...>` pour `SUM`, `AVG`, `MIN` et `MAX`, qui valent `NULL` pour les groupes vides. Pour des rapports plus conséquents, décodez dans une struct avec [`#[derive(FromRow)]`](models).

## Expressions et arithmétique

Les colonnes numériques prennent en charge `add`, `sub`, `mul` et `div`, qui construisent des expressions. Les expressions (type `Expr`) disposent aussi de méthodes de comparaison non typées (`eq`, `ne`, `lt`, `le`, `gt`, `ge`) qui acceptent une autre expression, une colonne ou une valeur enveloppée avec `Expr::val` :

```rust
let doubled = User::query().filter(User::SCORE.mul(2.0).gt(Expr::val(5.0))).all(&db).await?;
// Carol (3.0 × 2), Dave (4.0 × 2)
```

> **Remarque** Les comparaisons sur une `Expr` ne sont pas vérifiées par le typage comme les comparaisons sur une colonne, car l'expression ne connaît plus son type Rust. Préférez les méthodes de colonne quand c'est possible.

## Voir le SQL

`to_sql(dialect)` génère le SQL de la requête pour PostgreSQL, SQLite ou MySQL sans l'exécuter (Redis et MongoDB n'utilisent pas le SQL, mais `to_sql` reste un moyen pratique de voir ce que signifie une requête). `to_sql_with_params` renvoie en plus les valeurs liées :

```rust
let q = User::query()
    .filter(User::AGE.ge(18) & User::NAME.contains("li"))
    .order_by(User::NAME.asc())
    .limit(20);

println!("{}", q.to_sql(Dialect::Postgres));
let (sql, params) = q.to_sql_with_params(Dialect::Sqlite);
println!("{sql}\n{params:?}");
```

PostgreSQL :

```sql
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active", "users"."score" FROM "users" WHERE ("users"."age" >= $1 AND "users"."name" LIKE $2 ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 20
```

SQLite :

```sql
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active", "users"."score" FROM "users" WHERE ("users"."age" >= ? AND "users"."name" LIKE ? ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 20
```

MySQL :

```sql
SELECT `users`.`id`, `users`.`email`, `users`.`name`, `users`.`age`, `users`.`active`, `users`.`score` FROM `users` WHERE (`users`.`age` >= ? AND `users`.`name` LIKE ? ESCAPE '\\') ORDER BY `users`.`name` ASC LIMIT 20
```

Paramètres : `[I32(18), Text("%li%")]`.

Les dialectes diffèrent sur quelques points, que Memoire gère pour vous :

| Aspect | PostgreSQL | SQLite | MySQL |
|---|---|---|---|
| Placeholders | `$1`, `$2`, ... | `?` | `?` |
| Guillemets des identifiants | `"users"` | `"users"` | `` `users` `` |
| Recherche insensible à la casse | `ILIKE` | `LOWER(col) LIKE LOWER(?)` | `LOWER(col) LIKE LOWER(?)` |
| `nulls_last()` | `NULLS LAST` | `NULLS LAST` | `col IS NULL ASC, col` (émulé) |
| `offset` sans `limit` | `OFFSET n` | `LIMIT -1 OFFSET n` | `LIMIT 18446744073709551615 OFFSET n` |
| Type du résultat de `SUM` / `AVG` | `CAST(... AS BIGINT / DOUBLE PRECISION)` | natif | `CAST(... AS SIGNED / DOUBLE)` |

Les builders `Update` et `Delete` ont eux aussi `to_sql` (voir [Créer, lire, modifier, supprimer](crud)).

## Ce qui ne compile pas

Les colonnes typées détectent de nombreuses erreurs à la compilation. Chacune de ces lignes est **refusée par le compilateur** :

```rust,ignore
// Comparer une colonne entière avec une chaîne :
User::AGE.gt("18");                      // error: `&str: Bind<i32>` is not satisfied

// Comparer une colonne de texte avec un nombre :
User::EMAIL.eq(42);                      // error: `{integer}: Bind<String>` is not satisfied

// Recherche textuelle sur une colonne qui n'est pas du texte :
User::AGE.contains("1");                 // error: method `contains` exists but its trait bounds were not satisfied

// Une faute de frappe dans un nom de colonne :
User::EMIAL.eq("a@b.c");                 // error: no associated function or constant named `EMIAL`

// Comparer avec None (utilisez `is_null()`) :
User::AGE.eq(None);                      // error: `Option<_>: Bind<i32>` is not satisfied

// Additionner une colonne de texte :
User::query().sum(User::NAME, &db);      // error: `String: Numeric` is not satisfied

// Modifier une colonne d'un autre modèle :
User::update_many().set(Post::TITLE, "x"); // error: expected `Column<User, _>`, found `Column<Post, String>`

// Mettre NULL dans une colonne non nullable :
User::update_many().set_null(User::NAME);  // error: expected `Column<User, Option<_>>`

// Chercher une clé i64 avec une chaîne :
User::find(&db, "1");                    // error: `&str: Bind<i64>` is not satisfied
```

## Étapes suivantes

- [Créer, lire, modifier, supprimer](crud) : `update_many`, `delete_many` et les opérations ligne par ligne.
- [Jointures et relations](relations) : interrogez plusieurs tables à la fois.
- [Aide-mémoire](cheatsheet) : toutes les opérations sur une seule page.
