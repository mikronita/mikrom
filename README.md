# mikrom

[![CI](https://github.com/antpard/mikrom/actions/workflows/ci.yml/badge.svg)](https://github.com/antpard/mikrom/actions/workflows/ci.yml)

Mikrom is a microVM orchestration system that deploys containerized workloads into [Firecracker](https://firecracker-microvm.github.io/) microVMs across a fleet of worker nodes.

## Architecture

```
HTTP Client
  → mikrom-api     (REST, port 5001)
    → mikrom-scheduler  (gRPC, port 5002)
      → mikrom-agent    (gRPC, port 5003)
        → FirecrackerManager  (in-memory stub; real Firecracker integration pending)
```

## Repository layout

| Directory | Description |
|---|---|
| `mikrom-api/` | Axum HTTP REST API — auth, deploy endpoint |
| `mikrom-scheduler/` | Tonic gRPC scheduler — worker selection and job tracking |
| `mikrom-agent/` | Tonic gRPC agent — runs on each worker node, manages VMs |
| `mikrom-proto/` | Shared protobuf definitions compiled to Rust |
| `mikrom-app/` | Next.js 16 frontend (React 19, Tailwind CSS 4) |
| `firecracker/` | Firecracker VMM source (vendored) |

The four Rust crates share a single Cargo workspace (`Cargo.toml`).

## Quick start

### Prerequisites

- Rust (stable toolchain)
- Docker (for PostgreSQL and integration tests)
- Node.js + pnpm (for the frontend)

### Run the backend services

```bash
# 1. Start PostgreSQL
cd mikrom-api && docker-compose up -d postgres

# 2. Run each service in a separate terminal
cd mikrom-api && cargo run          # port 5001
cargo run -p mikrom-scheduler       # port 5002
cargo run -p mikrom-agent           # port 5003
```

### Run the frontend

```bash
cd mikrom-app
pnpm install
pnpm dev   # port 3000
```

## Environment variables

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | — | mikrom-api |
| `JWT_SECRET` | — | mikrom-api |
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | mikrom-api, mikrom-agent |
| `AGENT_PORT` | `5003` | mikrom-agent |
| `HOST_ID` | random UUID | mikrom-agent |
| `USE_TLS` | `false` | mikrom-agent, mikrom-scheduler |

## Testing

```bash
# Unit tests (no Docker required)
cargo test --lib

# Integration tests for mikrom-api (requires Docker)
cd mikrom-api && cargo test --test integration

# Run a single test by name
cargo test <test_name>
```

## Build

```bash
cargo build           # all Rust crates
cd mikrom-app && pnpm build   # Next.js production build
```

See each subdirectory's README for service-specific details.
