#![cfg(feature = "memory")]
mod common;

use memoire_orm::MemoryDb;

#[test]
fn memory_suite() {
    memoire_orm::block_on(common::run_suite(&MemoryDb::new())).unwrap();
}

#[test]
fn memory_transactions_detect_conflicts() {
    use common::{User, user};
    use memoire_orm::prelude::*;

    memoire_orm::block_on(async {
        let db = MemoryDb::new();
        db.create_table::<User>().await?;
        let mut a = user("a@x.dev", "A", None, 0.0);
        a.insert(&db).await?;

        let t1 = db.begin().await?;
        let t2 = db.begin().await?;
        User::update_many().set(User::NAME, "from t1").execute(&t1).await?;
        User::update_many().set(User::NAME, "from t2").execute(&t2).await?;
        // Snapshot isolation: t2 does not see t1's uncommitted write.
        assert_eq!(User::get(&t2, a.id).await?.name, "from t2");
        t1.commit().await?;
        assert!(t2.commit().await.unwrap_err().is_conflict());
        assert_eq!(User::get(&db, a.id).await?.name, "from t1");

        // Read-only transactions never conflict.
        let t3 = db.begin().await?;
        let _ = User::count(&t3).await?;
        a.name = "outside".into();
        a.update(&db).await?;
        t3.commit().await?;
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn memory_raw_sql_is_rejected() {
    use memoire_orm::prelude::*;
    let db = MemoryDb::new();
    let err = memoire_orm::block_on(db.raw::<i64>("SELECT 1", &[])).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
}
