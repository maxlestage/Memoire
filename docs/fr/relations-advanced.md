# Filtres de relations, comptages et polymorphisme

La [page des relations](relations) charge les lignes liées. Celle-ci va plus loin, avec les outils popularisés par Eloquent de Laravel : filtrer sur des relations (`has`, `whereHas`), compter les lignes liées (`withCount`), charger en un appel (`with`), les relations polymorphes (`morphMany`, `morphTo`) et les colonnes supplémentaires d'une table de liaison plusieurs-à-plusieurs (`withPivot`). Tout est déduit des clés étrangères que vous déclarez déjà, et fonctionne sur chaque base.

Les exemples utilisent ces modèles :

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
    #[memoire(references = "users(id)", index)]
    pub author_id: Option<i64>,
    pub title: String,
    pub published: bool,
}
```

## Filtrer sur des relations

| Méthode | Garde les lignes... | Eloquent |
|---|---|---|
| `has::<R>()` | qui ont au moins un `R` lié | `has('posts')` |
| `doesnt_have::<R>()` | qui n'ont aucun `R` lié | `doesntHave('posts')` |
| `where_has::<R>(\|q\| ...)` | qui ont au moins un `R` lié correspondant à la requête | `whereHas('posts', fn)` |
| `where_doesnt_have::<R>(\|q\| ...)` | qui n'ont aucun `R` lié correspondant à la requête | `whereDoesntHave` |
| `has_at_least::<R>(n)` | qui ont `n` `R` liés ou plus | `has('posts', '>=', n)` |
| `where_has_via::<J, T>(\|q\| ...)` | liées par la table de liaison `J` à un `T` correspondant à la requête | `whereHas` sur un `belongsToMany` |

```rust
// Les utilisateurs qui ont publié quelque chose.
let writers = User::query()
    .where_has::<Post>(|posts| posts.filter(Post::PUBLISHED.eq(true)))
    .order_by(User::NAME)
    .all(&db)
    .await?;

// Les utilisateurs sans aucun post, et les prolifiques.
let silent = User::query().doesnt_have::<Post>().all(&db).await?;
let prolific = User::query().has_at_least::<Post>(10).count(&db).await?;
```

La relation marche dans les deux sens : `Post::query().has::<User>()` garde les posts qui appartiennent à un utilisateur, et `Post::query().doesnt_have::<User>()` ceux dont `author_id` est `NULL` ou ne pointe vers rien. Chaque filtre est une [sous-requête](queries) (`"users"."id" IN (SELECT "posts"."author_id" FROM "posts" WHERE ...)`) : une instruction sur les bases SQL, deux sur le moteur en mémoire, Redis et MongoDB. La closure reçoit `R::query()`, donc les lignes en [suppression douce](lifecycle) sont exclues, sauf si vous appelez `with_trashed()` dessus.

Les modèles doivent être liés par exactement une clé étrangère ; sinon la méthode panique avec un message qui dit ce qui manque (filtrez alors vous-même sur la clé étrangère avec `in_query`).

## Compter les lignes liées

`load_count` compte les enfants de chaque ligne d'une slice avec une seule requête `GROUP BY`, sans les charger ; `load_count_with` ne compte que ceux qui correspondent à une requête. `all_with_count` exécute votre requête et associe chaque ligne à son total :

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<u64> = users.load_count::<Post>(&db).await?;          // aligné sur `users`
let drafts = users.load_count_with(&db, Post::query().filter(Post::PUBLISHED.eq(false))).await?;

for (user, n) in User::query().all_with_count::<Post>(&db).await? {
    println!("{} a écrit {n} posts", user.name);
}
```

## Chargement anticipé en un appel

`all_with::<C>()` exécute la requête et charge les enfants de chaque ligne, `all_with_parent::<P>()` le parent de chaque ligne : deux requêtes, quel que soit le nombre de lignes.

```rust
for (user, posts) in User::query().all_with::<Post>(&db).await? { /* ... */ }
for (post, author) in Post::query().all_with_parent::<User>(&db).await? { /* author : Option<User> */ }
```

Pour charger des niveaux plus profonds, appelez [`load_children`](relations) sur les enfants chargés : une requête de plus par niveau.

## Relations polymorphes

Un commentaire peut appartenir à un post *ou* à une vidéo. Déclarez `#[memoire(morph = "commentable")]` sur le modèle avec deux champs, `commentable_type` (une `String`, le **nom de table** du parent) et `commentable_id` (sa clé primaire) :

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "comments", morph = "commentable")]
pub struct Comment {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(index)]
    pub commentable_type: String,
    #[memoire(index)]
    pub commentable_id: i64,
    pub body: String,
}

let mut c = Comment { id: 0, commentable_type: String::new(), commentable_id: 0, body: "Bravo !".into() };
c.set_morph_parent(&post)?;          // commentable_type = "posts", commentable_id = post.id
c.insert(&db).await?;

let on_post = Comment::morphed_by(&post).all(&db).await?;           // morphMany, en requête
let parent: Option<Post> = c.morph_parent(&db).await?;               // morphTo (None si ce n'est pas un post)
let per_post = posts.load_morph_many::<Comment>(&db).await?;        // anticipé, une requête
let parents: Vec<Option<Post>> = comments.load_morph_parent(&db).await?;
```

Un champ `commentable_type` ou `commentable_id` manquant est une erreur de compilation. La base ne peut pas vérifier un tel lien (il n'y a pas de clé étrangère) : supprimez vous-même les commentaires avec leur parent.

## Plusieurs-à-plusieurs avec colonnes pivot

Un modèle de liaison peut porter ses propres colonnes, un rôle ou une date. `load_via_pivot` renvoie chaque partenaire avec la ligne de liaison qui le relie ; `load_via_pivot_with` part de votre requête sur la liaison (`wherePivot`, `orderByPivot`) :

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "memberships")]
pub struct Membership {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)")]
    pub user_id: i64,
    #[memoire(references = "teams(id)")]
    pub team_id: i64,
    pub role: String,
}

let teams: Vec<Vec<(Team, Membership)>> = users.load_via_pivot::<Membership, Team>(&db).await?;
for (team, membership) in &teams[0] {
    println!("{} dans {} en tant que {}", users[0].name, team.name, membership.role);
}
let led = users
    .load_via_pivot_with::<Membership, Team>(&db, Membership::query().filter(Membership::ROLE.eq("lead")))
    .await?;
```

## Étapes suivantes

- [Factories et seeders](factories) : remplissez une base pour les tests en quelques lignes.
- [Verrouillage optimiste et identity map](optimistic-locking) : des écritures concurrentes sûres.
