# 1. Planner stage
FROM lukemathwalker/cargo-chef:latest-rust-1.97-trixie AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 2. Builder stage
FROM chef AS builder
RUN apt-get update && apt-get install -y protobuf-compiler cmake && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching layer
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release -p mikrom-builder && \
    strip target/release/mikrom-builder

# 3. Runtime stage
FROM debian:trixie-slim AS runner
WORKDIR /app
# mikrom-builder needs docker to build images
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates docker.io git && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mikrom-builder /usr/local/bin/
# No ports needed for NATS-based builder
CMD ["mikrom-builder"]
