//! A backend-agnostic test suite: every backend must behave identically.
#![allow(dead_code)]

use memoire_orm::prelude::*;
use memoire_orm::{Database, Transaction};

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    #[memoire(index)]
    pub name: String,
    pub age: Option<i32>,
    pub active: bool,
    pub score: f64,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "posts")]
pub struct Post {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(references = "users(id)", on_delete = "cascade", index)]
    pub user_id: i64,
    pub title: String,
    pub views: i32,
}

#[derive(Model, Debug, Clone, PartialEq)]
#[memoire(table = "tags")]
pub struct Tag {
    #[memoire(primary_key)]
    pub slug: String,
    pub label: String,
}

#[derive(FromRow, Debug, PartialEq)]
pub struct UserStats {
    pub user_id: i64,
    pub posts: i64,
    pub total_views: Option<i64>,
}

pub fn user(email: &str, name: &str, age: Option<i32>, score: f64) -> User {
    User { id: 0, email: email.into(), name: name.into(), age, active: true, score }
}

async fn reset<E: Executor>(db: &E) -> Result<()> {
    db.drop_table_if_exists::<Post>().await?;
    db.drop_table_if_exists::<User>().await?;
    db.drop_table_if_exists::<Tag>().await?;
    db.execute(memoire_orm::Query::DropTable { name: "_memoire_migrations", if_exists: true }).await?;
    db.create_table::<User>().await?;
    db.create_table::<Post>().await?;
    db.create_table::<Tag>().await?;
    Ok(())
}

async fn seed<E: Executor>(db: &E) -> Result<Vec<User>> {
    let mut users = vec![
        user("alice@example.com", "Alice", Some(31), 1.5),
        user("bob@example.com", "Bob", Some(17), 2.5),
        user("carol@example.com", "Carol", None, 3.0),
        user("dave@example.com", "Dave", Some(45), 4.0),
        user("eve@example.com", "Eve", Some(22), 0.5),
    ];
    for u in &mut users {
        u.insert(db).await?;
    }
    Ok(users)
}

pub async fn run_suite<D: Database>(db: &D) -> Result<()> {
    reset(db).await?;
    crud(db).await?;
    reset(db).await?;
    queries(db).await?;
    reset(db).await?;
    updates_and_deletes(db).await?;
    reset(db).await?;
    joins_and_groups(db).await?;
    reset(db).await?;
    transactions(db).await?;
    reset(db).await?;
    bulk(db).await?;
    reset(db).await?;
    errors(db).await?;
    migrations(db).await?;
    Ok(())
}

async fn crud<E: Executor>(db: &E) -> Result<()> {
    let users = seed(db).await?;
    assert_eq!(users.iter().map(|u| u.id).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5], "generated keys");

    let alice = User::get(db, 1).await?;
    assert_eq!(alice, users[0]);
    assert_eq!(User::find(db, 99).await?, None);
    assert!(User::get(db, 99).await.unwrap_err().is_not_found());
    assert_eq!(User::count(db).await?, 5);

    let mut bob = User::get(db, 2).await?;
    bob.name = "Robert".into();
    bob.age = None;
    bob.update(db).await?;
    let reloaded = User::get(db, 2).await?;
    assert_eq!(reloaded.name, "Robert");
    assert_eq!(reloaded.age, None);

    let mut ghost = user("ghost@example.com", "Ghost", None, 0.0);
    ghost.id = 999;
    assert!(ghost.update(db).await.unwrap_err().is_not_found());

    assert!(bob.delete(db).await?);
    assert!(!bob.delete(db).await?);
    assert_eq!(User::count(db).await?, 4);
    assert!(User::delete_by_key(db, 3).await?);

    let mut eve = User::get(db, 5).await?;
    eve.score = 99.0;
    eve.update(db).await?;
    eve.score = 0.0;
    eve.reload(db).await?;
    assert_eq!(eve.score, 99.0);

    // Non auto-increment string key + upsert.
    let mut tag = Tag { slug: "rust".into(), label: "Rust".into() };
    tag.insert(db).await?;
    tag.label = "Rust 🦀".into();
    tag.upsert(db).await?;
    Tag { slug: "orm".into(), label: "ORM".into() }.upsert(db).await?;
    assert_eq!(Tag::get(db, "rust").await?.label, "Rust 🦀");
    assert_eq!(Tag::count(db).await?, 2);
    Ok(())
}

