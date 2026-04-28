# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository layout

```
mikrom-api/        Axum HTTP REST API (port 5001), PostgreSQL via SQLx
mikrom-scheduler/  NATS-based scheduler
mikrom-agent/      NATS-based agent, manages Firecracker VMs
mikrom-proto/      Shared protobuf definitions (agent + scheduler messages)
mikrom-app/        Next.js 16.2.3 frontend (React 19, Tailwind CSS 4, pnpm)
firecracker/       Firecracker VMM source (vendored)
```

The four Rust crates form a Cargo workspace (`Cargo.toml` at the root).

## Commands

### Rust workspace

```bash
# Build all crates
cargo build

# Run mikrom-api (port 5001)
cd mikrom-api && cargo run

# Run mikrom-scheduler
cargo run -p mikrom-scheduler

# Run mikrom-agent
cargo run -p mikrom-agent

# Run unit tests only (no Docker required)
cargo nextest run --lib

# Run mikrom-api integration tests (requires Docker)
cd mikrom-api && docker-compose up -d postgres && cargo nextest run --test integration

# Run a single test by name
cargo nextest run <test_name>
```

### mikrom-app (Next.js)

```bash
cd mikrom-app
pnpm dev      # development server
pnpm build    # production build
pnpm lint     # eslint
```

> **Next.js 16.2.3 warning**: This version has breaking changes relative to widely-known versions. Read `mikrom-app/node_modules/next/dist/docs/` before writing any Next.js code; do not rely on training-data assumptions about the framework.

## Environment variables

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | — | mikrom-api (runtime) |
| `TEST_DATABASE_URL` | `postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test` | mikrom-api integration tests |
| `JWT_SECRET` | — | mikrom-api |
| `NATS_URL` | `nats://127.0.0.1:4222` | mikrom-api, mikrom-agent, mikrom-scheduler |
| `HOST_ID` | random UUID | mikrom-agent |
| `USE_TLS` | `false` | mikrom-agent, mikrom-scheduler, mikrom-api |

`docker-compose up -d postgres nats` in the root starts the PostgreSQL and NATS dependencies.

## Architecture

### Request flow

```
HTTP Client
  → mikrom-api (REST, port 5001)
    → mikrom-scheduler (NATS)
      → mikrom-agent (NATS)
        → FirecrackerManager (in-memory stub; real Firecracker integration is pending)
```

### Service responsibilities

**mikrom-api** exposes three REST routes: `GET /health`, `POST /auth/register`, `POST /auth/login`, and `POST /deploy`. The `/deploy` handler publishes a deployment request to the scheduler over NATS on every request. Auth uses bcrypt + JWT (`jsonwebtoken`). Database access follows a repository trait pattern (`UserRepository`) so that `mockall` mocks can be swapped in for unit tests; integration tests spin up a real PostgreSQL container via `testcontainers`.

**mikrom-scheduler** maintains an in-memory `WorkerRegistry` (keyed by `host_id`) and an `AppScheduler` (keyed by `job_id`). On `DeployApp` it picks the highest-scoring available worker using `HostMetrics::calculate_score` with a cap of `MAX_APPS_PER_HOST = 10`, then sends a `StartVm` request to that agent over NATS.

**mikrom-agent** starts a background task on startup that: (1) registers itself with the scheduler via `RegisterWorker` (NATS request/reply), then (2) loops every 5 seconds collecting `sysinfo` metrics and publishing them via `ReportMetrics`. The NATS handler implements the agent logic (start/stop/status VM, get metrics). `FirecrackerManager` is currently an in-memory state machine — it does not yet invoke the Firecracker binary.

**mikrom-proto** contains the `.proto` files and their pre-compiled `.rs` outputs. When `.proto` files change, regenerate with `cargo build` (the `build.rs` in the crate handles `prost-build`). TLS cert loading for NATS lives in `mikrom-proto::tls`.

### Key data types

- `Job` (`mikrom-scheduler/src/job.rs`) — deployment unit tracked by the scheduler; mirrors `DeployStatus` proto enum.
- `HostMetrics` (`mikrom-scheduler/src/metrics.rs`) — resource snapshot used for worker selection scoring.
- `AppState` (`mikrom-api/src/lib.rs`) — Axum shared state holding `PgPool` and a NATS connection.

