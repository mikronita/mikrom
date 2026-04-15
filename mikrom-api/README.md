# mikrom-api

HTTP REST API for the mikrom orchestration system. Built with [Axum](https://github.com/tokio-rs/axum) and [SQLx](https://github.com/launchbakery/sqlx) on Tokio.

**Port:** `5001`

## Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/health` | — | Service health and version |
| `POST` | `/auth/register` | — | Create a new user account |
| `POST` | `/auth/login` | — | Authenticate and receive a JWT |
| `POST` | `/deploy` | JWT | Deploy an application to a Firecracker VM |

### `POST /deploy`

```json
{
  "app_name": "my-app",
  "image": "nginx:latest",
  "vcpus": 1,
  "memory_mib": 256,
  "disk_mib": 1024,
  "env": { "PORT": "3000" }
}
```

`vcpus`, `memory_mib`, `disk_mib`, and `env` are optional (defaults: 1 vCPU, 256 MiB RAM, 1024 MiB disk).

On each request the handler opens a new gRPC connection to `mikrom-scheduler` (`SCHEDULER_ADDR`) and calls `DeployApp`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `JWT_SECRET` | — | Secret used to sign/verify JWT tokens |
| `SCHEDULER_ADDR` | `http://127.0.0.1:5002` | gRPC address of the scheduler |

Copy `.env.example` (if present) or set these variables in your shell / Docker environment.

## Development

```bash
# Start the database
docker-compose up -d postgres

# Run the service
cargo run

# Unit tests (no Docker)
cargo test --lib

# Integration tests (requires running PostgreSQL)
cargo test --test integration
```

The integration tests use `testcontainers` to spin up a real PostgreSQL instance; `TEST_DATABASE_URL` can override the default connection string (`postgres://mikrom:mikrom_password@localhost:5432/mikrom_api`).

## Code structure

```
src/
  main.rs                        Entry point — loads env, connects DB and scheduler, starts Axum
  lib.rs                         Router factory, AppState, health handler
  auth/
    handlers.rs                  /auth/register and /auth/login handlers
    jwt.rs                       JWT creation and validation
  deploy/
    mod.rs                       /deploy handler — proxies to scheduler via gRPC
  models/
    user.rs                      User model
  db/
    mod.rs                       Database pool initialisation
  repositories/
    user_repository.rs           UserRepository trait (mockable)
    postgres_user_repository.rs  SQLx implementation
```

Auth uses bcrypt for password hashing and `jsonwebtoken` for JWT creation. The `UserRepository` trait allows `mockall` mocks to be swapped in during unit tests.
