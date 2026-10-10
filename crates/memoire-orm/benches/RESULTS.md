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
