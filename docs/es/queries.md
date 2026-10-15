# Constructor de consultas

El constructor de consultas te permite describir un `SELECT` en Rust con columnas tipadas. Esta página recorre los filtros, la ordenación, la paginación, los métodos de obtención, los agregados, las proyecciones y los informes con `GROUP BY`, y muestra el SQL que Memoire genera para cada base de datos.

## Los ejemplos de esta página

Todos los ejemplos usan estos dos modelos y estos datos:

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

Posts: Alice (usuario 1) escribió "Hello" (10 visitas) y "Rust" (30 visitas), Bob (usuario 2) escribió "Bob's" (5 visitas) y Dave (usuario 4) escribió "Dave" (0 visitas).

## Construir una consulta

Toda consulta empieza con `Model::query()`, que significa "todas las filas de esta tabla". Cada método devuelve una consulta nueva y más refinada, y nada toca la base de datos hasta que llamas a un método de obtención como `.all(&db)`:

```rust
let adults = User::query()
    .filter(User::AGE.ge(18))
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
// Alice, Dave, Eve
```

Las consultas son valores normales: puedes guardarlas, pasarlas de un lado a otro y hacerles `clone()` para reutilizar una base común:

```rust
let base = User::query().filter(User::ACTIVE.eq(true));
let young = base.clone().filter(User::AGE.lt(30)).all(&db).await?; // Bob
let total = base.count(&db).await?; // 3
```

## Filtrar

### Comparaciones

| Método | SQL | Ejemplo |
|---|---|---|
| `eq(v)` | `column = v` | `User::NAME.eq("Bob")` |
| `ne(v)` | `column <> v` | `User::NAME.ne("Bob")` |
| `lt(v)` | `column < v` | `User::AGE.lt(18)` |
| `le(v)` | `column <= v` | `User::AGE.le(18)` |
| `gt(v)` | `column > v` | `User::SCORE.gt(2.0)` |
| `ge(v)` | `column >= v` | `User::AGE.ge(18)` |

El valor debe coincidir con el tipo de Rust de la columna. Para las columnas de texto puedes pasar `&str`, `&String` o `String`.

### Combinar condiciones

Usa `&` para AND, `|` para OR y `!` para NOT. Los paréntesis funcionan como en Rust normal:

```rust
let q = User::query().filter(User::AGE.ge(18) & (User::NAME.eq("Alice") | User::NAME.eq("Eve")));
// Alice, Eve

let inactive = User::query().filter(!User::ACTIVE.eq(true)).all(&db).await?;
// Carol, Eve
```

Llamar a `filter` varias veces combina las condiciones con AND, así que estas dos consultas son idénticas:

```rust
let a = User::query().filter(User::ACTIVE.eq(true)).filter(User::AGE.ge(18)).all(&db).await?;
let b = User::query().filter(User::ACTIVE.eq(true) & User::AGE.ge(18)).all(&db).await?;
assert_eq!(a, b);
```

`or_filter` combina con OR: significa "todo lo filtrado hasta ahora, **o** esto":

```rust
let q = User::query()
    .filter(User::ACTIVE.eq(true))
    .filter(User::AGE.ge(40))
    .or_filter(User::NAME.eq("Eve"));
// WHERE (("users"."active" = ? AND "users"."age" >= ?) OR "users"."name" = ?)
// Dave, Eve
```

> **Consejo** Cuando una condición mezcla AND y OR, escribirla en un solo `filter(...)` con paréntesis explícitos suele ser lo más claro.

### Listas y rangos

```rust
let some = User::query().filter(User::ID.is_in([1, 3, 5])).all(&db).await?;
// Alice, Carol, Eve

let wanted = vec!["Bob".to_string(), "Eve".to_string()];
let others = User::query().filter(User::NAME.not_in(&wanted)).all(&db).await?;
// Alice, Carol, Dave

let twenties = User::query().filter(User::AGE.between(20, 29)).all(&db).await?;
// Eve
```

| Método | SQL | Notas |
|---|---|---|
| `is_in(values)` | `column IN (...)` | Acepta arrays, vectores, slices e iteradores. Una lista vacía no coincide con nada |
| `not_in(values)` | `column NOT IN (...)` | Una lista vacía coincide con todo |
| `between(low, high)` | `column BETWEEN low AND high` | Ambos límites están incluidos |
| `not_between(low, high)` | `column NOT BETWEEN low AND high` | |

