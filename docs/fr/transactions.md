# Transactions

Une transaction regroupe plusieurs modifications pour qu'elles s'appliquent toutes ensemble ou pas du tout. Cette page montre comment ouvrir, valider (commit) et annuler (rollback) des transactions, comment réessayer quand deux transactions entrent en collision, et comment se comporte chaque backend.

## Pourquoi des transactions ?

Virer de l'argent d'un compte à un autre demande deux modifications : retirer du premier compte, ajouter au second. Si le programme plante, ou si la seconde modification échoue, entre les deux, de l'argent disparaît. Dans une transaction, soit les deux modifications sont enregistrées, soit aucune ne l'est.

Les transactions vous offrent aussi une vue cohérente des données pendant que vous travaillez, et elles rendent les écritures en masse beaucoup plus rapides sur SQLite.

## Utilisation de base

```rust
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    pub owner: String,
    pub balance: i64,
}

/// Transfère `amount` d'un compte à un autre.
/// Renvoie `false` (sans rien modifier) si le compte source n'a pas assez d'argent.
async fn transfer<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let tx = db.begin().await?;

    let source = Account::get(&tx, from).await?;
    if source.balance < amount {
        tx.rollback().await?;
        return Ok(false);
    }

    Account::update_many()
        .decrement(Account::BALANCE, amount)
        .filter(Account::ID.eq(from))
        .execute(&tx)
        .await?;
    Account::update_many()
        .increment(Account::BALANCE, amount)
        .filter(Account::ID.eq(to))
        .execute(&tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}
```

Les trois étapes :

1. `db.begin().await?` démarre une transaction et renvoie un handle de transaction.
2. Passez `&tx` au lieu de `&db` à chaque opération qui fait partie de la transaction. Une transaction est un `Executor` : toutes les méthodes de modèle, les requêtes et les appels SQL bruts l'acceptent.
3. `tx.commit().await?` enregistre tout. `tx.rollback().await?` abandonne tout.

`commit` et `rollback` consomment la transaction : le compilateur vous empêche donc de l'utiliser ensuite.

`begin` fait partie du trait `Database`, implémenté par `MemoryDb`, `SqliteDb` et `PgPool`. Écrire `D: Database` permet à `transfer` de fonctionner avec les trois.

## Dropper une transaction l'annule

Si une transaction est droppée sans `commit`, elle est annulée. La gestion des erreurs en devient simple : quand `?` fait sortir prématurément de la fonction, la transaction sort de la portée et ses modifications sont abandonnées.

```rust
async fn create_two(db: &MemoryDb) -> memoire_orm::Result<()> {
    let tx = db.begin().await?;
    Account { id: 0, owner: "Ada".into(), balance: 10 }.insert(&tx).await?;
    Account::get(&tx, 999).await?; // échoue avec NotFound : `?` sort de la fonction...
    tx.commit().await              // ...donc cette ligne ne s'exécute jamais, et Ada n'est pas enregistrée
}
```

> **Astuce** Vous avez rarement besoin d'un `rollback` explicite : renvoyer une erreur avec `?` suffit. Appelez `rollback` lorsque vous décidez d'annuler sans qu'il y ait d'erreur, comme dans `transfer` ci-dessus.

## Des fonctions qui marchent avec ou sans transaction

Écrivez vos fonctions utilitaires en vous appuyant sur le trait `Executor`. Elles acceptent alors un handle de base de données *ou* une transaction, et c'est l'appelant qui décide :

```rust
async fn open_account<E: Executor>(db: &E, owner: &str) -> memoire_orm::Result<Account> {
    let mut account = Account { id: 0, owner: owner.into(), balance: 0 };
    account.insert(db).await?;
    Ok(account)
}

async fn open_family_accounts(db: &MemoryDb) -> memoire_orm::Result<()> {
    open_account(db, "standalone").await?; // s'exécute seul

    let tx = db.begin().await?;
    open_account(&tx, "parent").await?; // les deux font partie de la même transaction
    open_account(&tx, "child").await?;
    tx.commit().await
}
```

## Comportement selon le backend

| | Moteur en mémoire | SQLite | PostgreSQL |
|---|---|---|---|
| Fonctionnement | Snapshot privé en copy-on-write | Monopolise l'unique connexion | Monopolise une connexion du pool |
| Autre code pendant la transaction | S'exécute normalement, ne voit que les données validées | **Attend** la fin de la transaction | S'exécute sur d'autres connexions, ne voit que les données validées |
| Écritures concurrentes en conflit | Détectées au `commit` : `Error::Conflict` | Impossible : les transactions s'exécutent une à la fois | Attend les verrous de ligne ; les échecs de sérialisation et les deadlocks donnent `Error::Conflict` |
| Droppée sans commit | Modifications abandonnées immédiatement | `ROLLBACK` exécuté immédiatement | `ROLLBACK` envoyé en arrière-plan (nécessite Tokio) |

### Moteur en mémoire : snapshots et conflits

