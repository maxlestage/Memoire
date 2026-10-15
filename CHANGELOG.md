# Changelog

All notable changes to this project are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- Links to the websites of Active, Vitesse, Securite, Memoire and Paquets (README, docs,
  crate metadata, `app.json`, the website's footer) point to their Heroku addresses instead
  of GitHub Pages; the license is linked at `/static/LICENSE.txt`, where the site serves it.
- Website: no longer published on GitHub Pages (the `pages.yml` workflow is removed): it is
  served by Heroku only, and GitHub only hosts the code. The READMEs, `CONTRIBUTING.md` and
  the Heroku deployment guide no longer describe a Pages deployment; `memoire-site export`
  still writes the static site, and the CI still checks it.
- The ecosystem is five projects: FerroSwift and Solidify are no longer listed by the
  README, the docs, `PUBLISHING.md` or the website's footer.
- Website: the root address, the one people share, has the share image too, and every
  card describes its image (`og:image:alt`).
- Website: the share image shows today's install command (`cargo add memoire-orm --git …`).
- In-memory engine: a non-unique index keeps a set of rows per value, so inserting into a
  low-cardinality indexed column (a `bool`) no longer slows down as the table grows;
  equality lookups on an indexed column are faster too (1.1 µs → 0.77 µs on 10 000 rows).
- Redis: a primary-key `IN` list repeating a key returned that row twice.
- `Select::paginate` runs the count and the page concurrently: on a connection pool,
  one round trip instead of two (×1.9 on a local PostgreSQL).
- In-memory engine (and Redis, MongoDB, which use it): an `IN (...)` list on an indexed
  column is answered by the index alone instead of being evaluated again on every
  candidate, and `IN` sets are probed without cloning the value (×1.3 on 1 000 rows).
- License: Memoire ORM is proprietary software by Maxime Nathan Lestage, all rights
  reserved (see `LICENSE`); it is free to use as a dependency, commercially too, but not to
  modify or redistribute. The MIT/Apache-2.0 dual license is withdrawn.
- Credits: Maxime Nathan Lestage is named as the author in the crate metadata, the README
  and the website's footer, which now links the seven projects of the ecosystem.
- Credits: Solidify joins the projects linked by the README, the license and the website's
  footer.
- `memoire_orm::VERSION`, the version of the crate.
- The website is rewritten with [Active](https://active-c1883a966650.herokuapp.com/) (it was a
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
- Fonts are served by the site itself (no request to Google Fonts).
- Heroku builds the website with the [emk/rust buildpack](https://elements.heroku.com/buildpacks/emk/heroku-buildpack-rust)
  on the heroku-26 stack: `RustConfig` (Rust pinned like the `Dockerfile`, the buildpack's
  own build skipped for `heroku-build.sh`), `heroku-build.sh` (the server, built in `site/`
  with `--release --locked`, then the WebAssembly islands, into `site/pkg`) and `Procfile`; `app.json` (the Deploy to Heroku button) uses them too. The
  "Deploy to Heroku" workflow uploads the source to Heroku, which builds it, after moving the
  app to heroku-26 and setting the buildpack; the repository variable `HEROKU_BUILD=docker`
  keeps the previous way (the image built on GitHub from the `Dockerfile` and pushed to the
  container registry). Without GitHub Actions: `heroku create --stack heroku-26 --buildpack
  emk/rust`, then `git push heroku master`.

### Added
- Model lifecycle, in the spirit of Laravel's Eloquent and Doctrine's lifecycle callbacks
  (module `memoire_orm::lifecycle`, page "Timestamps, soft deletes & hooks"):
  - Automatic timestamps: `#[memoire(timestamps)]` (fields `created_at`, `updated_at`) or
    `#[memoire(created_at)]` / `#[memoire(updated_at)]` on any field; filled by `insert`,
    `save`, `insert_many`, `upsert`, refreshed by `update`, `update_only`, `update_many`
    and the new `touch`. `i64` (milliseconds), `i32`/`f64` (seconds), `String` (RFC 3339)
    and `chrono` types; any other type is rejected at compile time.
  - Soft deletes: `#[memoire(soft_deletes)]` (field `deleted_at`) or
    `#[memoire(soft_delete)]`; `delete`, `delete_by_key` and `delete_many` stamp the
    column, every query of the model leaves those rows out (`find`, `count`, `paginate`,
    relations), `with_trashed`, `only_trashed`, `without_trashed`, `restore`, `trashed`,
    `force_delete`, `delete_many().force()`.
  - Lifecycle hooks: `#[memoire(hooks)]` and `impl Hooks` (`before_save`,
    `before_insert`/`after_insert`, `before_update`/`after_update`,
    `before_delete`/`after_delete`); a `before_*` hook returning an error cancels the
    write. New error variant `Error::Validation` and `Error::is_validation`.
  - `Model::save` (insert a new row, update a stored one), `Model::touch`.
- `Select::first_or_create` and `Select::update_or_create` (Eloquent's `firstOrCreate` and
  `updateOrCreate`; `first_or_create` returns the row another writer inserted meanwhile).
- `Select::chunks(n)` (`Chunks::next`, `Chunks::for_each`): read a large result in bounded
  batches by primary key, and `Select::cursor_paginate` (`CursorPage`): keyset pagination.
- `Select::when`, `Select::when_some` and `Select::apply` (conditional parts and reusable
  scopes of a query).
- Query log (module `memoire_orm::log`, page "Query log"): `Logged::new(db)` records every
  statement run through a handle and its transactions, with its SQL, parameters, duration,
  row count and error (`queries`, `take_queries`, `clear`, `listen`, `with_limit`), on
  every backend.
- Releases are automatic: once CI is green on `master`, a version of the root `Cargo.toml`
  that has no tag yet is tagged (`v0.1.0` for `0.1.0`), gets its GitHub release with its
  section of `CHANGELOG.md` as notes, and both crates are published on crates.io. Publishing
  a version is bumping it, writing its changelog section and pushing (see `PUBLISHING.md`).
- Browser storage (feature `web`, module `memoire_orm::web`), tested in Chromium:
  - `Store<S>`, application state in the spirit of Pinia: `Store::new("cart")` returns
    the same store anywhere; getters (`get`), actions (`update`), `set`, `patch` (JSON
    merge), `reset`, `subscribe`, `subscribe_local` (for the signals of UI frameworks,
    which are not `Send`), `dispose`; saved to `localStorage` on every change, restored on
    the next visit, versions with migrations, kept in step across tabs through `storage`
    events.
  - `LocalStorageDb`, a complete Memoire database in `localStorage`: one readable JSON
    value per row, tables restored on first use, a version per table so tabs read each
    other's changes and never reuse an id (a transaction overwriting another tab's write
    fails with `Error::Conflict`), all-or-nothing writes when the quota is exceeded,
    `on_external_change`, `clear`, `close`.
  - `KeyValueStorage`, implemented by `LocalStorage`, `SessionStorage` (wasm32) and
    `MemoryStorage` (everywhere: tests, server rendering, with tabs and a quota).
  - `Error::Storage`.
- Indexes over several columns, on every backend: `#[memoire(unique(a, b))]` and
  `#[memoire(index(a, b))]` on the struct (repeatable). SQL `CREATE [UNIQUE] INDEX`,
  MongoDB compound indexes, Redis unique keys, multi-column hash indexes in the in-memory
  engine (used for lookups too). Violations report `column: "a, b"` everywhere; a `NULL`
  in one of the columns lifts the constraint, like SQL. `TableSchema::indexes`, `IndexDef`.
- Full-text search on every backend: `Column::matches("rust orm")` keeps the rows whose
  text holds every word as a whole word, ignoring case, with the same rule everywhere
  (PostgreSQL `tsvector`, a `memoire_match` function registered on SQLite connections,
  `REGEXP` on MySQL, `$regex` on MongoDB, the in-memory engine for itself and Redis).
  `#[memoire(fulltext)]` adds a GIN index on PostgreSQL and a word index in the in-memory
  engine. `text_words`, `TextSearch`.
- Change notifications on every backend: `Observed::new(db)` wraps any handle; its
  `subscribe` and `channel` receive a `Change` (table, kind, rows, key, row) after each
  write made through it, transactions at commit and never on rollback. `Executor` and
  `Database` are implemented for `&D`.
- `MemoryDb::open(path)`: the in-memory engine stored in a file (an append-only journal of
  committed row changes with checksums, replayed on open, a torn tail dropped), with
  `compact()` (also automatic), `StorageOptions::sync` and, with the new `encryption`
  feature, `StorageOptions::password` (AES-256-GCM, Argon2id). Tables come back when their
  model is used, matched by column name.
- Documentation: "Change notifications" page; full-text search, indexes over several
  columns and file storage in the guides; Nitrite in the comparison page.
- Relations and eager loading, read from the foreign keys declared with
  `#[memoire(references = "table(column)")]` (no extra declaration):
  `Post::belonging_to(&user)` (a query on the children of one row),
  `post.find_parent::<User, _>(&db)`, and the `LoadRelated` trait (in the prelude) on
  slices of models: `users.load_children::<Post>(&db)`, `load_children_with` (your own
  query on the children), `posts.load_parent::<User>(&db)` and many-to-many
  `posts.load_via::<PostTag, Tag>(&db)`. One query per level instead of one per row
  (×12 for 100 users and their posts on a local PostgreSQL); long key lists are split
  under the database's parameter limit. `Error::Relation` reports a missing or
  ambiguous foreign key.
- `Model::find_many(&db, keys)`: many rows by primary key in one query.
- `Model::update_only(&db, (User::NAME, User::AGE))`: partial update by primary key;
  the other columns are not written. Columns of another model do not compile
  (`ModelColumns`).
- `Model::value_at(index)`: one column value without converting the whole row
  (generated by `#[derive(Model)]`).
- Documentation: "Memoire vs Diesel vs SeaORM", a feature comparison with migration
  tables, in English, French and Spanish.
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
