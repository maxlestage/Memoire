# Query builder

The query builder lets you describe a `SELECT` in Rust with typed columns. This page walks through filters, sorting, pagination, fetching methods, aggregates, projections and `GROUP BY` reports, and shows the SQL Memoire generates for each database.

## The examples on this page

All examples use these two models and this data:

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

Posts: Alice (user 1) wrote "Hello" (10 views) and "Rust" (30 views), Bob (user 2) wrote "Bob's" (5 views) and Dave (user 4) wrote "Dave" (0 views).

## Building a query

Every query starts with `Model::query()`, which means "all rows of this table". Each method returns a new, refined query, and nothing touches the database until you call a fetching method such as `.all(&db)`:

```rust
let adults = User::query()
    .filter(User::AGE.ge(18))
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
// Alice, Dave, Eve
```

Queries are plain values: you can store them, pass them around and `clone()` them to reuse a common base:

```rust
let base = User::query().filter(User::ACTIVE.eq(true));
let young = base.clone().filter(User::AGE.lt(30)).all(&db).await?; // Bob
let total = base.count(&db).await?; // 3
```

## Filtering

### Comparisons

| Method | SQL | Example |
|---|---|---|
| `eq(v)` | `column = v` | `User::NAME.eq("Bob")` |
| `ne(v)` | `column <> v` | `User::NAME.ne("Bob")` |
| `lt(v)` | `column < v` | `User::AGE.lt(18)` |
| `le(v)` | `column <= v` | `User::AGE.le(18)` |
| `gt(v)` | `column > v` | `User::SCORE.gt(2.0)` |
| `ge(v)` | `column >= v` | `User::AGE.ge(18)` |

The value must match the column's Rust type. For text columns you can pass `&str`, `&String` or `String`.

### Combining conditions

Use `&` for AND, `|` for OR and `!` for NOT. Parentheses work as in normal Rust:

```rust
let q = User::query().filter(User::AGE.ge(18) & (User::NAME.eq("Alice") | User::NAME.eq("Eve")));
// Alice, Eve

let inactive = User::query().filter(!User::ACTIVE.eq(true)).all(&db).await?;
// Carol, Eve
```

Calling `filter` several times combines the conditions with AND, so these two queries are identical:

```rust
let a = User::query().filter(User::ACTIVE.eq(true)).filter(User::AGE.ge(18)).all(&db).await?;
let b = User::query().filter(User::ACTIVE.eq(true) & User::AGE.ge(18)).all(&db).await?;
assert_eq!(a, b);
```

`or_filter` combines with OR: it means "everything filtered so far, **or** this":

```rust
let q = User::query()
    .filter(User::ACTIVE.eq(true))
    .filter(User::AGE.ge(40))
    .or_filter(User::NAME.eq("Eve"));
// WHERE (("users"."active" = ? AND "users"."age" >= ?) OR "users"."name" = ?)
// Dave, Eve
```

> **Tip** When a condition mixes AND and OR, writing it in a single `filter(...)` with explicit parentheses is usually the clearest.

### Lists and ranges

```rust
let some = User::query().filter(User::ID.is_in([1, 3, 5])).all(&db).await?;
// Alice, Carol, Eve

let wanted = vec!["Bob".to_string(), "Eve".to_string()];
let others = User::query().filter(User::NAME.not_in(&wanted)).all(&db).await?;
// Alice, Carol, Dave

let twenties = User::query().filter(User::AGE.between(20, 29)).all(&db).await?;
// Eve
```

| Method | SQL | Notes |
|---|---|---|
| `is_in(values)` | `column IN (...)` | Accepts arrays, vectors, slices and iterators. An empty list matches nothing |
| `not_in(values)` | `column NOT IN (...)` | An empty list matches everything |
| `between(low, high)` | `column BETWEEN low AND high` | Both bounds are included |
| `not_between(low, high)` | `column NOT BETWEEN low AND high` | |

### NULL values

`NULL` means "unknown", and SQL comparisons with an unknown value are neither true nor false. Use the dedicated methods:

```rust
let unknown = User::query().filter(User::AGE.is_null()).all(&db).await?; // Carol
let known = User::query().filter(User::AGE.is_not_null()).count(&db).await?; // 4
```

> **Warning** `!User::AGE.ge(18)` returns only Bob, **not** Carol: her age is `NULL`, so "age >= 18" is unknown and so is its negation. If you want her too, say so: `!User::AGE.ge(18) | User::AGE.is_null()`. You cannot write `User::AGE.eq(None)`; use `is_null()` instead.

### Text matching

