# mikrom-scheduler

`mikrom-scheduler` is the placement engine for Mikrom. It maintains worker state, scores candidates, and coordinates deployment state over NATS.

**Port:** NATS connection

## Key Responsibilities

- Worker registry and heartbeat tracking.
- Placement decisions based on capacity and recent metrics.
- Job lifecycle management for scheduled workloads.
- Coordination with `mikrom-api` and `mikrom-agent`.

## Notes

- The scheduler focuses on placement and worker state; IPv6 routing metadata lives in the control plane and worker/network services.
- Internal networking is handled through the IPv6-first control plane and the worker-side networking stack.
- Most changes should be validated through the workspace Dagger profiles, not by running this crate in isolation.

## Local Development

```bash
cargo run -p mikrom-scheduler
cargo nextest run -p mikrom-scheduler
make ci-smoke
make ci-fast
make ci-full
```

## Test Database

- Integration tests use `TEST_DATABASE_URL` and default to `postgres://mikrom:mikrom_password@localhost:5432/mikrom_scheduler_test` when it is unset.
- The helper creates an ephemeral database per test binary, runs migrations, and drops it on teardown.
- The helper rejects non-test database names, so `DATABASE_URL` from the development environment will not be used for scheduler tests.
- The ignored NATS and scheduler-e2e integration suites are covered by `make ci-external-tests`.

## Runtime Configuration

- `AGENT_REQUEST_TIMEOUT_SECS` controls the timeout used for scheduler requests to `mikrom-agent`. The default is `30`.
- `VM_CLEANUP_INTERVAL_SECS` controls how often the beta VM cleanup sweep runs. The default is `3600`.
- `VM_CLEANUP_TTL_SECS` controls how old a VM must be before the cleanup sweep deletes it. The default is `3600`.
- `BETA_DEPLOYMENT_CLEANUP_ENABLED` enables the beta deployment sweep that deletes every deployment and its VM on a fixed interval. Leave it `false` in production.
- `BETA_DEPLOYMENT_CLEANUP_INTERVAL_SECS` controls how often that beta deployment sweep runs. The default is `3600`.
