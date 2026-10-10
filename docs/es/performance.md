# Por qué es rápido

Memoire está diseñado para que la capa del ORM añada el menor coste posible sobre la propia base de datos. Esta página explica de dónde sale la velocidad y luego da consejos prácticos para que tu propia aplicación siga siendo rápida.

## De dónde sale la velocidad

### SQL generado en tiempo de compilación

Para las operaciones que más ejecutas (`find`, `get`, `insert`, `update`, `delete`, `upsert`, `reload`), `#[derive(Model)]` escribe el SQL mientras tu código compila, para PostgreSQL, SQLite y MySQL. En tiempo de ejecución, esas sentencias son simples constantes de cadena: no se construye, formatea ni reserva memoria para ningún SQL. Puedes verlas en `User::SCHEMA.sql(Dialect::Postgres)` (consulta [Modelos](models)).

Las consultas creadas con el constructor de consultas se generan en tiempo de ejecución en un único búfer con el tamaño reservado de antemano, en una sola pasada sobre un pequeño árbol sintáctico. Generar un `SELECT` típico con un filtro, una ordenación y un límite tarda alrededor de medio microsegundo.

### Sin copias de tus parámetros

Cada campo se convierte una sola vez en un `Value`. A partir de ahí, el generador de SQL y los drivers solo *toman prestados* esos valores: no se vuelven a copiar de camino a la base de datos. En SQLite, los valores de texto y de bytes se pasan a SQLite por referencia.

### Filas decodificadas por posición, valores movidos

Cuando llegan las filas, Memoire las decodifica **por posición** en una sola pasada: la columna 0 va al primer campo, la columna 1 al segundo, y así sucesivamente. No hay búsqueda por nombre de columna ni ningún mapa intermedio. Cada valor se *mueve* a tu struct en lugar de clonarse.

### Sentencias preparadas, en caché

Cada sentencia se prepara una vez por conexión y se reutiliza:

| Backend | Caché de sentencias |
|---|---|
| SQLite | Hasta 256 sentencias por conexión |
| PostgreSQL | Todas las sentencias por conexión, con un reinicio tras 1024 sentencias distintas |

Como el constructor produce el mismo texto SQL para las consultas con la misma forma (los valores siempre son parámetros), una consulta dentro de un bucle solo se analiza y planifica una vez.

### Inserciones masivas en pocas sentencias

`insert_many` convierte un slice de filas en sentencias `INSERT ... VALUES (...), (...), ...` de varias filas. Cada sentencia lleva tantas filas como permite la base de datos: el número de parámetros enlazados se mantiene por debajo del límite (32 766 en SQLite, 65 535 en PostgreSQL), con un máximo de 10 000 filas por sentencia. Insertar 1000 filas en una transacción de esta forma es unas 4 veces más rápido que un bucle de inserciones de una sola fila con código `rusqlite` escrito a mano (consulta [Benchmarks](benchmarks)).

### Un motor en memoria hecho a medida

`MemoryDb` no analiza SQL en absoluto: ejecuta directamente las estructuras de consulta.

- Índices hash sobre la clave primaria y las columnas `unique`: las búsquedas por clave tardan un tiempo constante.
- Los filtros se compilan una vez por consulta (posiciones de columna resueltas, listas `IN` largas convertidas en conjuntos hash, patrones `LIKE` analizados previamente) antes de recorrer las filas.
- Los joins por igualdad de columnas se ejecutan como hash joins.
- `order_by` + `limit` selecciona las primeras filas sin ordenarlo todo.
- Las transacciones usan instantáneas con copia en escritura: iniciar una es barato, y una tabla solo se copia cuando la transacción escribe en ella por primera vez.

### Ligero por construcción

Los constructores son envoltorios tipados muy finos: los tipos de columna y el tipo del resultado solo existen en tiempo de compilación y no cuestan nada en tiempo de ejecución. No hay reflexión ni descubrimiento del esquema en tiempo de ejecución.

## Consejos para aplicaciones rápidas

### Inserta de forma masiva

```rust
// Lento: una sentencia (y un viaje de ida y vuelta) por fila.
for user in &new_users {
    let mut user = user.clone();
    user.insert(&db).await?;
}

// Rápido: unas pocas sentencias de varias filas.
User::insert_many(&db, &new_users).await?;
```

