# mikrom - Agent Instructions

This file provides guidance to AI agents (Gemini, Claude, etc.) when working with the Mikrom repository. It combines architectural overview, development conventions, and operational instructions.

## Project Overview

Mikrom is a microVM orchestration system that deploys containerized workloads into Firecracker microVMs. It follows a distributed microservices architecture coordinated via NATS.

### Repository Layout

- **mikrom-api**: Axum HTTP REST API (port 5001), PostgreSQL via SQLx.
- **mikrom-scheduler**: NATS-based scheduler for resource management and IPAM.
- **mikrom-agent**: NATS-based agent managing Firecracker/Cloud-Hypervisor microVMs.
- **mikrom-network**: Shared library and utility for WireGuard mesh networking.
- **mikrom-builder**: Automated build engine using Railpack to turn Git repos into OCI images.
- **mikrom-router**: High-performance dynamic ingress router based on Caddy and Go.
- **mikrom-proto**: Shared Protocol Buffer definitions and generated code.
- **mikrom-app**: SvelteKit frontend (React 19, Tailwind CSS 4, shadcn/ui).

## Architecture

### Request & Traffic Flow

1. **Management**: User (CLI/Web) → `mikrom-api` → `mikrom-builder` → `mikrom-scheduler` → `mikrom-agent`.
2. **Traffic**: External User → `mikrom-router` → App MicroVM (via IPv6/6PN).

### Service Responsibilities

- **mikrom-api**: Management, auth (bcrypt + JWT), and deployments. Uses a repository trait pattern for testability.
- **mikrom-scheduler**: Maintains worker registry and app state. Picks workers based on scoring and capacity.
- **mikrom-agent**: Manages VM lifecycle, collects metrics, and handles NATS commands.
- **mikrom-network**: Orchestrates the WireGuard mesh, ensures route synchronization, and manages secure keys (zeroize).

## Development Conventions

### General
- **Rust Services**: Standard idiomatic Rust. Use `thiserror` for library errors and `anyhow` for applications.
- **Protocol Buffers**: Changes to `mikrom-proto/proto/*.proto` require regeneration via `cargo build`.
- **Security**: Internal communication uses **NATS with mutual TLS (mTLS)**.
- **Environment**: Driven by environment variables. See `mikrom-api/.env.example`.

### Frontend (mikrom-app)
- **Framework**: SvelteKit with React 19 components.
- **Styling**: Tailwind CSS 4.
- **UI Components**: **Strictly use shadcn/ui components.** Standard components must be imported from `@/components/ui`.
- **Form Composition**: Follow strict rules: `FieldGroup` + `Field` instead of generic `div`, `InputGroup` for icons/buttons, `FieldSet` for groups.

### Concurrency & Lifecycle
- **DeploymentFlowGuard**: Use RAII pattern to prevent concurrent deployment flows for the same app.
- **Agent Boot**: Uses `mikrom-init.sh` (PID 1) and `app-run.sh` for application entrypoint.

## Building and Running

The project uses a `Makefile` for high-level operations.

### Docker Compose
```bash
make up          # Start all dependencies and services
make logs        # Check combined logs
make down        # Stop containers
```

### Local Development
```bash
make db-start      # Run Postgres only
make run-api       # Start API
make run-scheduler # Start Scheduler
make run-builder   # Start Builder
make run-agent     # Start Agent
make run-router    # Start Router
make run-app       # Start Frontend App
```

### Tests
```bash
make test              # Run all unit tests
make test-integration  # Run integration tests (requires Docker)
make test-all          # Complete test suite
```

## Environment Variables Reference

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | — | mikrom-api, mikrom-router |
| `TEST_DATABASE_URL` | `postgres://...` | integration tests |
| `JWT_SECRET` | — | mikrom-api |
| `NATS_URL` | `nats://127.0.0.1:4222` | All Rust services |
| `MIKROM_HOST_ID` | random UUID | mikrom-agent, mikrom-network |
| `MIKROM_WG_PORT` | `51823` | WireGuard mesh |
| `USE_TLS` | `false` | All Rust services |

---
*Note: This file replaces the legacy GEMINI.md and CLAUDE.md files.*
