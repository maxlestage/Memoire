//! Criterion benchmarks.
//!
//! ```bash
//! cargo bench -p memoire-orm --features memory,sqlite
//! ```
//!
//! The SQLite benchmarks compare Memoire with hand-written `rusqlite` code (the
//! theoretical floor: same driver, same prepared statements, no ORM) so the
//! numbers show the real overhead of the ORM layer.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use memoire_orm::block_on;
use memoire_orm::prelude::*;

#[derive(Model, Debug, Clone)]
#[memoire(table = "users")]
pub struct User {
    #[memoire(primary_key, auto_increment)]
    pub id: i64,
    #[memoire(unique)]
    pub email: String,
    #[memoire(index)]
    pub name: String,
    pub age: Option<i32>,
    pub score: f64,
}

fn sample(i: i64) -> User {
    User {
        id: 0,
        email: format!("user{i}@bench.dev"),
        name: format!("User {i:06}"),
        age: Some((i % 90) as i32),
        score: i as f64,
    }
}

fn seed<E: Executor>(db: &E, n: i64) {
    let users: Vec<User> = (0..n).map(sample).collect();
    block_on(User::insert_many(db, &users)).unwrap();
}

fn sql_rendering(c: &mut Criterion) {
    let mut g = c.benchmark_group("sql_render");
    g.bench_function("select_filter_order_limit", |b| {
        b.iter(|| {
            let q = User::query()
                .filter(User::AGE.ge(black_box(18)) & User::NAME.contains(black_box("li")))
                .order_by(User::NAME.asc())
                .limit(20);
            black_box(q.to_sql(Dialect::Postgres))
        })
    });
    g.finish();
}

fn memory(c: &mut Criterion) {
    let mut g = c.benchmark_group("memory");

    let db = MemoryDb::new();
    block_on(db.create_table::<User>()).unwrap();
    seed(&db, 10_000);

    g.bench_function("find_by_pk_10k", |b| {
        let mut i = 0;
        b.iter(|| {
            i = i % 10_000 + 1;
            black_box(block_on(User::find(&db, i)).unwrap())
        })
    });

    g.bench_function("filter_order_limit_10k", |b| {
        b.iter(|| {
            let rows = block_on(
                User::query()
                    .filter(User::AGE.between(black_box(20), 40) & User::SCORE.gt(100.0))
                    .order_by(User::SCORE.desc())
                    .limit(20)
                    .all(&db),
            )
            .unwrap();
            black_box(rows)
        })
    });

    g.bench_function("count_where_10k", |b| {
        b.iter(|| black_box(block_on(User::query().filter(User::AGE.eq(black_box(42))).count(&db)).unwrap()))
    });

    // Average cost of one insert while a table grows from 0 to `iters` rows.
    g.bench_function("insert_one", |b| {
        b.iter_custom(|iters| {
            let db = MemoryDb::new();
            block_on(db.create_table::<User>()).unwrap();
            let users: Vec<User> = (0..iters as i64).map(sample).collect();
            let start = std::time::Instant::now();
            for mut u in users {
                block_on(u.insert(&db)).unwrap();
                black_box(u.id);
            }
            start.elapsed()
        })
    });

    g.bench_function("insert_many_1000", |b| {
        let rows: Vec<User> = (0..1000).map(sample).collect();
        b.iter_batched(
            || {
                let db = MemoryDb::new();
                block_on(db.create_table::<User>()).unwrap();
                db
            },
            |db| black_box(block_on(User::insert_many(&db, &rows)).unwrap()),
            BatchSize::SmallInput,
        )
    });
    g.finish();
}

const RAW_INSERT: &str =
    "INSERT INTO \"users\" (\"email\", \"name\", \"age\", \"score\") VALUES (?, ?, ?, ?) RETURNING \"id\"";
const RAW_FIND: &str = "SELECT \"id\", \"email\", \"name\", \"age\", \"score\" FROM \"users\" WHERE \"id\" = ?";
const RAW_SELECT_1000: &str =
    "SELECT \"id\", \"email\", \"name\", \"age\", \"score\" FROM \"users\" ORDER BY \"id\" LIMIT 1000";

fn raw_user(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User { id: r.get(0)?, email: r.get(1)?, name: r.get(2)?, age: r.get(3)?, score: r.get(4)? })
}

