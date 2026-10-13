#!/usr/bin/env bash
# Builds the server and the islands (WebAssembly) of site/, as the
# Dockerfile does: run on Heroku by RustConfig once the emk/rust buildpack has
# installed Rust. Works by hand too, from the repository: ./heroku-build.sh
# The Procfile starts site/target/release/memoire-site.
set -euo pipefail

# The repositories are private: Cargo fetches the git dependencies (the other
# repositories of the ecosystem) through git, which asks GitHub's credentials to a
# helper reading the REPOS_TOKEN config var, a GitHub token that can read them (the
# buildpack hands config vars over in ENV_DIR). The token stays out of every URL.
if [ -z "${REPOS_TOKEN:-}" ] && [ -n "${ENV_DIR:-}" ] && [ -f "$ENV_DIR/REPOS_TOKEN" ]; then
  REPOS_TOKEN=$(<"$ENV_DIR/REPOS_TOKEN")
fi
if [ -n "${REPOS_TOKEN:-}" ]; then
  export REPOS_TOKEN CARGO_NET_GIT_FETCH_WITH_CLI=true GIT_CONFIG_COUNT=1 \
    GIT_CONFIG_KEY_0=credential.https://github.com.helper \
    GIT_CONFIG_VALUE_0='!f() { test "$1" = get && echo username=x-access-token && echo "password=$REPOS_TOKEN"; }; f'
else
  echo "       no REPOS_TOKEN config var: the private git dependencies cannot be fetched"
fi
cd "$(dirname "$0")/site"
# On Heroku, Cargo builds into the buildpack's cache (CARGO_TARGET_DIR).
target="${CARGO_TARGET_DIR:-target}"

echo "-----> Building the server"
cargo build --release --locked
mkdir -p target/release
if [ "$target" != target ]; then
  cp "$target/release/memoire-site" target/release/
fi
echo "       target/release/memoire-site ready"

echo "-----> Building the islands (WebAssembly)"
rustup target add wasm32-unknown-unknown
cargo build --lib --locked --target wasm32-unknown-unknown --profile wasm-release

# wasm-bindgen must be the version of site/Cargo.lock: a prebuilt binary, or compiled
# if it cannot be downloaded.
version=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
bin="$target/wasm-bindgen-$version"
if [ ! -x "$bin/wasm-bindgen" ]; then
  mkdir -p "$bin"
  curl -sSfL "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/wasm-bindgen-${version}-x86_64-unknown-linux-musl.tar.gz" \
    | tar -xz --strip-components=1 -C "$bin" \
    || cargo install wasm-bindgen-cli --version "$version" --locked --root "$bin" --bin wasm-bindgen
  [ -x "$bin/wasm-bindgen" ] || mv "$bin/bin/wasm-bindgen" "$bin/"
fi
"$bin/wasm-bindgen" --target web --no-typescript --out-dir pkg \
  "$target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm"
echo "       islands ready in site/pkg"
