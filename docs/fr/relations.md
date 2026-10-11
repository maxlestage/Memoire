# Jointures et relations

Les données réelles sont réparties sur plusieurs tables : les utilisateurs écrivent des posts, les posts reçoivent des commentaires. Cette page montre comment modéliser ces liens, comment interroger plusieurs tables à la fois avec des jointures et comment charger efficacement les lignes liées.

## Modéliser une relation un-à-plusieurs

« Un utilisateur a plusieurs posts » se stocke en donnant à chaque post l'id de son auteur. Déclarez cette colonne avec `references` pour que la base de données connaisse le lien :

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub author_id: i64,
    pub title: String,
    pub views: i32,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "comments")]
pub struct Comment {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "posts(id)", on_delete = "cascade", index)]
    pub post_id: i64,
    pub body: String,
}
```

Ce que font les attributs de `author_id` :

- `references = "users(id)"` crée une clé étrangère : la base de données refuse un post dont l'`author_id` ne correspond à aucun utilisateur existant (`Error::Constraint`).
- `on_delete = "cascade"` supprime automatiquement les posts d'un utilisateur quand celui-ci est supprimé.
- `index` rend rapide la recherche « les posts de cet utilisateur ». Les colonnes de clé étrangère méritent presque toujours un index.

Créez les tables en commençant par le parent, car une clé étrangère ne peut pointer que vers une table existante :

```rust
db.create_table::<User>().await?;
db.create_table::<Post>().await?;
db.create_table::<Comment>().await?;
```

Les exemples ci-dessous utilisent ces données :

| Utilisateur | Posts |
|---|---|
| Alice (1) | "Hello" (120 vues), "Rust" (30 vues) |
| Bob (2) | "Bob's" (5 vues) |
| Carol (3) | aucun |

Commentaires : "Nice" et "Great" sur "Hello", "Hmm" sur "Bob's".

> **Remarque** Les clés étrangères sont appliquées par SQLite, PostgreSQL et MySQL. Le moteur en mémoire, Redis et MongoDB acceptent la déclaration mais ne la vérifient pas et ne propagent pas les suppressions en cascade : testez donc le comportement en cascade sur un backend SQL.

## Jointure interne : les lignes qui ont une correspondance

`inner_join::<P>(condition)` ajoute le modèle `P` à la requête. Le résultat devient un tuple : le modèle de départ d'abord, puis chaque modèle joint, dans l'ordre.

```rust
let rows: Vec<(Post, User)> = Post::query()
    .inner_join::<User>(User::ID.eq_col(Post::AUTHOR_ID))
    .order_by(Post::VIEWS.desc())
    .all(&db)
    .await?;

for (post, author) in &rows {
    println!("{} by {} ({} views)", post.title, author.name, post.views);
}
```

```text
Hello by Alice (120 views)
Rust by Alice (30 views)
Bob's by Bob (5 views)
```

La condition indique comment les lignes se correspondent : `User::ID.eq_col(Post::AUTHOR_ID)` signifie « l'id de l'utilisateur est égal à l'`author_id` du post ». `eq_col` compare deux colonnes de même type. Une fois la jointure faite, vous pouvez filtrer et trier sur les colonnes de toutes les tables de la requête.

Une jointure interne ne renvoie que les combinaisons qui correspondent : Carol n'a aucun post, elle n'apparaît donc pas.

## Jointure externe gauche : garder les lignes sans correspondance

`left_join` conserve toutes les lignes de la table de départ. Quand il n'y a pas de correspondance, le modèle joint vaut `None` :

```rust
let rows: Vec<(User, Option<Post>)> = User::query()
    .left_join::<Post>(Post::AUTHOR_ID.eq_col(User::ID))
    .order_by(User::ID)
    .order_by(Post::ID)
    .all(&db)
    .await?;

for (user, post) in &rows {
    match post {
        Some(post) => println!("{} wrote {}", user.name, post.title),
        None => println!("{} has not written anything", user.name),
    }
}
```

```text
Alice wrote Hello
Alice wrote Rust
Bob wrote Bob's
Carol has not written anything
```

Remarquez qu'Alice apparaît deux fois : une jointure produit une ligne par *combinaison*. Pour trouver les utilisateurs qui n'ont écrit aucun post, ne gardez que les lignes où le post est absent :

```rust
let lonely: Vec<(User, Option<Post>)> = User::query()
    .left_join::<Post>(Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::ID.is_null())
    .all(&db)
    .await?;
// [(Carol, None)]
```

## Joindre plus de deux tables

Enchaînez les jointures pour suivre les liens plus loin. Chaque jointure ajoute un élément au tuple :

```rust
let rows: Vec<(Comment, Post, User)> = Comment::query()
    .inner_join::<Post>(Post::ID.eq_col(Comment::POST_ID))
    .inner_join::<User>(User::ID.eq_col(Post::AUTHOR_ID))
    .order_by(Comment::ID)
    .all(&db)
    .await?;

