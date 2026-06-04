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
- Database records persist the PostgreSQL major version and expose it back through list, detail, and create responses.

## Runtime Notes

- Uses PostgreSQL as the system of record.
- Uses NATS for scheduler and worker coordination.
- Supports optional Neon configuration through `NEON_*` environment variables.
- Defaults new Neon databases to PostgreSQL 16 unless the caller selects another supported major version.
- Uses Let's Encrypt production by default for ACME unless `ACME_STAGING=true` is set explicitly.
- Tracks the router's default redirect certificate for `debaser.spluca.org` through the same ACME worker, but the TLS storage tables themselves remain owned by `mikrom-router`.
- Stores the desired ACME mode and one-shot reissue flag for managed hostnames in `acme_managed_domains`.
- The runtime Docker image sets `ACME_STAGING=false` and `ROUTER_TLS_HOSTNAME=debaser.spluca.org` by default.
- Local repository tests use `TestDb` and expect PostgreSQL to be available.

Common environment variables:

- `DATABASE_URL`
- `NATS_URL`
- `JWT_SECRET`
- `MASTER_KEY`
- `ACME_EMAIL`
- `ACME_STAGING`
- `ROUTER_TLS_HOSTNAME`
- `ROUTER_ADDR`

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
