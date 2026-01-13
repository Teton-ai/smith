# Development Dockerfile for API with hot reloading
# Build uses .sqlx metadata (no DB required)
# Runtime uses live postgres DB for type checking (via docker-compose)

ARG RUST_VERSION=1.91.0
FROM rust:${RUST_VERSION}

WORKDIR /app

# Install cargo-watch for hot reloading
RUN cargo install cargo-watch
RUN rustup component add rustfmt clippy

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace configuration files first for better caching
COPY Cargo.toml Cargo.lock ./

# Copy all workspace members
COPY api ./api
COPY smithd ./smithd
COPY models ./models
COPY updater ./updater
COPY cli ./cli

# Pre-build dependencies with offline mode (no DB available during build)
# At runtime, cargo-watch will use the live database for type checking
RUN SQLX_OFFLINE=true cargo build --package api

# Set environment variables
ENV ROLES_PATH=./api/roles.toml

# Expose API port (adjust if needed)
EXPOSE 8080

# Use cargo-watch to rebuild and restart on file changes
# The -x run will execute cargo run, -w watches for changes, -c clears screen
CMD ["cargo", "watch", "-x", "run --package api", "-w", "api", "-w", "models", "-w", "smithd"]
