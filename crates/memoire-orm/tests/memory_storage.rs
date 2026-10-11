#![cfg(feature = "memory")]
//! `MemoryDb::open`: the in-memory engine stored in a file.
mod common;

use std::path::PathBuf;

use common::{Post, User, user};
use memoire_orm::prelude::*;
use memoire_orm::{StorageOptions, block_on};

/// A fresh file path, removed with its directory when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("memoire-storage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempFile(dir.join("db.memoire"))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[test]
fn the_shared_suite_runs_on_a_stored_database() {
    let file = TempFile::new("suite");
    block_on(common::run_suite(&MemoryDb::open(&file.0).unwrap())).unwrap();
    // Everything the suite left behind comes back, row for row.
    let before = MemoryDb::open(&file.0).unwrap();
    let names = before.table_names();
    assert!(names.contains(&"users".to_owned()), "{names:?}");
    let users: Vec<User> = block_on(User::query().order_by(User::ID).all(&before)).unwrap();
    drop(before);
    let after = MemoryDb::open(&file.0).unwrap();
    assert_eq!(block_on(User::query().order_by(User::ID).all(&after)).unwrap(), users);
}

#[test]
fn every_kind_of_write_is_replayed() {
    let file = TempFile::new("writes");
    block_on(async {
        let db = MemoryDb::open(&file.0)?;
        db.create_table::<User>().await?;
        db.create_table::<Post>().await?;
        let mut alice = user("alice@x.dev", "Alice", Some(30), 1.0);
        alice.insert(&db).await?;
        User::insert_many(&db, &[user("bob@x.dev", "Bob", None, 2.0), user("carol@x.dev", "Carol", Some(5), 3.0)])
            .await?;
        alice.name = "Alicia".into();
        alice.update(&db).await?;
        User::update_many().increment(User::SCORE, 10.0).filter(User::NAME.eq("Bob")).execute(&db).await?;
        let mut dave = user("dave@x.dev", "Dave", None, 0.0);
        dave.id = 40;
        dave.upsert(&db).await?;
        User::delete_by_key(&db, 3).await?;
        Post { id: 0, user_id: 1, title: "kept".into(), views: 1 }.insert(&db).await?;

        // Committed transactions are stored, rolled back ones are not.
        let tx = db.begin().await?;
        user("tx@x.dev", "Tx", None, 0.0).insert(&tx).await?;
        tx.commit().await?;
        let tx = db.begin().await?;
        user("gone@x.dev", "Gone", None, 0.0).insert(&tx).await?;
        tx.rollback().await?;
        let expected: Vec<User> = User::query().order_by(User::ID).all(&db).await?;
        drop(db);

        let db = MemoryDb::open(&file.0)?;
        assert_eq!(db.row_count("users")?, expected.len());
        assert_eq!(User::query().order_by(User::ID).all(&db).await?, expected);
        assert_eq!(User::get(&db, 2).await?.score, 12.0);
        assert_eq!(Post::count(&db).await?, 1);
        // Generated ids continue after the largest one ever used.
        let mut next = user("next@x.dev", "Next", None, 0.0);
        next.insert(&db).await?;
        assert_eq!(next.id, 42);
        // Constraints and indexes are rebuilt.
        assert!(user("alice@x.dev", "Dup", None, 0.0).insert(&db).await.unwrap_err().is_unique_violation());
        // A dropped table stays dropped.
        db.drop_table::<Post>().await?;
        drop(db);
        let db = MemoryDb::open(&file.0)?;
        assert!(!db.table_names().contains(&"posts".to_owned()));
        assert!(matches!(Post::count(&db).await.unwrap_err(), Error::NoSuchTable(_)));
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn a_torn_tail_is_dropped() {
    let file = TempFile::new("torn");
    block_on(async {
        let db = MemoryDb::open(&file.0)?;
        db.create_table::<User>().await?;
        user("a@x.dev", "A", None, 0.0).insert(&db).await?;
        drop(db);
        // A crash in the middle of an append leaves half a record.
        let len = std::fs::metadata(&file.0).unwrap().len();
        let mut bytes = std::fs::read(&file.0).unwrap();
        bytes.extend_from_slice(&[200, 0, 0, 0, 3, 1, 2]);
        std::fs::write(&file.0, &bytes).unwrap();

        let db = MemoryDb::open(&file.0)?;
        assert_eq!(std::fs::metadata(&file.0).unwrap().len(), len, "tail truncated");
        assert_eq!(User::count(&db).await?, 1);
        user("b@x.dev", "B", None, 0.0).insert(&db).await?;
        drop(db);
        assert_eq!(User::count(&MemoryDb::open(&file.0)?).await?, 2);
        Ok::<(), Error>(())
    })
    .unwrap();
}

mod v1 {
    use memoire_orm::prelude::*;
    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "people")]
    pub struct Person {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        pub name: String,
        pub legacy: i32,
    }
}

mod v2 {
    use memoire_orm::prelude::*;
    /// `legacy` removed, `email` added (nullable).
    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "people")]
    pub struct Person {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        pub email: Option<String>,
        pub name: String,
    }
}

