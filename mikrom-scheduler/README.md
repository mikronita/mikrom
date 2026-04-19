# mikrom-scheduler

gRPC scheduler service for the mikrom orchestration system. Built with [Tonic](https://github.com/hyperium/tonic).

**Port:** `5002`

## Architecture

```
mikrom-api (REST)
    │
    │ DeployApp RPC
    ▼
mikrom-scheduler
    │
    ├─ WorkerRegistry: tracks registered agents
    ├─ AppScheduler: manages job lifecycle
    └─ HostMetrics: scores workers by available resources
         │
         │ StartVm RPC
         ▼
    mikrom-agent
```

## Responsibilities

- Maintains an in-memory registry of worker nodes (`WorkerRegistry`).
- Receives `DeployApp` requests from `mikrom-api` and selects the best available worker.
- Forwards `StartVm` RPCs to the chosen `mikrom-agent`.
- Tracks the lifecycle of each deployment as a `Job`.

## Worker selection

When a deploy request arrives the scheduler:

1. **Filter**: Workers must have reported metrics within 30 seconds (configurable via `METRICS_TTL_SECS`).
2. **Capacity check**: Workers must have enough `memory_mib` + `disk_mib` for the requested VM.
3. **Slot check**: Workers can host at most `MAX_APPS_PER_HOST` (10) applications.
4. **Score**: Each candidate is scored with `HostMetrics::calculate_score`:
   - `score = (available_memory_mib / total_memory_mib) + (available_disk_mib / total_disk_mib)`
   - Higher score = more headroom.
5. **Select**: The highest-scoring worker is chosen.

If no workers are registered the scheduler returns `NoWorkersAvailable`. If workers exist but none can fit the VM requirements it returns `NoFit`.

## gRPC API

Defined in `mikrom-proto/proto/scheduler.proto`.

| RPC | Direction | Description |
|---|---|---|
| `DeployApp` | mikrom-api → scheduler | Schedule and launch a new VM |
| `RegisterWorker` | agent → scheduler | Register an agent node |
| `ReportMetrics` | agent → scheduler | Update resource metrics |
| `StopVm` | mikrom-api → scheduler | Stop a running VM |
| `GetVmStatus` | mikrom-api → scheduler | Get VM status |

## Configuration

| Variable | Default | Description |
|---|---|---|
| `METRICS_TTL_SECS` | `30` | Seconds before metrics are considered stale |
| `MAX_APPS_PER_HOST` | `10` | Maximum VMs per worker node |
| `USE_TLS` | `false` | Enable mutual TLS for gRPC |
| `CERTS_DIR` | — | Directory containing TLS certificates (required when `USE_TLS=true`) |

## Deployment flow

```
1. API receives POST /deploy
2. API calls DeployApp RPC → Scheduler
3. Scheduler selects best worker (highest score)
4. Scheduler calls StartVm RPC → Agent
5. Agent starts Firecracker VM (stubbed)
6. Agent returns VM details to Scheduler
7. Scheduler returns job details to API
8. API returns job_id to client
```

## Development

```bash
# Run the scheduler
cargo run -p mikrom-scheduler

# Unit tests
cargo test --lib -p mikrom-scheduler
```

## Code structure

```
src/
  main.rs            Entry point — binds gRPC server
  lib.rs             Public re-exports
  server.rs          Tonic service implementation (DeployApp, RegisterWorker, ReportMetrics)
  scheduler.rs       AppScheduler — job tracking and worker selection logic
  worker_registry.rs WorkerRegistry — in-memory store of known agents
  metrics.rs         HostMetrics — resource snapshot and scoring function
  job.rs             Job and JobStatus types
```
