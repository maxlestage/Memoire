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

The website (`site/`) is written with [Active](https://active-c1883a966650.herokuapp.com/): the
server renders every page, the islands (playground, code tabs, docs search) and the page
motion run in the browser as WebAssembly, and the site is a PWA (manifest and offline service
worker). It embeds `docs/{en,fr,es}` at compile time.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # the version in site/Cargo.lock
cd site
# Browser bundle (rebuild it after changing an island or src/client/)
cargo build --lib --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm
cargo run                                          # http://localhost:8080
BASE_PATH=/Memoire cargo run -- export dist-static  # the static site, as on GitHub Pages
```

Before opening a pull request that touches the site (the CI runs the same):

```bash
cd site
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --lib --target wasm32-unknown-unknown -- -D warnings
cargo test        # with pkg/ built: pages, SEO, PWA, static export, docs in every language
```

To add a documentation page, write `docs/en/<slug>.md`, `docs/fr/<slug>.md` and
`docs/es/<slug>.md`, then list it in `site/src/docs.rs` and `site/src/server/docs.rs`.

## License

Memoire ORM is proprietary software by Maxime Nathan Lestage (see [LICENSE](LICENSE)). By
contributing, you assign the rights in your contribution to the author, as section 3 of the
license states; contributions are welcome under those terms.
