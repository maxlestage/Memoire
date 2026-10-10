# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-10

### Added
- `#[derive(Model)]` with compile-time SQL for PostgreSQL, SQLite and MySQL
  (`find`, `insert`, `update`, `delete`, `upsert`), typed column constants and
  attributes `table`, `primary_key`, `auto_increment`, `unique`, `index`, `column`,
  `sql_type`, `default`, `references`, `on_delete`, `on_update`, `skip`.
- `#[derive(FromRow)]` for custom projections and raw SQL.
- Type-safe query builder: filters combined with `&`, `|`, `!`, `IN`, `BETWEEN`,
  `LIKE`/`ILIKE` helpers with escaping, ordering with `NULLS FIRST/LAST`,
  pagination, `INNER`/`LEFT JOIN` decoded into tuples, `GROUP BY`/`HAVING`,
  aggregates, typed column projections.
- `update_many` (with `increment`/`decrement`/`set_null`) and `delete_many`.
- `insert_many` with automatic batching under each database's parameter limit.
- Backends: `MemoryDb` (pure Rust, hash indexes, hash joins, snapshot
  transactions, `wasm32` support), `SqliteDb` and `PgPool`.
- Transactions (`Database::begin`, `Transaction::commit`/`rollback`, rollback on drop).
- Versioned migrations (`Migrator`, `Migration`).
- Normalised errors (`NotFound`, `UniqueViolation`, `Constraint`, `NoSuchTable`,
  `TableExists`, `Conflict`, ...) across every backend.
- `chrono`, `uuid` and `json` (`Json<T>`) features.
- Criterion benchmarks against hand-written `rusqlite`.
