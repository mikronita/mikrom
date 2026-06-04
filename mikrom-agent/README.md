# mikrom-agent

`mikrom-agent` is the worker daemon that runs on each compute node. It manages microVM lifecycle, reports host metrics, streams logs, and integrates with the scheduler, network mesh, and eBPF data plane.

**Port:** `5003`

## Stack

- Tokio
- NATS
- Firecracker
- Cloud Hypervisor
- WireGuard integration through `mikrom-network`
- Host-wide NAT64 translation for IPv4 egress via `tundra-nat64`
- eBPF support through `mikrom-agent-ebpf`

## Responsibilities

- Worker registration and heartbeats.
- VM creation, stop, pause, resume, and cleanup.
- Host CPU, RAM, disk, and VM metrics.
- Log shipping for running workloads.
- Embedding and loading the compiled eBPF payload.

## Runtime Notes

- The agent expects access to the host networking stack.
- The agent relies on `mikrom-init` and the runtime boot scripts inside microVM images.
- The agent starts a singleton NAT64 translator on the host bridge and expects `mikrom-dns` to provide DNS64 answers for external names.
- Build output for the eBPF program is consumed from `target/bpfel-unknown-none/release/mikrom-agent-ebpf`.

## Local Development

```bash
make run-agent
make ci-smoke
make ci-fast
make ci-full
```

## Testing

- Prefer workspace-level CI profiles for the full agent + eBPF path.
- When making agent lifecycle changes, validate the worker with the scheduler and networking services together.
- For host and VM smoke validation of NAT64/DNS64, use [docs/nat64-dns64-smoke-checklist.md](/home/apardo/Work/mikrom.rust/docs/nat64-dns64-smoke-checklist.md).
