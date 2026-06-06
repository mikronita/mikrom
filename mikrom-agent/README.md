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

### Timeout configuration

The agent now exposes runtime-configurable timeouts for NATS, Firecracker, and Cloud Hypervisor operations:

- `AGENT_NATS_CONNECT_TIMEOUT_SECS` default `5`
- `AGENT_NATS_MAX_BACKOFF_SECS` default `60`
- `AGENT_NATS_CIRCUIT_BREAKER_COOLDOWN_SECS` default `300`
- `AGENT_CLOUD_HYPERVISOR_SOCKET_WAIT_TIMEOUT_SECS` default `10`
- `AGENT_CLOUD_HYPERVISOR_API_CONNECT_TIMEOUT_SECS` default `5`
- `AGENT_CLOUD_HYPERVISOR_API_STATUS_TIMEOUT_SECS` default `30`
- `AGENT_CLOUD_HYPERVISOR_API_HEADER_TIMEOUT_SECS` default `10`
- `AGENT_CLOUD_HYPERVISOR_API_BODY_TIMEOUT_SECS` default `60`
- `AGENT_CLOUD_HYPERVISOR_CONFIGURE_CLIENT_TIMEOUT_SECS` default `5`
- `AGENT_CLOUD_HYPERVISOR_CONFIGURE_REQUEST_TIMEOUT_SECS` default `6`
- `AGENT_CLOUD_HYPERVISOR_CONFIGURE_BACKOFF_MAX_SECS` default `5`
- `FC_SOCKET_WAIT_TIMEOUT_SECS` default `120`
- `FC_SOCKET_WAIT_CHROOT_SECS` default `10`
- `FC_API_CONNECT_TIMEOUT_SECS` default `2`
- `FC_API_STATUS_TIMEOUT_SECS` default `30`
- `FC_API_HEADER_TIMEOUT_SECS` default `10`
- `FC_API_BODY_TIMEOUT_SECS` default `60`
- `FC_PROCESS_TERMINATE_TIMEOUT_SECS` default `10`
- `FC_PROCESS_KILL_TIMEOUT_SECS` default `2`
- `FC_VFS_TERMINATE_TIMEOUT_SECS` default `5`
- `FC_VFS_KILL_TIMEOUT_SECS` default `2`

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
