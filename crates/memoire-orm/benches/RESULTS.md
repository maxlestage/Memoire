# Benchmark results

Measured with `cargo bench -p memoire-orm --features memory,sqlite` (criterion,
2 s warm-up, 5 s measurement) on an Intel Xeon @ 2.80 GHz, Rust 1.97, Linux.
SQLite runs in-memory to remove disk noise. Numbers are medians; lower is better.

## In-memory engine

The "before" column is the previous engine, measured with the same benchmark
file on the same machine, back to back.

| Benchmark (10 000-row table) | Before | Now | Speed-up |
|---|---:|---:|---:|
| Equality on an `#[memoire(index)]` column | 140.9 µs | **1.1 µs** | **×123** |
| `COUNT(*) WHERE age = ?` | 106.2 µs | **11.1 µs** | **×9.6** |
| Filter (`BETWEEN` + `>`), `ORDER BY`, `LIMIT 20` | 231.2 µs | **36.3 µs** | **×6.4** |
| `update_many` with a filter | 177.0 µs | **35.8 µs** | **×4.9** |
| `insert_many`, 1 000 rows | 422.9 µs | **246.0 µs** | **×1.7** |
| Insert one row | 776 ns | **572 ns** | **×1.4** |
| Find by primary key | 226 ns | 224 ns | ×1.0 |
| Render `SELECT … WHERE … ORDER BY … LIMIT` to SQL (every backend) | 526 ns | **446 ns** | **×1.2** |

Where the time went, and what changed:

- **Vectorised scans.** A `WHERE` clause is split into its `AND` terms; each
  one filters the rows with a loop specialised for its column and constant
  (integers, floats, text, ranges, `IS NULL`) instead of walking a generic
  expression tree for every row. Anything unusual (mixed types, `NaN`, `OR`,
  functions...) still goes through the general evaluator, with identical
  results and errors: unit tests compare both on tricky data.
- **Typed column copies.** Every numeric column is also kept as a contiguous
  `Vec<i64>` / `Vec<f64>` plus one state byte per row, updated with the rows.
  A numeric filter reads 9 bytes per row, without a branch, instead of
  following a pointer to each row: about 1 ns per row.
- **`COUNT(*)` without rows.** It counts the matches; nothing is built.
- **Sorting on packed keys.** For `ORDER BY` one column, the key, NULL
  placement, direction and row position are packed once into a `u128` whose
  integer order is the SQL order, then partially selected (`LIMIT`) and sorted
  with plain integer comparisons.
- **Indexes that copy nothing.** Primary-key, `unique` and (new) `index`
  columns have hash indexes that store row positions only; a lookup hashes the
  value and compares it in the row. `#[memoire(index)]` now turns an equality
  filter (a foreign key, typically) into a lookup instead of a scan.
- **Less work per write.** Each indexed value is hashed once per insert;
  `insert_many` stores the rows it is handed without copying them; an update
  only touches the indexes whose column changes.
- **No intermediate rows.** Results are decoded into your structs straight
  from the stored rows (one clone per field), without building a
  `Vec<Value>` per row first.

The Redis and MongoDB backends evaluate filters, joins and aggregates with this
same engine, so they benefit too.

## SQLite: Memoire vs hand-written `rusqlite`

| Benchmark | Memoire | Raw `rusqlite` (no ORM) | Ratio |
|---|---:|---:|---:|
| Find by primary key | 961 ns | 746 ns | 1.29× |
| Select + decode 1 000 rows | 315 µs | 308 µs | 1.02× |
| Insert one row (returning id) | 6.26 µs | 5.92 µs | 1.06× |
| Insert 1 000 rows in a transaction | **1.95 ms** (`insert_many`) | 6.64 ms (prepared loop) | **0.29×** (3.4× faster) |

The "raw rusqlite" column is hand-written code using the same driver and cached
prepared statements: it is the floor any SQLite ORM can reach. Most of the time
is spent in SQLite itself; on bulk reads and writes Memoire is within a few
percent of it, and `insert_many` beats the hand-written loop by batching rows
into multi-row `INSERT`s. (A single primary-key lookup takes under a
microsecond either way; the difference is the ORM's query and row handling.)

## Relations: N+1 queries vs `load_children`

100 users with 10 posts each; "N+1" loads the users then runs
`Post::belonging_to(user)` for each one, `load_children` loads every post in one
`WHERE user_id IN (...)` query and groups them.

| Backend | N+1 (101 queries) | `load_children` (2 queries) | Speed-up |
|---|---:|---:|---:|
| PostgreSQL 16, local socket over TCP | 13.5 ms | **1.14 ms** | **×12** |
| SQLite, in-memory (`relations/sqlite_*`) | 354 µs | 347 µs | ×1.0 |
| In-memory engine (`relations/memory_*`) | 145 µs | 165 µs | ×0.9 |

The gain is the round trips: with a database across a network, every saved
query saves a full network latency, so the speed-up grows well beyond ×12. An
embedded database (SQLite, the in-memory engine) has no round trip; there both
versions cost about the same, dominated by decoding the 1 000 posts.
`load_children` also sorts the children by primary key, which the N+1 version
does not do.

`Select::paginate` runs its two queries (count and page) concurrently. On the
same PostgreSQL server, 20 rows per page: 342 µs sequential, **176 µs**
concurrent (×1.9).

The PostgreSQL numbers come from a timing loop (200 and 2 000 iterations after
warm-up) against PostgreSQL 16 on the same machine, not from criterion.

## Indexes with many rows per value

A non-unique index used to file every row of a value under the same hash, so
each insert into a column with few distinct values (a `bool`, a status) probed
past all the rows already holding that value. Each value now owns a set of
rows (a single row needs no allocation). Inserting into a table with an
`#[memoire(index)]` on a `bool` column, timing loop:

| Rows already in the table | Before | Now |
|---|---:|---:|
| 0 to 10 000 | 517 ns per insert | **289 to 306 ns** |
| 10 000 to 50 000 | 1.39 µs per insert, growing | **329 to 531 ns** |

The criterion suite above moved with it: equality on an indexed column
1.1 µs → 0.77 µs, `insert_many` of 1 000 rows 246 µs → 227 µs, insert one row
572 ns → 509 ns, find by primary key unchanged (224 → 232 ns, within noise).

## Full-text search and file storage

20 000 rows with a `#[memoire(fulltext)]` column, timing loop on the same
machine:

| Operation | `MemoryDb::new()` | `MemoryDb::open(path)` | with a password |
|---|---:|---:|---:|
| Insert one row (word index, unique index) | 1.5 to 2.2 µs | 2.3 to 3.3 µs | 2.7 to 3.0 µs |
| `matches("likes 42")` count | 41 to 62 µs | 37 to 52 µs | 37 to 49 µs |

Reopening the file (replaying 20 000 rows and rebuilding the indexes) takes
36 to 39 ms. With `StorageOptions::sync(true)` an insert waits for the disk:
130 to 190 µs on this machine's disk.

