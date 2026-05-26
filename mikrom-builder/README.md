# mikrom-builder

The automated build engine for the Mikrom PaaS. It turns Git repositories into OCI images ready for deployment.

**Port:** NATS connection

## Key Responsibilities

- **Repository Orchestration**: Clones source code from Git providers into secure temporary directories.
- **Zero-Config Building**: Utilizes [Railpack](https://railpack.com/) to automatically detect languages, install dependencies, and build production-ready images.
- **Dockerfile Support**: Fallback to standard `docker build` if a custom `Dockerfile` is present in the repository root.
- **Registry Integration**: Automatically authenticates with and pushes built images to the Mikrom private OCI registry.

## Build Pipeline

1.  **Clone**: Fetches the specified Git URL and branch/commit.
2.  **Analyze**: Checks for a `Dockerfile`. If missing, hands over to Railpack.
3.  **Build**: 
    - **Railpack**: Fast, cache-efficient building using industry-standard patterns.
    - **Docker**: Custom builds for specialized applications.
4.  **Push**: Pushes the resulting image to `<registry>/<app_name>:<tag>`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | `nats://localhost:4222` | URL of the NATS server |
| `REGISTRY_URL` | `localhost:5000` | Target OCI registry |
| `REGISTRY_USER` | — | Registry username |
| `REGISTRY_PASS` | — | Registry password |
| `MAX_CONCURRENT_BUILDS` | `2` | Maximum builds processed in parallel |
| `BUILD_STATE_TTL_SECS` | `3600` | Retention window for finished build state |
| `BUILD_STATE_PATH` | `/tmp/mikrom-builder-state.json` | Persistent build status store |
| `BUILDKIT_HOST` | `docker-container://mikromrust-buildkit-1` | Build backend (local Docker or buildkitd) |
| `ENABLE_TELEMETRY` | `true` | Enable OTLP export of logs, traces, and metrics to SigNoz |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://192.168.122.128:4317` | OTLP gRPC endpoint for SigNoz |

Operational notes:

- Build status survives process restarts through the local state file.
- Builds can be cancelled cleanly on shutdown.
- Git URLs, image names and tags are validated before a build starts.
- `mikrom.builder.get_metrics` returns a protobuf `GetBuildMetricsResponse` snapshot.
- The protobuf API now exposes `GetBuildMetrics` with `BuildMetrics` and per-build event logs.

## Development

```bash
# Run the builder
cargo run -p mikrom-builder

# Install prerequisites
curl -sSL https://railpack.com/install.sh | sh
```

## Internal Architecture

```
  src/
    main.rs      # Configuration and NATS setup
    server.rs    # NATS subscriber and build state management
    builder.rs   # Core build logic (Git -> workspace -> Railpack/Docker -> Registry)
    config.rs    # Environment-based configuration
```