`MemoryDb::begin` prend un snapshot de la base de données. La transaction lit et écrit sa propre copie (les tables ne sont copiées qu'à la première écriture) : elle ne voit donc jamais les modifications concurrentes, et personne ne voit ses modifications avant le `commit`.

Au moment du commit, Memoire vérifie si une autre écriture a modifié l'une des tables que cette transaction a elle aussi modifiées. Si c'est le cas, le commit échoue avec `Error::Conflict` et rien n'est appliqué : le premier qui écrit l'emporte.

```rust
let t1 = db.begin().await?;
let t2 = db.begin().await?;
Account::update_many().increment(Account::BALANCE, 10).execute(&t1).await?;
Account::update_many().increment(Account::BALANCE, 20).execute(&t2).await?;

t1.commit().await?;                       // la première écriture l'emporte
let err = t2.commit().await.unwrap_err(); // la seconde est rejetée
assert!(err.is_conflict());
```

Bon à savoir :

- Les conflits sont détectés **par table**, et non par ligne : deux transactions qui écrivent des lignes différentes de la même table entrent quand même en conflit.
- Une transaction qui ne fait que lire n'entre jamais en conflit.
- Toute écriture dans la table compte, y compris les écritures faites directement sur `db` en dehors d'une transaction.

### Réessayer en cas de conflit

Un conflit n'est pas un bug : il signifie « quelqu'un d'autre a modifié ces données pendant que vous travailliez ; recommencez à partir de données fraîches ». Comme une transaction qui échoue n'applique rien, réessayer est toujours sans risque. Enveloppez toute la transaction dans une boucle :

```rust
async fn transfer_with_retry<D: Database>(db: &D, from: i64, to: i64, amount: i64) -> memoire_orm::Result<bool> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match transfer(db, from, to, amount).await {
            Err(err) if err.is_conflict() && attempts < 5 => continue, // on réessaie avec des données fraîches
            result => return result,
        }
    }
}
```

Réessayez la fonction *entière*, lectures comprises : la nouvelle tentative doit démarrer une nouvelle transaction et voir les données les plus récentes. La même boucle fonctionne sur PostgreSQL, où les échecs de sérialisation et les deadlocks sont eux aussi signalés comme `Error::Conflict`.

### SQLite : une transaction à la fois

Un `SqliteDb` utilise une seule connexion. Une transaction prend cette connexion pour elle seule jusqu'à ce qu'elle soit validée, annulée ou droppée : les transactions SQLite n'entrent donc jamais en conflit, elles s'exécutent simplement l'une après l'autre.

> **Attention** Tant qu'une transaction SQLite est ouverte, elle monopolise l'unique connexion. Une requête lancée sur le handle `SqliteDb` lui-même (`&db` au lieu de `&tx`) attend la fin de la transaction : dans la même tâche, elle attend donc indéfiniment. C'est un deadlock. Dans une transaction, passez toujours `&tx` :
>
> ```rust,ignore
> let tx = db.begin().await?;
> Account::count(&db).await?; // ne rend jamais la main : utilisez `&tx` ici
> ```

Les autres tâches ou threads qui utilisent le même `SqliteDb` attendent simplement la fin de la transaction : gardez donc vos transactions courtes. Pour les insertions en masse, regrouper des milliers d'insertions dans une seule transaction est spectaculairement plus rapide que de les insérer une par une.

### PostgreSQL : connexions du pool

`PgPool::begin` prend une connexion dans le pool et la garde jusqu'à la fin de la transaction. Les autres requêtes continuent de s'exécuter sur d'autres connexions et ne voient que les données validées.

- Le niveau d'isolation est celui par défaut de PostgreSQL, `READ COMMITTED`. Pour le relever, exécutez `SET TRANSACTION` comme première instruction de la transaction :

  ```rust
  async fn strict(db: &PgPool) -> memoire_orm::Result<()> {
      let tx = db.begin().await?;
      tx.raw_execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE", &[]).await?;
      // ... lectures et écritures ...
      tx.commit().await
  }
  ```

  En `REPEATABLE READ` et en `SERIALIZABLE`, PostgreSQL peut interrompre une transaction qui entre en collision avec une autre. Memoire le signale comme `Error::Conflict` : la boucle de nouvelle tentative ci-dessus s'applique donc telle quelle. Les deadlocks sont signalés de la même façon.
- Une transaction droppée est annulée en arrière-plan sur le runtime Tokio. Sans runtime, la connexion est fermée à la place, ce qui annule aussi la transaction.
- Utiliser `&db` au lieu de `&tx` dans une transaction ne bloque pas, mais la requête s'exécute sur une autre connexion : elle ne voit pas les modifications non validées de la transaction et n'en fait pas partie.

## Limites

- **Pas de transactions imbriquées.** Une transaction ne peut pas en démarrer une autre (il n'y a pas d'API de savepoints). Passez plutôt le `&tx` existant à vos fonctions utilitaires.
- `Error::TransactionFinished` est renvoyée si une transaction est utilisée après avoir été terminée, ce que l'API rend normalement impossible.

## Étapes suivantes

- [Migrations](migrations) : chaque migration s'exécute dans sa propre transaction.
- [Moteur en mémoire](memory), [SQLite](sqlite) et [PostgreSQL](postgres) : les détails propres à chaque backend.
- [Pourquoi c'est rapide](performance) : transactions et écritures en masse.
