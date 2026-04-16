# mikrom-cli

Command-line client for the [mikrom](https://github.com/antpard/mikrom) orchestration platform.

## Installation

**From source (Cargo):**

```bash
cargo install --path mikrom-cli
# installs `mikrom` to ~/.cargo/bin/
```

**From the workspace root:**

```bash
cargo build --release -p mikrom-cli
# binary at ./target/release/mikrom
```

**Docker:**

```bash
docker pull ghcr.io/antpard/mikrom/mikrom-cli:latest
docker run --rm ghcr.io/antpard/mikrom/mikrom-cli health
```

## Configuration

After `mikrom auth login`, the JWT token and API URL are stored in:

```
~/.config/mikrom/config.toml
```

Example:

```toml
api_url = "http://localhost:5001"
token = "eyJhbGciOiJIUzI1NiJ9..."
```

The API URL can also be set per-command with `--api-url` or the `MIKROM_API_URL` environment variable. Both override the config file.

## Commands

### `health`

Check that the API is reachable and print its version.

```bash
mikrom health
# status:  ok
# version: 0.1.0
```

### `auth register`

Create a new account.

```bash
mikrom auth register --email user@example.com --password secret123
# User registered successfully
# user_id: 550e8400-e29b-41d4-a716-446655440000
```

### `auth login`

Authenticate and save the session token to `~/.config/mikrom/config.toml`.

```bash
mikrom auth login --email user@example.com --password secret123
# Logged in. Token saved to ~/.config/mikrom/config.toml
```

### `deploy`

Deploy an application. The token saved by `auth login` is sent automatically.

```bash
# Minimal — defaults: 1 vCPU, 256 MiB RAM, 1024 MiB disk
mikrom deploy --app my-service --image nginx:latest

# Full options
mikrom deploy \
  --app my-service \
  --image nginx:1.25 \
  --vcpus 2 \
  --memory 512 \
  --disk 2048 \
  --env PORT=8080 \
  --env ENV=production

# job_id:  3f7a1b2c-...
# status:  Scheduled
# message: Application scheduled on worker
# host_id: worker-abc123
```

## Global flags

| Flag | Env var | Description |
|------|---------|-------------|
| `--api-url <URL>` | `MIKROM_API_URL` | Override the API base URL |

```bash
# Point at a remote API for a single command
mikrom --api-url https://api.example.com health

# Or export for the whole session
export MIKROM_API_URL=https://api.example.com
mikrom health
mikrom deploy --app svc --image alpine
```
