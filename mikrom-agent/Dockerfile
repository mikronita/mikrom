# 1. Planner stage
FROM lukemathwalker/cargo-chef:latest-rust-1.95-trixie AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# 2. Builder stage
FROM chef AS builder
ARG ZIG_VERSION=0.17.0-dev.704+b8cb78023
RUN apt-get update && apt-get install -y build-essential ca-certificates protobuf-compiler cmake curl xz-utils musl-tools librados-dev librbd-dev && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
RUN curl -fsSL https://ziglang.org/builds/zig-x86_64-linux-${ZIG_VERSION}.tar.xz | tar -xJ -C /opt && \
    ln -s /opt/zig-x86_64-linux-${ZIG_VERSION}/zig /usr/local/bin/zig
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching layer
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cmake -S /app/tundra-nat64 -B /tmp/tundra-build -DCMAKE_BUILD_TYPE=Release && \
    cmake --build /tmp/tundra-build --parallel && \
    strip /tmp/tundra-build/tundra-nat64
RUN cargo build --release -p mikrom-agent && \
    strip target/release/mikrom-agent
RUN make build-init

# 3. Runtime stage
FROM debian:trixie-slim AS runner
WORKDIR /app
# Agent needs iproute2 for TAP management and ca-certificates for TLS
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    iptables \
    iproute2 \
    e2fsprogs \
    kmod \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mikrom-agent /usr/local/bin/
COPY --from=builder /app/target/release/mikrom-init /usr/bin/mikrom-init
COPY --from=builder /app/target/release/mikrom-init-zig /usr/bin/mikrom-init-zig
COPY --from=builder /tmp/tundra-build/tundra-nat64 /usr/local/bin/tundra-nat64
EXPOSE 5003
CMD ["mikrom-agent"]
