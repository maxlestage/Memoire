#![cfg(feature = "mongodb")]
//! Runs when `MEMOIRE_MONGO_URL` is set, e.g.
//! `MEMOIRE_MONGO_URL=mongodb://127.0.0.1:27017/?directConnection=true cargo test --all-features`.
//! A replica set (even a single node) is needed for transactions.
mod common;

use memoire_orm::prelude::*;

async fn connect(name: &str) -> Option<MongoDb> {
    let url = std::env::var("MEMOIRE_MONGO_URL").ok().filter(|s| !s.is_empty())?;
    let db = format!("memoire_test_{name}_{}", std::process::id());
    Some(MongoDb::connect_with_database(&url, &db).await.unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mongodb_suite() {
    let Some(db) = connect("suite").await else {
        eprintln!("MEMOIRE_MONGO_URL not set: skipping MongoDB tests");
        return;
    };
    assert!(db.supports_transactions(), "the test server must be a replica set");
    common::run_suite(&db).await.unwrap();
    db.database().drop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mongodb_transactions_and_documents() {
    use common::{User, user};
    let Some(db) = connect("tx").await else { return };
    db.create_table::<User>().await.unwrap();
    let mut a = user("a@x.dev", "A", None, 0.0);
    a.insert(&db).await.unwrap();

    // Concurrent transactions writing the same document: one of them conflicts.
    let t1 = db.begin().await.unwrap();
    let t2 = db.begin().await.unwrap();
    User::update_many().set(User::NAME, "from t1").execute(&t1).await.unwrap();
    let second = User::update_many().set(User::NAME, "from t2").execute(&t2).await;
    let outcome = match second {
        Err(e) => {
            t1.rollback().await.unwrap();
            let _ = t2.rollback().await;
            e
        }
        Ok(_) => {
            t1.commit().await.unwrap();
            t2.commit().await.unwrap_err()
        }
    };
    assert!(outcome.is_conflict(), "{outcome:?}");

    // NULLs are absent fields, so a unique column accepts many of them.
    let raw = db.database().collection::<mongodb::bson::Document>("users");
    let doc = raw.find_one(mongodb::bson::doc! { "_id": a.id }).await.unwrap().unwrap();
    assert!(!doc.contains_key("age"), "{doc:?}");
    assert_eq!(doc.get_str("email").unwrap(), "a@x.dev");
    db.database().drop().await.unwrap();
}

/// A standalone server (no replica set) has no transactions: `begin()` says so,
/// migrations still run, and failed batches are cleaned up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mongodb_standalone_server() {
    use common::{User, user};
    let Some(url) = std::env::var("MEMOIRE_MONGO_STANDALONE_URL").ok().filter(|s| !s.is_empty()) else { return };
    let db =
        MongoDb::connect_with_database(&url, &format!("memoire_test_standalone_{}", std::process::id())).await.unwrap();
    assert!(!db.supports_transactions());
    assert!(matches!(db.begin().await.unwrap_err(), Error::Unsupported(_)));

    let m = Migrator::new().add(Migration::new(1, "users").create_table::<User>());
    assert_eq!(m.run(&db).await.unwrap(), vec![1]);
    assert!(m.run(&db).await.unwrap().is_empty());

    user("dup@x.dev", "Existing", None, 0.0).insert(&db).await.unwrap();
    let batch = vec![user("new1@x.dev", "N1", None, 0.0), user("dup@x.dev", "N2", None, 0.0)];
    assert!(User::insert_many(&db, &batch).await.unwrap_err().is_unique_violation());
    assert_eq!(User::count(&db).await.unwrap(), 1, "the partial batch was removed");
    assert_eq!(User::query().filter(User::AGE.is_null()).count(&db).await.unwrap(), 1);
    db.database().drop().await.unwrap();
}
