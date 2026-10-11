#![cfg(feature = "web")]
//! `LocalStorageDb` and `Store` on `MemoryStorage`, which behaves like the
//! browser's storage (strings, quota, tabs and their `storage` events).
mod common;

use std::sync::{Arc, Mutex};

use common::{User, user};
use memoire_orm::block_on;
use memoire_orm::prelude::*;
use memoire_orm::web::{KeyValueStorage, LocalStorageDb, MemoryStorage};

#[test]
fn the_shared_suite_runs_on_local_storage() {
    let storage = MemoryStorage::new();
    let users: Vec<User> = {
        let db = LocalStorageDb::open_with("suite", storage.clone()).unwrap();
        block_on(common::run_suite(&db)).unwrap();
        block_on(db.create_table_if_not_exists::<User>()).unwrap();
        block_on(user("after@x.dev", "After", Some(3), 1.5).insert(&db)).unwrap();
        let all = block_on(User::query().order_by(User::ID).all(&db)).unwrap();
        db.close();
        all
    };
    // A new page load: everything comes back from the storage.
    let db = LocalStorageDb::open_with("suite", storage.clone()).unwrap();
    assert_eq!(block_on(User::query().order_by(User::ID).all(&db)).unwrap(), users);
    // Readable JSON, one key per row.
    let first = users.first().expect("the suite leaves users");
    let json = storage.get(&format!("suite:users:{}", first.id)).unwrap().unwrap();
    assert!(json.contains(&format!("\"email\":\"{}\"", first.email)), "{json}");
}

