# Architecture

## Request Flow

- User or CLI starts in `mikrom-app` or `mikrom-cli`.
- Both call `mikrom-api` for auth, app lifecycle, deployments, secrets, and database provisioning.
- `mikrom-api` coordinates with `mikrom-builder` to turn Git repos into OCI images.
- `mikrom-api` then hands placement and execution coordination to `mikrom-scheduler`.
- `mikrom-scheduler` talks to `mikrom-agent`, which creates and manages microVMs on worker nodes.
- External traffic does not go through the control plane; it enters through `mikrom-router` and is routed to app microVMs.

```text
User / CLI
   |
   v
mikrom-app / mikrom-cli
   |
   v
 mikrom-api
   |\
   | \--> mikrom-builder -> OCI image -> registry
   |
   \----> mikrom-scheduler -> mikrom-agent -> microVMs

External traffic -> mikrom-router -> app microVMs
```

## Service Responsibilities

- `mikrom-api`: control plane and business logic.
- `mikrom-app`: dashboard for operators and users.
- `mikrom-cli`: terminal client for automation and day-to-day operations.
- `mikrom-builder`: source-to-image build service.
- `mikrom-scheduler`: placement engine and worker registry.
- `mikrom-agent`: worker daemon, microVM lifecycle, metrics, logs, and host coordination.
- `mikrom-router`: ingress, TLS/ACME, health checks, and traffic routing.
- `mikrom-network`: WireGuard mesh and host identity coordination.
- `mikrom-dns`: internal DNS for services, workers, and tenant resources.
- `mikrom-proto`: shared protobuf contracts for internal service communication.
- `mikrom-init`: boot binary that runs inside microVMs.
- `mikrom-agent-ebpf` and `mikrom-agent-ebpf-common`: agent data plane support.

## Platform Services

- `mikrom-network` maintains the WireGuard mesh and host identity coordination.
- `mikrom-dns` resolves internal service, worker, and tenant names and synthesizes DNS64 responses for external lookups when required.
- `mikrom-init` boots the guest workload environment inside each microVM.
- `mikrom-agent-ebpf` provides the compiled eBPF payload loaded by the agent for host-side networking and metrics support.

## Coordination Notes

- Control-plane state starts in `mikrom-api` and is fanned out over NATS to the builder, scheduler, and worker services.
- The scheduler is responsible for placement decisions, not routing or DNS.
- The router owns ingress, TLS state, and persisted traffic-plane routing data.
- Network and DNS services are separate from the scheduler so host identity and name resolution can evolve independently of workload placement.

## Dependencies At A Glance

- `mikrom-app` depends on `mikrom-api`.
- `mikrom-cli` depends on `mikrom-api`.
- `mikrom-api` depends on PostgreSQL, NATS, `mikrom-builder`, `mikrom-scheduler`, and database providers such as Neon when enabled.
- `mikrom-scheduler` depends on NATS and coordinates with `mikrom-agent`.
- `mikrom-agent` depends on NATS, host networking, Firecracker or Cloud Hypervisor, `mikrom-init`, `mikrom-network`, `mikrom-dns`, and the eBPF payload.
- `mikrom-router` depends on PostgreSQL, NATS, and WireGuard tooling for traffic-plane state.
- `mikrom-network` depends on NATS and host identity.
- `mikrom-dns` depends on NATS and upstream DNS resolvers.

## Service Summary

| Service | Purpose | Primary Inputs | Primary Outputs / Dependencies |
|---|---|---|---|
| `mikrom-app` | Operator dashboard | Browser, `API_UPSTREAM_URL` | Calls `mikrom-api` |
| `mikrom-cli` | Terminal client | User commands, config file | Calls `mikrom-api` |
| `mikrom-api` | Control plane | HTTP requests, PostgreSQL, NATS | Coordinates builder, scheduler, databases |
| `mikrom-builder` | Build service | Git repo, build config, NATS | OCI image, registry push |
| `mikrom-scheduler` | Placement engine | Worker metrics, NATS, DB state | Worker assignment, agent coordination |
| `mikrom-agent` | Worker daemon | Scheduler commands, host access, NATS | MicroVM lifecycle, logs, metrics |
| `mikrom-router` | Traffic ingress | External traffic, PostgreSQL, NATS, WireGuard | Routes to app microVMs |
| `mikrom-network` | Mesh networking | Host identity, NATS | WireGuard peer state and routes |
| `mikrom-dns` | Internal DNS | NATS events, upstream resolvers | Internal name resolution, DNS64 answers |
| `mikrom-proto` | Shared contracts | Protobuf definitions | Generated Rust types for internal comms |
| `mikrom-init` | MicroVM boot binary | VM boot environment | Starts the workload runtime inside the VM |
| `mikrom-agent-ebpf` | eBPF payload | Agent build pipeline | Host-side network/data-plane support |
| `ci` | Local CI runner | Workspace source, Docker, Dagger | Validation, image builds, publish flows |
