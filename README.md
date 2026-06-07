# Mikrom - Open-Source Edge Platform

Mikrom is a Rust-first edge platform that deploys containerized workloads into lightweight microVMs on Firecracker or Cloud Hypervisor. The workspace now includes a SvelteKit dashboard, a Pingora-based router, WireGuard and DNS control-plane services, eBPF support in the agent, and a Dagger-backed local CI/CD pipeline that runs the same validation logic locally and in GitHub Actions.

## What Changed Recently

- Added a Dagger runner in `ci/` with `smoke`, `fast`, `full`, `images`, and `publish-release` profiles.
- Moved GitHub Actions to the same Rust-based Dagger runner instead of duplicating CI logic in YAML.
- Migrated the dashboard to SvelteKit, Svelte 5, Vite, Tailwind CSS 4, shadcn-svelte, and bits-ui.
- Updated the router to Pingora and expanded the platform with `mikrom-dns` and `mikrom-network`.
- Added eBPF validation for `mikrom-agent-ebpf` with a nightly `build-std` path.
- Tightened local development targets in the `Makefile` for Docker Compose, tmux-based dev, and Dagger profiles.

## Key Features

- Zero-config app deployment from Git repositories through the builder, scheduler, and agent pipeline.
- MicroVM isolation with Firecracker or Cloud Hypervisor per workload.
- PostgreSQL databases are provisioned through Neon and run in Cloud Hypervisor-backed microVMs when deployed on the platform.
- Dynamic ingress routing with automatic TLS and mesh-aware routing.
- Internal DNS and WireGuard mesh management for platform services and workloads.
- A SvelteKit dashboard and Rust CLI for day-to-day operations.
- Dagger-backed CI/CD profiles that can be run locally or in GitHub Actions.
- OpenTelemetry-based observability across services.
- An eBPF data plane for agent-side network handling and metrics.

## Architecture

- Control plane: CLI/Web -> `mikrom-app` / `mikrom-cli` -> `mikrom-api` -> `mikrom-builder` -> `mikrom-scheduler` -> `mikrom-agent`.
- Traffic plane: External traffic -> `mikrom-router` -> app microVM.
- Platform services: `mikrom-network` maintains the WireGuard mesh, `mikrom-dns` serves internal name resolution, `mikrom-init` boots microVMs, and `mikrom-agent-ebpf` provides the eBPF program used by the agent.

## Technology Stack

- Backend: Rust 2024, Tokio, Axum, SQLx, async-nats, reqwest, tracing, OpenTelemetry.
- Router: Rust + Pingora, PostgreSQL, NATS, OpenSSL, WireGuard tooling.
- Frontend: SvelteKit, Svelte 5, Vite, Tailwind CSS 4, shadcn-svelte, bits-ui, Lucide, Vitest, Playwright.
- Platform and tooling: Firecracker, Cloud Hypervisor, PostgreSQL, Neon, NATS, Docker Compose, Dagger, BuildKit, GitHub Actions.

## Repository Layout

| Directory | Description |
|---|---|
| `mikrom-api/` | Control plane API for auth, deployments, databases, and integrations. |
| `mikrom-app/` | SvelteKit dashboard and frontend application. |
| `mikrom-agent/` | Worker daemon that manages microVM lifecycle and host-side coordination. |
| `mikrom-agent-ebpf/` | eBPF program compiled for the agent data plane. |
| `mikrom-agent-ebpf-common/` | Shared types for the agent and its eBPF program. |
| `mikrom-builder/` | Build engine that turns Git repositories into OCI images. |
| `mikrom-cli/` | Command-line client for managing the platform. |
| `mikrom-dns/` | Internal DNS service for platform and workload name resolution. |
| `mikrom-init/` | Init process used inside the microVMs. |
| `mikrom-network/` | WireGuard mesh and network coordination service. |
| `mikrom-proto/` | Shared protobuf definitions and generated Rust code. |
| `mikrom-router/` | Pingora-based ingress router and traffic plane. |
| `mikrom-scheduler/` | Resource scheduler and placement engine. |
| `ci/` | Dagger-backed local CI/CD runner in Rust. |

## Quick Start

### Prerequisites

- Rust stable toolchain
- Docker and Docker Compose
- Node.js and pnpm
- Dagger CLI for the `make ci-*` profiles

### Local Development

Use Docker Compose for the full stack:

```bash
make db-start   # Start PostgreSQL for local development
make up         # Start all services with Docker Compose
make logs       # Follow logs for the full stack
make down       # Stop and remove containers
```

For the tmux-based dev loop:

```bash
make dev
```

If you want to run services individually, the main targets are:

```bash
make run-api
make run-scheduler
make run-builder
make run-agent
make run-router
make run-app
```

### Using the CLI

```bash
make install-cli
mikrom auth login --email user@example.com --password secret
mikrom app create --name my-app --git-url https://github.com/user/my-app.git
mikrom app deploy --name my-app --cpu 2 --memory 1G
mikrom deployment list
```

For Neon-backed databases, the CLI can now print the connection flow directly:

```bash
mikrom db create orders
mikrom db list
mikrom db connection <database-id>
```

`mikrom db connection <database-id>` returns:

- the SSH tunnel command to reach the VM
- the `psql` command to connect through the tunnel
- a JSON form with the same connection metadata via `--output json`

### Deployment Presets

When you deploy an app from the UI or CLI, Mikrom currently offers the same resource presets:

- CPU: `1`, `2`, `3`, `4`
- RAM: `512M`, `1G`, `2G`, `4G`

The default selection is `1` CPU and `512M` RAM.

## Testing

```bash
make test              # Unit tests only
make test-integration  # Integration tests (requires Docker)
make test-all          # Complete suite
```

- For host and VM smoke validation of NAT64/DNS64, see [docs/nat64-dns64-smoke-checklist.md](/home/apardo/Work/mikrom.rust/docs/nat64-dns64-smoke-checklist.md).

The pre-commit hook in `scripts/pre-commit.sh` uses the Dagger-backed targets:

- `make ci-fast` for Rust and shared workspace changes.
- `make ci-app` for `mikrom-app` changes.

## CI Profiles

Use the Dagger-backed CI runner directly for the fastest feedback loop. The profiles are ordered from cheapest to most expensive:

```bash
make ci-smoke   # fmt + clippy + mikrom-app validation
make ci-fast    # smoke + workspace tests with ephemeral Postgres and NATS
make ci-full    # fast + release build + eBPF validation
make ci         # alias for the full profile
make ci-images  # build service images from the Dockerfiles
make ci-release # full validation + image build + registry publish
```

Recommended usage:

1. Use `make ci-smoke` for everyday development and quick feedback.
2. Use `make ci-fast` before pushing changes that affect shared Rust code or service contracts.
3. Use `make ci-full` before merging or when you change build logic, native dependencies, or the eBPF path.
4. Use `make ci-images` after touching any Dockerfile or image build context.
5. Use `make ci-release` only for release tags or when you want to exercise the full publish flow end to end.

## Release Flow

Use the release-oriented target when you want to validate and publish images in one step:

```bash
make ci-release
```

This runs the full validation profile first, then builds and publishes the service images using the registry credentials provided through the environment.

## Contributing

Mikrom is an open-source project. Feel free to open issues or pull requests.

## License

Apache-2.0
