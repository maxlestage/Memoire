//! Code samples shown on the site. They mirror the real `memoire_orm` API.

pub const MODEL: &str = r#"use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    pub name: String,
    pub age: Option<i32>,
}"#;

pub const QUERY: &str = r#"let db = MemoryDb::new();            // or SqliteDb::open("app.db")?, PgPool::connect(url).await?
db.create_table::<User>().await?;

let mut alice = User { id: 0, email: "alice@rust.dev".into(), name: "Alice".into(), age: Some(31) };
alice.insert(&db).await?;            // alice.id is now set

let adults = User::query()
    .filter(User::AGE.ge(18) & User::NAME.contains("li"))
    .order_by(User::NAME.asc())
    .limit(20)
    .all(&db)
    .await?;"#;

pub const TRANSACTION: &str = r#"let tx = db.begin().await?;
alice.name = "Alice Cooper".into();
alice.update(&tx).await?;
Post::delete_many().filter(Post::USER_ID.eq(alice.id)).execute(&tx).await?;
tx.commit().await?;"#;

pub const MIGRATION: &str = r#"let migrator = Migrator::new()
    .add(Migration::new(1, "create_users").create_table::<User>())
    .add(Migration::new(2, "create_posts").create_table::<Post>());
migrator.run(&db).await?;"#;

pub const SQL_EXAMPLE: &str = r#"SELECT "users"."id", "users"."email", "users"."name", "users"."age" FROM "users" WHERE ("users"."age" >= $1 AND "users"."name" LIKE $2 ESCAPE '\') ORDER BY "users"."name" ASC LIMIT 20"#;

pub const INSTALL: &str = "cargo add memoire-orm";

/// A line of the fake terminal: typed commands vs. printed output.
#[derive(Clone, Copy, PartialEq)]
pub enum TermLine {
    Cmd(&'static str),
    Out(&'static str),
    Ok(&'static str),
}

/// Terminal script for each code tab (same order as the tabs).
pub const TERMINAL: [&[TermLine]; 4] = [
    &[
        TermLine::Cmd("cargo add memoire-orm"),
        TermLine::Out("    Updating crates.io index"),
        TermLine::Out("      Adding memoire-orm to dependencies"),
        TermLine::Cmd("cargo build --release"),
        TermLine::Out("   Compiling memoire-orm"),
        TermLine::Ok("    Finished `release` profile [optimized]"),
    ],
    &[
        TermLine::Cmd("cargo run --release"),
        TermLine::Out("-- PostgreSQL"),
        TermLine::Out(SQL_EXAMPLE),
        TermLine::Ok("✓ query built and type-checked at compile time"),
    ],
    &[
        TermLine::Cmd("cargo run --example transaction"),
        TermLine::Out("BEGIN"),
        TermLine::Out("UPDATE \"users\" SET … WHERE \"users\".\"id\" = $1"),
        TermLine::Out("DELETE FROM \"posts\" WHERE \"posts\".\"user_id\" = $1"),
        TermLine::Ok("COMMIT ✓"),
    ],
    &[
        TermLine::Cmd("cargo run -- migrate"),
        TermLine::Out("→ 1 create_users"),
        TermLine::Out("→ 2 create_posts"),
        TermLine::Ok("✓ schema is up to date"),
    ],
];

/// Short per-feature snippets for the /features page (same order as the features).
pub const FEATURE_SNIPPETS: [(&str, &str); 8] = [
    ("bash", "cargo add memoire-orm"),
    (
        "rust",
        "#[derive(Model, Debug, Clone)]\n#[memoire(table = \"users\")]\npub struct User {\n    #[memoire(primary_key, auto_increment)]\n    pub id: i64,\n    pub name: String,\n}",
    ),
    (
        "rust",
        "User::query()\n    .filter(User::AGE.ge(18) & User::NAME.contains(\"li\"))\n    .order_by(User::NAME.asc())",
    ),
    (
        "rust",
        "let adults: Vec<User> = User::query()\n    .limit(20)\n    .all(&db)\n    .await?;",
    ),
    (
        "rust",
        "let db = MemoryDb::new();\n// or SqliteDb::open(\"app.db\")?\n// or PgPool::connect(url).await?",
    ),
    ("rust", "let db = MemoryDb::new();\ndb.create_table::<User>().await?;"),
    ("rust", "let tx = db.begin().await?;\nalice.update(&tx).await?;\ntx.commit().await?;"),
    (
        "rust",
        "let migrator = Migrator::new()\n    .add(Migration::new(1, \"create_users\").create_table::<User>());\nmigrator.run(&db).await?;",
    ),
];
