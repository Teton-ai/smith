# Multi-stage build for smithd and smith-updater
# This emulates an Ubuntu IoT device running both services

ARG RUST_VERSION=1.91.0
FROM rust:${RUST_VERSION} AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy the workspace
COPY . .

# Build smithd and smith-updater
RUN cargo build --package smith --bin smithd
RUN cargo build --package smith-updater

# Runtime stage - Ubuntu to emulate real devices
FROM ubuntu:24.04

# Install runtime dependencies and systemd
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    systemd \
    systemd-sysv \
    dbus \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create the /etc/smith working directory
RUN mkdir -p /etc/smith

# Copy binaries from builder
COPY --from=builder /build/target/debug/smithd /usr/bin/smithd
COPY --from=builder /build/target/debug/smith-updater /usr/bin/smith-updater

# Copy systemd service files
COPY smithd/debian/smithd.service /etc/systemd/system/smithd.service
COPY updater/debian/smith-updater.service /etc/systemd/system/smith-updater.service

# Copy D-Bus configuration if it exists
COPY smithd/src/dbus/smithd.conf /etc/dbus-1/system.d/smithd.conf

# Make binaries executable
RUN chmod +x /usr/bin/smithd /usr/bin/smith-updater

# Enable services
RUN systemctl enable smithd.service
RUN systemctl enable smith-updater.service

# Set environment variables
ENV RUST_LOG=INFO

WORKDIR /etc/smith

# Use systemd as the init system
CMD ["/lib/systemd/systemd"]