| Method | Matches | Case |
|---|---|---|
| `like(pattern)` | SQL pattern: `%` is any text, `_` is one character | Sensitive |
| `not_like(pattern)` | The opposite of `like` | Sensitive |
| `ilike(pattern)` | Like `like` | Insensitive |
| `contains(text)` | `text` anywhere | Sensitive |
| `icontains(text)` | `text` anywhere | Insensitive |
| `starts_with(text)` | Values beginning with `text` | Sensitive |
| `ends_with(text)` | Values ending with `text` | Sensitive |

```rust
let bob = User::query().filter(User::EMAIL.like("_ob@%")).all(&db).await?;
let carol = User::query().filter(User::NAME.icontains("AR")).all(&db).await?;
let dave = User::query().filter(User::NAME.starts_with("Da")).all(&db).await?;
let all = User::query().filter(User::EMAIL.ends_with("@example.com")).count(&db).await?; // 5
```

`contains`, `icontains`, `starts_with` and `ends_with` **escape** `%`, `_` and `\` in the text you pass, so user input is matched literally. Searching for `"50%"` really looks for the characters `50%`:

```rust
let q = User::query().filter(User::NAME.contains("50%"));
// SQL:    ... WHERE "users"."name" LIKE $1 ESCAPE '\'
// value:  "%50\%%"
```

> **Tip** For search boxes and other user input, prefer `contains`, `icontains`, `starts_with` and `ends_with` over `like`: they are safe from accidental wildcards.

Case sensitivity is the same on every backend Memoire runs:

| Backend | `like`, `contains`, ... | `ilike`, `icontains` |
|---|---|---|
| In-memory engine | Case-sensitive | Case-insensitive (Unicode) |
| SQLite | Case-sensitive (Memoire enables `case_sensitive_like`) | Case-insensitive for ASCII letters only (`LOWER()`) |
| PostgreSQL | Case-sensitive | Case-insensitive (native `ILIKE`) |
| MySQL | Depends on the column collation (usually insensitive) | Case-insensitive (`LOWER()`) |

> **Note** On SQLite, `icontains("émi")` does not match `"Émile"`, because SQLite's `LOWER()` only changes ASCII letters. The memory engine and PostgreSQL handle accented letters.

### Comparing two columns

Each comparison has a `_col` twin that compares with another column instead of a value: `eq_col`, `ne_col`, `lt_col`, `le_col`, `gt_col`, `ge_col`. Both columns must have the same Rust type. They are mostly used in join conditions, such as `Post::USER_ID.eq_col(User::ID)` (see [Joins & relations](relations)).

### Building filters dynamically

Because a query is just a value, you can add filters conditionally, for example from optional search parameters:

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

`Expr::all` and `Expr::any` join a list of conditions with AND or OR:

```rust
let names_wanted = ["Alice", "Dave"];
let any = Expr::any(names_wanted.iter().map(|n| User::NAME.eq(*n)));
let found = User::query().filter(any).all(&db).await?; // Alice, Dave

