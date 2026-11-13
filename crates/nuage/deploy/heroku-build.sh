#!/usr/bin/env bash
# Builds the `nuage` binary (Nuage DB server) for Heroku: run by RustConfig once
# the emk/rust buildpack has installed Rust. Works by hand too: ./heroku-build.sh
set -euo pipefail
cd "$(dirname "$0")"
# On Heroku, Cargo builds into the buildpack's cache (CARGO_TARGET_DIR).
target="${CARGO_TARGET_DIR:-target}"
echo "-----> Building Nuage DB"
cargo build --release --locked -p nuage --bin nuage
mkdir -p target/release
if [ "$target" != target ]; then
  cp "$target/release/nuage" target/release/
fi