mod v3 {
    use memoire_orm::prelude::*;
    /// A new column that cannot be NULL.
    #[derive(Model, Debug, Clone, PartialEq)]
    #[memoire(table = "people")]
    pub struct Person {
        #[memoire(primary_key, auto_increment)]
        pub id: i64,
        pub name: String,
        pub age: i32,
    }
}

#[test]
fn models_can_gain_and_lose_columns() {
    let file = TempFile::new("evolve");
    block_on(async {
        let db = MemoryDb::open(&file.0)?;
        db.create_table::<v1::Person>().await?;
        v1::Person { id: 0, name: "Ada".into(), legacy: 7 }.insert(&db).await?;
        drop(db);

        let db = MemoryDb::open(&file.0)?;
        assert!(v3::Person::count(&db).await.is_err(), "a new NOT NULL column needs a migration");
        let ada = v2::Person::get(&db, 1).await?;
        assert_eq!(ada, v2::Person { id: 1, email: None, name: "Ada".into() });
        v2::Person { id: 0, email: Some("b@x.dev".into()), name: "Bob".into() }.insert(&db).await?;
        drop(db);

        let db = MemoryDb::open(&file.0)?;
        let all = v2::Person::query().order_by(v2::Person::ID).all(&db).await?;
        assert_eq!(all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Ada", "Bob"]);
        assert_eq!(all[1].email.as_deref(), Some("b@x.dev"));
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn compaction_keeps_only_the_current_rows() {
    let file = TempFile::new("compact");
    block_on(async {
        let db = MemoryDb::open(&file.0)?;
        db.create_table::<User>().await?;
        let mut u = user("c@x.dev", "C", None, 0.0);
        u.insert(&db).await?;
        for i in 0..3000 {
            u.score = f64::from(i);
            u.update(&db).await?;
        }
        let big = std::fs::metadata(&file.0).unwrap().len();
        db.compact()?;
        let small = std::fs::metadata(&file.0).unwrap().len();
        assert!(small * 20 < big, "{small} vs {big}");
        u.score = -1.0;
        u.update(&db).await?;
        drop(db);
        let db = MemoryDb::open(&file.0)?;
        assert_eq!(User::get(&db, u.id).await?.score, -1.0);

        // It also happens on its own when the journal dwarfs the data.
        for i in 0..12_000 {
            u.score = f64::from(i);
            u.update(&db).await?;
        }
        assert!(std::fs::metadata(&file.0).unwrap().len() < big, "compacted automatically");
        drop(db);
        assert_eq!(User::get(&MemoryDb::open(&file.0)?, u.id).await?.score, 11_999.0);
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn not_a_storage_file() {
    let file = TempFile::new("garbage");
    std::fs::write(&file.0, b"hello, world").unwrap();
    assert!(MemoryDb::open(&file.0).is_err());
}

#[cfg(feature = "encryption")]
#[test]
fn encrypted_files_need_the_password() {
    let file = TempFile::new("encrypted");
    let secret = StorageOptions::new().password("correct horse battery staple");
    block_on(async {
        let db = MemoryDb::open_with(&file.0, secret.clone())?;
        db.create_table::<User>().await?;
        user("secret@x.dev", "Top Secret Name", None, 0.0).insert(&db).await?;
        drop(db);

        let raw = std::fs::read(&file.0).unwrap();
        let needle = b"Top Secret Name";
        assert!(!raw.windows(needle.len()).any(|w| w == needle), "the file must not hold the plain text");

        let wrong = MemoryDb::open_with(&file.0, StorageOptions::new().password("nope")).unwrap_err();
        assert!(wrong.to_string().contains("wrong password"), "{wrong}");
        assert!(MemoryDb::open(&file.0).is_err(), "no password");

        let db = MemoryDb::open_with(&file.0, secret.clone())?;
        assert_eq!(User::get(&db, 1).await?.name, "Top Secret Name");
        db.compact()?;
        drop(db);
        let db = MemoryDb::open_with(&file.0, secret)?;
        assert_eq!(User::count(&db).await?, 1);
        Ok::<(), Error>(())
    })
    .unwrap();
}

#[test]
fn sync_option_writes_through() {
    let file = TempFile::new("sync");
    block_on(async {
        let db = MemoryDb::open_with(&file.0, StorageOptions::new().sync(true))?;
        db.create_table::<User>().await?;
        user("s@x.dev", "S", None, 0.0).insert(&db).await?;
        drop(db);
        assert_eq!(User::count(&MemoryDb::open(&file.0)?).await?, 1);
        Ok::<(), Error>(())
    })
    .unwrap();
}
