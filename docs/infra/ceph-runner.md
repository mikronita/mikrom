# Ceph Runner Runbook

Use this runbook to provision and maintain the self-hosted GitHub Actions runner that executes the Ceph-only `mikrom-agent` integration job.

Related docs:

- [Runner bootstrap example](./ceph-runner-bootstrap.example.sh)
- [Platform task checklist](./ceph-runner-platform-task.md)
- [Operational checklist](/home/apardo/Work/mikrom.rust/docs/ceph-runner-checklist.md)

## Scope

The runner is dedicated to Ceph-backed validation only.

- GitHub Actions job: `ceph-tests`
- Local target: `make ci-ceph-tests`
- Test binary: `mikrom-agent/tests/ceph_integration_tests.rs`

## Required Labels

Register the runner with these labels:

- `self-hosted`
- `linux`
- `ceph`

Do not attach this label set to general-purpose runners.

## Host Requirements

The runner host must provide:

- Rust stable toolchain
- Cargo and the workspace build dependencies used by `mikrom-agent`
- Ceph client libraries and headers required by the agent build
- Read access to `/etc/ceph/ceph.conf`
- Read access to `/etc/ceph/admin.secret`
- Network reachability to the Ceph monitors used by the cluster

The Ceph files may come from local provisioning, a secret mount, or configuration management, but they must be present before the job starts.

## Registration

Provision the runner using the standard GitHub Actions self-hosted runner package or your existing fleet manager.

Recommended registration properties:

- Name the runner after the host role, for example `mikrom-ceph-runner-1`.
- Apply the `self-hosted`, `linux`, and `ceph` labels during registration.
- Keep the runner on a dedicated machine or VM with Ceph access.
- Avoid co-locating unrelated workloads on the same runner host.

## Verification

Before enabling the runner in GitHub Actions, verify the host:

```bash
test -r /etc/ceph/ceph.conf
test -r /etc/ceph/admin.secret
make ci-ceph-tests
```

Expected outcome:

- the Ceph integration tests compile successfully
- `test_ceph_rbd_lifecycle_native` passes
- `test_ceph_restore_busy_image_failure` passes

## Operating Notes

- The job intentionally runs only on `push` to `main`.
- The job is isolated from the normal Dagger-based CI path.
- If the Ceph cluster is unavailable, disable the runner rather than letting the job fail repeatedly.
- If secrets rotate, update both `/etc/ceph/ceph.conf` and `/etc/ceph/admin.secret` before re-enabling the runner.
