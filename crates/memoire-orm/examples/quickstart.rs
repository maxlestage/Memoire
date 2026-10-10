//! The 30-second tour, on the in-memory engine.
//!
//! ```bash
//! cargo run -p memoire-orm --example quickstart
//! ```

use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}

fn main() -> Result<()> {
    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<User>().await?;

        for (email, name, age) in [
            ("alice@rust.dev", "Alice", Some(31)),
            ("bob@rust.dev", "Bob", Some(17)),
            ("carol@rust.dev", "Carol", None),
            ("dave@rust.dev", "Dave", Some(45)),
        ] {
            let mut user = User { id: 0, email: email.into(), name: name.into(), age };
            user.insert(&db).await?;
            println!("inserted #{} {}", user.id, user.name);
        }

        let query = User::query().filter(User::AGE.ge(18)).order_by(User::NAME.asc()).limit(10);
        println!("\nPostgreSQL: {}", query.to_sql(Dialect::Postgres));
        println!("MySQL:      {}", query.to_sql(Dialect::MySql));

        for user in query.all(&db).await? {
            println!("adult: {} ({:?})", user.name, user.age);
        }

        let average = User::query().avg(User::AGE, &db).await?;
        println!("\naverage age: {:.1}", average.unwrap_or_default());

        let n = User::update_many().increment(User::AGE, 1).filter(User::AGE.is_not_null()).execute(&db).await?;
        println!("happy birthday to {n} users");

        let bob = User::query().filter(User::NAME.eq("Bob")).one(&db).await?;
        println!("Bob is now {:?}", bob.age);
        Ok(())
    })
}
