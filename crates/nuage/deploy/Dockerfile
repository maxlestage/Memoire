# Nuage DB server image, for any container host. Build from the repository root:
#   docker build -f crates/nuage/deploy/Dockerfile -t nuage .
#   docker run -p 8080:8080 -v nuage-data:/data -e NUAGE_TOKEN=... -e NUAGE_ADMIN_PASSWORD=... nuage
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --locked -p nuage --bin nuage

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/nuage /usr/local/bin/nuage
ENV NUAGE_DATA_DIR=/data PORT=8080
VOLUME /data
EXPOSE 8080
CMD ["nuage"]
