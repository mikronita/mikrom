# mikrom — Open-Source PaaS on Firecracker

[![CI](https://github.com/antpard/mikrom/actions/workflows/ci.yml/badge.svg)](https://github.com/antpard/mikrom/actions/workflows/ci.yml)

Mikrom is a modern **Platform-as-a-Service (PaaS)** that deploys containerized workloads into [Firecracker](https://firecracker-microvm.github.io/) microVMs. Inspired by platforms like Fly.io and Railway, Mikrom provides a **Zero-Config** experience: point it at a Git repository, and it will build, deploy, and route traffic to your application automatically.

## Key Features

- **🚀 Zero-Config**: Automatic language detection and building via [Railpack](https://railpack.com/).
- **⚡ MicroVM Isolation**: Every application runs in its own dedicated Firecracker microVM for maximum security and performance.
- **🌐 Dynamic Routing**: Built-in ingress router with automatic hostname assignment (`app.apps.mikrom.spluca.org`).
- **🦀 Built with Rust**: High-performance, memory-safe backend services.
- **📊 Real-time Dashboard**: Modern web interface built with Next.js and Tailwind CSS.
- **🛠️ Power CLI**: Full control over your apps and deployments from your terminal.

## Architecture

Mikrom follows a distributed microservices architecture:

```
User (CLI/Web) 
  → mikrom-api      (REST Management & Auth)
    → mikrom-builder   (Git cloning & Railpack building)
    → mikrom-scheduler (Resource allocation & IPAM)
      → mikrom-agent   (Worker node, Firecracker lifecycle)
User (Traffic)
  → mikrom-router   (Dynamic Ingress Proxy)
    → App MicroVM      (Target instance)
```

## Repository Layout

| Directory | Description |
|---|---|
| `mikrom-api/` | The central brain. Manages Users, Apps, and Deployments. |
| `mikrom-builder/` | The build engine. Clones Git repos and builds OCI images using Railpack. |
| `mikrom-router/` | High-performance dynamic reverse proxy for app ingress. |
| `mikrom-scheduler/` | Intelligent resource manager. Places workloads on the best workers. |
| `mikrom-agent/` | The worker daemon. Manages microVMs and network isolation. |
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
make run-router     # port 8080
make run-app        # port 3000
```

### Using the CLI

```bash
# Install the CLI
make install-cli

# Login
mikrom auth login --email user@example.com --password secret

# Create and deploy an app
mikrom apps create --name my-app --git-url https://github.com/user/my-app.git
mikrom apps deploy --app-id <app-id>

# Check status
mikrom deployments
```

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
