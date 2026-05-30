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
- **Abuse Protection**: Request rate limiting with separate policies for public, authenticated, and streaming endpoints.
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
| `ENABLE_TELEMETRY` | `true` | Enable OTLP export of logs, traces, and metrics to SigNoz |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://192.168.122.128:4317` | OTLP gRPC endpoint for SigNoz |
| `DEPLOYMENT_ENV` | `development` | Selects the default rate-limit profile: `development`, `staging`, or `production` |
| `RATE_LIMIT_PUBLIC_RPM` | profile default | Requests per minute for unauthenticated public endpoints |
| `RATE_LIMIT_AUTH_LOGIN_RPM` | profile default | Requests per minute for `/v1/auth/login` |
| `RATE_LIMIT_AUTH_REGISTER_RPM` | profile default | Requests per minute for `/v1/auth/register` |
| `RATE_LIMIT_GITHUB_INSTALL_RPM` | profile default | Requests per minute for `/v1/github/install` |
| `RATE_LIMIT_APPS_CREATE_RPM` | profile default | Requests per minute for `POST /v1/apps` |
| `RATE_LIMIT_APPS_DEPLOY_RPM` | profile default | Requests per minute for `POST /v1/apps/:app_name/deploy` |
| `RATE_LIMIT_WEBHOOKS_GITHUB_GENERIC_RPM` | profile default | Requests per minute for `POST /v1/webhooks/github` |
| `RATE_LIMIT_WEBHOOKS_GITHUB_NAMED_RPM` | profile default | Requests per minute for `POST /v1/webhooks/github/:app_name` |
| `RATE_LIMIT_AUTHENTICATED_READ_RPM` | profile default | Requests per minute for authenticated read endpoints |
| `RATE_LIMIT_AUTHENTICATED_WRITE_RPM` | profile default | Requests per minute for authenticated write endpoints |
| `RATE_LIMIT_AUTHENTICATED_STREAM_RPM` | profile default | Stream openings per minute for SSE/log endpoints |
| `RATE_LIMIT_ENTRY_TTL_SECS` | `900` | Idle time before rate-limit buckets are evicted |
| `RATE_LIMIT_CLEANUP_INTERVAL_SECS` | `60` | How often the in-memory store scans for stale buckets |
| `RATE_LIMIT_TRUST_PROXY_HEADERS` | `false` | Trust `X-Forwarded-For` or `X-Real-IP` for client identity |
| `NEON_PAGESERVER_URL` | - | Pageserver base URL for Neon database provisioning |
| `NEON_SAFEKEEPER_HTTP_URL` | - | Safekeeper management API URL used to register timelines |
| `NEON_BEARER_TOKEN` | - | Bearer token used when talking to the pageserver |
| `NEON_SAFEKEEPER_TOKEN` | - | Bearer token with `SafekeeperData` scope used to register safekeeper timelines |
| `NEON_JWKS_JSON` | - | Inline JWKS JSON injected into database VMs |
| `NEON_JWKS_PATH` | - | Path on the API host to a JWKS JSON file that will be read and injected into database VMs |
| `NEON_INSTANCE_ID` | - | Stable compute instance ID written into the VM config |
| `MIKROM_NEON_DEV_MODE` | `true` | If `false`, `mikrom-init` omits `--dev` for database VMs |
| `NEON_CONFIGURE_PRIVATE_KEY_PEM` | - | Inline RSA private key used by `mikrom-api` to mint the configure JWT |
| `NEON_CONFIGURE_PRIVATE_KEY_PATH` | - | RSA private key path used by `mikrom-api` to mint the configure JWT |
| `NEON_CONFIGURE_TOKEN` | - | Optional override token if you want to supply a prebuilt JWT |

### Recommended profiles

| `DEPLOYMENT_ENV` | Public | Login | Register | GitHub Install | App Create | App Deploy | Webhooks | Read | Write | Streams |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `development` | `300` | `120` | `60` | `120` | `120` | `120` | `240` | `1200` | `600` | `120` |
| `staging` | `120` | `20` | `20` | `30` | `20` | `30` | `30` / `60` | `600` | `240` | `60` |
| `production` | `60` | `10` | `10` | `15` | `10` | `20` | `20` / `30` | `300` | `120` | `30` |

The `webhooks` column shows generic/named webhook limits.

For local deployment, use `.env.staging` and `.env.production` as ready-made templates and adjust secrets before using them in real environments.

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
