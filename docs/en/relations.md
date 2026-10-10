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

## Loading children without the N+1 problem

A very common need: show each user with their posts. The tempting version runs one query per user:

```rust
// Slow: 1 query for the users + 1 query per user ("N+1 queries").
let users = User::all(&db).await?;
for user in &users {
    let posts = Post::query().filter(Post::AUTHOR_ID.eq(user.id)).all(&db).await?;
    println!("{}: {} post(s)", user.name, posts.len());
}
```

With 1,000 users that is 1,001 round trips to the database. Instead, load all the children in **one** query with `is_in`, then group them in memory:

```rust
use std::collections::HashMap;

let users = User::query().order_by(User::ID).all(&db).await?;

let posts = Post::query()
    .filter(Post::AUTHOR_ID.is_in(users.iter().map(|u| u.id)))
    .all(&db)
    .await?;

let mut posts_by_author: HashMap<i64, Vec<Post>> = HashMap::new();
for post in posts {
    posts_by_author.entry(post.author_id).or_default().push(post);
}

for user in &users {
    let count = posts_by_author.get(&user.id).map_or(0, Vec::len);
    println!("{}: {} post(s)", user.name, count);
}
```

```text
Alice: 2 post(s)
Bob: 1 post(s)
Carol: 0 post(s)
```

Two queries in total, however many users there are. The same pattern works for any depth: load the comments of all those posts with a third query.

> **Tip** If the list of ids can be very long (tens of thousands), split it into chunks of a few thousand ids, because databases limit how many parameters one statement may have (32,766 on SQLite, 65,535 on PostgreSQL and MySQL).

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

## Tips and limitations

- **Keep join conditions simple.** A single column equality such as `Post::AUTHOR_ID.eq_col(User::ID)` is the fast path everywhere; the in-memory engine (also used by Redis) runs it as a hash join, and MongoDB only fetches the documents whose join key matches. Put the other conditions in `filter`. (For a left join, a condition in `filter` and the same condition in the join condition give different results: the first removes rows, the second only removes matches.)
- **No self-joins.** A table cannot appear twice in one query (for example "users and their manager, who is also a user"), because Memoire does not generate table aliases. The in-memory engine, Redis and MongoDB return `Error::Unsupported`, and SQL databases reject the statement. Use [raw SQL](raw-sql) with aliases on a SQL database, or two queries.
- **At most five models per tuple.** Joins with `inner_join`/`left_join` return up to five models. `join` (filter only) has no such limit.

## Next steps

- [Transactions](transactions): change related rows together, all or nothing.
- [Raw SQL](raw-sql): for the few queries the builder cannot express.
- [Why it's fast](performance): more ways to keep queries quick.
