ARG RUST_VERSION=1.91.0
FROM lukemathwalker/cargo-chef:0.1.71-rust-$RUST_VERSION AS chef

ENV SQLX_OFFLINE=true
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin api

FROM chef AS builder

RUN apt update && apt install lld clang libssl-dev build-essential cmake -y

COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json --bin api
# Build App
COPY . .
# Build our project
RUN cargo build --release --package api

FROM debian:12-slim AS runtime

RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl ca-certificates \
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin

ARG CONFIG_PATH=/app/api/roles.toml
ARG RELEASE_PATH=/app/target/release

COPY --from=builder $RELEASE_PATH/api .
COPY --from=builder $CONFIG_PATH ./roles.toml

# Set environment variable for your app
ENV ROLES_PATH=/usr/local/bin/roles.toml

ENTRYPOINT ["./api"]
