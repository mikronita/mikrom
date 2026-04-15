# mikrom-scheduler

gRPC scheduler service for the mikrom orchestration system. Built with [Tonic](https://github.com/hyperium/tonic).

**Port:** `5002`

## Responsibilities

- Maintains an in-memory registry of worker nodes (`WorkerRegistry`).
- Receives `DeployApp` requests from `mikrom-api` and selects the best available worker.
- Forwards `StartVm` RPCs to the chosen `mikrom-agent`.
- Tracks the lifecycle of each deployment as a `Job`.

## Worker selection

When a deploy request arrives the scheduler:

1. Filters workers to those that have reported metrics and can fit the requested VM (`memory_mib` + `disk_mib`).
2. Scores each candidate with `HostMetrics::calculate_score` (higher score = more headroom).
3. Workers that already host `MAX_APPS_PER_HOST` (10) applications are excluded.
4. The highest-scoring worker is selected.

If no workers are registered the scheduler returns `NoWorkers`. If workers exist but none can fit the VM requirements it returns `NoFit`.

## gRPC API

Defined in `mikrom-proto/proto/scheduler.proto`.

| RPC | Called by | Description |
|---|---|---|
| `DeployApp` | mikrom-api | Schedule and launch a new VM |
| `RegisterWorker` | mikrom-agent | Register an agent node |
| `ReportMetrics` | mikrom-agent | Update resource metrics for a node |

## Configuration

| Variable | Default | Description |
|---|---|---|
| `USE_TLS` | `false` | Enable mutual TLS for gRPC |

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
