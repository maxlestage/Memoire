#![cfg(feature = "sqlite")]
mod common;

use memoire_orm::prelude::*;

#[test]
fn sqlite_in_memory_suite() {
    let db = SqliteDb::open_in_memory().unwrap();
    memoire_orm::block_on(common::run_suite(&db)).unwrap();
}

#[test]
fn sqlite_lifecycle_suite() {
    let db = SqliteDb::open_in_memory().unwrap();
    memoire_orm::block_on(common::lifecycle::run(&db)).unwrap();
}

#[tokio::test]
async fn sqlite_file_suite_on_tokio() {
    let dir = std::env::temp_dir().join(format!("memoire-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("suite.db");
    let _ = std::fs::remove_file(&path);
    let db = SqliteDb::open(&path).unwrap();
    common::run_suite(&db).await.unwrap();
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sqlite_raw_sql_and_foreign_keys() {
    use common::{Post, User, user};
    memoire_orm::block_on(async {
        let db = SqliteDb::open_in_memory()?;
        db.create_table::<User>().await?;
        db.create_table::<Post>().await?;
        let mut u = user("fk@x.dev", "Fk", Some(3), 1.0);
        u.insert(&db).await?;
        Post { id: 0, user_id: u.id, title: "t".into(), views: 1 }.insert(&db).await?;

        // ON DELETE CASCADE is enforced (foreign keys are switched on).
        u.delete(&db).await?;
        assert_eq!(Post::count(&db).await?, 0);
        let err = Post { id: 0, user_id: 42, title: "orphan".into(), views: 0 }.insert(&db).await.unwrap_err();
        assert!(matches!(err, Error::Constraint(_)), "{err:?}");

        let n = db
            .raw_execute(
                "INSERT INTO users (email, name, age, active, score) VALUES (?, ?, ?, ?, ?)",
                &params!["raw@x.dev", "Raw", 7, true, 0.25],
            )
            .await?;
        assert_eq!(n, 1);
        let rows: Vec<(i64, String)> = db.raw("SELECT id, name FROM users WHERE age = ?", &params![7]).await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "Raw");
        Ok::<(), Error>(())
    })
    .unwrap();
}