fn raw_connection() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    for stmt in memoire_orm::sql::create_table(<User as Model>::SCHEMA, Dialect::Sqlite, false) {
        conn.execute_batch(&stmt).unwrap();
    }
    conn
}

fn raw_seed(conn: &rusqlite::Connection, n: i64) {
    let tx = conn.unchecked_transaction().unwrap();
    {
        let mut stmt = tx.prepare_cached(RAW_INSERT).unwrap();
        for i in 0..n {
            let u = sample(i);
            stmt.query_row(rusqlite::params![u.email, u.name, u.age, u.score], |r| r.get::<_, i64>(0)).unwrap();
        }
    }
    tx.commit().unwrap();
}

fn sqlite(c: &mut Criterion) {
    let mut g = c.benchmark_group("sqlite");

    // --- find by primary key -------------------------------------------------
    let db = SqliteDb::open_in_memory().unwrap();
    block_on(db.create_table::<User>()).unwrap();
    seed(&db, 10_000);
    let conn = raw_connection();
    raw_seed(&conn, 10_000);

    g.bench_function("find_by_pk/memoire", |b| {
        let mut i = 0;
        b.iter(|| {
            i = i % 10_000 + 1;
            black_box(block_on(User::find(&db, i)).unwrap())
        })
    });
    g.bench_function("find_by_pk/raw_rusqlite", |b| {
        let mut i = 0;
        b.iter(|| {
            i = i % 10_000 + 1;
            let mut stmt = conn.prepare_cached(RAW_FIND).unwrap();
            black_box(stmt.query_row([i], raw_user).unwrap())
        })
    });

    // --- select & decode 1000 rows --------------------------------------------
    g.bench_function("select_1000_rows/memoire", |b| {
        b.iter(|| black_box(block_on(User::query().order_by(User::ID.asc()).limit(1000).all(&db)).unwrap()))
    });
    g.bench_function("select_1000_rows/raw_rusqlite", |b| {
        b.iter(|| {
            let mut stmt = conn.prepare_cached(RAW_SELECT_1000).unwrap();
            let rows: Vec<User> = stmt.query_map([], raw_user).unwrap().collect::<Result<_, _>>().unwrap();
            black_box(rows)
        })
    });

    // --- insert one row -----------------------------------------------------
    let counter = std::cell::Cell::new(1_000_000_i64);
    g.bench_function("insert_one/memoire", |b| {
        b.iter(|| {
            counter.set(counter.get() + 1);
            let mut u = sample(counter.get());
            block_on(u.insert(&db)).unwrap();
            black_box(u.id)
        })
    });
    let counter = std::cell::Cell::new(1_000_000_i64);
    g.bench_function("insert_one/raw_rusqlite", |b| {
        b.iter(|| {
            counter.set(counter.get() + 1);
            let u = sample(counter.get());
            let mut stmt = conn.prepare_cached(RAW_INSERT).unwrap();
            black_box(
                stmt.query_row(rusqlite::params![u.email, u.name, u.age, u.score], |r| r.get::<_, i64>(0)).unwrap(),
            )
        })
    });

    // --- bulk insert 1000 rows in a transaction -------------------------------
    let rows: Vec<User> = (0..1000).map(sample).collect();
    g.bench_function("insert_1000_tx/memoire_insert_many", |b| {
        b.iter_batched(
            || {
                let db = SqliteDb::open_in_memory().unwrap();
                block_on(db.create_table::<User>()).unwrap();
                db
            },
            |db| {
                block_on(async {
                    let tx = db.begin().await.unwrap();
                    let n = User::insert_many(&tx, &rows).await.unwrap();
                    tx.commit().await.unwrap();
                    black_box(n)
                })
            },
            BatchSize::SmallInput,
        )
    });
    g.bench_function("insert_1000_tx/raw_rusqlite_loop", |b| {
        b.iter_batched(
            raw_connection,
            |conn| {
                let tx = conn.unchecked_transaction().unwrap();
                {
                    let mut stmt = tx.prepare_cached(RAW_INSERT).unwrap();
                    for u in &rows {
                        stmt.query_row(rusqlite::params![u.email, u.name, u.age, u.score], |r| r.get::<_, i64>(0))
                            .unwrap();
                    }
                }
                tx.commit().unwrap();
                black_box(conn)
            },
            BatchSize::SmallInput,
        )
    });
    g.finish();
}

criterion_group!(benches, sql_rendering, memory, sqlite);
criterion_main!(benches);
