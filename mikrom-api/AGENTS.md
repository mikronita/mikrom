# mikrom-api Agent Notes

## Scope

`mikrom-api` is the control plane API for Mikrom. It handles auth, application lifecycle, deployment orchestration, secrets, and Neon-backed database provisioning.
Database records also persist the PostgreSQL major version and expose it in the user-facing database responses.

## Current Runtime

- Axum HTTP API on port `5001`.
- PostgreSQL via SQLx.
- NATS for internal control plane events and integrations.
- Optional Neon provisioning through `NEON_*` environment variables.
- Polar billing uses a backend Organization Access Token; keep `POLAR_ACCESS_TOKEN` in the `mikrom-api` environment together with `POLAR_WEBHOOK_SECRET` and `POLAR_CHECKOUT_PRODUCT_ID`.
- New Neon databases default to PostgreSQL 16 unless the request specifies a different version.

## Test Expectations

- Unit tests use mocked repositories where possible.
- Repository tests use `TestDb` and therefore require a working PostgreSQL instance or a Dagger-provided test database.
- Several HTTP handler tests expect `NATS_URL` to be reachable or use the mock NATS client in `test_utils`.

## Useful Commands

```bash
make run-api
make test-cli
make test-integration
make ci-smoke
make ci-fast
make ci-full
```

## Notes

- Keep `mikrom-api` aligned with the workspace-level Dagger runner; do not duplicate CI logic here.
- When adding new database-backed tests, prefer `TestDb` from `src/test_utils.rs`.
- Changes to protobuf contracts must be regenerated through the normal Cargo build flow.
