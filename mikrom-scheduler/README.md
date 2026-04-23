# mikrom-scheduler

The intelligent resource manager for the Mikrom PaaS. It orchestrates the placement of microVMs across a cluster of worker nodes, ensuring optimal resource utilization and high availability. Built with [Tonic](https://github.com/hyperium/tonic).

**Port:** `5002`

## Key Responsibilities

- **Worker Registry**: Tracks all active worker nodes, their identity, and their networking configuration.
- **Resource Orchestration**: Selects the best worker for every deployment using intelligent scoring.
- **Job Lifecycle**: Manages the transitions between `PENDING`, `SCHEDULED`, `RUNNING`, and `FAILED` for every microVM.
- **IPAM (IP Address Management)**: Automatically allocates and releases internal IP addresses for microVMs within each worker's subnet.
- **Health Monitoring**: Detects stale workers and automatically marks their workloads as unreachable.

## Intelligent Placement

When a deployment is requested, the scheduler evaluates candidates based on:

1.  **Strict Filters**: Candidates must have enough CPU, RAM, and Disk, and must have reported metrics within the last 30 seconds.
2.  **Scoring**:
    - **Resource Headroom**: Favors nodes with more free memory and disk.
    - **Soft Anti-Affinity**: Mikrom tries to spread instances of the same application across different physical hosts to maximize reliability. Each existing instance of an app on a node applies a penalty to its placement score.
3.  **Strategies**:
    - **Least Loaded (Default)**: Spreads work across all nodes.
    - **Bin Packing**: Fills nodes sequentially to allow idle nodes to be powered down.

## gRPC API

Defined in `mikrom-proto/proto/scheduler.proto`.

| RPC | Direction | Description |
|---|---|---|
| `DeployApp` | API → Scheduler | Orchestrate a new deployment |
| `RegisterWorker` | Agent → Scheduler | Join the cluster as a worker |
| `ReportMetrics` | Agent → Scheduler | Heartbeat with resource usage |
| `DeleteApp` | API → Scheduler | Permanently stop and remove a job |
| `GetAppStatus` | API → Scheduler | Retrieve real-time VM information |

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SCHEDULER_PORT` | `5002` | gRPC port |
| `METRICS_TTL_SECS` | `30` | Heartbeat timeout |
| `MAX_APPS_PER_HOST` | `10` | Resource isolation limit |
| `USE_TLS` | `false` | Enable mutual TLS |

## Development

```bash
# Run the scheduler
cargo run -p mikrom-scheduler

# Run tests
cargo test -p mikrom-scheduler
```

## Internal Architecture

```
src/
  server.rs          # Tonic implementation and request validation
  scheduler/         # Placement algorithms and state management
    ipam.rs          # IP Address Management (subnet-based)
  worker_registry.rs # Thread-safe store of cluster nodes
  metrics.rs         # Resource snapshots and scoring logic
  job.rs             # Job and JobStatus definitions
```
