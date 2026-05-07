# mikrom-api

The central management service for the Mikrom PaaS. It provides a REST API for authentication, application management, and deployment orchestration. Built with [Axum](https://github.com/tokio-rs/axum) and [SQLx](https://github.com/launchbakery/sqlx) on Tokio.

**Port:** `5001`

## Key Responsibilities

- **Authentication**: User registration, login (JWT), and profile management.
- **App Management**: CRUD operations for applications (Git URLs, hostnames, ports).
- **Deployment Orchestration**: Coordinating with `mikrom-builder` and `mikrom-scheduler` to turn source code into running microVMs.
- **Automatic TLS**: Managing ACME accounts and certificates for secure application ingress.
- **GitHub Integration**: Handling webhooks to trigger automatic builds on repository changes.
- **Secret Management**: Storing and injecting encrypted environment variables into deployments.
- **State Persistence**: Managing the PostgreSQL database for all system metadata.

## Endpoints

### Authentication
| Method | Path | Auth | Description |
|---|---|---|---|
| `POST` | `/auth/register` | — | Create a new user account |
| `POST` | `/auth/login` | — | Authenticate and receive a JWT |
| `GET` | `/auth/whoami` | JWT | Get current user info |

### Applications & Deployments
| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/apps` | JWT | List all applications |
| `POST` | `/apps` | JWT | Create a new application (Git URL required) |
| `DELETE` | `/apps/{id}` | JWT | Delete an application and all its deployments |
| `POST` | `/apps/{id}/deploy` | JWT | Trigger a new deployment for an application |
| `GET` | `/apps/{id}/secrets` | JWT | List application secrets |
| `POST` | `/apps/{id}/secrets` | JWT | Add or update an application secret |
| `GET` | `/deployments` | JWT | List all deployments |
| `GET` | `/deployments/{id}` | JWT | Get deployment status |
| `DELETE` | `/deployments/{id}` | JWT | Stop and remove a deployment |
| `GET` | `/deployments/{id}/logs` | JWT | Get real-time microVM logs (SSE) |

## Database Schema

Mikrom uses PostgreSQL to track the state of the cluster:
- **`users`**: Account information and credentials.
- **`apps`**: Project definitions (name, git repo, assigned hostname, GitHub config, Healthchecks).
- **`deployments`**: Historical and active deployment runs (image tags, job IDs, IP addresses).
- **`app_secrets`**: Encrypted environment variables for applications.
- **`acme_accounts`**: Credentials and state for Let's Encrypt certificate management.
- **`github_accounts`**: OAuth tokens and configurations for GitHub integration.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `JWT_SECRET` | — | Secret used to sign/verify JWT tokens |
| `NATS_URL` | `nats://127.0.0.1:4222` | URL of the NATS server |
| `USE_TLS` | `false` | Enable mutual TLS for NATS communication |

## Development

```bash
# Start PostgreSQL locally
make db-start

# Run the API with hot-reload
cd mikrom-api && cargo watch -x run

# Run tests
cargo nextest run
```

## Internal Architecture

```
src/
  auth/           # JWT, Bcrypt, and Auth handlers
  deploy/         # Application-centric deployment logic
  vms/            # Legacy VM-centric handlers (now mapping to deployments)
  repositories/   # Data access layer (Postgres implementation)
  models/         # App, Deployment, and User structs
  sync.rs         # Background task to sync VM IPs from scheduler to DB
```
