# mikrom-router

`mikrom-router` is the ingress router for Mikrom. It runs on Pingora and handles traffic routing, ACME/TLS state, health checks, and control-plane synchronization.

## Stack

- Rust 2024
- Pingora
- NATS
- PostgreSQL
- WireGuard integration
- OpenTelemetry

## Responsibilities

- Route inbound traffic to app microVMs.
- Maintain router state and ACME challenge data.
- Synchronize control-plane updates over NATS.
- Use PostgreSQL for persisted routing and certificate state.
- Expose health endpoints for startup and readiness checks.

## Runtime Requirements

- PostgreSQL for router state.
- NATS for control-plane and traffic-plane coordination.
- WireGuard and `CAP_NET_ADMIN` for mesh operations.
- Optional OTLP endpoint for tracing and metrics.

## Configuration

The router loads configuration from the environment and validates the important fields on startup.

Common variables:

- `DATABASE_URL`
- `NATS_URL`
- `NATS_USE_TLS`
- `NATS_CERTS_DIR` or `CERTS_DIR`
- `UPSTREAM_CA_CERTS_DIR`
- `MASTER_KEY`
- `ROUTER_ID`
- `ADVERTISE_ADDRESS`
- `DATA_DIR`
- `STATE_CACHE_PATH`
- `WIREGUARD_PORT`
- `ACME_STAGING`
- `DEFAULT_SITE_HOST`
- `DEFAULT_SITE_REDIRECT_URL`
- `API_UPSTREAM_TARGETS`
- `WEB_UPSTREAM_TARGETS`
- `RPS_LIMIT`
- `ROUTER_THREADS`

Timeout tuning:

- `STARTUP_CONNECT_TIMEOUT_SECS` default `5`
- `DOWNSTREAM_REQUEST_TIMEOUT_SECS` default `10`
- `DOWNSTREAM_RESPONSE_TIMEOUT_SECS` default `30`
- `UPSTREAM_CONNECT_TIMEOUT_SECS` default `5`
- `UPSTREAM_READ_TIMEOUT_SECS` default `30`
- `UPSTREAM_WRITE_TIMEOUT_SECS` default `30`
- `UPSTREAM_IDLE_TIMEOUT_SECS` default `60`
- `ROUTE_WAIT_TIMEOUT_SECS` default `30`

The packaged default configuration redirects `debaser.spluca.org` to `https://spluca.org/` with a temporary redirect. For that to work over TLS, the API-side ACME flow must issue a Let's Encrypt production certificate for `debaser.spluca.org` and the router must load it from `tls_certificates`.

The router also treats `api.mikrom.spluca.org` as a built-in upstream and proxies it to `mikrom-api` on port `5001`. The API service must still obtain a TLS certificate for that hostname through the ACME worker.
In the packaged environment, `API_UPSTREAM_TARGETS=[fd00::3bc2:7b88:289:62e6]:5001`.
Set `API_UPSTREAM_TARGETS` if the API listens somewhere else in your deployment.

The router also treats `mikrom.spluca.org` as a built-in upstream and proxies it to the app frontend on port `5173`. The packaged environment uses `WEB_UPSTREAM_TARGETS=[fd00::3bc2:7b88:289:62e6]:5173`, and the ACME worker must issue a certificate for `mikrom.spluca.org` as well.

## Health Endpoints

- `GET /health/live`
- `GET /health/ready`
- `GET /health/deps`
- `GET /health/control-plane`

## Development

```bash
cargo test -p mikrom-router
cargo clippy -p mikrom-router --all-targets
make ci-smoke
make ci-fast
make ci-full
```

## Test Database

- Integration tests that need PostgreSQL use `TestDb` from `src/test_utils.rs`.
- The helper creates an ephemeral database per test binary, runs migrations, and drops it on teardown.
- It defaults to `postgres://mikrom:mikrom_password@localhost:5432/mikrom_router_test` when `TEST_DATABASE_URL` is unset.
- The helper rejects non-test database names, so development `DATABASE_URL` values are not reused.
