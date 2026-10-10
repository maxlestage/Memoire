# syntax=docker/dockerfile:1
#
# Memoire ORM website — production image.
# Used by Heroku (heroku.yml → container stack) and works with any Docker host:
#
#   docker build -t memoire-site .
#   docker run --rm -p 8080:8080 memoire-site      # then open http://localhost:8080
#
# Stage 1 builds the Yew/WebAssembly site with Trunk and the tiny axum server;
# stage 2 only ships the server binary + the static files (no Rust toolchain).

ARG RUST_VERSION=1
ARG TRUNK_VERSION=0.21.14

# ---------------------------------------------------------------------------
# 1. Build
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-bookworm AS builder
ARG TRUNK_VERSION
ENV CARGO_TERM_COLOR=never \
    CARGO_NET_RETRY=5

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl brotli \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown

# Trunk from its prebuilt release binary (much faster than `cargo install trunk`).
RUN curl -fsSL --retry 3 \
      "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin trunk \
 && trunk --version

WORKDIR /app

# --- Server: build dependencies first so this layer is cached between deploys.
COPY server/Cargo.toml server/Cargo.loc[k] server/
RUN mkdir -p server/src \
 && echo "fn main() {}" > server/src/main.rs \
 && cargo build --release --manifest-path server/Cargo.toml \
 && rm -rf server/src
COPY server/src server/src
RUN touch server/src/main.rs \
 && cargo build --release --manifest-path server/Cargo.toml

# --- Site: same trick for the wasm dependencies (yew, pulldown-cmark, …).
COPY site/Cargo.toml site/Cargo.loc[k] site/
RUN mkdir -p site/src \
 && echo "fn main() {}" > site/src/main.rs \
 && cargo build --release --target wasm32-unknown-unknown --manifest-path site/Cargo.toml \
 && rm -rf site/src
# The documentation markdown is embedded into the wasm at compile time.
COPY docs docs
COPY site site
RUN cd site \
 && touch src/main.rs \
 && trunk build --release

# Pre-compress static files; the server serves .br / .gz siblings when the
# browser accepts them.
RUN find site/dist -type f \
      \( -name '*.wasm' -o -name '*.js' -o -name '*.css' -o -name '*.html' -o -name '*.svg' -o -name '*.txt' \) \
      -exec gzip -9 -k -f {} \; \
      -exec brotli -q 11 -k -f {} \;

# ---------------------------------------------------------------------------
# 2. Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin memoire

WORKDIR /app
COPY --from=builder /app/server/target/release/memoire-site-server /app/memoire-site-server
COPY --from=builder /app/site/dist /app/dist

# Heroku injects $PORT at runtime; 8080 is only the local default.
ENV PORT=8080 \
    STATIC_DIR=/app/dist \
    RUST_LOG=info

USER 10001
EXPOSE 8080
CMD ["/app/memoire-site-server"]