let conditions = vec![User::ACTIVE.eq(true), User::SCORE.gt(1.0)];
let found = User::query().filter(Expr::all(conditions)).all(&db).await?; // Alice, Bob, Dave
```

`Expr::all` of an empty list is always true and `Expr::any` of an empty list is always false.

## Sorting

```rust
let by_age = User::query()
    .order_by(User::AGE.desc())
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
```

- `order_by(User::NAME)` sorts ascending; `.asc()` and `.desc()` make the direction explicit.
- Call `order_by` several times to break ties: the first call is the main sort key.
- Without `order_by`, the database returns rows in no particular order. Always sort when the order matters (pagination especially).

### Where do NULLs go?

Databases disagree on where `NULL` values appear when sorting:

| Backend | `ASC` | `DESC` |
|---|---|---|
| In-memory engine | NULLs last | NULLs first |
| PostgreSQL | NULLs last | NULLs first |
| SQLite | NULLs first | NULLs last |

Make it explicit with `nulls_first()` or `nulls_last()` to get the same result everywhere:

```rust
let q = User::query().order_by(User::AGE.asc().nulls_first()).all(&db).await?;
// Carol, Bob, Eve, Alice, Dave on every backend
```

## Limiting and paging

| Method | Effect |
|---|---|
| `limit(n)` | At most `n` rows |
| `offset(n)` | Skip the first `n` rows |
| `page(page, per_page)` | Shortcut for `limit(per_page).offset((page - 1) * per_page)`; pages start at 1 |

```rust
let rows = User::query().order_by(User::ID).limit(2).offset(2).all(&db).await?; // Carol, Dave
let same = User::query().order_by(User::ID).page(2, 2).all(&db).await?;         // Carol, Dave
```

For a page plus the information a pager needs, use `paginate`. It runs two queries (a `COUNT` and the page itself) and returns a `Page`:

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

| `Page` item | Meaning |
|---|---|
| `items` | The rows of this page |
| `page` | The page number (1-based; `0` is treated as `1`) |
| `per_page` | The page size |
| `total` | Number of matching rows across all pages |
| `total_pages()` | Number of pages |
| `has_next()` / `has_prev()` | Whether there is a page after / before this one |

> **Tip** Large offsets get slow, because the database still walks over every skipped row. For "infinite scroll" style lists, use *keyset pagination* instead: `filter(User::ID.gt(last_seen_id)).order_by(User::ID).limit(20)`. See [Why it's fast](performance).

## Running the query

| Method | Returns | Notes |
|---|---|---|
| `all(&db)` | `Vec<T>` | Every matching row |
| `first(&db)` | `Option<T>` | The first row; adds `LIMIT 1` |
| `one(&db)` | `T` | The first row, or `Error::NotFound` |
| `count(&db)` | `u64` | Number of matching rows; ignores `order_by`, `limit` and `offset` |
| `exists(&db)` | `bool` | Whether at least one row matches |
| `paginate(&db, page, per_page)` | `Page<T>` | See above |

```rust
let oldest: Option<User> = User::query().order_by(User::AGE.desc().nulls_last()).first(&db).await?;
let bob: User = User::query().filter(User::NAME.eq("Bob")).one(&db).await?;
let active: u64 = User::query().filter(User::ACTIVE.eq(true)).count(&db).await?;   // 3
let any_teen: bool = User::query().filter(User::AGE.lt(18)).exists(&db).await?;    // true
```

`&db` can be any database handle or a [transaction](transactions).

## Aggregates

Aggregates compute one value over all matching rows:

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

| Method | Works on | Result type |
|---|---|---|
| `sum(column, &db)` | Numeric columns | `Option<i64>` for integers, `Option<f64>` for floats |
| `avg(column, &db)` | Numeric columns | `Option<f64>` |
| `min(column, &db)` | Any column | `Option<T>` |
| `max(column, &db)` | Any column | `Option<T>` |

The result is `None` when no row matches, or when every value is `NULL`. `NULL` values are ignored by all aggregates (the average age above is computed over the four known ages). Filters and joins apply as usual.

## Selecting only some columns

`columns` replaces "whole rows" with the columns you list. The result type follows automatically:

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

Pass one column or a tuple of up to 8 columns. Nullable columns come back as `Option<T>`. Add `distinct()` to remove duplicate rows:

```rust
let states: Vec<bool> = User::query().columns(User::ACTIVE).distinct().order_by(User::ACTIVE).all(&db).await?;
// [false, true]
```

> **Tip** Selecting only the columns you need sends less data and allocates fewer strings. It is an easy win on wide tables.

## GROUP BY reports

To compute aggregates per group, combine `group_by` with `select_exprs`. `select_exprs` takes a list of *expressions*: turn a column into one with `.expr()`, and use the aggregate helpers below. The row type is written on the variable (or with a turbofish) and decoded by position:

```rust
let per_user: Vec<(i64, i64)> = Post::query()
    .group_by(Post::USER_ID)
    .select_exprs([Post::USER_ID.expr(), count_all()])
    .order_by(Post::USER_ID.asc())
    .all(&db)
    .await?;
// [(1, 2), (2, 1), (4, 1)]: user 1 wrote 2 posts, users 2 and 4 one each
```

| Expression | SQL |
|---|---|
| `count_all()` | `COUNT(*)` |
| `Column::count()` | `COUNT(column)`: counts non-NULL values |
| `Column::count_distinct()` | `COUNT(DISTINCT column)` |
| `Column::sum()` | `SUM(column)` (numeric columns) |
| `Column::avg()` | `AVG(column)` (numeric columns) |
| `Column::min()` / `Column::max()` | `MIN(column)` / `MAX(column)` |

`having` filters groups after aggregation, and you can sort by an aggregate:

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

Without `group_by`, aggregates cover the whole table. `one` is handy for a single summary row:

```rust
let (count, distinct_ages, avg_score): (i64, i64, Option<f64>) = User::query()
    .select_exprs([count_all(), User::AGE.count_distinct(), User::SCORE.avg()])
    .one(&db)
    .await?;
// (5, 4, Some(2.3))
```

Memoire normalises result types so they decode the same everywhere: `COUNT` is an `i64`, `SUM` of integers is an `i64`, `SUM` of floats and every `AVG` are `f64`. Use `Option<...>` for `SUM`, `AVG`, `MIN` and `MAX`, which are `NULL` for empty groups. For bigger reports, decode into a struct with [`#[derive(FromRow)]`](models).

