# syntax=docker/dockerfile:1.7
#
# Builds the teleporta-server binary. Build context is the repo root (the whole
# workspace is needed because Cargo resolves all members at parse time, and the
# `migrations/` directory is embedded at compile time via sqlx::migrate!).
#
#   docker build -t teleporta:latest -f Dockerfile .

FROM rust:1-slim-bookworm AS builder
WORKDIR /build

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Workspace + every member manifest must be present for cargo to resolve the
# workspace, even when only one bin is built.
COPY Cargo.toml Cargo.lock* ./
COPY crates/teleporta-core/Cargo.toml   crates/teleporta-core/Cargo.toml
COPY crates/teleporta-server/Cargo.toml crates/teleporta-server/Cargo.toml
COPY crates/teleporta-core/src   crates/teleporta-core/src
COPY crates/teleporta-server/src crates/teleporta-server/src
COPY migrations migrations

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin teleporta-server \
 && cp target/release/teleporta-server /usr/local/bin/teleporta-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 -m -s /usr/sbin/nologin teleporta

COPY --from=builder /usr/local/bin/teleporta-server /usr/local/bin/teleporta-server

USER teleporta
EXPOSE 8080
ENV TELEPORTA_SERVER_HOST=0.0.0.0 \
    TELEPORTA_SERVER_PORT=8080
ENTRYPOINT ["/usr/local/bin/teleporta-server"]
