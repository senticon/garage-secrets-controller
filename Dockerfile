FROM rust:1.87-bookworm AS base
WORKDIR /app
ARG GSC_GIT_HASH
ARG GSC_VERSION
ARG GSC_PR_NUMBER
ENV GSC_GIT_HASH=${GSC_GIT_HASH}
ENV GSC_VERSION=${GSC_VERSION}
ENV GSC_PR_NUMBER=${GSC_PR_NUMBER}

COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY src ./src

FROM base AS builder
RUN cargo build --release --locked

FROM base AS test
RUN rustup override set stable && rustup component add llvm-tools-preview
RUN cargo install cargo-llvm-cov
RUN cargo llvm-cov test --locked --json --ignore-filename-regex '.*target/.*' > coverage-report.json

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/garage-secrets-controller /usr/local/bin/garage-secrets-controller

ENTRYPOINT ["garage-secrets-controller"]