async fn queries<E: Executor>(db: &E) -> Result<()> {
    seed(db).await?;

    let names = |v: Vec<User>| v.into_iter().map(|u| u.name).collect::<Vec<_>>();

    let adults = User::query().filter(User::AGE.ge(18)).order_by(User::NAME.asc()).all(db).await?;
    assert_eq!(names(adults), ["Alice", "Dave", "Eve"]);

    let q = User::query().filter(User::AGE.ge(18) & User::NAME.contains("li")).all(db).await?;
    assert_eq!(names(q), ["Alice"]);

    // LIKE is case-sensitive everywhere, ILIKE is not.
    assert!(User::query().filter(User::NAME.contains("LI")).all(db).await?.is_empty());
    assert_eq!(names(User::query().filter(User::NAME.icontains("LI")).all(db).await?), ["Alice"]);
    assert_eq!(names(User::query().filter(User::NAME.starts_with("Da")).all(db).await?), ["Dave"]);
    assert_eq!(
        names(User::query().filter(User::NAME.ends_with("ve")).order_by(User::ID).all(db).await?),
        ["Dave", "Eve"]
    );
    assert_eq!(names(User::query().filter(User::EMAIL.like("_ob@%")).all(db).await?), ["Bob"]);
    // Wildcards in user input are escaped.
    assert!(User::query().filter(User::NAME.contains("%")).all(db).await?.is_empty());

    let q = User::query().filter(User::ID.is_in([1, 3, 5])).order_by(User::ID.desc()).all(db).await?;
    assert_eq!(names(q), ["Eve", "Carol", "Alice"]);
    assert!(User::query().filter(User::ID.is_in(Vec::<i64>::new())).all(db).await?.is_empty());
    assert_eq!(User::query().filter(User::ID.not_in(Vec::<i64>::new())).count(db).await?, 5);
    assert_eq!(
        names(User::query().filter(User::AGE.between(20, 31)).order_by(User::ID).all(db).await?),
        ["Alice", "Eve"]
    );
    assert_eq!(names(User::query().filter(User::AGE.is_null()).all(db).await?), ["Carol"]);
    assert_eq!(User::query().filter(User::AGE.is_not_null()).count(db).await?, 4);
    // NULL never matches a comparison (three-valued logic).
    assert_eq!(User::query().filter(!User::AGE.gt(18)).count(db).await?, 1);
    assert_eq!(
        names(User::query().filter(User::NAME.eq("Bob") | User::NAME.eq("Eve")).order_by(User::ID).all(db).await?),
        ["Bob", "Eve"]
    );
    assert_eq!(
        names(User::query().filter(User::SCORE.gt(2.0)).order_by(User::SCORE.desc()).all(db).await?),
        ["Dave", "Carol", "Bob"]
    );

    // NULLS FIRST / LAST.
    let q = User::query().order_by(User::AGE.asc().nulls_first()).all(db).await?;
    assert_eq!(q[0].name, "Carol");
    let q = User::query().order_by(User::AGE.asc().nulls_last()).all(db).await?;
    assert_eq!(q[4].name, "Carol");
    assert_eq!(q[0].name, "Bob");

    // Pagination.
    let page = User::query().order_by(User::ID).paginate(db, 2, 2).await?;
    assert_eq!((page.total, page.total_pages(), page.has_next(), page.has_prev()), (5, 3, true, true));
    assert_eq!(names(page.items), ["Carol", "Dave"]);
    assert_eq!(names(User::query().order_by(User::ID).offset(3).all(db).await?), ["Dave", "Eve"]);

    // first / one / exists.
    assert_eq!(
        User::query().order_by(User::AGE.desc().nulls_last()).first(db).await?.map(|u| u.name),
        Some("Dave".into())
    );
    assert!(User::query().filter(User::NAME.eq("Nobody")).one(db).await.unwrap_err().is_not_found());
    assert!(User::query().filter(User::NAME.eq("Eve")).exists(db).await?);
    assert!(!User::query().filter(User::NAME.eq("Nobody")).exists(db).await?);

    // Aggregates.
    assert_eq!(User::query().sum(User::AGE, db).await?, Some(115));
    assert_eq!(User::query().filter(User::NAME.eq("Nobody")).sum(User::AGE, db).await?, None);
    assert_eq!(User::query().max(User::AGE, db).await?, Some(45));
    assert_eq!(User::query().min(User::NAME, db).await?, Some("Alice".to_owned()));
    let avg = User::query().avg(User::AGE, db).await?.unwrap();
    assert!((avg - 28.75).abs() < 1e-9, "avg = {avg}");
    assert_eq!(User::query().sum(User::SCORE, db).await?, Some(11.5));

    // Typed projections.
    let pairs: Vec<(i64, String)> =
        User::query().columns((User::ID, User::NAME)).order_by(User::ID).limit(2).all(db).await?;
    assert_eq!(pairs, vec![(1, "Alice".to_owned()), (2, "Bob".to_owned())]);
    let ages: Vec<Option<i32>> = User::query().columns(User::AGE).order_by(User::ID).all(db).await?;
    assert_eq!(ages, vec![Some(31), Some(17), None, Some(45), Some(22)]);
    let distinct_active: Vec<bool> = User::query().columns(User::ACTIVE).distinct().all(db).await?;
    assert_eq!(distinct_active, vec![true]);
    assert_eq!(User::query().columns(User::ACTIVE).distinct().count(db).await?, 1);
    Ok(())
}

