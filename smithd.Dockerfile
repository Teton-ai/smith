FROM nvcr.io/nvidia/l4t-base:r36.2.0

RUN apt-get clean && apt-get update --allow-unauthenticated --allow-insecure-repositories
RUN apt-get install openssh-server vim curl build-essential libdbus-1-dev pkg-config libssl-dev systemd systemd-sysv -y

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Add cargo binaries to path
ENV PATH="/usr/local/cargo/bin:${PATH}"

# Create workspace directory
WORKDIR /workspace

# Pre-install common Rust development tools
RUN rustup component add rustfmt clippy

RUN cargo install cargo-watch
RUN cargo install sqlx-cli

# Ensure target directory exists and has right permissions
RUN mkdir -p /workspace/target

RUN mkdir -p /var/run/dbus

# Copy dbus configuration file
COPY smithd/src/dbus/smithd.conf /etc/dbus-1/system.d/smithd.conf
RUN chmod 0644 /etc/dbus-1/system.d/smithd.conf

RUN systemctl enable dbus

# Create systemd service file for smithd
RUN echo '[Unit]\n\
Description=Smith Daemon\n\
After=network.target dbus.service\n\
Requires=dbus.service\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart=/workspace/target/debug/smithd\n\
Restart=always\n\
StandardOutput=journal\n\
StandardError=journal\n\
\n\
[Install]\n\
WantedBy=multi-user.target' > /etc/systemd/system/smithd.service

# Enable the service (optional - you can start it manually too)
# RUN systemctl enable smithd

# Configure SSH server
RUN mkdir -p /var/run/sshd
RUN echo 'PermitRootLogin yes' >> /etc/ssh/sshd_config
RUN echo 'PasswordAuthentication no' >> /etc/ssh/sshd_config
RUN mkdir -p /root/.ssh

# Expose SSH port
EXPOSE 22

# Use systemd as init system
CMD ["/lib/systemd/systemd"]