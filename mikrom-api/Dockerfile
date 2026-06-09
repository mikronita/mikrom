# 1. Planner stage
FROM lukemathwalker/cargo-chef:latest-rust-1.95-trixie AS chef
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
RUN cargo build --release -p mikrom-api && \
    strip target/release/mikrom-api

# 3. Runtime stage
FROM debian:trixie-slim AS runner
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
ENV ACME_STAGING=false
ENV ROUTER_TLS_HOSTNAME=api.mikrom.spluca.org
ENV FRONTEND_TLS_HOSTNAME=dashboard.mikrom.spluca.org
COPY --from=builder /app/target/release/mikrom-api /usr/local/bin/
EXPOSE 5001
CMD ["mikrom-api"]
