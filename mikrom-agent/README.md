# mikrom-agent

`mikrom-agent` is the worker daemon that runs on each compute node. It manages microVM lifecycle, reports host metrics, streams logs, and integrates with the scheduler, network mesh, and eBPF data plane.

**Port:** `5003`

## Stack

- Tokio
- NATS
- Firecracker
- Cloud Hypervisor
- WireGuard integration through `mikrom-network`
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
