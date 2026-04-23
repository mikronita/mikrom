# mikrom-agent

The worker daemon for the Mikrom PaaS. It runs on every worker node and manages the full lifecycle of Firecracker microVMs. Built with [Tonic](https://github.com/hyperium/tonic), [sysinfo](https://github.com/GuillaumeGomez/sysinfo), and deep Linux integration.

**Port:** `5003`

## Key Responsibilities

- **Worker Registration**: Automatically registers with `mikrom-scheduler` on startup and maintains a heartbeat.
- **Resource Monitoring**: Reports real-time CPU, RAM, and Disk metrics to the scheduler for intelligent placement.
- **Image Conversion**: Converts standard OCI/Docker images into Firecracker-compatible `.ext4` root filesystems on the fly.
- **MicroVM Management**: Handles Firecracker process orchestration, Jailer isolation, and TAP network interface configuration.
- **Robust Boot System**: Injects a custom multi-stage boot sequence into microVMs to handle complex PaaS application entrypoints.

## The Boot System

Mikrom-agent uses a sophisticated dual-script system to ensure applications start reliably:

1.  **`mikrom-init.sh`**: Acts as the `init` process (PID 1). It mounts essential Linux pseudo-filesystems (`/proc`, `/sys`, `/dev`, `/dev/shm`), configures the loopback interface, sets terminal dimensions for clean logs, and exports environment variables.
2.  **`app-run.sh`**: Encapsulates the application's exact command-line arguments (extracted from Docker metadata). Using strict JSON-based quoting and `exec`, it ensures that commands like `pnpm start` or `/bin/bash -c "..."` run with their original intent preserved.

## Docker to Firecracker Flow

When a deployment is triggered:
1.  **Pull**: The agent pulls the OCI image from the registry.
2.  **Metadata**: Extracts `Entrypoint`, `CMD`, `WorkingDir`, and `Env` using `docker inspect`.
3.  **Convert**: Mounts an empty `.ext4` loop device and uses `docker export` to populate it.
4.  **Inject**: Writes the custom boot scripts into the filesystem and performs an explicit `sync`.
5.  **Launch**: Spawns the Firecracker process via `jailer` for maximum security.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | gRPC address of the scheduler |
| `AGENT_PORT` | `5003` | Port the agent listens on |
| `HOST_ID` | random UUID | Stable identifier for this node |
| `AGENT_HOSTNAME` | — | IP/Hostname advertised to the scheduler |
| `USE_TLS` | `false` | Enable mutual TLS for gRPC |

## Development

```bash
# Run the agent
cargo run -p mikrom-agent

# Run tests (requires sudo for some Firecracker operations)
cargo test -p mikrom-agent
```

## Internal Architecture

```
src/
  server.rs        # Tonic gRPC server and scheduler heartbeat
  builder.rs       # Image conversion logic (Docker -> ext4 + boot scripts)
  metrics.rs       # Host resource collection
  firecracker/     # Core VMM management
    manager.rs     # Firecracker lifecycle and Jailer orchestration
    config.rs      # VM configuration and status types
    process.rs     # Process management and log capturing
```
