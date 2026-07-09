# Mikrom - Agent Instructions

This file gives AI agents the operational context for the Mikrom monorepo.

## Project Overview

Mikrom is a Rust-first platform-as-a-service that deploys containerized workloads into Firecracker or Cloud Hypervisor microVMs. The platform is split into a control plane, a traffic plane, and supporting network/DNS services, and it now uses a Dagger-backed local CI/CD runner for repeatable validation.

## Repository Layout

- `mikrom-api`: Axum REST API for auth, user notification settings, Personal Access Tokens (PATs), apps, deployments, VM/database snapshots, and Neon-backed database provisioning.
- `mikrom-app`: SvelteKit dashboard built with Svelte 5, Tailwind CSS 4, shadcn-svelte, and bits-ui.
- `mikrom-agent`: Worker daemon that manages microVM lifecycle (Firecracker/Cloud Hypervisor, process recovery, atomic state persistence), metrics, and host coordination.
- `mikrom-agent-ebpf`: eBPF program for the agent data plane.
- `mikrom-agent-ebpf-common`: Shared types between the agent and its eBPF program.
- `mikrom-builder`: Build engine that turns Git repositories into OCI images.
- `mikrom-cli`: Command-line client for the platform.
- `mikrom-dns`: Internal DNS service for platform and workload discovery.
- `mikrom-init`: Zig source for the `mikrom-init` binary used inside microVMs.
- `mikrom-network`: WireGuard mesh and network coordination service.
- `mikrom-proto`: Shared protobuf definitions and generated code.
- `mikrom-router`: Pingora-based ingress router and traffic plane.
- `mikrom-scheduler`: Resource scheduler and worker placement engine.
- `ci`: Dagger runner written in Rust.

## Architecture

### Request and Traffic Flow

1. Management: user or CLI -> `mikrom-app` / `mikrom-cli` -> `mikrom-api` -> `mikrom-builder` -> `mikrom-scheduler` -> `mikrom-agent`.
2. Traffic: external user -> `mikrom-router` -> app microVM.
3. Platform services: `mikrom-network` maintains the WireGuard mesh, `mikrom-dns` serves internal name resolution, and `mikrom-agent-ebpf` supports the agent data plane.

### Service Responsibilities

- `mikrom-api`: auth, personal access tokens (PATs), user notification preferences, app lifecycle, deployment orchestration, VM/database snapshots/backups/branching, secrets, and Neon-backed database provisioning.
- `mikrom-scheduler`: worker registry, scheduling decisions, and cluster state coordination over NATS.
- `mikrom-agent`: VM lifecycle (with Firecracker/Cloud Hypervisor, atomic state persistence, and OS-level recovery checks), host metrics, NATS command handling, and eBPF-backed data plane integration.
- `mikrom-network`: mesh orchestration, route synchronization, and key handling.
- `mikrom-router`: ingress routing, health checks, TLS, and traffic observability.

## Development Conventions

- Rust services use idiomatic Rust. Prefer `thiserror` for library errors and `anyhow` for applications.
- Changes to `mikrom-proto/proto/*.proto` require regeneration through the normal Cargo build flow.
- Internal communication uses NATS; some services support mTLS via `USE_TLS`.
- Frontend work in `mikrom-app` must use the existing shadcn-svelte component set under `src/lib/components/ui`.
- The Dagger CI runner is the preferred local validation path for workspace-wide checks.
- Use `make ci-fast` for the normal Rust validation path and `make ci-external-tests` for opt-in NATS/PostgreSQL integration suites. Keep Ceph-specific validation manual unless the host provides the required cluster.
- Use `make ci-ceph-tests` or the dedicated `ceph-tests` workflow job only on a self-hosted runner that has access to the Ceph cluster and host-level `/etc/ceph` configuration.
- The Ceph runner is expected to carry the `self-hosted`, `linux`, and `ceph` labels and expose `/etc/ceph/ceph.conf` plus `/etc/ceph/admin.secret`.

## Building and Running

The project uses the `Makefile` for common tasks.

### Local Development

```bash
make db-start
make up
make logs
make down
make dev
```

### Service Targets

```bash
make run-api
make run-scheduler
make run-builder
make run-agent
make run-router
make run-app
```

### Testing

```bash
make test
make test-integration
make test-all
make ci-smoke
make ci-fast
make ci-full
make ci-images
make ci-release
```

The pre-commit hook in `scripts/pre-commit.sh` delegates to the Dagger-backed targets:

- `make ci-fast` for Rust and shared workspace changes.
- `make ci-app` for `mikrom-app` changes.

## Environment Variables Reference

| Variable | Used by |
|---|---|
| `DATABASE_URL` | `mikrom-api`, `mikrom-router` |
| `TEST_DATABASE_URL` | `mikrom-api` tests and Dagger workspace tests |
| `JWT_SECRET` | `mikrom-api` |
| `NATS_URL` | Most Rust services |
| `TEST_NATS_URL` | Test helpers and Dagger workspace tests |
| `MIKROM_HOST_ID` | `mikrom-agent`, `mikrom-network` |
| `MIKROM_WG_PORT` | WireGuard mesh |
| `USE_TLS` | NATS mTLS-enabled services |
| `MIKROM_IMAGE_PREFIX` | Dagger image build/publish profiles |
| `MIKROM_IMAGE_TAG` | Dagger publish profile |
| `MIKROM_REGISTRY_USERNAME` | Dagger publish profile |
| `MIKROM_REGISTRY_TOKEN` | Dagger publish profile |

## Notes

- Keep docs aligned with the current workspace stack. Avoid reintroducing references to older implementations unless they are still present in the repository.
- Prefer updating the service README and AGENTS file together when you change a service boundary or its runtime assumptions.

---
*This file replaces the legacy GEMINI.md and CLAUDE.md files.*
