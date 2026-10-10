#![cfg(feature = "postgres")]
//! Runs when `MEMOIRE_PG_URL` is set, e.g.
//! `MEMOIRE_PG_URL=postgres://postgres:postgres@localhost/memoire_test cargo test --all-features`.
mod common;

use memoire_orm::prelude::*;

fn url() -> Option<String> {
    std::env::var("MEMOIRE_PG_URL").ok().filter(|s| !s.is_empty())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_suite() {
    let Some(url) = url() else {
        eprintln!("MEMOIRE_PG_URL not set: skipping PostgreSQL tests");
        return;
    };
    let db = PgPool::connect(&url).await.unwrap();
    common::run_suite(&db).await.unwrap();

    // Raw SQL, foreign keys and NUMERIC decoding.
    use common::{Post, user};
    let mut u = user("pg@x.dev", "Pg", Some(5), 1.0);
    u.insert(&db).await.unwrap();
    Post { id: 0, user_id: u.id, title: "t".into(), views: 1 }.insert(&db).await.unwrap();
    u.delete(&db).await.unwrap();
    assert_eq!(Post::count(&db).await.unwrap(), 0, "ON DELETE CASCADE");
    let err = Post { id: 0, user_id: 4242, title: "orphan".into(), views: 0 }.insert(&db).await.unwrap_err();
    assert!(matches!(err, Error::Constraint(_)), "{err:?}");

    let rows: Vec<(String, f64, i64)> =
        db.raw("SELECT $1::text, 12.5::numeric, $2::bigint", &params!["x", 7_i64]).await.unwrap();
    assert_eq!(rows, vec![("x".to_owned(), 12.5, 7)]);
}
