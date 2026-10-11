# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- License: Memoire ORM is proprietary software by Maxime Nathan Lestage, all rights
  reserved (see `LICENSE`); it is free to use as a dependency, commercially too, but not to
  modify or redistribute. The MIT/Apache-2.0 dual license is withdrawn.
- Credits: Maxime Nathan Lestage is named as the author in the crate metadata, the README
  and the website's footer, which now links the six projects of the ecosystem.
- `memoire_orm::VERSION`, the version of the crate.
- The website is rewritten with [Active](https://maxlestage.github.io/Active/) (it was a
  client-side WebAssembly app prerendered at build time): a Rust server renders every page
  with complete SEO tags (title, description, canonical, `hreflang`, Open Graph, Twitter,
  JSON-LD, sitemap) in English, French and Spanish (`/en/`, `/fr/`, `/es/`), the
  documentation is rendered on the server with its search, and only the interactive parts
  (playground, code tabs, docs search, typing tagline) are hydrated in the browser as
  WebAssembly islands. The playground still
  runs `memoire-orm`'s in-memory engine in the browser, and on the server for the first
  render. The old addresses (`/features`, `/docs/models`…) lead to the visitor's language.
- The site's server (`site/`, binary `memoire-site`) replaces `server/`; `memoire-site
  export <dir>` writes the static site for GitHub Pages (replacing `site/build-static.sh`).
  Heroku keeps building the root `Dockerfile`.
- Fonts are served by the site itself (no request to Google Fonts).

### Added
- The website is a PWA: a web app manifest per language and icons, and an offline service
  worker. Once opened, the site works without network, playground included.
- `#[memoire(crate = "path")]` struct attribute for `#[derive(Model)]` and
  `#[derive(FromRow)]`: the generated code finds Memoire through `path` instead of
  `::memoire_orm`, so frameworks that re-export it (such as Paquets) work without a
  direct `memoire-orm` dependency.

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
- `postgres-tls` feature: rustls-based TLS honouring `sslmode`
  (`disable`, `prefer`, `require`, `verify-ca`, `verify-full`) and
  `PgPool::connect_with_rustls` for custom certificates.
- `chrono`, `uuid` and `json` (`Json<T>`) features.
- Criterion benchmarks against hand-written `rusqlite`.
