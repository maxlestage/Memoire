# Factories y seeders

Los tests y las demos necesitan datos. Las factories de modelos describen una fila válida una vez y luego construyen tantas como quieras, al estilo de las factories y seeders de Laravel y los fixtures de Rails. Están en `memoire_orm::factory` y funcionan en todas las bases de datos.

## Una factory

Implementa `Factory` con una `definition(n)`: una fila válida, única gracias al número de secuencia `n` (1, 2, 3... por modelo, durante todo el proceso). Deja una clave `auto_increment` a `0`.

```rust
use memoire_orm::factory::Factory;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub admin: bool,
}

impl Factory for User {
    fn definition(n: u64) -> Self {
        User { id: 0, email: format!("user{n}@example.com"), admin: false }
    }
}
```

## Construir filas

```rust
let one = User::factory().create_one(&db).await?;                     // insertada, con id
let many = User::factory().count(10).create(&db).await?;              // 10 filas, en orden
let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
let mixed = User::factory().count(4).sequence(|u, i| u.admin = i % 2 == 0).create(&db).await?;
let unsaved: Vec<User> = User::factory().count(3).make()?;            // sin guardar
let posts = Post::factory().count(3).for_parent(&one).create(&db).await?; // author_id = one.id
```

| Método | Efecto |
|---|---|
| `count(n)` | cuántas filas (1 por defecto) |
| `state(\|row\| ...)` | cambia cada fila (un "state" de Laravel); los states se aplican en orden |
| `sequence(\|row, i\| ...)` | cambia cada fila según su posición en el lote |
| `for_parent(&parent)` | hace que la clave foránea apunte a `parent` (el `for` de Laravel) |
| `make()`, `make_one()` | las filas, sin guardar |
| `create(&db)`, `create_one(&db)` | las filas, insertadas con `insert`: se aplican hooks, marcas de tiempo y claves generadas |

`Factory::reset_sequence()` reinicia en 1 los números de un modelo, para tests que comparan valores generados.

## Seeders

Un seeder es una pieza de preparación con nombre. `seed` lo ejecuta en una transacción, así que un seeder que falla no escribe nada; una tupla de seeders los ejecuta en orden:

```rust
use memoire_orm::factory::{Seeder, seed};

struct Users;
impl Seeder for Users {
    async fn run<E: Executor>(&self, db: &E) -> Result<()> {
        User::factory().count(50).create(db).await?;
        User::factory().state(|u| u.admin = true).create_one(db).await?;
        Ok(())
    }
}

struct Catalog;
impl Seeder for Catalog {
    async fn run<E: Executor>(&self, db: &E) -> Result<()> {
        Product::factory().count(200).create(db).await?;
        Ok(())
    }
}

seed(&db, &(Users, Catalog)).await?;
```

Con `MemoryDb::new()` un test obtiene una base de datos nueva en microsegundos: crea las tablas, llénalas, comprueba.

## Próximos pasos

- [Filtros de relaciones, conteos y polimorfismo](relations-advanced)
- [Bloqueo optimista e identity map](optimistic-locking)
