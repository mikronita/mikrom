# mikrom - Workspace Context

## Project Overview

Mikrom is a microVM orchestration system that deploys containerized workloads into Firecracker microVMs. It consists of several services working together:

- **mikrom-api**: Axum/SQLx based REST API for management, auth, and deployments (Port 5001).
- **mikrom-scheduler**: NATS-based scheduler for resource management and IPAM.
- **mikrom-agent**: NATS-based agent running on worker nodes for microVM lifecycle.
- **mikrom-builder**: Automated build engine using Railpack to turn Git repos into OCI images.
- **mikrom-router**: High-performance dynamic ingress router based on Caddy and Go (Ports 80/443).
- **mikrom-cli**: Command-line interface to interact with the system.
- **mikrom-proto**: Shared Protocol Buffer definitions and generated code.
- **mikrom-app**: Next.js 16 frontend application (React 19, Tailwind CSS 4).

## Architecture

Mikrom follows a distributed microservices architecture:

### Management & Deployment Flow
User (CLI/Web) → mikrom-api → mikrom-builder → mikrom-scheduler → mikrom-agent

### Traffic Flow
User (Traffic) → mikrom-router → App MicroVM

### Integrations
- **GitHub**: Automated deployments via Webhooks.
- **ACME**: Automatic TLS certificate management via Let's Encrypt.
- **Secrets**: Encrypted environment variable management.
- **Healthchecks**: Configurable liveness and readiness probes for apps (using strict IPv6/6PN connectivity).
- **Networking**: IPv6-only control plane with 6PN mesh. Local IPv4 NAT managed by the Agent for outbound internet access.

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

- **Frontend UI (mikrom-app)**: **Strictly use shadcn/ui components.** Do not use Flowbite React or other external component libraries. Standard components must be imported from `@/components/ui`. Always prefer composing existing shadcn/ui primitives over creating custom HTML/CSS wrappers. Specifically for forms, you MUST follow the project's strict shadcn composition rules: use `FieldGroup` + `Field` instead of `div`, `InputGroup` + `InputGroupAddon` for inputs with icons or buttons, and `FieldSet` + `FieldLegend` for groups of inputs/switches.
- **Rust Services**: Standard Rust workspace conventions.
- **Protocol Buffers**: Changes to `mikrom-proto/proto/*.proto` require regenerating code (managed by build scripts).
- **Security**: Internal communication between services uses **NATS with mutual TLS (mTLS)** for encryption and authentication.
- **Concurrency Control**: Use the RAII **`DeploymentFlowGuard`** pattern (managed via `AppState::try_start_flow`) to prevent concurrent zero-downtime deployment flows for the same application. Guards should be acquired early in handlers and moved into background tasks to ensure consistent state and prevent "lock leaks" during panics.
- **Agent Boot System**: Uses a dual-script system: `mikrom-init.sh` (PID 1) and `app-run.sh` (executes the app entrypoint).
- **Environment**: Configuration is driven by environment variables. Check `.env.example` in `mikrom-api/` for reference.

<!-- SPECKIT START -->
For additional context about technologies to be used, project structure,
shell commands, and other important information, read the current plan
<!-- SPECKIT END -->