Usa `insert` cuando necesites recuperar cada id generado; usa `insert_many` para todo lo demás.

### Agrupa las escrituras en una transacción

Sobre todo en SQLite, cada escritura fuera de una transacción es una transacción en sí misma. Envolver muchas escrituras en una sola transacción suele ser un orden de magnitud más rápido:

```rust
let tx = db.begin().await?;
User::insert_many(&tx, &new_users).await?;
User::update_many().set(User::ACTIVE, true).filter(User::AGE.ge(18)).execute(&tx).await?;
tx.commit().await?;
```

### Actualiza y elimina en la base de datos

No cargues filas solo para cambiarlas una a una. Deja que la base de datos lo haga en una sola sentencia:

```rust
// Una sola sentencia, coincidan las filas que coincidan.
User::update_many().increment(User::VISITS, 1).filter(User::ACTIVE.eq(true)).execute(&db).await?;
User::delete_many().filter(User::ACTIVE.eq(false)).execute(&db).await?;
```

### Selecciona solo lo que necesitas

Cargar filas completas implica transferir todas las columnas y reservar memoria para ellas. Cuando solo necesitas unas pocas, usa `columns`:

```rust
let emails: Vec<String> = User::query().columns(User::EMAIL).filter(User::ACTIVE.eq(true)).all(&db).await?;
```

### Haz preguntas precisas

| En lugar de | Usa | Por qué |
|---|---|---|
| `query.all(&db).await?.len()` | `query.count(&db)` | Cuenta en la base de datos y transfiere un solo número |
| `query.count(&db).await? > 0` | `query.exists(&db)` | Se detiene en la primera fila que coincide |
| `query.all(&db).await?.into_iter().next()` | `query.first(&db)` | Añade `LIMIT 1` |
| Cargar filas para sumar una columna | `query.sum(Column, &db)` | Agrega en la base de datos |

### Indexa las columnas por las que buscas

Añade `#[memoire(index)]` a las columnas por las que filtras, haces joins u ordenas a menudo, en particular las claves foráneas. Las columnas `unique` se indexan automáticamente. En el motor en memoria, las búsquedas por la clave primaria y por las columnas `unique` usan índices hash.

### Evita las consultas N+1

Cargar las filas relacionadas con una consulta por cada padre es el problema de rendimiento más habitual en el código que usa un ORM. Carga todos los hijos con una sola consulta `is_in` y agrúpalos en memoria, como se muestra en [Uniones y relaciones](relations).

### Pagina con claves, no con offsets

`offset(10_000)` obliga a la base de datos a recorrer 10 000 filas antes de devolver nada. Para listas largas y scroll infinito, recuerda el último id que mostraste y continúa a partir de ahí:

```rust
async fn next_page<E: Executor>(db: &E, after_id: i64) -> memoire_orm::Result<Vec<User>> {
    User::query()
        .filter(User::ID.gt(after_id))
        .order_by(User::ID.asc())
        .limit(50)
        .all(db)
        .await
}
```

Esto sigue siendo rápido en cualquier página. Reserva `paginate` (que además ejecuta un `COUNT`) para las pantallas que de verdad necesitan números de página.

### Reutiliza el manejador de la base de datos

Abre la base de datos (o el pool de PostgreSQL) una sola vez al arrancar y compártela: clonar `MemoryDb`, `SqliteDb` y `PgPool` es barato (backends MySQL/MariaDB, Redis y MongoDB: consulta sus páginas, [MySQL y MariaDB](mysql), [Redis](redis) y [MongoDB](mongodb)). Abrir un pool nuevo en cada petición desperdicia las conexiones y las cachés de sentencias. Dimensiona el pool de PostgreSQL según lo que permita tu servidor (consulta [PostgreSQL](postgres)).

### Mide en modo release

Las compilaciones de depuración del código Rust son muchas veces más lentas. Haz los benchmarks y despliega con `cargo build --release`. Para entender una consulta lenta, imprime su SQL con `to_sql` y ejecútalo con `EXPLAIN` mediante [SQL sin procesar](raw-sql).

## Próximos pasos

- [Benchmarks](benchmarks): cifras medidas y cómo reproducirlas.
- [Uniones y relaciones](relations): cargar las filas relacionadas de forma eficiente.
- [Transacciones](transactions): agrupar las escrituras.
