FROM rust:1.87-bookworm AS base
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

FROM base AS builder
RUN cargo build --release --locked

FROM base AS test
RUN cargo test --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/garage-secrets-controller /usr/local/bin/garage-secrets-controller

ENTRYPOINT ["garage-secrets-controller"]
