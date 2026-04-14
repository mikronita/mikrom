# Build context: workspace root
# docker build -f mikrom-scheduler/Dockerfile .

FROM rust:trixie AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY mikrom-proto/ mikrom-proto/
COPY mikrom-agent/ mikrom-agent/
COPY mikrom-api/ mikrom-api/
COPY mikrom-scheduler/ mikrom-scheduler/

RUN cargo build --release -p mikrom-scheduler


FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mikrom-scheduler /usr/local/bin/mikrom-scheduler

EXPOSE 5002

CMD ["mikrom-scheduler"]
