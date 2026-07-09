# mikrom-cli

`mikrom-cli` is the command-line client for Mikrom. It covers authentication, project switching, application management, deployment operations, and system health checks.

## Installation

```bash
cargo install --path mikrom-cli
```

## Output Formats

- Default output is human-friendly and interactive.
- Use `--output json` or `-o json` for scripting and automation.

## Core Areas

- **Authentication**: `mikrom auth register`, `mikrom auth login`, `mikrom auth whoami`, `mikrom auth update`
- **Projects**: `mikrom project list`, `mikrom project create`, `mikrom project switch`
- **Apps**: `mikrom app list`, `mikrom app create`, `mikrom app deploy`, `mikrom app activate`, `mikrom app deployments`, `mikrom app secret`, `mikrom app scale`, `mikrom app delete`
- **Deployments (Live Instances)**: `mikrom deployment list`, `mikrom deployment status`, `mikrom deployment stop`, `mikrom deployment pause`, `mikrom deployment resume`, `mikrom deployment delete`, `mikrom deployment snapshots`, `mikrom deployment snapshot-create`, `mikrom deployment snapshot-restore`, `mikrom deployment snapshot-delete`
- **Storage Volumes**: `mikrom volume list`, `mikrom volume create`, `mikrom volume attach`, `mikrom volume detach`, `mikrom volume snapshot`, `mikrom volume restore`, `mikrom volume snapshots list`, `mikrom volume snapshots delete`, `mikrom volume delete`
- **Databases**: `mikrom db list`, `mikrom db create`, `mikrom db info`, `mikrom db connection`, `mikrom db delete`, `mikrom db branches`, `mikrom db backup`, `mikrom db snapshots`, `mikrom db snapshot-create`, `mikrom db snapshot-restore`, `mikrom db snapshot-delete`
- **Personal Access Tokens (PATs)**: `mikrom pat list`, `mikrom pat create`, `mikrom pat revoke`
- **Notifications**: `mikrom notification list`, `mikrom notification read`, `mikrom notification read-all`
- **System**: `mikrom system health`
- **Config**: `mikrom config show`, `mikrom config set`

## Usage Notes

- `mikrom app deploy` supports the same CPU and memory presets used by the dashboard.
- `mikrom db create` accepts `--version` to choose the PostgreSQL major version, defaulting to `16`.
- `mikrom db list` and `mikrom db info` show the PostgreSQL major version alongside the rest of the database metadata.
- `mikrom db connection <database-id>` prints the SSH tunnel command and the `psql` command for a Neon-backed database.
- Configuration is stored under `~/.config/mikrom/config.toml`.
- The CLI is validated through the workspace Dagger profiles as well as its own Rust tests.

Timeout tuning:

- `MIKROM_REQUEST_TIMEOUT_SECS` default `30`
- `MIKROM_DELETE_TIMEOUT_SECS` default `120`
- `MIKROM_RESTORE_TIMEOUT_SECS` default `60`
- `MIKROM_LONG_TIMEOUT_SECS` default `30`

## Development

```bash
cargo run -p mikrom-cli -- --help
cargo nextest run -p mikrom-cli
make ci-smoke
make ci-fast
```