## Expressions and arithmetic

Numeric columns support `add`, `sub`, `mul` and `div`, which build expressions. Expressions (type `Expr`) also have untyped comparison methods (`eq`, `ne`, `lt`, `le`, `gt`, `ge`) that accept another expression, a column or a value wrapped with `Expr::val`:

```rust
let doubled = User::query().filter(User::SCORE.mul(2.0).gt(Expr::val(5.0))).all(&db).await?;
// Carol (3.0 × 2), Dave (4.0 × 2)
```

> **Note** Comparisons on an `Expr` are not type-checked like comparisons on a column, because the expression no longer knows its Rust type. Prefer column methods when you can.

## Seeing the SQL

`to_sql(dialect)` renders the query for PostgreSQL, SQLite or MySQL without running it. `to_sql_with_params` also returns the bound values:

```rust
let q = User::query()
    .filter(User::AGE.ge(18) & User::NAME.contains("li"))
    .order_by(User::NAME.asc())
    .limit(20);

println!("{}", q.to_sql(Dialect::Postgres));
let (sql, params) = q.to_sql_with_params(Dialect::Sqlite);
println!("{sql}\n{params:?}");
```

PostgreSQL:

```sql
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active", "users"."score" FROM "users" WHERE ("users"."age" >= $1 AND "users"."name" LIKE $2 ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 20
```

SQLite:

```sql
SELECT "users"."id", "users"."email", "users"."name", "users"."age", "users"."active", "users"."score" FROM "users" WHERE ("users"."age" >= ? AND "users"."name" LIKE ? ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 20
```

MySQL:

```sql
SELECT `users`.`id`, `users`.`email`, `users`.`name`, `users`.`age`, `users`.`active`, `users`.`score` FROM `users` WHERE (`users`.`age` >= ? AND `users`.`name` LIKE ? ESCAPE '\\') ORDER BY `users`.`name` ASC LIMIT 20
```

Parameters: `[I32(18), Text("%li%")]`.

The dialects differ in a few places, which Memoire handles for you:

| Feature | PostgreSQL | SQLite | MySQL |
|---|---|---|---|
| Placeholders | `$1`, `$2`, ... | `?` | `?` |
| Identifier quotes | `"users"` | `"users"` | `` `users` `` |
| Case-insensitive match | `ILIKE` | `LOWER(col) LIKE LOWER(?)` | `LOWER(col) LIKE LOWER(?)` |
| `nulls_last()` | `NULLS LAST` | `NULLS LAST` | `col IS NULL ASC, col` (emulated) |
| `offset` without `limit` | `OFFSET n` | `LIMIT -1 OFFSET n` | `LIMIT 18446744073709551615 OFFSET n` |
| `SUM` / `AVG` result type | `CAST(... AS BIGINT / DOUBLE PRECISION)` | native | `CAST(... AS SIGNED / DOUBLE)` |

`Update` and `Delete` builders have `to_sql` too (see [Create, read, update, delete](crud)).

## What does not compile

Typed columns catch many mistakes at compile time. Each of these lines is **rejected by the compiler**:

```rust,ignore
// Comparing an integer column with a string:
User::AGE.gt("18");                      // error: `&str: Bind<i32>` is not satisfied

// Comparing a text column with a number:
User::EMAIL.eq(42);                      // error: `{integer}: Bind<String>` is not satisfied

// Text matching on a non-text column:
User::AGE.contains("1");                 // error: method `contains` exists but its trait bounds were not satisfied

// A typo in a column name:
User::EMIAL.eq("a@b.c");                 // error: no associated function or constant named `EMIAL`

// Comparing with None (use `is_null()`):
User::AGE.eq(None);                      // error: `Option<_>: Bind<i32>` is not satisfied

// Summing a text column:
User::query().sum(User::NAME, &db);      // error: `String: Numeric` is not satisfied

// Updating a column of another model:
User::update_many().set(Post::TITLE, "x"); // error: expected `Column<User, _>`, found `Column<Post, String>`

// Setting NULL on a non-nullable column:
User::update_many().set_null(User::NAME);  // error: expected `Column<User, Option<_>>`

// Looking up an i64 key with a string:
User::find(&db, "1");                    // error: `&str: Bind<i64>` is not satisfied
```

## Next steps

- [Create, read, update, delete](crud): `update_many`, `delete_many` and the per-row operations.
- [Joins & relations](relations): query several tables at once.
- [Cheat sheet](cheatsheet): every operation on one page.
