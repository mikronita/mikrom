# Build context: workspace root
# docker build -f mikrom-cli/Dockerfile .

FROM lukemathwalker/cargo-chef:latest-rust-1.95-trixie AS chef

WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler cmake \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release -p mikrom-cli


FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mikrom /usr/local/bin/mikrom

ENTRYPOINT ["mikrom"]
