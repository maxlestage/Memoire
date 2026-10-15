# Joins & relations

Real data is spread over several tables: users write posts, posts receive comments. This page shows how to model those links, how to query several tables at once with joins, and how to load related rows efficiently.

## Modelling a one-to-many relation

"One user has many posts" is stored by giving each post the id of its author. Declare that column with `references` so the database knows about the link:

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

What the attributes on `author_id` do:

- `references = "users(id)"` creates a foreign key: the database refuses a post whose `author_id` does not match an existing user (`Error::Constraint`).
- `on_delete = "cascade"` deletes a user's posts automatically when the user is deleted.
- `index` makes "find the posts of this user" fast. Foreign key columns almost always deserve an index.

Create the tables parent first, because a foreign key can only point to an existing table:

```rust
db.create_table::<User>().await?;
db.create_table::<Post>().await?;
db.create_table::<Comment>().await?;
```

The examples below use this data:

| User | Posts |
|---|---|
| Alice (1) | "Hello" (120 views), "Rust" (30 views) |
| Bob (2) | "Bob's" (5 views) |
| Carol (3) | none |

Comments: "Nice" and "Great" on "Hello", "Hmm" on "Bob's".

> **Note** Foreign keys are enforced by SQLite, PostgreSQL and MySQL. The in-memory engine, Redis and MongoDB accept the declaration but do not check it and do not cascade deletes, so test cascade behaviour on a SQL backend.

## Inner join: rows that have a match

`inner_join::<P>(condition)` adds the model `P` to the query. The result becomes a tuple: the starting model first, then each joined model in order.

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

The condition says how rows match: `User::ID.eq_col(Post::AUTHOR_ID)` means "the user's id equals the post's `author_id`". `eq_col` compares two columns of the same type. Once joined, you can filter and sort on the columns of every table in the query.

An inner join only returns combinations that match: Carol has no post, so she does not appear.

## Left join: keep rows without a match

`left_join` keeps every row of the starting table. When there is no match, the joined model is `None`:

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

Notice that Alice appears twice: a join produces one row per *combination*. To find the users with no post at all, keep only the rows where the post is missing:

```rust
let lonely: Vec<(User, Option<Post>)> = User::query()
    .left_join::<Post>(Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::ID.is_null())
    .all(&db)
    .await?;
// [(Carol, None)]
```

## Joining more than two tables

Chain joins to follow links further. Each join appends one element to the tuple:

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

A query can return up to five models this way (the starting one plus four joins).

## Join only to filter

Sometimes you want rows of one table, filtered by another table, without getting the other table back. Use `join` with a `JoinKind`: the result type does not change. Add `distinct()` so a user with several matching posts appears once:

```rust
let popular_authors: Vec<User> = User::query()
    .join::<Post>(JoinKind::Inner, Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::VIEWS.ge(100))
    .distinct()
    .all(&db)
    .await?;
// [Alice]
```

`JoinKind::Inner` and `JoinKind::Left` are available.

## Picking columns across tables

After joining, `columns` can mix columns from all the tables in the query:

```rust
let pairs: Vec<(String, String)> = Post::query()
    .inner_join::<User>(User::ID.eq_col(Post::AUTHOR_ID))
    .columns((User::NAME, Post::TITLE))
    .order_by(Post::ID)
    .all(&db)
    .await?;
// [("Alice", "Hello"), ("Alice", "Rust"), ("Bob", "Bob's")]
```

Call `columns` after the joins.

## Counting children

Combine a left join with `group_by` to count related rows, including zero:

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

`Post::ID.count()` counts non-NULL post ids, which is why Carol gets `0` rather than `1`.

## The children of one row

`belonging_to` starts a query on the rows that point to a given row. Memoire finds the link from the `references` attribute, so there is nothing else to declare, and you can keep refining the query:

```rust
let alice = User::get(&db, 1).await?;
let posts: Vec<Post> = Post::belonging_to(&alice)
    .filter(Post::VIEWS.ge(10))
    .order_by(Post::VIEWS.desc())
    .all(&db)
    .await?;
// ["Hello", "Rust"]
```

The other way round, `find_parent` follows a foreign key:

```rust
let post = Post::get(&db, 3).await?;
let author: Option<User> = post.find_parent::<User, _>(&db).await?; // Some(Bob)
```

It returns `None` when the foreign key is `NULL` (an `Option<i64>` column) or points to a row that no longer exists.

## Loading children without the N+1 problem

A very common need: show each user with their posts. The tempting version runs one query per user:

```rust
// Slow: 1 query for the users + 1 query per user ("N+1 queries").
let users = User::all(&db).await?;
for user in &users {
    let posts = Post::belonging_to(user).all(&db).await?;
    println!("{}: {} post(s)", user.name, posts.len());
}
```

