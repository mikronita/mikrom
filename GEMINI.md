# mikrom - Workspace Context

## Project Overview

Mikrom is a microVM orchestration system that deploys containerized workloads into Firecracker microVMs. It consists of several services working together:

- **mikrom-api**: Axum/SQLx based REST API for management, auth, and deployments (Port 5001).
- **mikrom-scheduler**: Tonic-based gRPC scheduler for resource management and IPAM (Port 5002).
- **mikrom-agent**: Tonic-based gRPC agent running on worker nodes for microVM lifecycle (Port 5003).
- **mikrom-builder**: Automated build engine using Railpack to turn Git repos into OCI images (Port 5004).
- **mikrom-router**: High-performance dynamic ingress router using Hyper and Moka (Port 8080).
- **mikrom-cli**: Command-line interface to interact with the system.
- **mikrom-proto**: Shared Protocol Buffer definitions and generated code.
- **mikrom-app**: Next.js 16 frontend application (React 19, Tailwind CSS 4).

## Architecture

Mikrom follows a distributed microservices architecture:

### Management & Deployment Flow
User (CLI/Web) → mikrom-api → mikrom-builder → mikrom-scheduler → mikrom-agent

### Traffic Flow
User (Traffic) → mikrom-router → App MicroVM

## Building and Running

The project uses `Makefile` for high-level operations.

### Docker Compose (Full Stack)
```bash
make up          # Start everything
make logs        # Check logs
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
make run-app       # Start Next.js App
```

### Tests
```bash
make test              # All unit tests
make test-integration  # Integration tests (needs Docker)
make test-all          # Complete test suite
```

## Development Conventions

- **Frontend UI (mikrom-app)**: **Strictly use Flowbite React components.** Do not create custom wrappers or abstractions in `components/ui` for primitive components like buttons or cards. Always import directly from `flowbite-react`.
- **Rust Services**: Standard Rust workspace conventions.
- **Protocol Buffers**: Changes to `mikrom-proto/proto/*.proto` require regenerating code (managed by build scripts).
- **Security**: Internal gRPC communication between services uses **mutual TLS (mTLS)** for encryption and authentication.
- **Agent Boot System**: Uses a dual-script system: `mikrom-init.sh` (PID 1) and `app-run.sh` (executes the app entrypoint).
- **Environment**: Configuration is driven by environment variables. Check `.env.example` in `mikrom-api/` for reference.
