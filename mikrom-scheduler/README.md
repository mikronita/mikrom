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
