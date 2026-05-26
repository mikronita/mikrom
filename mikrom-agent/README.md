# mikrom-agent

The worker daemon for the Mikrom PaaS. It runs on every worker node and manages the full lifecycle of Firecracker microVMs. Built with NATS, [sysinfo](https://github.com/GuillaumeGomez/sysinfo), and deep Linux integration.

**Port:** NATS connection

## Key Responsibilities

- **Worker Registration**: Automatically registers with `mikrom-scheduler` on startup and maintains a heartbeat.
- **Resource Monitoring**: Reports real-time CPU, RAM, and Disk metrics to the scheduler for placement and to `mikrom-telemetry` for observability.
- **Log Streaming**: Captures stdout/stderr from every microVM and streams it via NATS to the observability stack.
- **Image Conversion**: Converts standard OCI/Docker images into Firecracker-compatible `.ext4` root filesystems on the fly.
- **MicroVM Management**: Handles Firecracker process orchestration, Jailer isolation, and TAP network interface configuration.
- **Robust Boot System**: Injects a custom multi-stage boot sequence into microVMs to handle complex PaaS application entrypoints.

## Telemetry Stream

The agent produces several real-time data streams via NATS:

| Subject | Description |
|---|---|
| `mikrom.agent.<id>.metrics` | Host-level system metrics (CPU, RAM, Disk, Load). |
| `mikrom.metrics.<app_id>.<vm_id>` | Individual microVM resource utilization. |
| `mikrom.logs.<app_id>.<vm_id>` | Buffered console logs from the application. |

## The Boot System

Mikrom-agent uses a sophisticated dual-script system to ensure applications start reliably:

1.  **`mikrom-init.sh`**: Acts as the `init` process (PID 1). It mounts essential Linux pseudo-filesystems (`/proc`, `/sys`, `/dev`, `/dev/shm`), configures the loopback interface, sets terminal dimensions for clean logs, and exports environment variables.
2.  **`app-run.sh`**: Encapsulates the application's exact command-line arguments (extracted from Docker metadata). The app payload is copied to `/app`, and execution is dropped to the `mikrom` user before launch so the workload does not run as root.

## Docker to Firecracker Flow

When a deployment is triggered:
1.  **Pull**: The agent pulls the OCI image from the registry.
2.  **Metadata**: Extracts `Entrypoint`, `CMD`, `WorkingDir`, and `Env` using `docker inspect`.
3.  **Convert**: Mounts an empty `.ext4` loop device and copies only the image `WORKDIR` into `/app`.
4.  **Inject**: Writes the custom boot scripts into the filesystem, runs the app as `mikrom`, and performs an explicit `sync`.
5.  **Launch**: Spawns the Firecracker process via `jailer` for maximum security.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | `nats://127.0.0.1:4222` | URL of the NATS server |
| `AGENT_HOST_ID` | random UUID | Stable identifier for this node; persisted under `data_path/host_id.txt` |
| `AGENT_HOSTNAME` | — | IP/Hostname advertised to the scheduler |
| `USE_TLS` | `false` | Enable mutual TLS for NATS |
| `ENABLE_TELEMETRY` | `true` | Enable OTLP export of logs, traces, and metrics to SigNoz |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://192.168.122.128:4317` | OTLP gRPC endpoint for SigNoz |

## Development

```bash
# Run the agent
cargo run -p mikrom-agent

# Run tests (requires sudo for some Firecracker operations)
cargo nextest run -p mikrom-agent
```

## Internal Architecture

```
src/
  server.rs        # NATS subscriber/handler and scheduler heartbeat
  builder.rs       # Image conversion logic (Docker -> ext4 + boot scripts)
  metrics.rs       # Host resource collection
  firecracker/     # Core VMM management
    manager.rs     # Firecracker lifecycle and Jailer orchestration
    config.rs      # VM configuration and status types
    process.rs     # Process management and log capturing
```
