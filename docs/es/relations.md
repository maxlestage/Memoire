# Uniones y relaciones

Los datos reales están repartidos en varias tablas: los usuarios escriben posts, los posts reciben comentarios. Esta página muestra cómo modelar esos vínculos, cómo consultar varias tablas a la vez con joins y cómo cargar las filas relacionadas de forma eficiente.

## Modelar una relación uno a muchos

"Un usuario tiene muchos posts" se almacena dando a cada post el id de su autor. Declara esa columna con `references` para que la base de datos conozca el vínculo:

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

Qué hacen los atributos de `author_id`:

- `references = "users(id)"` crea una clave foránea: la base de datos rechaza un post cuyo `author_id` no corresponda a un usuario existente (`Error::Constraint`).
- `on_delete = "cascade"` elimina automáticamente los posts de un usuario cuando se elimina ese usuario.
- `index` hace que "buscar los posts de este usuario" sea rápido. Las columnas de clave foránea casi siempre merecen un índice.

Crea primero las tablas padre, porque una clave foránea solo puede apuntar a una tabla que ya exista:

```rust
db.create_table::<User>().await?;
db.create_table::<Post>().await?;
db.create_table::<Comment>().await?;
```

Los ejemplos siguientes usan estos datos:

| Usuario | Posts |
|---|---|
| Alice (1) | "Hello" (120 visitas), "Rust" (30 visitas) |
| Bob (2) | "Bob's" (5 visitas) |
| Carol (3) | ninguno |

Comentarios: "Nice" y "Great" en "Hello", "Hmm" en "Bob's".

> **Nota** SQLite, PostgreSQL y MySQL aplican las claves foráneas. El motor en memoria, Redis y MongoDB aceptan la declaración, pero no la comprueban ni eliminan en cascada, así que prueba el comportamiento en cascada en un backend SQL.

## Inner join: filas que tienen correspondencia

`inner_join::<P>(condition)` añade el modelo `P` a la consulta. El resultado pasa a ser una tupla: primero el modelo de partida y después cada modelo unido, en orden.

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

La condición indica cómo se corresponden las filas: `User::ID.eq_col(Post::AUTHOR_ID)` significa "el id del usuario es igual al `author_id` del post". `eq_col` compara dos columnas del mismo tipo. Una vez hecho el join, puedes filtrar y ordenar por las columnas de cualquier tabla de la consulta.

Un inner join solo devuelve las combinaciones que se corresponden: Carol no tiene ningún post, así que no aparece.

## Left join: conservar las filas sin correspondencia

`left_join` conserva todas las filas de la tabla de partida. Cuando no hay correspondencia, el modelo unido es `None`:

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

Fíjate en que Alice aparece dos veces: un join produce una fila por cada *combinación*. Para encontrar los usuarios que no tienen ningún post, quédate solo con las filas en las que falta el post:

```rust
let lonely: Vec<(User, Option<Post>)> = User::query()
    .left_join::<Post>(Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::ID.is_null())
    .all(&db)
    .await?;
// [(Carol, None)]
```

## Unir más de dos tablas

Encadena joins para seguir los vínculos más allá. Cada join añade un elemento a la tupla:

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

De esta forma, una consulta puede devolver hasta cinco modelos (el de partida más cuatro joins).

## Join solo para filtrar

A veces quieres las filas de una tabla filtradas según otra tabla, sin recuperar esa otra tabla. Usa `join` con un `JoinKind`: el tipo del resultado no cambia. Añade `distinct()` para que un usuario con varios posts que coinciden aparezca una sola vez:

```rust
let popular_authors: Vec<User> = User::query()
    .join::<Post>(JoinKind::Inner, Post::AUTHOR_ID.eq_col(User::ID))
    .filter(Post::VIEWS.ge(100))
    .distinct()
    .all(&db)
    .await?;
// [Alice]
```

Están disponibles `JoinKind::Inner` y `JoinKind::Left`.

## Elegir columnas de varias tablas

