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
| `mikrom-cli/` | CLI client for the mikrom API (`mikrom` binary) |
| `mikrom-app/` | Next.js 16 frontend (React 19, Tailwind CSS 4) |
| `firecracker/` | Firecracker VMM source (vendored) |

The five Rust crates share a single Cargo workspace (`Cargo.toml`).

## Quick start

### Prerequisites

- Rust (stable toolchain)
- Docker (for PostgreSQL and integration tests)
- Node.js + pnpm (for the frontend)

### Option A — Docker Compose (all services)

```bash
# Copy and edit environment (JWT_SECRET is required)
cp .env.example .env   # if provided, or export JWT_SECRET manually

make up          # build images and start all services
make logs        # follow logs
make down        # stop and remove containers
```

Services start on their default ports (5001 API, 5002 scheduler, 5003 agent, 3000 app).

### Option B — Run locally

```bash
# 1. Start PostgreSQL
make db-start

# 2. Run each service in a separate terminal
make run-api        # port 5001
make run-scheduler  # port 5002
make run-agent      # port 5003
make run-app        # port 3000
```

### mikrom-cli

Install the CLI and point it at a running API:

```bash
make install-cli          # installs `mikrom` to ~/.cargo/bin
mikrom health
mikrom auth login --email user@example.com --password secret
mikrom deploy --app my-service --image nginx:latest
```

## Environment variables

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | — | mikrom-api |
| `TEST_DATABASE_URL` | `postgres://mikrom:mikrom_password@localhost:5432/mikrom_api` | mikrom-api integration tests |
| `JWT_SECRET` | — | mikrom-api |
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | mikrom-api, mikrom-agent |
| `AGENT_PORT` | `5003` | mikrom-agent |
| `AGENT_HOSTNAME` | — | mikrom-agent (overrides auto-detected IP for registration) |
| `HOST_ID` | random UUID | mikrom-agent |
| `USE_TLS` | `false` | mikrom-agent, mikrom-scheduler, mikrom-api |
| `CERTS_DIR` | — | mikrom-agent, mikrom-scheduler, mikrom-api (when `USE_TLS=true`) |
| `MIKROM_API_URL` | `http://localhost:5001` | mikrom-cli |

## Testing

```bash
make test              # unit tests only (no Docker required)
make test-integration  # integration tests (starts/stops PostgreSQL via Docker)
make test-all          # unit + integration

# Run a single test by name
make test-one NAME=test_score_idle
```

## Build

```bash
make build          # all Rust crates (release)
make app-build      # Next.js production build
```

See each subdirectory's README for service-specific details.