### Valores NULL

`NULL` significa "desconocido", y las comparaciones SQL con un valor desconocido no son ni verdaderas ni falsas. Usa los métodos específicos:

```rust
let unknown = User::query().filter(User::AGE.is_null()).all(&db).await?; // Carol
let known = User::query().filter(User::AGE.is_not_null()).count(&db).await?; // 4
```

> **Atención** `!User::AGE.ge(18)` devuelve solo a Bob, **no** a Carol: su edad es `NULL`, así que "age >= 18" es desconocido y su negación también. Si también la quieres a ella, dilo explícitamente: `!User::AGE.ge(18) | User::AGE.is_null()`. No puedes escribir `User::AGE.eq(None)`; usa `is_null()` en su lugar.

### Búsqueda de texto

| Método | Coincide con | Mayúsculas/minúsculas |
|---|---|---|
| `like(pattern)` | Patrón SQL: `%` es cualquier texto, `_` es un carácter | Las distingue |
| `not_like(pattern)` | Lo contrario de `like` | Las distingue |
| `ilike(pattern)` | Igual que `like` | No las distingue |
| `contains(text)` | `text` en cualquier posición | Las distingue |
| `icontains(text)` | `text` en cualquier posición | No las distingue |
| `starts_with(text)` | Valores que empiezan por `text` | Las distingue |
| `ends_with(text)` | Valores que terminan en `text` | Las distingue |

```rust
let bob = User::query().filter(User::EMAIL.like("_ob@%")).all(&db).await?;
let carol = User::query().filter(User::NAME.icontains("AR")).all(&db).await?;
let dave = User::query().filter(User::NAME.starts_with("Da")).all(&db).await?;
let all = User::query().filter(User::EMAIL.ends_with("@example.com")).count(&db).await?; // 5
```

`contains`, `icontains`, `starts_with` y `ends_with` **escapan** `%`, `_` y `\` en el texto que les pasas, así que la entrada del usuario se busca de forma literal. Buscar `"50%"` busca de verdad los caracteres `50%`:

```rust
let q = User::query().filter(User::NAME.contains("50%"));
// SQL:    ... WHERE "users"."name" LIKE $1 ESCAPE '\'
// valor:  "%50\%%"
```

> **Consejo** Para cuadros de búsqueda y cualquier otra entrada del usuario, es preferible usar `contains`, `icontains`, `starts_with` y `ends_with` en lugar de `like`: están a salvo de comodines accidentales.

En un patrón `like` escrito a mano, `\` escapa el carácter siguiente en todos los backends (Memoire siempre emite `ESCAPE '\'`). Para mezclar tus propios comodines con la entrada del usuario, escapa la entrada con `memoire_orm::escape_like`:

```rust
let input = "50%";
let q = User::query().filter(User::NAME.like(format!("{}%", memoire_orm::escape_like(input))));
// coincide con los nombres que empiezan por los cuatro caracteres `50%`
```

La distinción entre mayúsculas y minúsculas es la misma en todos los backends en los que funciona Memoire:

| Backend | `like`, `contains`, ... | `ilike`, `icontains` |
|---|---|---|
| Motor en memoria | Distingue mayúsculas y minúsculas | No las distingue (Unicode) |
| SQLite | Distingue mayúsculas y minúsculas (Memoire activa `case_sensitive_like`) | No las distingue, solo para letras ASCII (`LOWER()`) |
| PostgreSQL | Distingue mayúsculas y minúsculas | No las distingue (`ILIKE` nativo) |
| MySQL / MariaDB | Distingue mayúsculas y minúsculas (Memoire crea las tablas con `utf8mb4_bin`) | No las distingue (`LOWER()`) |
| Redis | Distingue mayúsculas y minúsculas (el mismo motor que en memoria) | No las distingue (Unicode) |
| MongoDB | Distingue mayúsculas y minúsculas (expresión regular anclada) | No las distingue (expresión regular con la opción `i`, Unicode) |

> **Nota** En SQLite, `icontains("émi")` no coincide con `"Émile"`, porque el `LOWER()` de SQLite solo cambia las letras ASCII. Todos los demás backends gestionan las letras acentuadas.

### Búsqueda de texto completo