Después de los joins, `columns` puede mezclar columnas de todas las tablas de la consulta:

```rust
let pairs: Vec<(String, String)> = Post::query()
    .inner_join::<User>(User::ID.eq_col(Post::AUTHOR_ID))
    .columns((User::NAME, Post::TITLE))
    .order_by(Post::ID)
    .all(&db)
    .await?;
// [("Alice", "Hello"), ("Alice", "Rust"), ("Bob", "Bob's")]
```

Llama a `columns` después de los joins.

## Contar los hijos

Combina un left join con `group_by` para contar las filas relacionadas, incluido el cero:

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

`Post::ID.count()` cuenta los ids de post que no son NULL; por eso Carol obtiene `0` y no `1`.

## Los hijos de una fila

`belonging_to` inicia una consulta sobre las filas que apuntan a una fila dada. Memoire encuentra el vínculo gracias al atributo `references`: no hay nada más que declarar, y la consulta se puede seguir refinando:

```rust
let alice = User::get(&db, 1).await?;
let posts: Vec<Post> = Post::belonging_to(&alice)
    .filter(Post::VIEWS.ge(10))
    .order_by(Post::VIEWS.desc())
    .all(&db)
    .await?;
// ["Hello", "Rust"]
```

En sentido contrario, `find_parent` sigue una clave foránea:

```rust
let post = Post::get(&db, 3).await?;
let author: Option<User> = post.find_parent::<User, _>(&db).await?; // Some(Bob)
```

Devuelve `None` cuando la clave foránea es `NULL` (una columna `Option<i64>`) o apunta a una fila que ya no existe.

## Cargar los hijos sin el problema N+1

Una necesidad muy común: mostrar cada usuario con sus artículos. La versión tentadora lanza una consulta por usuario:

```rust
// Lento: 1 consulta para los usuarios + 1 consulta por usuario ("N+1 consultas").
let users = User::all(&db).await?;
for user in &users {
    let posts = Post::belonging_to(user).all(&db).await?;
    println!("{}: {} artículo(s)", user.name, posts.len());
}
```

Con 1.000 usuarios son 1.001 idas y vueltas a la base de datos. `load_children` carga los hijos de toda una lista en **una sola** consulta (`WHERE author_id IN (...)`) y los devuelve alineados con la lista: `posts[i]` contiene los artículos de `users[i]`.

```rust
let users = User::query().order_by(User::ID).all(&db).await?;
let posts: Vec<Vec<Post>> = users.load_children(&db).await?;

for (user, posts) in users.iter().zip(&posts) {
    println!("{}: {} artículo(s)", user.name, posts.len());
}
```

```text
Alice: 2 artículo(s)
Bob: 1 artículo(s)
Carol: 0 artículo(s)
```

Dos consultas en total, sea cual sea el número de usuarios. En PostgreSQL, para 100 usuarios con 10 artículos cada uno, son 1,1 ms en lugar de 13,5 ms (×12) en la misma máquina, y la diferencia crece con la latencia de red hacia la base.

Todos los cargadores vienen del trait `LoadRelated` (en el preludio) y funcionan con slices y `Vec`:

| Método | Devuelve | Consultas |
|---|---|---|
| `users.load_children::<Post>(&db)` | `Vec<Vec<Post>>`, hijos ordenados por clave primaria | 1 |
| `users.load_children_with(&db, Post::query()...)` | `Vec<Vec<Post>>`, en el orden de tu consulta | 1 |
| `posts.load_parent::<User>(&db)` | `Vec<Option<User>>`, uno por artículo | 1 |
| `posts.load_via::<PostTag, Tag>(&db)` | `Vec<Vec<Tag>>` (muchos a muchos, más abajo) | 2 |

`load_children_with` recibe tu propia consulta sobre los hijos, para filtrarlos u ordenarlos:

```rust
let popular: Vec<Vec<Post>> = users
    .load_children_with(&db, Post::query().filter(Post::VIEWS.ge(100)).order_by(Post::VIEWS.desc()))
    .await?;
```

