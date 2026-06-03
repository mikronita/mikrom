# Build context: workspace root
# docker build -f mikrom-cli/Dockerfile .

FROM rust:trixie AS builder

WORKDIR /app

COPY . .

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler cmake \
    && rm -rf /var/lib/apt/lists/* \
    && cargo build --release -p mikrom-cli


FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mikrom /usr/local/bin/mikrom

ENTRYPOINT ["mikrom"]