`matches` encuentra las filas cuyo texto contiene **todas las palabras** de una búsqueda, como palabras completas, sin distinguir mayúsculas. Las palabras son secuencias de letras y dígitos; todo lo demás (espacios, puntuación, `_`, `@`...) las separa.

```rust
let posts = Post::query()
    .filter(Post::BODY.matches("rust orm"))
    .order_by(Post::ID)
    .all(&db)
    .await?;
```

| Texto | `matches("rust orm")` |
|---|---|
| "An ORM for Rust!" | sí |
| "rust-orm" | sí |
| "Rusty ORM" | no ("Rusty" es otra palabra) |
| "Rust" | no (falta "orm") |
| `NULL` | no |

Una búsqueda sin ninguna palabra (`""`, `"?!"`) no encuentra nada. `matches` se combina con cualquier otra condición, `!` incluido: `!Post::BODY.matches("borrador")` conserva los textos sin esa palabra.

La regla es la misma en todas las bases; solo cambia la velocidad:

| Base | Cómo | Con `#[memoire(fulltext)]` |
|---|---|---|
| Motor en memoria | Palabras comparadas en Rust | Índice de palabras: solo se revisan las filas que contienen la palabra más rara |
| SQLite | Una función `memoire_match` registrada en la conexión | Las filas se revisan una a una |
| PostgreSQL | `to_tsvector('simple', ...) @@ plainto_tsquery('simple', ...)` | Índice GIN sobre la misma expresión |
| MySQL / MariaDB | Un `REGEXP` por palabra | Las filas se revisan una a una |
| Redis | El mismo motor que en memoria | Las filas se cargan y luego se revisan |
| MongoDB | Un `$regex` por palabra, ejecutado por el servidor | Los documentos se revisan uno a uno |

> **Nota** No hay lematización ni palabras vacías: `matches("corriendo")` no encuentra "correr". En PostgreSQL, las letras fuera de ASCII necesitan una base con una locale UTF-8 (el valor por defecto casi en todas partes, incluida la imagen oficial de Docker).

### Comparar dos columnas

Cada comparación tiene una gemela `_col` que compara con otra columna en lugar de con un valor: `eq_col`, `ne_col`, `lt_col`, `le_col`, `gt_col`, `ge_col`. Ambas columnas deben tener el mismo tipo de Rust. Se usan sobre todo en las condiciones de los joins, como `Post::USER_ID.eq_col(User::ID)` (consulta [Uniones y relaciones](relations)).

### Construir filtros de forma dinámica

Como una consulta no es más que un valor, puedes añadir filtros de forma condicional, por ejemplo a partir de parámetros de búsqueda opcionales:

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

`Expr::all` y `Expr::any` unen una lista de condiciones con AND o con OR:

```rust
let names_wanted = ["Alice", "Dave"];
let any = Expr::any(names_wanted.iter().map(|n| User::NAME.eq(*n)));
let found = User::query().filter(any).all(&db).await?; // Alice, Dave

let conditions = vec![User::ACTIVE.eq(true), User::SCORE.gt(1.0)];
let found = User::query().filter(Expr::all(conditions)).all(&db).await?; // Alice, Bob, Dave
```

`Expr::all` de una lista vacía siempre es verdadero y `Expr::any` de una lista vacía siempre es falso.

### Consultas condicionales y ámbitos reutilizables

`when`, `when_some` y `apply` mantienen la cadena de llamadas en lugar de reasignar una variable, como `when()` y los ámbitos locales (local scopes) de Laravel:

```rust
fn active(q: Select<User>) -> Select<User> {
    q.filter(User::ACTIVE.eq(true)) // un «ámbito» reutilizable
}

let users = User::query()
    .apply(active)
    .when_some(min_age, |q, min| q.filter(User::AGE.ge(min)))
    .when_some(search, |q, text| q.filter(User::NAME.icontains(text)))
    .when(newest_first, |q| q.order_by(User::ID.desc()))
    .all(&db)
    .await?;
```

| Método | Efecto |
|---|---|
| `apply(ámbito)` | `ámbito(consulta)`: cualquier `fn(Select<T>) -> Select<T>` |
| `when(condición, \|q\| ...)` | Aplica la closure solo si `condición` es verdadera |
| `when_some(opción, \|q, valor\| ...)` | Aplica la closure con el valor, solo si lo hay |

`Select` es `memoire_orm::Select`.

## Ordenar

