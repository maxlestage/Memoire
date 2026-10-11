# Motor en memoria

`MemoryDb` es un motor de base de datos escrito en Rust puro que mantiene todas las tablas en memoria. No necesita ningún archivo, ningún servidor ni ningún runtime asíncrono, ejecuta las mismas consultas que los demás backends, e incluso funciona en WebAssembly. Está activado por defecto (feature `memory`).

## Cuándo usarlo

| Ideal para | No adecuado para |
|---|---|
| Pruebas unitarias y de integración: rápidas, aisladas, nada que limpiar | Datos que deben sobrevivir a un reinicio (no se escribe nada en disco) |
| Prototipos y demos | Datos más grandes que la memoria disponible |
| Cachés y datos temporales de cada proceso | Compartir datos entre varios procesos |
| Aplicaciones de navegador compiladas a WebAssembly | SQL sin procesar (no hay analizador de SQL) |

## Primeros pasos

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "notes")]
pub struct Note {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub slug: String,
    pub text: String,
}

fn main() -> memoire_orm::Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<Note>().await?;

        let mut note = Note { id: 0, slug: "hello".into(), text: "Hello!".into() };
        note.insert(&db).await?;

        let found = Note::query().filter(Note::SLUG.eq("hello")).one(&db).await?;
        println!("{}", found.text);
        Ok(())
    })
}
```

No hace falta ningún runtime: el motor nunca espera a la E/S, así que `memoire_orm::block_on` ejecuta sus futures directamente. Funciona igual de bien dentro de Tokio o de cualquier otro ejecutor.

### Compartir una base de datos

Clonar un `MemoryDb` es barato, y todos los clones ven los mismos datos. Dale un clon a cada hilo o tarea:

```rust
let db = MemoryDb::new();
let for_worker = db.clone(); // mismas tablas, mismas filas
```

Los datos viven mientras exista al menos un clon. Es seguro usarlo desde muchos hilos a la vez: las lecturas se ejecutan en paralelo y las escrituras se serializan.

### Inspeccionar la base de datos

| Método | Devuelve |
|---|---|
| `db.table_names()` | Los nombres de todas las tablas, ordenados |
| `db.row_count("notes")` | El número de filas de una tabla (`Error::NoSuchTable` si no existe) |
| `format!("{db:?}")` | Todas las tablas con su número de filas, muy útil cuando falla una prueba |

```rust
println!("{:?}", db.table_names());   // ["notes"]
println!("{}", db.row_count("notes")?); // 1
println!("{db:?}");                    // MemoryDb { tables: [("notes", 1)] }
```

## Cómo funciona

El motor no genera ni analiza SQL. Interpreta directamente las estructuras de consulta de Memoire, con algoritmos elegidos por su velocidad:

| Operación | Estrategia |
|---|---|
| `find`, `get`, `update`, `delete` por clave | Índice hash sobre la clave primaria: tiempo constante |
| Filtros como `eq` o `is_in` sobre la clave primaria, una columna `unique` o `index` | Búsqueda en el índice hash en lugar de un recorrido completo |
| Otros filtros | Una pasada vectorizada: la condición se divide en sus términos `AND` y cada uno filtra las filas con un bucle especializado para su columna y su constante; las columnas numéricas se leen de una copia tipada contigua (unos 1 ns por fila). Las demás condiciones (`OR`, `LIKE`, funciones...) se compilan una vez por consulta |
| Joins sobre `a.column = b.column` | Hash join (o una búsqueda directa cuando la columna unida es una clave, `unique` o `index`) |
| Otras condiciones de join | Bucle anidado |
| `order_by` + `limit` | Selección parcial de las primeras filas sobre claves enteras compactas, en lugar de ordenarlo todo |
| `count` | Cuenta las filas que coinciden sin construirlas |
| `group_by`, `distinct` | Tablas hash |
| Transacciones | Instantáneas con copia en escritura (copy-on-write) |

El backend [Redis](redis) evalúa las consultas con este mismo motor, y [MongoDB](mongodb) lo usa para completar los joins, los `GROUP BY` y los agregados, así que comparten su semántica.

> **Nota** Las columnas `#[memoire(index)]` también tienen aquí un índice hash, como en las bases de datos SQL y en MongoDB. Los índices solo guardan posiciones de filas: las claves nunca se copian.

