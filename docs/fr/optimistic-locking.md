# Verrouillage optimiste et identity map

Deux outils venus de Doctrine (et du `lock_version` d'ActiveRecord) pour les applications où plusieurs personnes ou tâches écrivent les mêmes lignes.

## Verrouillage optimiste

Alice et Bob ouvrent le même compte en même temps. Alice enregistre ; puis Bob enregistre, et écrase sans bruit la modification d'Alice. Le verrouillage optimiste empêche cette mise à jour perdue : marquez un champ entier `#[memoire(version)]`, et chaque écriture vérifie que la ligne a toujours la version que vous avez lue.

```rust
#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub balance: i64,
    #[memoire(version)]
    pub version: i32,
}

let mut alice = Account::get(&db, 1).await?;
let mut bob = Account::get(&db, 1).await?;

alice.balance -= 30;
alice.save(&db).await?;                   // version 0 -> 1, réécrite dans `alice`

bob.balance += 10;
match bob.save(&db).await {
    Err(e) if e.is_stale() => {
        bob.reload(&db).await?;           // relire la nouvelle version, refaire la modification
        bob.balance += 10;
        bob.save(&db).await?;
    }
    other => other?,
}
```

| Opération | Avec `#[memoire(version)]` |
|---|---|
| `save`, `update`, `update_only` | `UPDATE ... SET ..., version = version + 1 WHERE id = ? AND version = ?` ; `Error::StaleRow` si la version a changé |
| `delete`, `force_delete` (et suppressions douces) | seulement si la version n'a pas changé, `Error::StaleRow` sinon |
| `update_many()` | incrémente la version de chaque ligne modifiée, pour que les copies lues avant soient périmées |
| `insert`, `upsert`, `insert_many` | écrivent la version telle quelle (commencez à `0`) |

`save` réécrit la nouvelle version dans la struct. `update` et `update_only` l'empruntent, donc ne le peuvent pas : après eux, faites `reload` (ou utilisez `save`). Une ligne absente donne toujours `Error::NotFound` ; `Error::is_stale()` distingue les deux cas. Le champ de version peut être un `i16`, `i32`, `i64` ou une `Option` de ceux-ci (une version `NULL` devient 1) ; tout autre type est refusé à la compilation.

## Identity map

Une `IdentityMap` lit chaque ligne une seule fois et renvoie le même objet partagé (`Arc<M>`) à chaque recherche ultérieure de sa clé, comme `EntityManager::find` de Doctrine. Utilisez-en une par unité de travail : une requête HTTP, une tâche, un test.

```rust
use memoire_orm::identity::IdentityMap;

let map = IdentityMap::new();
let a = map.find::<User, _>(&db, 1).await?;           // une requête
let b = map.find::<User, _>(&db, 1).await?;           // aucune : le même Arc
let team = map.find_many::<User, _, _>(&db, [1, 2, 3]).await?; // une requête, pour 2 et 3 seulement
let rows = map.merge(User::query().all(&db).await?);  // une ligne déjà connue reste le même objet

map.forget::<User>(1);   // après l'avoir modifiée ou supprimée
map.clear();             // EntityManager::clear
```

La map est un cache qui n'expire jamais et ne voit pas les écritures faites ailleurs : gardez-la de courte durée, et faites `remember` (ou `forget`) d'une ligne après l'avoir écrite.

## Étapes suivantes

- [Transactions](transactions) : rendez plusieurs écritures tout ou rien.
- [Factories et seeders](factories)