```rust
let by_age = User::query()
    .order_by(User::AGE.desc())
    .order_by(User::NAME.asc())
    .all(&db)
    .await?;
```

- `order_by(User::NAME)` ordena de forma ascendente; `.asc()` y `.desc()` hacen explícita la dirección.
- Llama a `order_by` varias veces para desempatar: la primera llamada es la clave de ordenación principal.
- Sin `order_by`, la base de datos devuelve las filas sin ningún orden concreto. Ordena siempre cuando el orden importe (sobre todo al paginar).

### ¿Dónde van los NULL?

Las bases de datos no se ponen de acuerdo sobre dónde aparecen los valores `NULL` al ordenar:

| Backend | `ASC` | `DESC` |
|---|---|---|
| Motor en memoria | NULL al final | NULL al principio |
| PostgreSQL | NULL al final | NULL al principio |
| SQLite | NULL al principio | NULL al final |
| MySQL / MariaDB | NULL al principio | NULL al final |
| Redis | NULL al final | NULL al principio |
| MongoDB | NULL al principio | NULL al final |

Hazlo explícito con `nulls_first()` o `nulls_last()` para obtener el mismo resultado en todas partes:

```rust
let q = User::query().order_by(User::AGE.asc().nulls_first()).all(&db).await?;
// Carol, Bob, Eve, Alice, Dave en todos los backends
```

## Limitar y paginar

| Método | Efecto |
|---|---|
| `limit(n)` | Como máximo `n` filas |
| `offset(n)` | Se salta las `n` primeras filas |
| `page(page, per_page)` | Atajo de `limit(per_page).offset((page - 1) * per_page)`; las páginas empiezan en 1 |

```rust
let rows = User::query().order_by(User::ID).limit(2).offset(2).all(&db).await?; // Carol, Dave
let same = User::query().order_by(User::ID).page(2, 2).all(&db).await?;         // Carol, Dave
```

Para obtener una página junto con la información que necesita un paginador, usa `paginate`. Ejecuta dos consultas (un `COUNT` y la propia página) y devuelve un `Page`:

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

| Elemento de `Page` | Significado |
|---|---|
| `items` | Las filas de esta página |
| `page` | El número de página (empieza en 1; `0` se trata como `1`) |
| `per_page` | El tamaño de página |
| `total` | Número de filas que coinciden en todas las páginas |
| `total_pages()` | Número de páginas |
| `has_next()` / `has_prev()` | Si hay una página después / antes de esta |

### Paginación por cursor

Los offsets grandes se vuelven lentos, porque la base sigue recorriendo cada fila que se salta. `cursor_paginate` pagina por clave primaria (`WHERE id > <cursor> ORDER BY id LIMIT n`), como `cursorPaginate` de Laravel: cada página cuesta lo mismo, y las filas insertadas mientras tanto no desplazan las páginas. Es ideal para el «scroll infinito» y las API:

```rust
let first = User::query().filter(User::ACTIVE.eq(true)).cursor_paginate(&db, 20, None).await?;
// first.items: hasta 20 usuarios; first.next_cursor: Some(clave del último), o None al final
if first.has_more() {
    let next = User::query().filter(User::ACTIVE.eq(true)).cursor_paginate(&db, 20, first.next_cursor).await?;
}
```

El cursor es un `Value` (la clave primaria de la última fila): envíalo al cliente y vuelve a convertirlo en `Value` (`Value::I64(id)`) en la siguiente petición. El `ORDER BY` y el `OFFSET` de la consulta se sustituyen por el orden de las claves.

### Leer por bloques

Para procesar una tabla grande sin cargarla de golpe (una exportación, una migración de datos), léela por bloques, por clave primaria, como `chunkById` de Laravel o `toIterable()` de Doctrine:

```rust
let mut chunks = User::query().filter(User::ACTIVE.eq(true)).chunks(500);
while let Some(users) = chunks.next(&db).await? {
    for user in users {
        // como mucho 500 usuarios en memoria a la vez
    }
}

// O con una closure; devuelve el número de filas leídas.
let n = User::query().chunks(500).for_each(&db, |users| { /* ... */ Ok(()) }).await?;
```

Cada bloque es una consulta que empieza después de la última clave del anterior: modificar o borrar filas durante la lectura no salta ni repite ninguna.

## Ejecutar la consulta

