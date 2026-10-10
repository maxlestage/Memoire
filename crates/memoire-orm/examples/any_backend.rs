//! The same application code on any backend, chosen by the URL scheme.
//!
//! ```bash
//! cargo run -p memoire-orm --example any_backend --features full -- memory
//! cargo run -p memoire-orm --example any_backend --features full -- sqlite::memory:
//! cargo run -p memoire-orm --example any_backend --features full -- postgres://postgres:postgres@localhost/app
//! cargo run -p memoire-orm --example any_backend --features full -- mysql://user:pass@localhost/app
//! cargo run -p memoire-orm --example any_backend --features full -- redis://127.0.0.1:6379
//! cargo run -p memoire-orm --example any_backend --features full -- "mongodb://localhost:27017/app?directConnection=true"
//! ```

use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "any_backend_tasks")]
pub struct Task {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub title: String,
    pub done: bool,
    pub priority: i32,
}

/// Written once, runs everywhere.
async fn app<D: Database>(db: &D) -> Result<()> {
    db.drop_table_if_exists::<Task>().await?;
    db.create_table::<Task>().await?;

    let tasks: Vec<Task> = [("Write the docs", 2), ("Ship v0.1", 1), ("Celebrate", 3), ("Fix the bug", 1)]
        .into_iter()
        .map(|(title, priority)| Task { id: 0, title: title.into(), done: false, priority })
        .collect();
    Task::insert_many(db, &tasks).await?;

    let tx = db.begin().await?;
    Task::update_many().set(Task::DONE, true).filter(Task::PRIORITY.eq(1)).execute(&tx).await?;
    tx.commit().await?;

    let todo = Task::query().filter(Task::DONE.eq(false)).order_by(Task::PRIORITY.asc()).all(db).await?;
    for t in &todo {
        println!("  [ ] {} (priority {})", t.title, t.priority);
    }
    println!("  {} done, {} to do", Task::query().filter(Task::DONE.eq(true)).count(db).await?, todo.len());

    let duplicate = Task { id: 0, title: "Celebrate".into(), done: false, priority: 9 };
    match duplicate.clone().insert(db).await {
        Err(e) if e.is_unique_violation() => println!("  unique constraint enforced: {e}"),
        other => panic!("expected a unique violation, got {other:?}"),
    }
    db.drop_table::<Task>().await
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args().nth(1).unwrap_or_else(|| "memory".into());
    println!("backend: {url}");
    if url == "memory" {
        app(&MemoryDb::new()).await
    } else if let Some(path) = url.strip_prefix("sqlite:") {
        let db =
            if path.is_empty() || path == ":memory:" { SqliteDb::open_in_memory()? } else { SqliteDb::open(path)? };
        app(&db).await
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        app(&PgPool::connect(&url).await?).await
    } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
        app(&MySqlPool::connect(&url.replacen("mariadb://", "mysql://", 1)).await?).await
    } else if url.starts_with("redis://") || url.starts_with("rediss://") {
        app(&RedisDb::connect_with_prefix(&url, "any_backend_example").await?).await
    } else if url.starts_with("mongodb://") || url.starts_with("mongodb+srv://") {
        app(&MongoDb::connect_with_database(&url, "any_backend_example").await?).await
    } else {
        Err(Error::Unsupported(format!("unknown database URL `{url}`")))
    }
}
