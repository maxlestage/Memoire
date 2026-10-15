# Filtros de relaciones, conteos y polimorfismo

La [página de relaciones](relations) carga filas relacionadas. Esta va más allá, con las herramientas que popularizó Eloquent de Laravel: filtrar por relaciones (`has`, `whereHas`), contar filas relacionadas (`withCount`), carga anticipada en una llamada (`with`), relaciones polimórficas (`morphMany`, `morphTo`) y las columnas extra de una tabla intermedia muchos-a-muchos (`withPivot`). Todo se deduce de las claves foráneas que ya declaras y funciona en todas las bases de datos.

Los ejemplos usan estos modelos:

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

## Filtrar por relaciones

| Método | Conserva las filas... | Eloquent |
|---|---|---|
| `has::<R>()` | con al menos un `R` relacionado | `has('posts')` |
| `doesnt_have::<R>()` | sin ningún `R` relacionado | `doesntHave('posts')` |
| `where_has::<R>(\|q\| ...)` | con al menos un `R` relacionado que cumple la consulta | `whereHas('posts', fn)` |
| `where_doesnt_have::<R>(\|q\| ...)` | sin ningún `R` relacionado que cumpla la consulta | `whereDoesntHave` |
| `has_at_least::<R>(n)` | con `n` `R` relacionados o más | `has('posts', '>=', n)` |
| `where_has_via::<J, T>(\|q\| ...)` | enlazadas por la tabla intermedia `J` a un `T` que cumple la consulta | `whereHas` en un `belongsToMany` |

```rust
// Los usuarios que publicaron algo.
let writers = User::query()
    .where_has::<Post>(|posts| posts.filter(Post::PUBLISHED.eq(true)))
    .order_by(User::NAME)
    .all(&db)
    .await?;

// Los usuarios sin ningún post, y los prolíficos.
let silent = User::query().doesnt_have::<Post>().all(&db).await?;
let prolific = User::query().has_at_least::<Post>(10).count(&db).await?;
```

La relación funciona en los dos sentidos: `Post::query().has::<User>()` conserva los posts que pertenecen a un usuario, y `Post::query().doesnt_have::<User>()` los que tienen `author_id` a `NULL` o apuntando a nada. Cada filtro es una [subconsulta](queries) (`"users"."id" IN (SELECT "posts"."author_id" FROM "posts" WHERE ...)`): una sentencia en las bases de datos SQL, dos en el motor en memoria, Redis y MongoDB. La closure recibe `R::query()`, así que las filas con [borrado lógico](lifecycle) quedan fuera salvo que llames a `with_trashed()` sobre ella.

Los modelos deben estar relacionados por exactamente una clave foránea; si no, el método hace panic con un mensaje que dice qué falta (en ese caso filtra tú por la clave foránea con `in_query`).

## Contar filas relacionadas

`load_count` cuenta los hijos de cada fila de un slice con una sola consulta `GROUP BY`, sin cargarlos; `load_count_with` cuenta solo los que cumplen una consulta. `all_with_count` ejecuta tu consulta y empareja cada fila con su total:

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<u64> = users.load_count::<Post>(&db).await?;          // alineado con `users`
let drafts = users.load_count_with(&db, Post::query().filter(Post::PUBLISHED.eq(false))).await?;

for (user, n) in User::query().all_with_count::<Post>(&db).await? {
    println!("{} escribió {n} posts", user.name);
}
```

## Carga anticipada en una llamada

`all_with::<C>()` ejecuta la consulta y carga los hijos de cada fila, `all_with_parent::<P>()` el padre de cada fila: dos consultas, sea cual sea el número de filas.

```rust
for (user, posts) in User::query().all_with::<Post>(&db).await? { /* ... */ }
for (post, author) in Post::query().all_with_parent::<User>(&db).await? { /* author: Option<User> */ }
```

Para cargar niveles más profundos, llama a [`load_children`](relations) sobre los hijos cargados: una consulta más por nivel.

## Relaciones polimórficas

Un comentario puede pertenecer a un post *o* a un vídeo. Declara `#[memoire(morph = "commentable")]` en el modelo con dos campos, `commentable_type` (un `String`, el **nombre de tabla** del padre) y `commentable_id` (su clave primaria):

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

let mut c = Comment { id: 0, commentable_type: String::new(), commentable_id: 0, body: "¡Genial!".into() };
c.set_morph_parent(&post)?;          // commentable_type = "posts", commentable_id = post.id
c.insert(&db).await?;

let on_post = Comment::morphed_by(&post).all(&db).await?;           // morphMany, como consulta
let parent: Option<Post> = c.morph_parent(&db).await?;               // morphTo (None si no es un post)
let per_post = posts.load_morph_many::<Comment>(&db).await?;        // anticipada, una consulta
let parents: Vec<Option<Post>> = comments.load_morph_parent(&db).await?;
```

Si falta el campo `commentable_type` o `commentable_id`, el código no compila. La base de datos no puede comprobar un enlace así (no hay clave foránea): borra tú los comentarios junto con su padre.

## Muchos-a-muchos con columnas pivote

Un modelo intermedio puede llevar sus propias columnas, un rol o una fecha. `load_via_pivot` devuelve cada pareja con la fila intermedia que la enlaza; `load_via_pivot_with` parte de tu consulta sobre la tabla intermedia (`wherePivot`, `orderByPivot`):

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
    println!("{} en {} como {}", users[0].name, team.name, membership.role);
}
let led = users
    .load_via_pivot_with::<Membership, Team>(&db, Membership::query().filter(Membership::ROLE.eq("lead")))
    .await?;
```

## Próximos pasos

- [Factories y seeders](factories): llena una base de datos para tests en pocas líneas.
- [Bloqueo optimista e identity map](optimistic-locking): escrituras concurrentes seguras.
