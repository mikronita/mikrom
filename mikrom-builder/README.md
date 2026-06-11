# mikrom-builder

`mikrom-builder` is the build service for Mikrom. It turns Git repositories into OCI images and publishes the result to the configured registry.

**Port:** NATS connection

## Stack

- Rust
- Tokio
- NATS
- BuildKit
- Dockerfile fallback builds
- Railpack for source-driven builds when no Dockerfile is present

## Responsibilities

- Clone source repositories into a temporary workspace.
- Detect whether a repository should be built with a Dockerfile or Railpack.
- Push images to the configured OCI registry.
- Track build state and metrics over NATS.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | `nats://localhost:4222` | NATS server URL |
| `REGISTRY` | `registry.mikrom.spluca.org/mikrom` | Target OCI registry |
| `REGISTRY_URL` | alias for `REGISTRY` | Backward-compatible environment name |
| `REGISTRY_USER` | - | Registry username |
| `REGISTRY_PASS` | - | Registry password |
| `MAX_CONCURRENT_BUILDS` | `2` | Number of concurrent builds |
| `BUILD_STATE_TTL_SECS` | `3600` | Retention window for build state |
| `BUILD_STATE_PATH` | `/tmp/mikrom-builder-state.json` | Persistent build state file |
| `BUILDKIT_HOST` | `docker-container://mikromrust-buildkit-1` | Build backend |
| `ENABLE_TELEMETRY` | `true` | Enable OTLP export |
| `DT_API_URL` | `http://192.168.122.128:4318/api/v2/otlp` | Dynatrace OTLP base URL |
| `DT_API_TOKEN` | - | Dynatrace API token for OTLP export |

## Development

```bash
cargo run -p mikrom-builder
cargo nextest run -p mikrom-builder
make ci-smoke
make ci-fast
```

## Notes

- Build metrics are exposed through the `mikrom.builder.get_metrics` NATS request.
- The service still supports both authenticated and anonymous registry pushes depending on the environment.
- The local builder state file survives process restarts.
- The ignored NATS integration tests are exercised by `make ci-external-tests`.
