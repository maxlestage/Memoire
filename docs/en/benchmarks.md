# Benchmarks

This page explains how Memoire is benchmarked, how to run the benchmarks on your own machine, and how to read the results. The goal is not only to show that Memoire is fast, but to measure honestly how much the ORM layer costs compared with hand-written code.

## Methodology

The benchmarks live in `crates/memoire-orm/benches/orm.rs` and use [Criterion](https://crates.io/crates/criterion), the standard statistics-driven benchmarking library for Rust. For each operation, Criterion warms up, runs the code many times, and reports the typical time per run with a confidence interval.

They are compiled with the `bench` profile of the workspace (`lto = "thin"`, `codegen-units = 1`), close to how an optimised application is built.

### What is measured

| Group | Benchmark | What it does |
|---|---|---|
| `sql_render` | `select_filter_order_limit` | Builds a `SELECT` with a filter, a sort and a limit, and renders it to PostgreSQL SQL (no database involved) |
| `memory` | `find_by_pk_10k` | `User::find` by primary key in a table of 10,000 rows |
| `memory` | `filter_order_limit_10k` | Filter (`between` and `gt`), sort and `limit(20)` over 10,000 rows |
| `memory` | `count_where_10k` | `count` with a filter over 10,000 rows |
| `memory` | `filter_indexed_eq_10k` | Equality filter on an `#[memoire(index)]` column (an index lookup) over 10,000 rows |
| `memory` | `update_where_10k` | `update_many` with a filter and an `increment` over 10,000 rows |
| `memory` | `insert_one` | Insert one row |
| `memory` | `insert_many_1000` | `insert_many` of 1,000 rows into an empty table |
| `sqlite` | `find_by_pk/...` | Find one row by primary key in 10,000 rows |
| `sqlite` | `select_1000_rows/...` | Select 1,000 rows and decode them into structs |
| `sqlite` | `insert_one/...` | Insert one row and read back its generated id |
| `sqlite` | `insert_1000_tx/...` | Insert 1,000 rows inside one transaction |

### The baseline: hand-written `rusqlite`

Every SQLite benchmark runs twice:

- `.../memoire`: the operation written with Memoire;
- `.../raw_rusqlite`: the same operation written by hand with `rusqlite`, the driver Memoire itself uses, with cached prepared statements and manual row mapping.

The hand-written version is the *floor*: no SQLite ORM can be faster than the driver it is built on. The difference between the two lines is exactly what the ORM costs. For bulk inserts, the baseline is a prepared-statement loop, which is how most people write it by hand, while Memoire uses `insert_many`.

SQLite runs in memory during the benchmarks, so disk speed does not blur the comparison.

## Running the benchmarks

From the root of the repository:

```bash
cargo bench -p memoire-orm --features memory,sqlite
```

A full run takes a few minutes. To run only some benchmarks, pass a filter after `--`; it matches benchmark names:

```bash
# Only the SQLite group
cargo bench -p memoire-orm --features memory,sqlite -- sqlite

# Only primary-key lookups, on every backend
cargo bench -p memoire-orm --features memory,sqlite -- find_by_pk
```

Results are printed in the terminal, and Criterion keeps the raw measurements in `target/criterion/`. When you run the benchmarks again, it compares with the previous run and tells you whether performance changed.

> **Tip** Close other heavy programs while benchmarking, and plug in your laptop: power saving and background work make results noisy.

## Results

<!-- BENCH_TABLE -->

## How to read the numbers

- **Times are per operation**: lower is better. `ns` is a nanosecond (a billionth of a second), `µs` a microsecond (1,000 ns) and `ms` a millisecond (1,000 µs).
- **Criterion prints three values**, for example `time: [961 ns 987 ns 1.01 µs]`. The middle one is the best estimate; the outer two bound the 95% confidence interval. When intervals of two benchmarks overlap, treat them as equal.
- **The ratio** compares Memoire with the hand-written baseline. `1.10×` means Memoire takes 10% longer than code written directly against the driver; below `1×`, Memoire is faster.
- **Compare operations, not backends.** The in-memory engine and SQLite do different amounts of work (SQLite parses SQL, manages pages and a journal). The numbers show the cost of each operation on each backend, not which database is "better".

## An honest note

Benchmark numbers depend heavily on the machine (processor, memory, operating system), on the Rust version and on the data. Absolute times on your computer will differ, possibly a lot. What tends to stay stable across machines is the *ratio* between Memoire and the hand-written baseline.

Micro-benchmarks also measure small, isolated operations. In a real application, network latency to a database server, disk speed and the queries themselves usually matter far more than the ORM layer. Measure your own workload before drawing conclusions, and see [Why it's fast](performance) for tips that make a real difference.

## Next steps

- [Why it's fast](performance): the design choices behind the numbers.
- [SQLite](sqlite): the backend used for the comparisons.
- [In-memory engine](memory): the algorithms used by `MemoryDb`.
