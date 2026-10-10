# Contributing to Memoire ORM

Thanks for your interest! Contributions of every size are welcome: bug reports,
documentation fixes, translations, new features.

## Getting started

```bash
git clone https://github.com/maxlestage/Memoire
cd Memoire
cargo test --workspace --features memory,sqlite
```

To also run the PostgreSQL tests, start a server and export its URL:

```bash
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=memoire_test postgres:16
MEMOIRE_PG_URL=postgres://postgres:postgres@localhost/memoire_test cargo test --workspace --all-features
```

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

- Every backend must behave the same: add your test to `crates/memoire-orm/tests/common/mod.rs`
  so it runs on the memory engine, SQLite and PostgreSQL.
- Public items need doc comments (`#![warn(missing_docs)]`).
- The crate is `#![forbid(unsafe_code)]`.
- Documentation lives in `docs/{en,fr,es}`; please update the three languages (or open an issue
  so someone can translate).

## Automatic merge

Pull requests targeting `master` are **merged automatically** (squash) as soon as every CI
check is green, then the website is redeployed. Open your pull request as a *draft* or add the
`no-auto-merge` label if you want a review first.

## Website

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd site && trunk serve
```

## License

By contributing, you agree that your contributions will be dual licensed under MIT and Apache-2.0.
