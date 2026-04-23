# mikrom-builder

The automated build engine for the Mikrom PaaS. It turns Git repositories into optimized OCI images ready for deployment.

**Port:** `5004`

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
4.  **Push**: Pushes the resulting image to `registry.mikrom.es/mikrom/<app_name>:<tag>`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `BUILDER_PORT` | `5004` | gRPC port the builder listens on |
| `REGISTRY_URL` | `registry.mikrom.es` | Target OCI registry |
| `REGISTRY_USER` | — | Registry username |
| `REGISTRY_PASS` | — | Registry password |
| `BUILDKIT_HOST` | `docker-daemon://` | Build backend (local Docker or buildkitd) |

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
  main.rs      # Configuration and gRPC setup
  server.rs    # Tonic implementation of BuilderService
  builder.rs   # Core build logic (Git -> Railpack/Docker -> Registry)
  config.rs    # Environment-based configuration
```
