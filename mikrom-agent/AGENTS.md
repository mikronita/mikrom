# mikrom-agent Agent Notes

## Scope

`mikrom-agent` runs on each worker node and manages microVM lifecycle, host metrics, log streaming, and network integration.

## Current Runtime

- NATS-based command/control plane.
- Firecracker and Cloud Hypervisor support.
- WireGuard mesh coordination through `mikrom-network`.
- eBPF-related workspace support via `mikrom-agent-ebpf` and `mikrom-agent-ebpf-common`.

## Key Files

- `src/main.rs`: process bootstrap and runtime wiring.
- `src/agent.rs`: VM orchestration and scheduler integration.
- `src/metrics.rs`: host metric collection.
- `src/ebpf/`: agent-side integration for the compiled eBPF payload.
- `build.rs`: embeds the eBPF artifact from `target/bpfel-unknown-none/release`.

## Useful Commands

```bash
make run-agent
make test-cli
make ci-smoke
make ci-fast
make ci-full
```

## Notes

- Keep the eBPF build path in sync with the Dagger runner, which currently uses nightly plus `-Z build-std=core`.
- Prefer the workspace caches and local `Makefile` targets over ad-hoc build commands.
- When updating VM lifecycle behavior, verify the worker-side integration with the scheduler and the data plane together.
- Runtime timeouts are configurable via environment variables. See `README.md` for the current `AGENT_*` and `FC_*` timeout list and defaults, including `FC_SOCKET_WAIT_TIMEOUT_SECS` defaulting to 120s for the non-jailer boot path.
- VM state is persisted to disk using atomic rename operations (writing first to `.json.tmp` files) to ensure robustness against crashes.
- Cloud Hypervisor process recovery stub leaks are fixed. Recovered CH processes are cleanly killed at the OS level (via libc SIGTERM/SIGKILL) since they have no Tokio child handle.
- Use `make ci-external-tests` for the ignored NATS integration suite. Keep Ceph tests opt-in with `MIKROM_RUN_CEPH_TESTS=1` and out of the default CI path unless the runner has a Ceph cluster.
