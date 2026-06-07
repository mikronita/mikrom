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
- Exposes Polar-backed billing endpoints for checkout, portal redirection, and webhook sync.
- Local repository tests use `TestDb` and expect PostgreSQL to be available.

Common environment variables:

- `DATABASE_URL`
- `NATS_URL`
- `NATS_REQUEST_TIMEOUT_SECS`
- `NATS_SCHEDULER_LONG_TIMEOUT_SECS`
- `NATS_SCHEDULER_DATABASE_TIMEOUT_SECS`
- `NATS_STORAGE_TIMEOUT_SECS`
- `JWT_SECRET`
- `MASTER_KEY`
- `ACME_EMAIL`
- `ACME_STAGING`
- `ROUTER_TLS_HOSTNAME`
- `ROUTER_ADDR`
- `POLAR_ACCESS_TOKEN`
- `POLAR_WEBHOOK_SECRET`
- `POLAR_CHECKOUT_PRODUCT_ID`
- `POLAR_API_BASE_URL` or `POLAR_SERVER`

Timeout defaults:

- `NATS_REQUEST_TIMEOUT_SECS`: `5`
- `NATS_SCHEDULER_LONG_TIMEOUT_SECS`: `15`
- `NATS_SCHEDULER_DATABASE_TIMEOUT_SECS`: `10`
- `NATS_STORAGE_TIMEOUT_SECS`: `30`

Rate limiting:

- Per-route RPM limits are optional. The API reads:
  - `RATE_LIMIT_PUBLIC_RPM`
  - `RATE_LIMIT_AUTH_LOGIN_RPM`
  - `RATE_LIMIT_AUTH_REGISTER_RPM`
  - `RATE_LIMIT_GITHUB_INSTALL_RPM`
  - `RATE_LIMIT_APPS_CREATE_RPM`
  - `RATE_LIMIT_APPS_DEPLOY_RPM`
  - `RATE_LIMIT_WEBHOOKS_GITHUB_GENERIC_RPM`
  - `RATE_LIMIT_WEBHOOKS_GITHUB_NAMED_RPM`
  - `RATE_LIMIT_AUTHENTICATED_READ_RPM`
  - `RATE_LIMIT_AUTHENTICATED_WRITE_RPM`
  - `RATE_LIMIT_AUTHENTICATED_STREAM_RPM`
- Shared tuning knobs:
  - `RATE_LIMIT_ENTRY_TTL_SECS` default `900`
  - `RATE_LIMIT_CLEANUP_INTERVAL_SECS` default `60`
  - `RATE_LIMIT_TRUST_PROXY_HEADERS` default `false`

Neon configuration:

- `NEON_PAGESERVER_URL`
- `NEON_SAFEKEEPER_HTTP_URL`
- `NEON_BEARER_TOKEN`
- `NEON_SAFEKEEPER_TOKEN`
- `NEON_JWKS_JSON`
- `NEON_JWKS_PATH`
- `NEON_INSTANCE_ID`
- `NEON_SAFEKEEPER_CONNSTRS`
- `MIKROM_NEON_DEV_MODE`
- `MIKROM_INIT_TRACE_FILES`
- `NEON_CONFIGURE_TOKEN`
- `NEON_CONFIGURE_PRIVATE_KEY_PEM`
- `NEON_CONFIGURE_PRIVATE_KEY_PATH`

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