`LIMIT` y `OFFSET` en esa consulta se aplican a todos los hijos a la vez, no a cada usuario.

`load_parent` va en el otro sentido: una consulta para los autores de todos los artículos, cada autor compartido por los artículos que apuntan a él (el modelo padre debe implementar `Clone`):

```rust
let posts = Post::all(&db).await?;
let authors: Vec<Option<User>> = posts.load_parent(&db).await?;
for (post, author) in posts.iter().zip(&authors) {
    println!("{} de {}", post.title, author.as_ref().map_or("?", |a| a.name.as_str()));
}
```

Los cargadores se encadenan a cualquier profundidad (usuarios → artículos → comentarios son tres consultas), y las listas muy largas se dividen automáticamente bajo el límite de parámetros de la base (32.766 en SQLite, 65.535 en PostgreSQL y MySQL).

> **Nota** La relación viene de la clave foránea. Si el hijo no tiene ninguna columna con `references` hacia la tabla del padre, o tiene varias (un `author_id` y un `reviewer_id` que apuntan ambos a `users`), los cargadores fallan con `Error::Relation` y `belonging_to` entra en pánico. En ese caso filtra tú mismo por la columna que quieres: `Post::query().filter(Post::REVIEWER_ID.is_in(...))`.

## Relaciones muchos a muchos

"Un post tiene muchas etiquetas y una etiqueta está en muchos posts" necesita una tabla de enlace con una fila por cada par (post, etiqueta):

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

Los modelos de Memoire tienen una clave primaria de una sola columna, así que la tabla de enlace tiene su propio `id`. Si quieres que la base de datos prohíba los pares duplicados, añade un índice único sobre ambas columnas con una [migración](migrations) SQL.

`load_via` carga los socios muchos a muchos de toda una lista en dos consultas, una sobre la tabla de enlace y otra sobre los destinos:

```rust
let posts = Post::query().order_by(Post::ID).all(&db).await?;
let tags: Vec<Vec<Tag>> = posts.load_via::<PostTag, Tag>(&db).await?;
// tags[i] contiene las etiquetas de posts[i], en el orden en que se crearon los enlaces
```

El modelo de enlace debe referenciar ambas tablas; funciona en los dos sentidos (`tags.load_via::<PostTag, Post>(&db)`).

## Consejos y limitaciones

- **Mantén simples las condiciones de los joins.** Una igualdad entre dos columnas como `Post::AUTHOR_ID.eq_col(User::ID)` es el camino rápido en todas partes; el motor en memoria (que también usa Redis) la ejecuta como un hash join, y MongoDB solo recupera los documentos cuya clave de join coincide. Pon las demás condiciones en `filter`. (En un left join, una condición en `filter` y la misma condición en la condición del join dan resultados distintos: la primera elimina filas, la segunda solo elimina correspondencias).
- **Sin auto-joins.** Una tabla no puede aparecer dos veces en la misma consulta (por ejemplo, "los usuarios y su responsable, que también es un usuario"), porque Memoire no genera alias de tabla. El motor en memoria, Redis y MongoDB devuelven `Error::Unsupported`, y las bases de datos SQL rechazan la sentencia. Usa [SQL sin procesar](raw-sql) con alias en una base de datos SQL, o dos consultas.
- **Como máximo cinco modelos por tupla.** Los joins con `inner_join`/`left_join` devuelven hasta cinco modelos. `join` (solo para filtrar) no tiene ese límite.

## Próximos pasos

- [Filtros de relaciones, conteos y polimorfismo](relations-advanced): `where_has`, `load_count`, `all_with`, relaciones polimórficas y columnas pivote.
- [Transacciones](transactions): cambia filas relacionadas a la vez, todo o nada.
- [SQL sin procesar](raw-sql): para las pocas consultas que el constructor no puede expresar.
- [Por qué es rápido](performance): más formas de mantener rápidas las consultas.
