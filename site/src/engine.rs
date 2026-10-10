//! The real Memoire ORM, compiled to WebAssembly, powering the playground.
//!
//! A [`MemoryDb`] is seeded once with sample users; every playground run builds
//! a genuine `memoire_orm` query, renders it for the three SQL dialects and
//! executes it on the in-memory engine inside the browser.

use memoire_orm::prelude::*;

use crate::pages::playground::{OrderField, PlaygroundOutput, PlaygroundParams};

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

const PEOPLE: &[(&str, &str, Option<i32>)] = &[
    ("Alice", "alice@rust.dev", Some(31)),
    ("Bob", "bob@rust.dev", Some(17)),
    ("Charlie", "charlie@rust.dev", Some(42)),
    ("Diana", "diana@rust.dev", Some(28)),
    ("Élodie", "elodie@rust.dev", Some(35)),
    ("Fernando", "fernando@rust.dev", None),
    ("Grace", "grace@rust.dev", Some(54)),
    ("Hugo", "hugo@rust.dev", Some(23)),
    ("Inès", "ines@rust.dev", Some(19)),
    ("Jules", "jules@rust.dev", Some(16)),
    ("Karim", "karim@rust.dev", Some(38)),
    ("Lucía", "lucia@rust.dev", Some(26)),
    ("Mateo", "mateo@rust.dev", Some(33)),
    ("Nora", "nora@rust.dev", None),
    ("Oliver", "oliver@rust.dev", Some(47)),
    ("Camille", "camille@rust.dev", Some(29)),
    ("Emilio", "emilio@rust.dev", Some(61)),
    ("Amelia", "amelia@rust.dev", Some(21)),
];

thread_local! {
    static DB: MemoryDb = seed();
}

fn seed() -> MemoryDb {
    let db = MemoryDb::new();
    let users: Vec<User> = PEOPLE
        .iter()
        .map(|(name, email, age)| User { id: 0, email: (*email).to_owned(), name: (*name).to_owned(), age: *age })
        .collect();
    memoire_orm::block_on(async {
        db.create_table::<User>().await?;
        User::insert_many(&db, &users).await
    })
    .expect("seeding the playground database");
    db
}

fn build(params: &PlaygroundParams) -> memoire_orm::Select<User> {
    let mut filter = User::AGE.ge(params.min_age);
    let needle = params.name_contains.trim();
    if !needle.is_empty() {
        filter = filter & User::NAME.contains(needle);
    }
    let order = match (params.order_by, params.descending) {
        (OrderField::Name, false) => User::NAME.asc(),
        (OrderField::Name, true) => User::NAME.desc(),
        (OrderField::Age, false) => User::AGE.asc(),
        (OrderField::Age, true) => User::AGE.desc(),
    };
    User::query().filter(filter).order_by(order).limit(u64::from(params.limit))
}

/// Builds the query, renders it for every dialect and runs it on the
/// in-memory engine. The reported time is the average over many runs because
/// browsers only expose coarse timers.
pub fn run(params: &PlaygroundParams) -> PlaygroundOutput {
    let query = build(params);
    let sql_postgres = query.to_sql(Dialect::Postgres);
    let sql_mysql = query.to_sql(Dialect::MySql);
    let sql_sqlite = query.to_sql(Dialect::Sqlite);

    DB.with(|db| {
        const RUNS: u32 = 200;
        let start = crate::dom::now_ms();
        let mut users = Vec::new();
        for _ in 0..RUNS {
            users = memoire_orm::block_on(query.clone().all(db)).unwrap_or_default();
        }
        let micros = (crate::dom::now_ms() - start) * 1000.0 / f64::from(RUNS);
        PlaygroundOutput {
            sql_postgres,
            sql_mysql,
            sql_sqlite,
            rows: users.into_iter().map(|u| (u.id, u.name, u.email, u.age)).collect(),
            micros,
        }
    })
}
