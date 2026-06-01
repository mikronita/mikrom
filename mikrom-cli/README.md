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

- Authentication: `mikrom auth register`, `mikrom auth login`, `mikrom auth whoami`
- Projects: `mikrom project list`, `mikrom project create`, `mikrom project switch`
- Apps: `mikrom app list`, `mikrom app create`, `mikrom app deploy`, `mikrom app deployments`, `mikrom app watch`, `mikrom app secret`, `mikrom app delete`
- Deployments: `mikrom deployment list`, `mikrom deployment status`, `mikrom deployment logs`, `mikrom deployment stop`, `mikrom deployment pause`, `mikrom deployment resume`, `mikrom deployment watch`, `mikrom deployment delete`
- System: `mikrom system health`, `mikrom system watch`, `mikrom config show`, `mikrom config set`

## Usage Notes

- `mikrom app deploy` supports the same CPU and memory presets used by the dashboard.
- Configuration is stored under `~/.config/mikrom/config.toml`.
- The CLI is validated through the workspace Dagger profiles as well as its own Rust tests.

## Development

```bash
cargo run -p mikrom-cli -- --help
cargo nextest run -p mikrom-cli
make ci-smoke
make ci-fast
```
