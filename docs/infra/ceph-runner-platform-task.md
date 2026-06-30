# Ceph Runner Platform Task

## Objective

Provision a dedicated self-hosted GitHub Actions runner for the Ceph-only `mikrom-agent` integration job.

## Scope

The runner must only carry Ceph validation traffic.

- GitHub Actions job: `ceph-tests`
- Local command: `make ci-ceph-tests`
- Test binary: `mikrom-agent/tests/ceph_integration_tests.rs`

## Acceptance Criteria

- The runner is registered with labels `self-hosted`, `linux`, and `ceph`.
- The runner host can read `/etc/ceph/ceph.conf` and `/etc/ceph/admin.secret`.
- The runner host can reach the Ceph cluster monitors.
- The workflow job `ceph-tests` runs successfully on `push` to `main`.
- The local command `make ci-ceph-tests` passes on the same host.

## Implementation Notes

- Use the example bootstrap script in [docs/infra/ceph-runner-bootstrap.example.sh](ceph-runner-bootstrap.example.sh) as a starting point.
- Keep the runner isolated from general-purpose workloads.
- Rotate Ceph secrets on the host before re-enabling the runner if credentials change.
- If the cluster is unavailable, disable the runner rather than letting the job fail repeatedly.

## Deliverables

- A registered runner with the correct labels.
- A documented onboarding procedure for operations.
- A verified dry run of the Ceph job on the runner host.
