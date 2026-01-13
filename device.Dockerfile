# This tries to emulate an Ubuntu device as we would see in the real world.
# It runs smithd and smith-updater with systemd, sets up ssh and such

# This needs to be run in privileged mode
# docker run -d --privileged device

FROM nvcr.io/nvidia/l4t-base:r36.2.0 AS builder

ARG RUST_VERSION=1.91.0

WORKDIR /build

# Install build dependencies and Rust
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl-dev \
    pkg-config \
    libdbus-1-dev \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain ${RUST_VERSION}
ENV PATH="/root/.cargo/bin:${PATH}"

# Copy the workspace
COPY . .

# Build smithd and smith-updater
RUN cargo build --package smith --bin smithd
RUN cargo build --package smith-updater

# Runtime stage - Linux for tegra to emulate real devices
FROM nvcr.io/nvidia/l4t-base:r36.2.0

# Install runtime dependencies and systemd
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    systemd \
    systemd-sysv \
    dbus \
    curl \
    openssh-server \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/smith
RUN mkdir -p /etc/ssh
RUN mkdir -p /workspace/target
RUN mkdir -p /var/run/dbus

# Create nightingale user for SSH tunneling
RUN useradd -m -s /bin/bash nightingale
RUN usermod -aG systemd-journal,adm nightingale
RUN mkdir -p /home/nightingale/.ssh
RUN chown -R nightingale:nightingale /home/nightingale/.ssh
RUN chmod 700 /home/nightingale/.ssh

# Copy dbus configuration file
COPY smithd/src/dbus/smithd.conf /etc/dbus-1/system.d/smithd.conf
RUN chmod 0644 /etc/dbus-1/system.d/smithd.conf

# Configure SSH server
RUN mkdir -p /var/run/sshd
RUN echo 'PermitRootLogin yes' >> /etc/ssh/sshd_config
RUN echo 'PasswordAuthentication no' >> /etc/ssh/sshd_config
RUN mkdir -p /root/.ssh

# Copy binaries from builder
COPY --from=builder /build/target/debug/smithd /usr/bin/smithd
COPY --from=builder /build/target/debug/smith-updater /usr/bin/smith-updater

# Copy systemd service files
COPY smithd/debian/smithd.service /etc/systemd/system/smithd.service
COPY updater/debian/smith-updater.service /etc/systemd/system/smith-updater.service

# Make binaries executable
RUN chmod +x /usr/bin/smithd /usr/bin/smith-updater

# Enable services
RUN systemctl enable smithd.service
RUN systemctl enable smith-updater.service

# Set environment variables
ENV RUST_LOG=INFO

WORKDIR /etc/smith

# Ensure the container doesn't exit
EXPOSE 22

# Use systemd as the init system
CMD ["/lib/systemd/systemd"]
