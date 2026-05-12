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
- `mikrom config show`: View active CLI settings.

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
| `mikrom app deploy` | Trigger a new build and deployment for an app. |
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
| `mikrom config set` | Set a configuration value (e.g., api-url). |

## Advanced Usage

### Override API URL
You can point the CLI at a specific Mikrom cluster with the config command:

```bash
mikrom config set api-url https://mikrom.production.es
```

### Scripting & Automation
Use `--output json` for machine-readable output in CI/CD pipelines and administrative scripts.