## Semántica SQL

El motor sigue las reglas de SQL, así que las pruebas sobre `MemoryDb` se comportan como la producción en los demás backends:

- **Lógica trivalente:** las comparaciones con `NULL` no son ni verdaderas ni falsas; usa `is_null()`.
- **Orden de los NULL como en PostgreSQL:** los NULL van al final en orden ascendente y al principio en orden descendente (`nulls_first()`/`nulls_last()` lo cambian).
- **`LIKE` distingue mayúsculas y minúsculas**, como SQLite (tal como lo configura Memoire) y PostgreSQL; `ilike` e `icontains` no las distinguen, incluidas las letras acentuadas.
- **Los agregados** ignoran los valores `NULL`; el `SUM` de enteros es un `i64` y `AVG` es un `f64`.
- **Restricciones:** se aplican las claves primarias, las columnas `unique` y `NOT NULL` (campos que no son `Option`), con los mismos errores que en las bases de datos SQL (`Error::UniqueViolation`, `Error::Constraint`).
- **Escrituras atómicas:** un `update_many` de varias filas, un lote de `insert_many` o un `delete_many` o bien tiene éxito por completo o no cambia nada.

Diferencias con las bases de datos SQL que debes tener en cuenta:

| Funcionalidad | Motor en memoria |
|---|---|
| Claves foráneas (`references`) | Se aceptan pero **no se aplican**; `on_delete = "cascade"` no hace nada |
| `default = "..."` | Se ignora (de todos modos, las inserciones de Memoire siempre proporcionan todos los valores) |
| SQL sin procesar (`raw`, `raw_execute`) | `Error::Unsupported` |
| Migraciones | Solo los pasos `create_table` y `drop_table`; `sql` falla y `sql_for` se omite |
| Auto-joins (una tabla dos veces en la misma consulta) | `Error::Unsupported` |
| Persistencia | `MemoryDb::new()`: ninguna, los datos desaparecen con el último clon. `MemoryDb::open(path)`: guardados en un archivo (más abajo) |

## Guardar los datos en un archivo

`MemoryDb::open(path)` conserva todo lo que hace rápido al motor (datos en memoria, índices, transacciones por instantáneas) y además escribe cada cambio confirmado en un archivo, que se vuelve a leer la próxima vez que se abre:

```rust
let db = MemoryDb::open("app.memoire")?;
db.create_table_if_not_exists::<Note>().await?;
```

- **Qué se escribe:** un pequeño registro por fila modificada, añadido cuando la sentencia o la transacción se confirma. Una transacción revertida no escribe nada.
- **Caídas:** cada registro lleva una suma de control. Si el programa se detiene en medio de una escritura, el registro incompleto se descarta en el siguiente `open`; todo lo anterior se conserva.
- **Durabilidad:** por defecto, una escritura termina en cuanto el sistema operativo tiene los datos: sobrevive a una caída del programa, no siempre a un corte de luz. `MemoryDb::open_with(path, StorageOptions::new().sync(true))` espera además al disco tras cada escritura (mucho más lento: unos 0,15 ms por escritura en la máquina de pruebas, frente a 2 o 3 µs).
- **Tamaño:** el archivo crece con cada cambio. `db.compact()` lo reescribe solo con las filas actuales; también ocurre por sí solo cuando el archivo contiene muchos más cambios que filas. Reabrir un archivo de 20.000 filas tarda unos 40 ms.
- **Modelos:** las tablas vuelven la primera vez que se usa su modelo. Las columnas se emparejan por nombre: añadir un campo `Option` conserva las filas (la nueva columna vale `NULL`), quitar un campo elimina esa columna. Un campo nuevo que no sea `Option` requiere migrar los datos.
- **Un proceso a la vez:** dos programas no deben abrir el mismo archivo a la vez.