With 1,000 users that is 1,001 round trips to the database. `load_children` loads the children of a whole list in **one** query (`WHERE author_id IN (...)`) and hands them back aligned with the list: `posts[i]` holds the posts of `users[i]`.

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

for (user, posts) in users.iter().zip(&posts) {
    println!("{}: {} post(s)", user.name, posts.len());
}
```

```text
Alice: 2 post(s)
Bob: 1 post(s)
Carol: 0 post(s)
```

Two queries in total, however many users there are. On PostgreSQL, for 100 users with 10 posts each, that is 1.1 ms instead of 13.5 ms (×12) on the same machine, and the gap grows with the network latency to the database.

Every loader comes from the `LoadRelated` trait (in the prelude) and works on slices and `Vec`s:

| Method | Returns | Queries |
|---|---|---|
| `users.load_children::<Post>(&db)` | `Vec<Vec<Post>>`, children ordered by primary key | 1 |
| `users.load_children_with(&db, Post::query()...)` | `Vec<Vec<Post>>`, in the order of your query | 1 |
| `posts.load_parent::<User>(&db)` | `Vec<Option<User>>`, one per post | 1 |
| `posts.load_via::<PostTag, Tag>(&db)` | `Vec<Vec<Tag>>` (many-to-many, below) | 2 |

`load_children_with` takes your own query on the children, to filter or sort them:

```rust
let popular: Vec<Vec<Post>> = users
    .load_children_with(&db, Post::query().filter(Post::VIEWS.ge(100)).order_by(Post::VIEWS.desc()))
    .await?;
```

`LIMIT` and `OFFSET` in that query apply to all the children at once, not to each user.

`load_parent` goes the other way: one query for the authors of all the posts, each author shared by the posts that point to it (the parent model must implement `Clone`):

```rust
let posts = Post::all(&db).await?;
let authors: Vec<Option<User>> = posts.load_parent(&db).await?;
for (post, author) in posts.iter().zip(&authors) {
    println!("{} by {}", post.title, author.as_ref().map_or("?", |a| a.name.as_str()));
}
```

Loaders chain to any depth (users → posts → comments is three queries), and very long lists are split automatically under the database's parameter limit (32,766 on SQLite, 65,535 on PostgreSQL and MySQL).

> **Note** The relation comes from the foreign key. If the child has no column with `references` to the parent's table, or has several (an `author_id` and a `reviewer_id` both pointing to `users`), the loaders fail with `Error::Relation` and `belonging_to` panics. Filter on the column you mean yourself in that case: `Post::query().filter(Post::REVIEWER_ID.is_in(...))`.

## Many-to-many relations

"A post has many tags and a tag is on many posts" needs a link table with one row per (post, tag) pair:

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

Memoire models have a single-column primary key, so the link table gets its own `id`. If you want the database to forbid duplicate pairs, add a unique index over both columns with a SQL [migration](migrations).

`load_via` loads the many-to-many partners of a whole list in two queries, one on the link table and one on the targets:

```rust
let posts = Post::query().order_by(Post::ID).all(&db).await?;
let tags: Vec<Vec<Tag>> = posts.load_via::<PostTag, Tag>(&db).await?;
// tags[i] holds the tags of posts[i], in the order the links were created
```

The link model must reference both tables; it works in both directions (`tags.load_via::<PostTag, Post>(&db)`).

## Tips and limitations

- **Keep join conditions simple.** A single column equality such as `Post::AUTHOR_ID.eq_col(User::ID)` is the fast path everywhere; the in-memory engine (also used by Redis) runs it as a hash join, and MongoDB only fetches the documents whose join key matches. Put the other conditions in `filter`. (For a left join, a condition in `filter` and the same condition in the join condition give different results: the first removes rows, the second only removes matches.)
- **No self-joins.** A table cannot appear twice in one query (for example "users and their manager, who is also a user"), because Memoire does not generate table aliases. The in-memory engine, Redis and MongoDB return `Error::Unsupported`, and SQL databases reject the statement. Use [raw SQL](raw-sql) with aliases on a SQL database, or two queries.
- **At most five models per tuple.** Joins with `inner_join`/`left_join` return up to five models. `join` (filter only) has no such limit.

## Next steps

- [Relation filters, counts & polymorphism](relations-advanced): `where_has`, `load_count`, `all_with`, polymorphic relations and pivot columns.
- [Transactions](transactions): change related rows together, all or nothing.
- [Raw SQL](raw-sql): for the few queries the builder cannot express.
- [Why it's fast](performance): more ways to keep queries quick.
