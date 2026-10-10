//! A small persistent app on SQLite: migrations, transactions, aggregates.
//!
//! ```bash
//! cargo run -p memoire-orm --example sqlite --features sqlite
//! ```

use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "accounts")]
pub struct Account {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub owner: String,
    pub balance: i64,
}

#[derive(Model, Debug, Clone)]
#[memoire(table = "transfers")]
pub struct Transfer {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "accounts(id)", index)]
    pub from_id: i64,
    #[memoire(references = "accounts(id)", index)]
    pub to_id: i64,
    pub amount: i64,
}

async fn transfer(db: &SqliteDb, from: i64, to: i64, amount: i64) -> Result<()> {
    let tx = db.begin().await?;
    let source = Account::get(&tx, from).await?;
    if source.balance < amount {
        tx.rollback().await?;
        println!("transfer of {amount} refused: insufficient funds");
        return Ok(());
    }
    Account::update_many().decrement(Account::BALANCE, amount).filter(Account::ID.eq(from)).execute(&tx).await?;
    Account::update_many().increment(Account::BALANCE, amount).filter(Account::ID.eq(to)).execute(&tx).await?;
    Transfer { id: 0, from_id: from, to_id: to, amount }.insert(&tx).await?;
    tx.commit().await
}

fn main() -> Result<()> {
    let path = std::env::temp_dir().join("memoire-example.db");
    let _ = std::fs::remove_file(&path);
    let db = SqliteDb::open(&path)?;

    memoire_orm::block_on(async {
        let migrator = Migrator::new()
            .add(Migration::new(1, "create_accounts").create_table::<Account>())
            .add(Migration::new(2, "create_transfers").create_table::<Transfer>());
        println!("applied migrations: {:?}", migrator.run(&db).await?);

        let mut alice = Account { id: 0, owner: "alice".into(), balance: 100 };
        let mut bob = Account { id: 0, owner: "bob".into(), balance: 20 };
        alice.insert(&db).await?;
        bob.insert(&db).await?;

        transfer(&db, alice.id, bob.id, 30).await?;
        transfer(&db, bob.id, alice.id, 500).await?;

        for account in Account::query().order_by(Account::OWNER.asc()).all(&db).await? {
            println!("{:>6}: {}", account.owner, account.balance);
        }
        let total = Account::query().sum(Account::BALANCE, &db).await?;
        println!("total money in the bank: {}", total.unwrap_or(0));
        println!("transfers recorded: {}", Transfer::count(&db).await?);
        Ok::<(), Error>(())
    })?;
    println!("database written to {}", path.display());
    Ok(())
}
