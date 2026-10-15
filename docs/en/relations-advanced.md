# Relation filters, counts & polymorphism

The [relations page](relations) loads related rows. This one goes further, with the tools Laravel's Eloquent made popular: filtering on relations (`has`, `whereHas`), counting related rows (`withCount`), eager loading in one call (`with`), polymorphic relations (`morphMany`, `morphTo`) and the extra columns of a many-to-many junction (`withPivot`). Everything is read from the foreign keys you already declare, and works on every backend.

The examples use these models:

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

## Filtering on relations

| Method | Keeps the rows... | Eloquent |
|---|---|---|
| `has::<R>()` | with at least one related `R` | `has('posts')` |
| `doesnt_have::<R>()` | without any related `R` | `doesntHave('posts')` |
| `where_has::<R>(\|q\| ...)` | with at least one related `R` matching the query | `whereHas('posts', fn)` |
| `where_doesnt_have::<R>(\|q\| ...)` | without any related `R` matching the query | `whereDoesntHave` |
| `has_at_least::<R>(n)` | with `n` related `R` or more | `has('posts', '>=', n)` |
| `where_has_via::<J, T>(\|q\| ...)` | linked through the junction `J` to a `T` matching the query | `whereHas` on a `belongsToMany` |

```rust
// Users who published something.
let writers = User::query()
    .where_has::<Post>(|posts| posts.filter(Post::PUBLISHED.eq(true)))
    .order_by(User::NAME)
    .all(&db)
    .await?;

// Users without any post, and prolific ones.
let silent = User::query().doesnt_have::<Post>().all(&db).await?;
let prolific = User::query().has_at_least::<Post>(10).count(&db).await?;
```

The relation works in both directions: `Post::query().has::<User>()` keeps the posts that belong to a user, and `Post::query().doesnt_have::<User>()` the ones whose `author_id` is `NULL` or points to nothing. Each filter is one [subquery](queries) (`"users"."id" IN (SELECT "posts"."author_id" FROM "posts" WHERE ...)`): one statement on SQL databases, two on the in-memory engine, Redis and MongoDB. The closure receives `R::query()`, so [soft-deleted](lifecycle) rows are left out unless you call `with_trashed()` on it.

The models must be related by exactly one foreign key; otherwise the method panics with a message saying what is missing (filter on the foreign key yourself with `in_query` then).

## Counting related rows

`load_count` counts the children of every row of a slice with one `GROUP BY` query, without loading them; `load_count_with` counts only the ones matching a query. `all_with_count` runs your query and pairs every row with its count:

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<u64> = users.load_count::<Post>(&db).await?;          // aligned with `users`
let drafts = users.load_count_with(&db, Post::query().filter(Post::PUBLISHED.eq(false))).await?;

for (user, n) in User::query().all_with_count::<Post>(&db).await? {
    println!("{} wrote {n} posts", user.name);
}
```

## Eager loading in one call

`all_with::<C>()` runs the query and loads the children of every row, `all_with_parent::<P>()` the parent of every row: two queries, whatever the number of rows.

```rust
for (user, posts) in User::query().all_with::<Post>(&db).await? { /* ... */ }
for (post, author) in Post::query().all_with_parent::<User>(&db).await? { /* author: Option<User> */ }
```

To load deeper levels, call [`load_children`](relations) on the loaded children: one more query per level.

## Polymorphic relations

A comment can belong to a post *or* to a video. Declare `#[memoire(morph = "commentable")]` on the model with two fields, `commentable_type` (a `String`, the **table name** of the parent) and `commentable_id` (its primary key):

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

let mut c = Comment { id: 0, commentable_type: String::new(), commentable_id: 0, body: "Nice!".into() };
c.set_morph_parent(&post)?;          // commentable_type = "posts", commentable_id = post.id
c.insert(&db).await?;

let on_post = Comment::morphed_by(&post).all(&db).await?;           // morphMany, as a query
let parent: Option<Post> = c.morph_parent(&db).await?;               // morphTo (None if not a post)
let per_post = posts.load_morph_many::<Comment>(&db).await?;        // eager, one query
let parents: Vec<Option<Post>> = comments.load_morph_parent(&db).await?;
```

A missing `commentable_type` or `commentable_id` field is a compile error. The database cannot check such a link (there is no foreign key), so delete the comments with their parent yourself.

## Many-to-many with pivot columns

A junction model can carry its own columns, a role or a date. `load_via_pivot` returns each partner with the junction row that links it; `load_via_pivot_with` starts from your query on the junction (`wherePivot`, `orderByPivot`):

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
    println!("{} in {} as {}", users[0].name, team.name, membership.role);
}
let led = users
    .load_via_pivot_with::<Membership, Team>(&db, Membership::query().filter(Membership::ROLE.eq("lead")))
    .await?;
```

## Next steps

- [Factories & seeders](factories): fill a database for tests in a few lines.
- [Optimistic locking & identity map](optimistic-locking): safe concurrent writes.