async fn updates_and_deletes<E: Executor>(db: &E) -> Result<()> {
    seed(db).await?;
    let n = User::update_many()
        .set(User::ACTIVE, false)
        .increment(User::AGE, 1)
        .filter(User::AGE.lt(30))
        .execute(db)
        .await?;
    assert_eq!(n, 2);
    assert_eq!(User::get(db, 2).await?.age, Some(18));
    assert!(!User::get(db, 5).await?.active);
    assert!(User::get(db, 1).await?.active);

    User::update_many().set_null(User::AGE).filter(User::NAME.eq("Dave")).execute(db).await?;
    assert_eq!(User::get(db, 4).await?.age, None);
    User::update_many().set(User::AGE, 50).filter(User::NAME.eq("Dave")).execute(db).await?;
    assert_eq!(User::get(db, 4).await?.age, Some(50));
    User::update_many().set(User::NAME, "Dave II").filter(User::ID.eq(4)).execute(db).await?;
    assert_eq!(User::get(db, 4).await?.name, "Dave II");

    // Unique violation in a multi-row update leaves the table untouched.
    let err = User::update_many().set(User::EMAIL, "same@example.com").execute(db).await.unwrap_err();
    assert!(err.is_unique_violation(), "{err:?}");
    assert_eq!(User::get(db, 1).await?.email, "alice@example.com");

    let deleted = User::delete_many().filter(User::ACTIVE.eq(false)).execute(db).await?;
    assert_eq!(deleted, 2);
    assert_eq!(User::count(db).await?, 3);
    assert_eq!(User::delete_many().execute(db).await?, 3);
    assert_eq!(User::count(db).await?, 0);
    Ok(())
}

async fn joins_and_groups<E: Executor>(db: &E) -> Result<()> {
    seed(db).await?;
    for (uid, title, views) in [(1, "Hello", 10), (1, "Rust", 30), (2, "Bob's", 5), (4, "Dave", 0)] {
        Post { id: 0, user_id: uid, title: title.into(), views }.insert(db).await?;
    }

    let rows: Vec<(User, Post)> =
        User::query().inner_join::<Post>(Post::USER_ID.eq_col(User::ID)).order_by(Post::ID.asc()).all(db).await?;
    assert_eq!(rows.len(), 4);
    assert_eq!((rows[1].0.name.as_str(), rows[1].1.title.as_str()), ("Alice", "Rust"));

    let rows: Vec<(User, Option<Post>)> = User::query()
        .left_join::<Post>(Post::USER_ID.eq_col(User::ID))
        .filter(User::NAME.is_in(["Carol", "Bob"]))
        .order_by(User::ID)
        .all(db)
        .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1.as_ref().map(|p| p.title.as_str()), Some("Bob's"));
    assert_eq!(rows[1].1, None);

    // Join used only for filtering.
    let writers = User::query()
        .join::<Post>(JoinKind::Inner, Post::USER_ID.eq_col(User::ID))
        .filter(Post::VIEWS.ge(5))
        .distinct()
        .order_by(User::ID)
        .all(db)
        .await?;
    assert_eq!(writers.iter().map(|u| u.id).collect::<Vec<_>>(), vec![1, 2]);

    // GROUP BY report decoded into a #[derive(FromRow)] struct.
    let stats: Vec<UserStats> = Post::query()
        .group_by(Post::USER_ID)
        .having(count_all().gt(Value::I64(1)))
        .select_exprs([Post::USER_ID.expr(), count_all(), Post::VIEWS.sum()])
        .all(db)
        .await?;
    assert_eq!(stats, vec![UserStats { user_id: 1, posts: 2, total_views: Some(40) }]);

    let per_user: Vec<(i64, i64)> = Post::query()
        .group_by(Post::USER_ID)
        .select_exprs([Post::USER_ID.expr(), count_all()])
        .order_by(Post::USER_ID.asc())
        .all(db)
        .await?;
    assert_eq!(per_user, vec![(1, 2), (2, 1), (4, 1)]);

    // A join with a projection of columns from both tables.
    let titles: Vec<(String, String)> = Post::query()
        .inner_join::<User>(User::ID.eq_col(Post::USER_ID))
        .columns((User::NAME, Post::TITLE))
        .filter(Post::VIEWS.gt(5))
        .order_by(Post::VIEWS.desc())
        .all(db)
        .await?;
    assert_eq!(titles, vec![("Alice".into(), "Rust".into()), ("Alice".into(), "Hello".into())]);
    Ok(())
}

