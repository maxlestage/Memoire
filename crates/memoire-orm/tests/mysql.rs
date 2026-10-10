#![cfg(feature = "mysql")]
//! Runs when `MEMOIRE_MYSQL_URL` is set, e.g.
//! `MEMOIRE_MYSQL_URL=mysql://memoire:memoire@127.0.0.1:3306/memoire_test cargo test --all-features`.
mod common;

use memoire_orm::prelude::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_suite() {
    let Some(url) = std::env::var("MEMOIRE_MYSQL_URL").ok().filter(|s| !s.is_empty()) else {
        eprintln!("MEMOIRE_MYSQL_URL not set: skipping MySQL tests");
        return;
    };
    let db = MySqlPool::connect(&url).await.unwrap();
    common::run_suite(&db).await.unwrap();

    // Foreign keys, raw SQL and DECIMAL decoding.
    use common::{Post, user};
    let mut u = user("my@x.dev", "My", Some(5), 1.0);
    u.insert(&db).await.unwrap();
    Post { id: 0, user_id: u.id, title: "t".into(), views: 1 }.insert(&db).await.unwrap();
    u.delete(&db).await.unwrap();
    assert_eq!(Post::count(&db).await.unwrap(), 0, "ON DELETE CASCADE");
    let err = Post { id: 0, user_id: 4242, title: "orphan".into(), views: 0 }.insert(&db).await.unwrap_err();
    assert!(matches!(err, Error::Constraint(_)), "{err:?}");

    let rows: Vec<(String, f64, i64)> =
        db.raw("SELECT ?, CAST(12.5 AS DECIMAL(10,2)), CAST(? AS SIGNED)", &params!["x", 7_i64]).await.unwrap();
    assert_eq!(rows, vec![("x".to_owned(), 12.5, 7)]);

    // A no-op update still finds its row (CLIENT_FOUND_ROWS).
    let mut v = user("noop@x.dev", "Noop", None, 0.0);
    v.insert(&db).await.unwrap();
    v.update(&db).await.unwrap();
}
