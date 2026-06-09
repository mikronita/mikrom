# Ceph Runner Checklist

Use this checklist when preparing the self-hosted GitHub Actions runner that executes the Ceph-only agent integration job.

For provisioning details, see [docs/infra/ceph-runner.md](/home/apardo/Work/mikrom.rust/docs/infra/ceph-runner.md).
For an automation starting point, see [docs/infra/ceph-runner-bootstrap.example.sh](/home/apardo/Work/mikrom.rust/docs/infra/ceph-runner-bootstrap.example.sh).
For the platform-facing acceptance criteria, see [docs/infra/ceph-runner-platform-task.md](/home/apardo/Work/mikrom.rust/docs/infra/ceph-runner-platform-task.md).

## 1. Runner Labels

Register the runner with these labels:

- `self-hosted`
- `linux`
- `ceph`

The workflow job lives in [`.github/workflows/ci.yml`](/home/apardo/Work/mikrom.rust/.github/workflows/ci.yml) as `ceph-tests`.

## 2. Host Prerequisites

On the runner host, ensure these paths exist and are readable by the runner process:

- `/etc/ceph/ceph.conf`
- `/etc/ceph/admin.secret`

The Ceph cluster referenced by those files must be reachable from the host.

## 3. What the Job Runs

The workflow job and the local target run the same command:

```bash
MIKROM_RUN_CEPH_TESTS=1 cargo test -p mikrom-agent --test ceph_integration_tests -- --ignored
```

The tests are ignored by default and only run when the environment variable is set.

## 4. Local Validation

Use the local target on a host that has Ceph available:

```bash
make ci-ceph-tests
```

Expected outcome:

- `test_ceph_rbd_lifecycle_native` passes.
- `test_ceph_restore_busy_image_failure` passes.
- The test binary reports both tests as `ignored` when `MIKROM_RUN_CEPH_TESTS` is not set.

## 5. Common Failures

- Missing `/etc/ceph/ceph.conf` or `/etc/ceph/admin.secret`: verify the runner host mount or configuration management.
- Permission denied when reading Ceph files: verify the runner user can read both files.
- Connection failures to Ceph: verify the host can reach the cluster monitors and that the Ceph config matches the deployed cluster.