### Cifrado

Con la feature `encryption`, el archivo puede cifrarse con una contraseña:

```rust
use memoire_orm::StorageOptions;

let db = MemoryDb::open_with("secrets.memoire", StorageOptions::new().password(password))?;
```

Cada registro se cifra con AES-256-GCM, con una clave derivada de la contraseña mediante Argon2id y una sal aleatoria guardada en el archivo. Abrir el archivo con otra contraseña falla con "wrong password", y nada en él se puede leer sin la contraseña. Perder la contraseña es perder los datos.

## Transacciones

Las transacciones son instantáneas. `db.begin()` da a la transacción su propia vista de la base de datos; las tablas se copian de forma perezosa, solo cuando la transacción escribe en ellas por primera vez. Los cambios se vuelven visibles para los demás en el `commit`, todos a la vez.

Si otro escritor ha modificado una tabla en la que la transacción también ha escrito, `commit` falla con `Error::Conflict` y no se aplica nada, así que puedes reintentar con seguridad. Las transacciones de solo lectura nunca entran en conflicto. Consulta [Transacciones](transactions) para ver un bucle de reintento.

```rust
let tx = db.begin().await?;
Note { id: 0, slug: "draft".into(), text: "...".into() }.insert(&tx).await?;
assert_eq!(Note::count(&tx).await?, 2); // la transacción ve su propia inserción
assert_eq!(Note::count(&db).await?, 1); // los demás todavía no
tx.commit().await?;
assert_eq!(Note::count(&db).await?, 2);
```

## Pruebas con `MemoryDb`

Escribe el código de tu aplicación contra el trait `Executor` (o `Database`) y pruébalo con un `MemoryDb` nuevo en cada prueba: cada prueba obtiene su propia base de datos aislada en microsegundos.

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
}

/// Código de la aplicación: funciona con cualquier base de datos.
pub async fn register<E: Executor>(db: &E, email: &str) -> memoire_orm::Result<User> {
    let mut user = User { id: 0, email: email.to_lowercase() };
    user.insert(db).await?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> memoire_orm::Result<MemoryDb> {
        let db = MemoryDb::new();
        db.create_table::<User>().await?;
        Ok(db)
    }

    #[test]
    fn emails_are_unique_ignoring_case() {
        memoire_orm::block_on(async {
            let db = test_db().await?;
            register(&db, "ada@example.com").await?;
            let err = register(&db, "ADA@example.com").await.unwrap_err();
            assert!(err.is_unique_violation());
            Ok::<(), Error>(())
        })
        .unwrap();
    }
}
```

> **Consejo** Para el motor en memoria bastan funciones `#[test]` normales con `block_on`; `#[tokio::test]` también funciona si lo prefieres.

## WebAssembly

El motor en memoria compila a `wasm32-unknown-unknown` sin ninguna configuración adicional, lo que permite ejecutar un motor de consultas de verdad en el navegador:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

En el navegador, ejecuta los futures con `memoire_orm::block_on` (el motor nunca espera realmente, así que no bloquea) o con `wasm_bindgen_futures::spawn_local`. Solo la feature `memory` funciona en `wasm32`: deja `sqlite` y `postgres` desactivadas para ese target.

> **Nota** En `wasm32-unknown-unknown` no hay reloj del sistema, así que la columna `applied_at` que escriben las [migraciones](migrations) vale `0`.

## Próximos pasos

- [SQLite](sqlite): la misma API, con los datos guardados en un archivo.
- [Transacciones](transactions): instantáneas, conflictos y reintentos en detalle.
- [Por qué es rápido](performance): qué hace que el motor sea rápido.
