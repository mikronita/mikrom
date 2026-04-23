# mikrom-cli

The powerful command-line interface for the Mikrom PaaS. Control your applications, deployments, and cluster nodes from the comfort of your terminal.

## Installation

```bash
# From source
cargo install --path mikrom-cli

# Verify installation
mikrom health
```

## Configuration

Mikrom stores its configuration in `~/.config/mikrom/config.toml`. Use the following commands to manage your session:

- `mikrom auth register`: Create a new Mikrom account.
- `mikrom auth login`: Authenticate and save your JWT token.
- `mikrom whoami`: Check your current identity.
- `mikrom config`: View active CLI settings.

## Core Commands

### Application Management
| Command | Description |
|---|---|
| `mikrom apps list` | List all your registered applications. |
| `mikrom apps create` | Register a new app with a name and Git URL. |
| `mikrom apps deploy` | Trigger a new build and deployment for an app. |
| `mikrom apps delete` | Permanently remove an application. |

### Deployment & Instance Control
| Command | Description |
|---|---|
| `mikrom deployments` | List all active and historical deployments. |
| `mikrom status <id>` | Get real-time status of a specific instance. |
| `mikrom logs <id>` | Stream live console output from the microVM. |
| `mikrom stop <id>` | Gracefully terminate a running deployment. |
| `mikrom delete <id>` | Permanently remove a deployment record. |
| `mikrom metrics <id>` | View CPU, RAM, and Disk usage for an instance. |

## Advanced Usage

### Override API URL
You can point the CLI at a specific Mikrom cluster using the `--api-url` flag or the `MIKROM_API_URL` environment variable:

```bash
mikrom --api-url https://mikrom.production.es apps list
```

### Scripting & Automation
The CLI output is designed to be clean and predictable, making it suitable for CI/CD pipelines and administrative scripts.
