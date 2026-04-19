# mikrom-agent

gRPC agent service for the mikrom orchestration system. Runs on every worker node and manages Firecracker microVMs. Built with [Tonic](https://github.com/hyperium/tonic) and [sysinfo](https://github.com/GuillaumeGomez/sysinfo).

**Port:** `5003` (configurable via `AGENT_PORT`)

## Architecture

```
                    ┌────────────────────────────────────┐
                    │    mikrom-scheduler              │
                    │  (gRPC client)             │
                    └──────────┬──────────────┘
                               │
                    RegisterWorker + ReportMetrics
                    (every 5s heartbeat)
                               │
                               ▼
┌──────────────────────────────────────────────────────┐
│              mikrom-agent                      │
│                                              │
│  ┌─────────────┐     ┌─────────────────┐    │
│  │  Metrics  │────▶│  Firecracker │    │
│  │ Collector │     │ Manager     │    │
│  └─────────────┘     └─────────────┘    │
│                                              │
│  - Host resources (CPU, memory, disk)     │
│  - VM lifecycle (start/stop/status)      │
└──────────────────────────────────────────────────────┘
```

## Responsibilities

- **Registration**: On startup, connects to `mikrom-scheduler` via `RegisterWorker`.
- **Heartbeat**: Sends `ReportMetrics` every 5 seconds with host resources.
- **VM Management**: Implements `AgentService` gRPC interface for VM lifecycle.
- **Delegation**: Delegates VM operations to `FirecrackerManager`.

> **Note:** `FirecrackerManager` is currently an in-memory state machine. Real Firecracker integration is pending.

## gRPC API

Defined in `mikrom-proto/proto/agent.proto`.

| RPC | Direction | Description |
|---|---|---|
| `StartVm` | scheduler → agent | Launch a new microVM |
| `StopVm` | scheduler → agent | Terminate a running VM |
| `PauseVm` | scheduler → agent | Pause a running VM |
| `ResumeVm` | scheduler → agent | Resume a paused VM |
| `GetVmStatus` | scheduler → agent | Query VM status |
| `GetHostMetrics` | scheduler → agent | Return host resource metrics |

## VM states

```
┌────────┐     ┌───────────┐     ┌────────┐
│Stopped │────▶│Starting │────▶│Running │
└────────┘     └─────────┘     └────────┘
                   │            │
                   │            ▼
                   │        ┌───────────┐
                   │        │Stopping  │
                   │        └──┬──────┘
                   │           │
                   │           ▼
                   └──────▶  Failed
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | gRPC address of the scheduler |
| `AGENT_PORT` | `5003` | Port the agent listens on |
| `HOST_ID` | random UUID | Stable identifier for this node |
| `AGENT_HOSTNAME` | — | Hostname/IP advertised to the scheduler (overrides auto-detected local IP; useful in Docker) |
| `USE_TLS` | `false` | Enable mutual TLS for gRPC |
| `CERTS_DIR` | — | Directory containing TLS certificates (required when `USE_TLS=true`) |

## Development

```bash
# Run the agent
cargo run -p mikrom-agent

# Unit tests
cargo test --lib -p mikrom-agent
```

## Code structure

```
src/
  main.rs          Entry point — resolves config, discovers local IP, starts gRPC server
  lib.rs           Public re-exports
  server.rs        Tonic AgentService implementation; background registration + metrics loop
  firecracker.rs   FirecrackerManager — in-memory VM state machine (VmInfo, VmStatus, VmConfig)
  metrics.rs       SystemMetrics and MetricsCollector (sysinfo wrapper)
```