async fn transactions<D: Database>(db: &D) -> Result<()> {
    let tx = db.begin().await?;
    let mut a = user("tx1@example.com", "Tx One", None, 0.0);
    a.insert(&tx).await?;
    assert_eq!(User::count(&tx).await?, 1);
    tx.commit().await?;
    assert_eq!(User::count(db).await?, 1);

    let tx = db.begin().await?;
    user("tx2@example.com", "Tx Two", None, 0.0).insert(&tx).await?;
    User::delete_many().filter(User::ID.eq(a.id)).execute(&tx).await?;
    tx.rollback().await?;
    assert_eq!(User::count(db).await?, 1);
    assert!(User::find(db, a.id).await?.is_some());

    {
        let tx = db.begin().await?;
        user("tx3@example.com", "Tx Three", None, 0.0).insert(&tx).await?;
        // dropped without commit
    }
    // Give asynchronous rollbacks (PostgreSQL) a chance to run.
    for _ in 0..50 {
        if User::count(db).await? == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(User::count(db).await?, 1);
    Ok(())
}

async fn bulk<E: Executor>(db: &E) -> Result<()> {
    let users: Vec<User> = (0..2500)
        .map(|i| user(&format!("user{i}@bulk.dev"), &format!("User {i:04}"), Some(i % 90), i as f64))
        .collect();
    assert_eq!(User::insert_many(db, &users).await?, 2500);
    assert_eq!(User::count(db).await?, 2500);
    assert_eq!(User::query().filter(User::AGE.eq(10)).count(db).await?, 28);
    let top = User::query().order_by(User::SCORE.desc()).limit(3).all(db).await?;
    assert_eq!(top.iter().map(|u| u.score).collect::<Vec<_>>(), vec![2499.0, 2498.0, 2497.0]);
    let page = User::query().order_by(User::NAME.asc()).offset(10).limit(2).all(db).await?;
    assert_eq!(page.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["User 0010", "User 0011"]);
    // A failing batch inserts nothing.
    let dupes = vec![user("x@bulk.dev", "X", None, 0.0), user("x@bulk.dev", "X2", None, 0.0)];
    assert!(User::insert_many(db, &dupes).await.unwrap_err().is_unique_violation());
    assert_eq!(User::count(db).await?, 2500);
    Ok(())
}

async fn errors<E: Executor>(db: &E) -> Result<()> {
    let mut a = user("dup@example.com", "Dup", None, 0.0);
    a.insert(db).await?;
    let err = user("dup@example.com", "Dup 2", None, 0.0).insert(db).await.unwrap_err();
    match &err {
        Error::UniqueViolation { column, .. } => assert_eq!(column, "email"),
        other => panic!("expected a unique violation, got {other:?}"),
    }
    let mut tag = Tag { slug: "k".into(), label: "K".into() };
    tag.insert(db).await?;
    assert!(tag.insert(db).await.unwrap_err().is_unique_violation());

    db.drop_table::<Tag>().await?;
    assert!(matches!(Tag::count(db).await.unwrap_err(), Error::NoSuchTable(t) if t == "tags"));
    assert!(matches!(db.create_table::<User>().await.unwrap_err(), Error::TableExists(t) if t == "users"));
    db.create_table_if_not_exists::<User>().await?;
    db.create_table::<Tag>().await?;
    Ok(())
}

async fn migrations<D: Database>(db: &D) -> Result<()> {
    db.drop_table_if_exists::<Post>().await?;
    db.drop_table_if_exists::<User>().await?;
    let m = Migrator::new()
        .add(Migration::new(2, "create_posts").create_table::<Post>())
        .add(Migration::new(1, "create_users").create_table::<User>());
    assert_eq!(m.run(db).await?, vec![1, 2]);
    assert!(m.run(db).await?.is_empty());
    assert_eq!(m.applied(db).await?.len(), 2);

    // A failing migration is rolled back and reported.
    let bad = m.clone().add(Migration::new(3, "broken").create_table::<User>());
    let err = bad.run(db).await.unwrap_err();
    assert!(matches!(err, Error::Migration { version: 3, .. }), "{err:?}");
    assert_eq!(bad.pending(db).await?.len(), 1);
    Ok(())
}