for (comment, post, author) in &rows {
    println!("\"{}\" on {} by {}", comment.body, post.title, author.name);
}
```

```text
"Nice" on Hello by Alice
"Great" on Hello by Alice
"Hmm" on Bob's by Bob
```

Une requête peut ainsi renvoyer jusqu'à cinq modèles (celui de départ plus quatre jointures).

## Joindre uniquement pour filtrer

Il arrive que vous vouliez les lignes d'une table, filtrées selon une autre table, sans récupérer cette autre table. Utilisez `join` avec un `JoinKind` : le type du résultat ne change pas. Ajoutez `distinct()` pour qu'un utilisateur ayant plusieurs posts correspondants n'apparaisse qu'une fois :

```rust
let popular_authors: Vec<User> = User::query()
    .join::<Post>(JoinKind::Inner, Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::VIEWS.ge(100))
    .distinct()
    .all(&db)
    .await?;
// [Alice]
```

`JoinKind::Inner` et `JoinKind::Left` sont disponibles.

## Choisir des colonnes sur plusieurs tables

Après une jointure, `columns` peut mélanger des colonnes de toutes les tables de la requête :

```rust
let pairs: Vec<(String, String)> = Post::query()
    .inner_join::<User>(User::ID.eq_col(Post::AUTHOR_ID))
    .columns((User::NAME, Post::TITLE))
    .order_by(Post::ID)
    .all(&db)
    .await?;
// [("Alice", "Hello"), ("Alice", "Rust"), ("Bob", "Bob's")]
```

Appelez `columns` après les jointures.

## Compter les enfants

Combinez une jointure externe gauche avec `group_by` pour compter les lignes liées, y compris lorsqu'il n'y en a aucune :

```rust
let counts: Vec<(i64, String, i64)> = User::query()
    .left_join::<Post>(Post::AUTHOR_ID.eq_col(User::ID))
    .group_by(User::ID)
    .group_by(User::NAME)
    .select_exprs([User::ID.expr(), User::NAME.expr(), Post::ID.count()])
    .order_by(User::ID)
    .all(&db)
    .await?;
// [(1, "Alice", 2), (2, "Bob", 1), (3, "Carol", 0)]
```

`Post::ID.count()` compte les ids de posts non NULL : c'est pourquoi Carol obtient `0` et non `1`.

## Les enfants d'une ligne

`belonging_to` démarre une requête sur les lignes qui pointent vers une ligne donnée. Memoire trouve le lien grâce à l'attribut `references` : rien d'autre à déclarer, et la requête peut encore être affinée :

```rust
let alice = User::get(&db, 1).await?;
let posts: Vec<Post> = Post::belonging_to(&alice)
    .filter(Post::VIEWS.ge(10))
    .order_by(Post::VIEWS.desc())
    .all(&db)
    .await?;
// ["Hello", "Rust"]
```

Dans l'autre sens, `find_parent` suit une clé étrangère :

```rust
let post = Post::get(&db, 3).await?;
let author: Option<User> = post.find_parent::<User, _>(&db).await?; // Some(Bob)
```

Elle renvoie `None` quand la clé étrangère vaut `NULL` (une colonne `Option<i64>`) ou désigne une ligne qui n'existe plus.

## Charger les enfants sans le problème N+1

Un besoin très courant : afficher chaque utilisateur avec ses articles. La version tentante lance une requête par utilisateur :

```rust
// Lent : 1 requête pour les utilisateurs + 1 requête par utilisateur (« N+1 requêtes »).
let users = User::all(&db).await?;
for user in &users {
    let posts = Post::belonging_to(user).all(&db).await?;
    println!("{} : {} article(s)", user.name, posts.len());
}
```

Avec 1 000 utilisateurs, cela fait 1 001 allers-retours avec la base. `load_children` charge les enfants de toute une liste en **une seule** requête (`WHERE author_id IN (...)`) et les rend alignés sur la liste : `posts[i]` contient les articles de `users[i]`.

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

for (user, posts) in users.iter().zip(&posts) {
    println!("{} : {} article(s)", user.name, posts.len());
}
```

```text
Alice : 2 article(s)
Bob : 1 article(s)
Carol : 0 article(s)
```

Deux requêtes au total, quel que soit le nombre d'utilisateurs. Sur PostgreSQL, pour 100 utilisateurs de 10 articles chacun, cela prend 1,1 ms au lieu de 13,5 ms (×12) sur la même machine, et l'écart grandit avec la latence réseau vers la base.

Tous les chargeurs viennent du trait `LoadRelated` (dans le prélude) et fonctionnent sur les tranches et les `Vec` :

| Méthode | Renvoie | Requêtes |
|---|---|---|
| `users.load_children::<Post>(&db)` | `Vec<Vec<Post>>`, enfants triés par clé primaire | 1 |
| `users.load_children_with(&db, Post::query()...)` | `Vec<Vec<Post>>`, dans l'ordre de votre requête | 1 |
| `posts.load_parent::<User>(&db)` | `Vec<Option<User>>`, un par article | 1 |
| `posts.load_via::<PostTag, Tag>(&db)` | `Vec<Vec<Tag>>` (plusieurs-à-plusieurs, plus bas) | 2 |