#[test]
fn rows_are_saved_as_they_change() {
    let storage = MemoryStorage::new();
    block_on(async {
        let db = LocalStorageDb::open_with("app", storage.clone())?;
        db.create_table::<User>().await?;
        let mut a = user("a@x.dev", "A", Some(1), 0.5);
        a.insert(&db).await?;
        User::insert_many(&db, &[user("b@x.dev", "B", None, 1.0), user("c@x.dev", "C", None, 2.0)]).await?;
        a.name = "Ann".into();
        a.update(&db).await?;
        User::delete_by_key(&db, 3).await?;
        User::update_many().set(User::ACTIVE, false).filter(User::ID.eq(2)).execute(&db).await?;

        let keys: Vec<String> = storage.snapshot().into_keys().collect();
        assert_eq!(keys, vec!["app:users", "app:users:1", "app:users:2"]);
        let meta: serde_json::Value = serde_json::from_str(&storage.get("app:users")?.unwrap()).unwrap();
        assert_eq!(meta["seq"], 4, "ids are not reused after a delete");
        let row: serde_json::Value = serde_json::from_str(&storage.get("app:users:1")?.unwrap()).unwrap();
        assert_eq!(row["name"], "Ann");
        assert_eq!(row["score"], 0.5);
        db.close();

        let db = LocalStorageDb::open_with("app", storage.clone())?;
        assert_eq!(db.row_count("users")?, 2);
        let mut d = user("d@x.dev", "D", None, 0.0);
        d.insert(&db).await?;
        assert_eq!(d.id, 4);
        assert!(user("a@x.dev", "Dup", None, 0.0).insert(&db).await.unwrap_err().is_unique_violation());
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn opening_twice_gives_the_same_database() {
    let storage = MemoryStorage::new();
    let a = LocalStorageDb::open_with("app", storage.clone()).unwrap();
    let b = LocalStorageDb::open_with("app", storage.clone()).unwrap();
    block_on(a.create_table::<User>()).unwrap();
    block_on(user("x@x.dev", "X", None, 0.0).insert(&a)).unwrap();
    assert_eq!(block_on(User::count(&b)).unwrap(), 1);
    assert!(LocalStorageDb::open_with("a:b", storage).is_err(), "`:` is reserved");
}

#[test]
fn two_tabs_share_the_database() {
    let storage = MemoryStorage::new();
    let tab_b_storage = storage.tab();
    block_on(async {
        let tab_a = LocalStorageDb::open_with("app", storage.clone())?;
        let tab_b = LocalStorageDb::open_with("app", tab_b_storage.clone())?;
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        let _watch = tab_b.on_external_change(move |t| log.lock().unwrap().push(t.map(str::to_owned)));
        let local_heard = std::rc::Rc::new(std::cell::Cell::new(0)); // not Send
        let local_log = std::rc::Rc::clone(&local_heard);
        let _local = tab_b.on_external_change_local(move |_| local_log.set(local_log.get() + 1));

        tab_a.create_table::<User>().await?;
        let mut a1 = user("a1@x.dev", "A1", None, 0.0);
        a1.insert(&tab_a).await?;
        // Tab B reads what tab A wrote, and its ids follow A's.
        assert_eq!(User::count(&tab_b).await?, 1);
        let mut b1 = user("b1@x.dev", "B1", None, 0.0);
        b1.insert(&tab_b).await?;
        assert_eq!(b1.id, 2, "no id collision");
        a1.name = "A1 bis".into();
        a1.update(&tab_a).await?;
        assert_eq!(User::get(&tab_b, a1.id).await?.name, "A1 bis");
        assert_eq!(User::count(&tab_a).await?, 2);
        assert!(heard.lock().unwrap().iter().all(|t| t.as_deref() == Some("users")));
        assert!(!heard.lock().unwrap().is_empty(), "tab B heard tab A");
        assert_eq!(local_heard.get(), heard.lock().unwrap().len(), "the thread-bound listener too");

        // A transaction of tab A conflicts with a write of tab B in between.
        let tx = tab_a.begin().await?;
        user("tx@x.dev", "Tx", None, 0.0).insert(&tx).await?;
        user("b2@x.dev", "B2", None, 0.0).insert(&tab_b).await?;
        assert!(tx.commit().await.unwrap_err().is_conflict());
        assert_eq!(User::count(&tab_a).await?, 3, "B's row, not the transaction's");
        assert!(tab_b_storage.get("app:users:4")?.is_none() || User::get(&tab_a, 4).await.is_ok());

        // Tab B clears the database: tab A sees no table any more.
        tab_b.clear().await?;
        assert!(matches!(User::count(&tab_a).await.unwrap_err(), Error::NoSuchTable(_)));
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn a_write_that_does_not_fit_changes_nothing() {
    let storage = MemoryStorage::with_quota(2_000);
    block_on(async {
        let db = LocalStorageDb::open_with("app", storage.clone())?;
        db.create_table::<User>().await?;
        user("small@x.dev", "Small", None, 0.0).insert(&db).await?;
        let before = storage.snapshot();

        let big = user("big@x.dev", &"x".repeat(5_000), None, 0.0);
        let err = big.clone().insert(&db).await.unwrap_err();
        assert!(matches!(err, Error::Storage(_)), "{err:?}");
        assert_eq!(storage.snapshot(), before, "the storage is untouched");
        assert_eq!(User::count(&db).await?, 1, "the database is untouched");

        // A batch: all or nothing.
        let batch: Vec<User> = (0..50).map(|i| user(&format!("u{i}@x.dev"), &"y".repeat(40), None, 0.0)).collect();
        assert!(User::insert_many(&db, &batch).await.is_err());
        assert_eq!(storage.snapshot(), before);
        assert_eq!(User::count(&db).await?, 1);

        user("fits@x.dev", "Fits", None, 0.0).insert(&db).await?;
        assert_eq!(User::count(&db).await?, 2);
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn clear_only_touches_its_own_keys() {
    let storage = MemoryStorage::new();
    storage.set("theme", "dark").unwrap();
    block_on(async {
        let db = LocalStorageDb::open_with("app", storage.clone())?;
        db.create_table::<User>().await?;
        user("a@x.dev", "A", None, 0.0).insert(&db).await?;
        let other = LocalStorageDb::open_with("other", storage.clone())?;
        other.create_table::<User>().await?;
        db.clear().await?;
        let keys: Vec<String> = storage.snapshot().into_keys().collect();
        assert_eq!(keys, vec!["other:users", "theme"]);
        Ok::<(), Error>(())
    })
    .unwrap();
}

mod v2 {
    use memoire_orm::prelude::*;
    /// `users` with a new nullable column and without `score`.
    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "users")]
    pub struct User {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        #[memoire(unique)]
        pub email: String,
        pub name: String,
        pub nickname: Option<String>,
    }
}

#[test]
fn models_can_change_between_visits() {
    let storage = MemoryStorage::new();
    block_on(async {
        {
            let db = LocalStorageDb::open_with("app", storage.clone())?;
            db.create_table::<User>().await?;
            user("a@x.dev", "Ada", Some(36), 1.0).insert(&db).await?;
            db.close();
        }
        let db = LocalStorageDb::open_with("app", storage.clone())?;
        let ada = v2::User::get(&db, 1).await?;
        assert_eq!((ada.name.as_str(), ada.nickname.as_deref()), ("Ada", None));
        let mut ada = ada;
        ada.nickname = Some("Countess".into());
        ada.update(&db).await?;
        let row: serde_json::Value = serde_json::from_str(&storage.get("app:users:1")?.unwrap()).unwrap();
        assert_eq!(row["nickname"], "Countess");
        assert!(row.get("score").is_none(), "the removed column is gone from the new row");
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn rows_edited_in_the_developer_tools_are_read() {
    let storage = MemoryStorage::new();
    block_on(async {
        let db = LocalStorageDb::open_with("app", storage.clone())?;
        db.create_table::<User>().await?;
        user("a@x.dev", "A", None, 0.0).insert(&db).await?;
        // Someone edits the row by hand (another tab, or the devtools) and bumps the version.
        let other = storage.tab();
        let mut row: serde_json::Value = serde_json::from_str(&other.get("app:users:1")?.unwrap()).unwrap();
        row["name"] = "Edited".into();
        other.set("app:users:1", &row.to_string())?;
        let mut meta: serde_json::Value = serde_json::from_str(&other.get("app:users")?.unwrap()).unwrap();
        meta["ver"] = (meta["ver"].as_u64().unwrap() + 1).into();
        other.set("app:users", &meta.to_string())?;
        assert_eq!(User::get(&db, 1).await?.name, "Edited");
        Ok::<(), Error>(())
    })
    .unwrap();
}
