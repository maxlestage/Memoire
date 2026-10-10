# syntax=docker/dockerfile:1
#
# Memoire ORM website — production image.
# Used by Heroku (heroku.yml → container stack) and works with any Docker host:
#
#   docker build -t memoire-site .
#   docker run --rm -p 8080:8080 memoire-site      # then open http://localhost:8080
#
# Stage 1 builds the site (site/, written with active): the browser bundle (WebAssembly
# islands, including the playground's in-memory ORM) with wasm-bindgen, then the server,
# which renders every page and embeds the documentation (docs/). Stage 2 only ships the
# server binary and the static files (no Rust toolchain).

# Pinned toolchain = reproducible deploys. Bump deliberately (and test) when needed.
ARG RUST_VERSION=1.99
# Must match the wasm-bindgen version in site/Cargo.lock.
ARG WASM_BINDGEN_VERSION=0.2.129

# ---------------------------------------------------------------------------
# 1. Build
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS builder
ARG WASM_BINDGEN_VERSION
ENV CARGO_TERM_COLOR=never \
    CARGO_NET_RETRY=5

RUN rustup target add wasm32-unknown-unknown

# wasm-bindgen from its prebuilt release binary (much faster than `cargo install`).
# ADD downloads it, so the build needs no extra system packages.
ADD https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/wasm-bindgen-${WASM_BINDGEN_VERSION}-x86_64-unknown-linux-musl.tar.gz /tmp/wasm-bindgen.tar.gz
RUN tar -xzf /tmp/wasm-bindgen.tar.gz --strip-components=1 -C /usr/local/bin \
 && rm /tmp/wasm-bindgen.tar.gz \
 && wasm-bindgen --version

WORKDIR /app

# --- Dependencies first, so this layer is cached between deploys. The site depends on
# the memoire-orm crate (and its workspace manifest): the playground runs the real ORM.
COPY Cargo.toml Cargo.toml
COPY crates crates
COPY site/Cargo.toml site/Cargo.lock site/
RUN mkdir -p site/src \
 && touch site/src/lib.rs \
 && echo "fn main() {}" > site/src/main.rs \
 && cd site \
 && cargo build --release --locked \
 && cargo build --lib --locked --target wasm32-unknown-unknown --profile wasm-release \
 && rm -rf src

# --- The site: browser bundle, then the server (the documentation is embedded at
# compile time).
COPY docs docs
COPY site site
RUN cd site \
 && touch src/lib.rs src/main.rs \
 && cargo build --lib --locked --target wasm32-unknown-unknown --profile wasm-release \
 && wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/wasm-release/memoire_site.wasm \
 && cargo build --release --locked

# ---------------------------------------------------------------------------
# 2. Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin memoire

WORKDIR /app
COPY --from=builder /app/site/target/release/memoire-site /app/memoire-site
COPY --from=builder /app/site/static /app/site/static
COPY --from=builder /app/site/pkg /app/site/pkg

# Heroku injects $PORT at runtime; 8080 is only the local default. Static files are read
# from $SITE_DIR at startup and served from memory.
ENV PORT=8080 \
    SITE_DIR=/app/site \
    RUST_LOG=info

USER 10001
EXPOSE 8080
CMD ["/app/memoire-site"]
