# mikrom-api

`mikrom-api` is the Mikrom control plane API. It exposes the management surface for authentication, projects, applications, deployments, secrets, GitHub webhooks, and Neon-backed PostgreSQL database provisioning.

**Port:** `5001`

## Stack

- Axum
- SQLx
- Tokio
- PostgreSQL
- NATS
- OpenTelemetry

## Responsibilities

- Authentication and profile management.
- Application lifecycle and deployment orchestration.
- Secret storage and retrieval for applications.
- GitHub webhook handling for automated deploys.
- Rate limiting and request classification.
- Database provisioning through Neon when configured, with PostgreSQL workloads running on the platform's Cloud Hypervisor-backed microVM path.

## Runtime Notes

- Uses PostgreSQL as the system of record.
- Uses NATS for scheduler and worker coordination.
- Supports optional Neon configuration through `NEON_*` environment variables.
- Local repository tests use `TestDb` and expect PostgreSQL to be available.

## Local Development

```bash
make run-api
make test-integration
make ci-smoke
make ci-fast
make ci-full
```

## Database-backed tests

- Repository tests and some handler tests use `TestDb` from `src/test_utils.rs`.
- The helper creates an ephemeral database per test binary, runs migrations, and drops it on teardown.
- It defaults to `postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test` when `TEST_DATABASE_URL` is unset.
- The helper rejects non-test database names, so development `DATABASE_URL` values are not reused.
