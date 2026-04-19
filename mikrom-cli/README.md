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

## VM management commands

### `vms`

List all VMs for the current user:

```bash
mikrom vms
# JOB_ID                               STATUS       APP_NAME        VM_ID                                IMAGE
# -------------------------------------------------------------------------------------------------------
# job-abc-123                         Running      my-app         vm-xyz                               nginx:latest
```

### `vm <job_id>`

Get detailed VM status:

```bash
mikrom vm job-abc-123
# job_id:       job-abc-123
# status:       Running
# host_id:      host-1
# vm_id:        vm-xyz
# scheduled_at: 1700000000
# started_at:   1700000005
```

### `stop <job_id>`

Stop a running VM:

```bash
mikrom stop job-abc-123
# Stopped job [job-abc-123]
# message: Application cancelled
```

### `logs <job_id>`

Stream VM logs in real-time (SSE):

```bash
mikrom logs job-abc-123
```

### `pause <job_id>`

Pause a running VM:

```bash
mikrom pause job-abc-123
# Paused job [job-abc-123]
```

### `resume <job_id>`

Resume a paused VM:

```bash
mikrom resume job-abc-123
# Resumed job [job-abc-123]
```

### `delete <job_id>`

Delete a VM from the registry:

```bash
mikrom delete job-abc-123
# Deleted job [job-abc-123]
```

### `restart <job_id>`

Restart a VM (stop then start):

```bash
mikrom restart job-abc-123
# Restarting job [job-abc-123]
```

### `metrics <job_id>`

Get VM resource metrics:

```bash
mikrom metrics job-abc-123
# VM: job-abc-123
# cpu_usage:    45.50%
# memory:     62.30%
# disk:      30.00%
# network_rx: 1024 bytes
# network_tx: 512 bytes
```

### `whoami`

Show current user info:

```bash
mikrom whoami
# user_id:   user-123
# email:    user@example.com
# created_at: 2024-01-01T00:00:00Z
```

### `config`

Show current configuration:

```bash
mikrom config
# api_url: http://localhost:5001
# token:  [configured]
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
