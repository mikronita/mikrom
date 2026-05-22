# mikrom — Open-Source PaaS on Firecracker

[![CI](https://github.com/antpard/mikrom/actions/workflows/ci.yml/badge.svg)](https://github.com/antpard/mikrom/actions/workflows/ci.yml)

Mikrom is a modern **Platform-as-a-Service (PaaS)** that deploys containerized workloads into lightweight microVMs ([Firecracker](https://firecracker-microvm.github.io/) or [QEMU microvm](https://www.qemu.org/docs/master/system/i386/microvm.html)). Inspired by platforms like Fly.io and Railway, Mikrom provides a **Zero-Config** experience: point it at a Git repository, and it will build, deploy, and route traffic to your application automatically.

## Key Features

- **🚀 Zero-Config**: Automatic language detection and building via [Railpack](https://railpack.com/).
- **⚡ MicroVM Isolation**: Every application runs in its own dedicated microVM (Firecracker or QEMU microvm) for maximum security and performance.
- **🌐 Dynamic Routing**: Built-in ingress router based on Caddy with automatic hostname assignment and TLS.
- **🔒 Automatic TLS**: Seamless ACME integration (Let's Encrypt) for all your applications.
- **🐙 GitHub Integration**: Connect your repositories for automated deployments on every push.
- **🦀 Built with Rust**: High-performance, memory-safe backend services.
- **📊 Real-time Observability**: Metrics and logs collection via NATS, Prometheus, and Loki.
- **🛠️ Power CLI**: Full control over your apps and deployments from your terminal.
- **🖥️ Dual Hypervisor**: Choose between Firecracker and QEMU microvm per deployment — or let the scheduler decide automatically.

## Architecture

Mikrom follows a distributed microservices architecture:

```
User (CLI/Web) 
  → mikrom-api      (REST Management & Auth)
    → mikrom-builder   (Git cloning & Railpack building)
    → mikrom-scheduler (Resource allocation & IPAM)
      → mikrom-agent   (Worker node, Firecracker/QEMU VM lifecycle)
User (Traffic)
  → mikrom-router   (Caddy-based Dynamic Ingress Proxy)
    → App MicroVM      (Target instance)
Observability
  → mikrom-telemetry (Prometheus & Loki metrics/logs)
```

## Repository Layout

| Directory | Description |
|---|---|
| `mikrom-api/` | The central brain. Manages Users, Apps, and Deployments. |
| `mikrom-builder/` | The build engine. Clones Git repos and builds OCI images using Railpack. |
| `mikrom-router/` | High-performance Caddy-based dynamic reverse proxy. |
| `mikrom-telemetry/` | Observability service for metrics (Prometheus) and logs (Loki). |
| `mikrom-scheduler/` | Intelligent resource manager. Places workloads on the best workers. |
| `mikrom-agent/` | The worker daemon. Manages microVMs (Firecracker or QEMU microvm via a pluggable `VmHypervisor` trait), network isolation, and host metrics. |
| `mikrom-app/` | Dashboard (Next.js 16, React 19, Tailwind CSS 4). |
| `mikrom-cli/` | Command-line interface (`mikrom`). |
| `mikrom-proto/` | Shared NATS/Protobuf definitions. |

## Quick Start

### Prerequisites

- **Rust** (stable toolchain)
- **Docker** (for build engine and local services)
- **Node.js + pnpm** (for the dashboard)
- **Railpack CLI** (for local building)

### Running Mikrom Locally

```bash
# 1. Start Infrastructure (PostgreSQL + Docker Registry)
make db-start

# 2. Run the core services (separate terminals)
make run-api        # port 5001
make run-scheduler  # Internal NATS
make run-builder    # Internal NATS
make run-agent      # port 5003
make run-router     # port 80/443
make run-telemetry  # port 9090
make run-app        # port 3000
```

### Using the CLI

```bash
# Install the CLI
make install-cli

# Login
mikrom auth login --email user@example.com --password secret

# Create and deploy an app
mikrom app create --name my-app --git-url https://github.com/user/my-app.git
mikrom app deploy --name my-app --cpu 2 --memory 1G

# Check status
mikrom deployment list
```

### Deployment presets

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

## Contributing

Mikrom is an open-source project. Feel free to open issues or pull requests.

## License

Apache-2.0