| Método | Devuelve | Notas |
|---|---|---|
| `all(&db)` | `Vec<T>` | Todas las filas que coinciden |
| `first(&db)` | `Option<T>` | La primera fila; añade `LIMIT 1` |
| `one(&db)` | `T` | La primera fila, o `Error::NotFound` |
| `count(&db)` | `u64` | Número de filas que coinciden; ignora `order_by`, `limit` y `offset` |
| `exists(&db)` | `bool` | Si al menos una fila coincide |
| `paginate(&db, page, per_page)` | `Page<T>` | Consulta más arriba |

```rust
let oldest: Option<User> = User::query().order_by(User::AGE.desc().nulls_last()).first(&db).await?;
let bob: User = User::query().filter(User::NAME.eq("Bob")).one(&db).await?;
let active: u64 = User::query().filter(User::ACTIVE.eq(true)).count(&db).await?;   // 3
let any_teen: bool = User::query().filter(User::AGE.lt(18)).exists(&db).await?;    // true
```

`&db` puede ser cualquier manejador de base de datos o una [transacción](transactions).

## Agregados

Los agregados calculan un único valor sobre todas las filas que coinciden:

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

| Método | Funciona con | Tipo del resultado |
|---|---|---|
| `sum(column, &db)` | Columnas numéricas | `Option<i64>` para enteros, `Option<f64>` para flotantes |
| `avg(column, &db)` | Columnas numéricas | `Option<f64>` |
| `min(column, &db)` | Cualquier columna | `Option<T>` |
| `max(column, &db)` | Cualquier columna | `Option<T>` |

El resultado es `None` cuando ninguna fila coincide o cuando todos los valores son `NULL`. Todos los agregados ignoran los valores `NULL` (la edad media de arriba se calcula sobre las cuatro edades conocidas). Los filtros y los joins se aplican como de costumbre.

## Seleccionar solo algunas columnas

`columns` sustituye las "filas completas" por las columnas que indiques. El tipo del resultado se deduce automáticamente:

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

Pasa una columna o una tupla de hasta 8 columnas. Las columnas que admiten nulos se devuelven como `Option<T>`. Añade `distinct()` para eliminar las filas duplicadas:

```rust
let states: Vec<bool> = User::query().columns(User::ACTIVE).distinct().order_by(User::ACTIVE).all(&db).await?;
// [false, true]
```

> **Consejo** Seleccionar solo las columnas que necesitas envía menos datos y reserva menos cadenas. Es una mejora fácil en tablas con muchas columnas.

## Informes con GROUP BY

Para calcular agregados por grupo, combina `group_by` con `select_exprs`. `select_exprs` recibe una lista de *expresiones*: convierte una columna en expresión con `.expr()` y usa los ayudantes de agregación de abajo. El tipo de fila se escribe en la variable (o con un turbofish) y se decodifica por posición:

```rust
let per_user: Vec<(i64, i64)> = Post::query()
    .group_by(Post::USER_ID)
    .select_exprs([Post::USER_ID.expr(), count_all()])
    .order_by(Post::USER_ID.asc())
    .all(&db)
    .await?;
// [(1, 2), (2, 1), (4, 1)]: el usuario 1 escribió 2 posts, los usuarios 2 y 4 uno cada uno
```

| Expresión | SQL |
|---|---|
| `count_all()` | `COUNT(*)` |
| `Column::count()` | `COUNT(column)`: cuenta los valores que no son NULL |
| `Column::count_distinct()` | `COUNT(DISTINCT column)` |
| `Column::sum()` | `SUM(column)` (columnas numéricas) |
| `Column::avg()` | `AVG(column)` (columnas numéricas) |
| `Column::min()` / `Column::max()` | `MIN(column)` / `MAX(column)` |

`having` filtra los grupos después de la agregación, y puedes ordenar por un agregado:

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

Sin `group_by`, los agregados abarcan toda la tabla. `one` resulta práctico para una sola fila de resumen:

```rust
let (count, distinct_ages, avg_score): (i64, i64, Option<f64>) = User::query()
    .select_exprs([count_all(), User::AGE.count_distinct(), User::SCORE.avg()])
    .one(&db)
    .await?;
// (5, 4, Some(2.3))
```