`load_children_with` prend votre propre requête sur les enfants, pour les filtrer ou les trier :

```rust
let popular: Vec<Vec<Post>> = users
    .load_children_with(&db, Post::query().filter(Post::VIEWS.ge(100)).order_by(Post::VIEWS.desc()))
    .await?;
```

`LIMIT` et `OFFSET` dans cette requête s'appliquent à l'ensemble des enfants, pas à chaque utilisateur.

`load_parent` va dans l'autre sens : une requête pour les auteurs de tous les articles, chaque auteur étant partagé par les articles qui pointent vers lui (le modèle parent doit implémenter `Clone`) :

```rust
let posts = Post::all(&db).await?;
let authors: Vec<Option<User>> = posts.load_parent(&db).await?;
for (post, author) in posts.iter().zip(&authors) {
    println!("{} par {}", post.title, author.as_ref().map_or("?", |a| a.name.as_str()));
}
```

Les chargeurs s'enchaînent à n'importe quelle profondeur (utilisateurs → articles → commentaires, trois requêtes), et les listes très longues sont découpées automatiquement sous la limite de paramètres de la base (32 766 sur SQLite, 65 535 sur PostgreSQL et MySQL).

> **Note** La relation vient de la clé étrangère. Si l'enfant n'a aucune colonne avec `references` vers la table du parent, ou en a plusieurs (un `author_id` et un `reviewer_id` qui pointent tous deux vers `users`), les chargeurs échouent avec `Error::Relation` et `belonging_to` panique. Filtrez alors vous-même sur la colonne voulue : `Post::query().filter(Post::REVIEWER_ID.is_in(...))`.

## Relations plusieurs-à-plusieurs

« Un post a plusieurs tags et un tag est présent sur plusieurs posts » nécessite une table de liaison avec une ligne par paire (post, tag) :

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub name: String,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "post_tags")]
pub struct PostTag {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "posts(id)", on_delete = "cascade", index)]
    pub post_id: i64,
    #[memoire(references = "tags(id)", on_delete = "cascade", index)]
    pub tag_id: i64,
}

async fn tags_of_post<E: Executor>(db: &E, post_id: i64) -> memoire_orm::Result<Vec<Tag>> {
    Tag::query()
        .join::<PostTag>(JoinKind::Inner, PostTag::TAG_ID.eq_col(Tag::ID))
        .filter(PostTag::POST_ID.eq(post_id))
        .order_by(Tag::NAME)
        .all(db)
        .await
}
```

Les modèles Memoire ont une clé primaire sur une seule colonne : la table de liaison reçoit donc son propre `id`. Si vous voulez que la base de données interdise les paires en double, ajoutez un index unique sur les deux colonnes avec une [migration](migrations) SQL.

`load_via` charge les partenaires plusieurs-à-plusieurs de toute une liste en deux requêtes, une sur la table de liaison et une sur les cibles :

```rust
let posts = Post::query().order_by(Post::ID).all(&db).await?;
let tags: Vec<Vec<Tag>> = posts.load_via::<PostTag, Tag>(&db).await?;
// tags[i] contient les étiquettes de posts[i], dans l'ordre de création des liens
```

Le modèle de liaison doit référencer les deux tables ; cela marche dans les deux sens (`tags.load_via::<PostTag, Post>(&db)`).

## Conseils et limites

- **Gardez des conditions de jointure simples.** Une simple égalité entre deux colonnes, comme `Post::AUTHOR_ID.eq_col(User::ID)`, est le chemin rapide partout ; le moteur en mémoire (utilisé aussi par Redis) l'exécute sous forme de hash join, et MongoDB ne récupère que les documents dont la clé de jointure correspond. Mettez les autres conditions dans `filter`. (Pour une jointure externe gauche, une condition dans `filter` et la même condition dans la condition de jointure donnent des résultats différents : la première supprime des lignes, la seconde ne supprime que des correspondances.)
- **Pas d'auto-jointures.** Une table ne peut pas apparaître deux fois dans une même requête (par exemple « les utilisateurs et leur responsable, qui est lui aussi un utilisateur »), car Memoire ne génère pas d'alias de table. Le moteur en mémoire, Redis et MongoDB renvoient `Error::Unsupported`, et les bases de données SQL rejettent la requête. Utilisez du [SQL brut](raw-sql) avec des alias sur une base de données SQL, ou deux requêtes.
- **Cinq modèles au maximum par tuple.** Les jointures avec `inner_join`/`left_join` renvoient jusqu'à cinq modèles. `join` (filtre uniquement) n'a pas cette limite.

## Étapes suivantes

- [Transactions](transactions) : modifiez des lignes liées ensemble, en mode tout ou rien.
- [SQL brut](raw-sql) : pour les rares requêtes que le builder ne sait pas exprimer.
- [Pourquoi c'est rapide](performance) : d'autres façons de garder des requêtes rapides.
