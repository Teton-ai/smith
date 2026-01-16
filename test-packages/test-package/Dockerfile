FROM rust:1-bookworm

RUN rustup target add x86_64-unknown-linux-gnu \
    && apt-get update \
    && apt-get install -y gcc-x86-64-linux-gnu \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-deb

WORKDIR /build
COPY . .

RUN cargo deb --target x86_64-unknown-linux-gnu

CMD ["cp", "-r", "target/x86_64-unknown-linux-gnu/debian/.", "/out"]