Memoire normaliza los tipos de los resultados para que se decodifiquen igual en todas partes: `COUNT` es un `i64`, `SUM` de enteros es un `i64`, y `SUM` de flotantes y cualquier `AVG` son `f64`. Usa `Option<...>` para `SUM`, `AVG`, `MIN` y `MAX`, que valen `NULL` en los grupos vacíos. Para informes más grandes, decodifica en un struct con [`#[derive(FromRow)]`](models).

## Expresiones y aritmética

Las columnas numéricas admiten `add`, `sub`, `mul` y `div`, que construyen expresiones. Las expresiones (de tipo `Expr`) también tienen métodos de comparación sin tipar (`eq`, `ne`, `lt`, `le`, `gt`, `ge`) que aceptan otra expresión, una columna o un valor envuelto con `Expr::val`:

```rust
let doubled = User::query().filter(User::SCORE.mul(2.0).gt(Expr::val(5.0))).all(&db).await?;
// Carol (3.0 × 2), Dave (4.0 × 2)
```

> **Nota** Las comparaciones sobre un `Expr` no se comprueban por tipo como las comparaciones sobre una columna, porque la expresión ya no conoce su tipo de Rust. Cuando puedas, usa los métodos de columna.

## Ver el SQL

`to_sql(dialect)` genera el SQL de la consulta para PostgreSQL, SQLite o MySQL sin ejecutarla (Redis y MongoDB no usan SQL, pero `to_sql` sigue siendo una forma práctica de ver qué significa una consulta). `to_sql_with_params` devuelve además los valores enlazados:

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

Parámetros: `[I32(18), Text("%li%")]`.

Los dialectos difieren en algunos puntos, de los que Memoire se encarga por ti:

| Característica | PostgreSQL | SQLite | MySQL |
|---|---|---|---|
| Marcadores de parámetro | `$1`, `$2`, ... | `?` | `?` |
| Comillas de identificadores | `"users"` | `"users"` | `` `users` `` |
| Coincidencia sin distinguir mayúsculas | `ILIKE` | `LOWER(col) LIKE LOWER(?)` | `LOWER(col) LIKE LOWER(?)` |
| `nulls_last()` | `NULLS LAST` | `NULLS LAST` | `col IS NULL ASC, col` (emulado) |
| `offset` sin `limit` | `OFFSET n` | `LIMIT -1 OFFSET n` | `LIMIT 18446744073709551615 OFFSET n` |
| Tipo del resultado de `SUM` / `AVG` | `CAST(... AS BIGINT / DOUBLE PRECISION)` | nativo | `CAST(... AS SIGNED / DOUBLE)` |

Los constructores `Update` y `Delete` también tienen `to_sql` (consulta [Crear, leer, actualizar, eliminar](crud)).

## Lo que no compila

Las columnas tipadas detectan muchos errores en tiempo de compilación. Cada una de estas líneas es **rechazada por el compilador**:

```rust,ignore
// Comparar una columna de enteros con una cadena:
User::AGE.gt("18");                      // error: `&str: Bind<i32>` is not satisfied

// Comparar una columna de texto con un número:
User::EMAIL.eq(42);                      // error: `{integer}: Bind<String>` is not satisfied

// Búsqueda de texto en una columna que no es de texto:
User::AGE.contains("1");                 // error: method `contains` exists but its trait bounds were not satisfied

// Una errata en el nombre de una columna:
User::EMIAL.eq("a@b.c");                 // error: no associated function or constant named `EMIAL`

// Comparar con None (usa `is_null()`):
User::AGE.eq(None);                      // error: `Option<_>: Bind<i32>` is not satisfied

// Sumar una columna de texto:
User::query().sum(User::NAME, &db);      // error: `String: Numeric` is not satisfied

// Actualizar una columna de otro modelo:
User::update_many().set(Post::TITLE, "x"); // error: expected `Column<User, _>`, found `Column<Post, String>`

// Asignar NULL a una columna que no admite nulos:
User::update_many().set_null(User::NAME);  // error: expected `Column<User, Option<_>>`

// Buscar una clave i64 con una cadena:
User::find(&db, "1");                    // error: `&str: Bind<i64>` is not satisfied
```

## Próximos pasos

- [Crear, leer, actualizar, eliminar](crud): `update_many`, `delete_many` y las operaciones por fila.
- [Uniones y relaciones](relations): consulta varias tablas a la vez.
- [Chuleta](cheatsheet): todas las operaciones en una sola página.
