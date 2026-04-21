# mikrom - Workspace Context

## Project Overview

Mikrom is a microVM orchestration system that deploys containerized workloads into Firecracker microVMs. It consists of several services working together:

- **mikrom-api**: Axum-based REST API for management, auth, and deployments (Port 5001).
- **mikrom-scheduler**: Tonic-based gRPC scheduler for resource management (Port 5002).
- **mikrom-agent**: Tonic-based gRPC agent running on worker nodes (Port 5003).
- **mikrom-cli**: Command-line interface to interact with the system.
- **mikrom-proto**: Shared Protocol Buffer definitions and generated code.
- **mikrom-app**: Next.js 16 frontend application (React 19, Tailwind CSS 4).

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
make db-start    # Run Postgres only
make run-api     # Start API
make run-scheduler # Start Scheduler
make run-agent   # Start Agent
make run-app     # Start Next.js App
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
- **Environment**: Configuration is driven by environment variables. Check `.env.example` in `mikrom-api/` for reference.