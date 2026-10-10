#![cfg(feature = "redis")]
//! Runs when `MEMOIRE_REDIS_URL` is set, e.g.
//! `MEMOIRE_REDIS_URL=redis://127.0.0.1:6379 cargo test --all-features`.
mod common;

use memoire_orm::prelude::*;

fn url() -> Option<String> {
    std::env::var("MEMOIRE_REDIS_URL").ok().filter(|s| !s.is_empty())
}

async fn connect(name: &str) -> Option<RedisDb> {
    let url = url()?;
    let prefix = format!("memoire_test_{name}_{}", std::process::id());
    Some(RedisDb::connect_with_prefix(&url, &prefix).await.unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redis_suite() {
    let Some(db) = connect("suite").await else {
        eprintln!("MEMOIRE_REDIS_URL not set: skipping Redis tests");
        return;
    };
    common::run_suite(&db).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redis_transactions_detect_conflicts() {
    use common::{User, user};
    let Some(db) = connect("conflicts").await else { return };
    db.drop_table_if_exists::<User>().await.unwrap();
    db.create_table::<User>().await.unwrap();
    let mut a = user("a@x.dev", "A", None, 0.0);
    a.insert(&db).await.unwrap();

    let t1 = db.begin().await.unwrap();
    let t2 = db.begin().await.unwrap();
    User::update_many().set(User::NAME, "from t1").execute(&t1).await.unwrap();
    User::update_many().set(User::NAME, "from t2").execute(&t2).await.unwrap();
    assert_eq!(User::get(&t2, a.id).await.unwrap().name, "from t2");
    t1.commit().await.unwrap();
    assert!(t2.commit().await.unwrap_err().is_conflict());
    assert_eq!(User::get(&db, a.id).await.unwrap().name, "from t1");

    // Concurrent single-row updates are retried transparently.
    let mut tasks = Vec::new();
    for i in 0..20 {
        let db = db.clone();
        tasks.push(tokio::spawn(async move {
            User::update_many().increment(User::SCORE, 1.0).filter(User::ID.eq(1)).execute(&db).await.map(|_| i)
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(User::get(&db, a.id).await.unwrap().score, 20.0);

    // Rows are plain JSON in Redis.
    let mut conn = db.connection();
    let json: String =
        redis_json(&mut conn, &format!("memoire_test_conflicts_{}:{{users}}:rows", std::process::id())).await;
    assert!(json.contains("\"name\":\"from t1\""), "{json}");
    db.drop_table::<User>().await.unwrap();
}

async fn redis_json(conn: &mut impl redis_conn::Conn, key: &str) -> String {
    conn.hget(key, "1").await
}

mod redis_conn {
    pub trait Conn {
        fn hget(&mut self, key: &str, field: &str) -> impl std::future::Future<Output = String>;
    }
    impl<T: redis::aio::ConnectionLike + Send> Conn for T {
        async fn hget(&mut self, key: &str, field: &str) -> String {
            redis::cmd("HGET").arg(key).arg(field).query_async(self).await.unwrap()
        }
    }
}
