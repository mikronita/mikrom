# mikrom-cli

The powerful command-line interface for the Mikrom PaaS. Control your applications, deployments, and cluster nodes from the comfort of your terminal.

## Installation

```bash
# From source
cargo install --path mikrom-cli

# Verify installation
mikrom system health
```

## Configuration

Mikrom stores its configuration in `~/.config/mikrom/config.toml`. Use the following commands to manage your session:

- `mikrom auth register`: Create a new Mikrom account.
- `mikrom auth login`: Authenticate and save your JWT token.
- `mikrom auth whoami`: Check your current identity.
- `mikrom config show`: View active CLI settings, including the current project.

## Output formats

The CLI defaults to colorful tables with emojis for interactive use:

```bash
mikrom app list
```

For scripts and automation, use JSON:

```bash
mikrom --output json app list
mikrom -o json system health
```

## Core Commands

### Application Management
| Command | Description |
|---|---|
| `mikrom app list` | List all your registered applications. |
| `mikrom app create` | Register a new app with a name and Git URL. |
| `mikrom app deploy` | Trigger a new build and deployment for an app. Supports `--cpu` (`1`-`4`) and `--memory` (`512M`, `1G`, `2G`, `4G`). |
| `mikrom app activate` | Rollback or activate a specific historical deployment. |
| `mikrom app deployments` | List deployment history for an application. |
| `mikrom app watch` | Stream build and deployment events for an app. |
| `mikrom app secret` | Show the GitHub webhook secret for an application. |
| `mikrom app delete` | Permanently remove an application. |

### Deployment & Instance Control
| Command | Description |
|---|---|
| `mikrom deployment list` | List all active deployments (jobs) across the cluster. |
| `mikrom deployment status` | Get detailed status of a specific instance. |
| `mikrom deployment logs` | Stream live console output from the microVM. |
| `mikrom deployment stop` | Gracefully terminate a running deployment. |
| `mikrom deployment pause` | Suspend the CPU of a running microVM. |
| `mikrom deployment resume` | Resume a paused microVM. |
| `mikrom deployment watch` | Stream all cluster-wide deployment events. |
| `mikrom deployment delete` | Remove a deployment record from history. |

### System & Configuration
| Command | Description |
|---|---|
| `mikrom system health` | Check the health of all system services. |
| `mikrom system watch` | Stream system health updates in real-time. |
| `mikrom config show` | View active CLI settings. |
| `mikrom config set` | Set a configuration value (e.g., `api-url`, `active-project`). |
| `mikrom project list` | List the projects you can access. |
| `mikrom project create` | Create a new project. |
| `mikrom project switch` | Switch the active project used by subsequent commands. |

## Advanced Usage

### Override API URL
You can point the CLI at a specific Mikrom cluster with the config command:

```bash
mikrom config set api-url https://mikrom.production.es
```

### Switch projects
`mikrom project switch` updates the active project stored in `~/.config/mikrom/config.toml` and scopes future requests to that project:

```bash
mikrom project list
mikrom project switch abc123
```

### Scripting & Automation
Use `--output json` for machine-readable output in CI/CD pipelines and administrative scripts.

### Deployment presets
`mikrom app deploy` accepts the same deployment presets in interactive and scripted use:

- CPU: `1`, `2`, `3`, `4`
- RAM: `512M`, `1G`, `2G`, `4G`

Examples:

```bash
mikrom app deploy --name my-app --cpu 2 --memory 1G
mikrom app deploy --name my-app
```

If you omit the flags, the CLI prompts for a preset and defaults to `1` CPU and `512M` RAM when you just press Enter.
