# mikrom-agent

gRPC agent service for the mikrom orchestration system. Runs on every worker node and manages Firecracker microVMs. Built with [Tonic](https://github.com/hyperium/tonic) and [sysinfo](https://github.com/GuillaumeGomez/sysinfo).

**Port:** `5003` (configurable via `AGENT_PORT`)

## Responsibilities

- On startup, registers itself with `mikrom-scheduler` via `RegisterWorker`.
- Sends a `ReportMetrics` heartbeat to the scheduler every 5 seconds.
- Implements the `AgentService` gRPC interface: start, stop, and query VM status.
- Delegates VM lifecycle operations to `FirecrackerManager`.

> **Note:** `FirecrackerManager` is currently an in-memory state machine. It does not yet invoke the Firecracker binary.

## gRPC API

Defined in `mikrom-proto/proto/agent.proto`.

| RPC | Called by | Description |
|---|---|---|
| `StartVm` | mikrom-scheduler | Launch a new microVM |
| `StopVm` | mikrom-scheduler | Terminate a running VM |
| `GetVmStatus` | mikrom-scheduler | Query the status of a VM |
| `GetMetrics` | mikrom-scheduler | Return current host resource metrics |

## VM states

```
Stopped → Starting → Running
                   → Stopping → Stopped
         → Failed
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | gRPC address of the scheduler |
| `AGENT_PORT` | `5003` | Port the agent listens on |
| `HOST_ID` | random UUID | Stable identifier for this node |
| `USE_TLS` | `false` | Enable mutual TLS for gRPC |

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
