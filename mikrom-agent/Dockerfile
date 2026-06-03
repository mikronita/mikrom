# 1. Planner stage
FROM lukemathwalker/cargo-chef:latest-rust-1.95-trixie AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 2. Builder stage
FROM chef AS builder
RUN apt-get update && apt-get install -y protobuf-compiler cmake musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching layer
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release -p mikrom-agent && \
    strip target/release/mikrom-agent
RUN cargo build --release -p mikrom-init --target x86_64-unknown-linux-musl

# 3. Runtime stage
FROM debian:trixie-slim AS runner
WORKDIR /app
# Agent needs iproute2 for TAP management and ca-certificates for TLS
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    iproute2 \
    e2fsprogs \
    kmod \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mikrom-agent /usr/local/bin/
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/mikrom-init /usr/bin/mikrom-init
EXPOSE 5003
CMD ["mikrom-agent"]
