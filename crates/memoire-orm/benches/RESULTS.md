# Benchmark results

Measured with `cargo bench -p memoire-orm --features memory,sqlite` (criterion,
2 s warm-up, 5 s measurement) on an Intel Xeon @ 2.80 GHz, Rust 1.97, Linux.
SQLite runs in-memory to remove disk noise. Numbers are medians; lower is better.

| Benchmark | Memoire | Raw `rusqlite` (no ORM) | Ratio |
|---|---:|---:|---:|
| Render `SELECT … WHERE … ORDER BY … LIMIT` to SQL | 529 ns | — | — |
| Memory engine: find by primary key (10k rows) | 231 ns | — | — |
| Memory engine: filter + sort + limit 20 (10k rows) | 241 µs | — | — |
| Memory engine: `COUNT(*) WHERE` (10k rows) | 112 µs | — | — |
| Memory engine: insert one row | 724 ns | — | — |
| Memory engine: `insert_many` 1 000 rows | 432 µs | — | — |
| SQLite: find by primary key | 987 ns | 840 ns | 1.17× |
| SQLite: select + decode 1 000 rows | 343 µs | 324 µs | 1.06× |
| SQLite: insert one row (returning id) | 8.88 µs | 8.21 µs | 1.08× |
| SQLite: insert 1 000 rows in a transaction | **1.80 ms** (`insert_many`) | 7.98 ms (prepared loop) | **0.23×** (4.4× faster) |

The "raw rusqlite" column is hand-written code using the same driver and cached
prepared statements: it is the floor any SQLite ORM can reach. Memoire stays
within 6–17 % of it for single operations, and `insert_many` beats the
hand-written loop by batching rows into multi-row `INSERT`s.
