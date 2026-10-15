# Factories et seeders

Les tests et les démos ont besoin de données. Les factories de modèles décrivent une ligne valide une fois, puis en construisent autant que vous voulez, dans l'esprit des factories et seeders de Laravel et des fixtures de Rails. Elles se trouvent dans `memoire_orm::factory` et fonctionnent sur chaque base.

## Une factory

Implémentez `Factory` avec une `definition(n)` : une ligne valide, rendue unique par le numéro de séquence `n` (1, 2, 3... par modèle, pour tout le processus). Laissez une clé `auto_increment` à `0`.

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

## Construire des lignes

```rust
let one = User::factory().create_one(&db).await?;                     // insérée, id renseigné
let many = User::factory().count(10).create(&db).await?;              // 10 lignes, dans l'ordre
let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
let mixed = User::factory().count(4).sequence(|u, i| u.admin = i % 2 == 0).create(&db).await?;
let unsaved: Vec<User> = User::factory().count(3).make()?;            // non stockées
let posts = Post::factory().count(3).for_parent(&one).create(&db).await?; // author_id = one.id
```

| Méthode | Effet |
|---|---|
| `count(n)` | combien de lignes (1 par défaut) |
| `state(\|row\| ...)` | modifie chaque ligne (un « state » de Laravel) ; les states s'appliquent dans l'ordre |
| `sequence(\|row, i\| ...)` | modifie chaque ligne selon sa position dans le lot |
| `for_parent(&parent)` | fait pointer la clé étrangère vers `parent` (le `for` de Laravel) |
| `make()`, `make_one()` | les lignes, non stockées |
| `create(&db)`, `create_one(&db)` | les lignes, insérées avec `insert` : hooks, horodatages et clés générées s'appliquent |

`Factory::reset_sequence()` fait repartir les numéros d'un modèle à 1, pour les tests qui comparent des valeurs générées.

## Seeders

Un seeder est un morceau de mise en place nommé. `seed` l'exécute dans une transaction, donc un seeder qui échoue n'écrit rien ; un tuple de seeders les exécute dans l'ordre :

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

Avec `MemoryDb::new()`, un test obtient une base neuve en quelques microsecondes : créez les tables, remplissez, vérifiez.

## Étapes suivantes

- [Filtres de relations, comptages et polymorphisme](relations-advanced)
- [Verrouillage optimiste et identity map](optimistic-locking)
