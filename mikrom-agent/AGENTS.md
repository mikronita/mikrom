# Mikrom Agent

This document provides operator-focused guidance for the `mikrom-agent` daemon.

## Overview

The agent runs on each compute node and is responsible for:
- Managing VMs via Firecracker and/or Cloud Hypervisor
- Receiving deployment commands from the scheduler over NATS
- Publishing system and VM metrics via NATS
- Maintaining WireGuard mesh networking
- Exposing health and Prometheus metrics over HTTP

## Architecture

```
┌─────────────────────────────────────────────┐
│                 mikrom-agent                 │
│  ┌──────────────┐  ┌──────────────────────┐  │
│  │ HTTP Server  │  │    NATS Client      │  │
│  │ :5002        │  │  (command listener)   │  │
│  │ /health      │  │  (heartbeat loop)   │  │
│  │ /metrics     │  │  (mesh listener)    │  │
│  └──────────────┘  └──────────────────────┘  │
│  ┌────────────────────────────────────────┐  │
│  │         Hypervisor Registry           │  │
│  │  ┌─────────────┐  ┌──────────────────┐  │  │
│  │  │ Firecracker │  │ Cloud Hypervisor │  │  │
│  │  │ Manager     │  │ Manager          │  │  │
│  │  └─────────────┘  └──────────────────┘  │  │
│  └────────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

## Configuration

All configuration is via environment variables (loaded from `.env` if present):

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | — | NATS server URL (e.g. `nats://127.0.0.1:4222`) |
| `HOST_ID` | random UUID | Unique identifier for this host |
| `DATA_PATH` | `/var/lib/mikrom-agent` | Directory for VM state, logs, and keys |
| `AGENT_HOSTNAME` | system hostname | Hostname advertised to scheduler |
| `AGENT_ADVERTISE_ADDRESS` | `AGENT_HOSTNAME` | IP/address for scheduler to reach this agent |
| `BRIDGE_IP` | `10.0.0.1/8` | IP of the `mikrom-br0` bridge |
| `CERTS_DIR` | `/certs/agent` | TLS certificate directory |
| `WIREGUARD_PORT` | — | WireGuard listen port (auto if unset) |
| `USE_TLS` | `false` | Enable mTLS for NATS |
| `HTTP_PORT` | `5002` | Port for health/metrics HTTP server |
| `MAX_VMS_PER_HOST` | `0` (unlimited) | Hard limit on concurrent VMs |

### Validation

On startup the agent validates:
- `NATS_URL` is a valid NATS endpoint
- `DATA_PATH` exists and is writable
- `HOST_ID` is non-empty
- Process has `CAP_NET_ADMIN` (warns only if missing)

## HTTP Endpoints

### `GET /health`

Returns `200 OK` with body `ok` if the agent is running.

Use this for Kubernetes liveness probes or load balancer health checks.

### `GET /metrics`

Returns Prometheus-compatible metrics:

```
mikrom_agent_up 1
mikrom_agent_hypervisors 2
mikrom_agent_vms_total{hypervisor="Firecracker"} 3
mikrom_agent_vms_running{hypervisor="Firecracker"} 2
mikrom_agent_vms_total{hypervisor="CloudHypervisor"} 1
mikrom_agent_vms_running{hypervisor="CloudHypervisor"} 1
```

## Graceful Shutdown

On `SIGTERM` or `SIGINT`:
1. Persist runtime state for all hypervisors (JSON state files)
2. Stop all running VMs via `stop_vm()`
3. Exit cleanly

In Kubernetes or systemd, set:
```ini
TimeoutStopSec=30
KillSignal=SIGTERM
```

## Hypervisors

### Firecracker
- Lighter weight, AWS microvm
- HTTP API over Unix socket
- Default when hypervisor is `Unspecified` (0)

### Cloud Hypervisor
- Modern VMM written in Rust
- HTTP API over Unix socket
- Supports snapshots, live migration, hot-plug, ballooning, serial console
- Selected when hypervisor = 3 in `StartVmRequest`

## Troubleshooting

### Agent fails to start with "Data path is not writable"
Ensure the `DATA_PATH` directory exists and the agent process has write permissions:
```bash
mkdir -p /var/lib/mikrom-agent
chown mikrom:mikrom /var/lib/mikrom-agent
```

### "Process lacks CAP_NET_ADMIN"
The agent needs `CAP_NET_ADMIN` to create bridges and TAP interfaces. In Docker/Kubernetes:
```yaml
securityContext:
  capabilities:
    add: ["NET_ADMIN"]
```

### NATS reconnection loop
Check the agent logs for backoff messages. If it says "circuit breaker open", the agent has failed to connect 10 times and is cooling down for 5 minutes. Verify `NATS_URL` and network connectivity.

### VM starts but health checks fail
Check `GET /metrics` to see if the VM is in `Running` state. Check the serial console log at `<DATA_PATH>/<vm_id>.stdout.log` (Firecracker) or `<DATA_PATH>/<vm_id>.log` (Cloud Hypervisor).

### Firecracker-specific issues

#### "Failed to create Firecracker socket" or "Socket already exists"
Firecracker uses a Unix domain socket for API control. If a previous agent restart left a stale socket, startup will fail. The agent auto-cleans stale sockets on startup, but if you see this error manually remove it:
```bash
rm /var/lib/mikrom-agent/<agent_id>/<vm_id>/firecracker.socket
```

#### Jailer chroot not cleaned up
When `USE_JAILER=true`, Firecracker runs inside a chroot. If the agent crashes, the chroot directory may be left behind. The agent detects and cleans stale chroots during GC, but you can force removal:
```bash
rm -rf /var/lib/mikrom-agent/jailer/<vm_id>
```

#### IPv6 host route missing
If a VM has an IPv6 address but cannot reach the gateway, check that the host route was added to `mikrom-br0`:
```bash
ip -6 route show fd40:b90d:fcaa:ac99::/64
```
If missing, restart the VM or add it manually:
```bash
ip -6 route add fd40:b90d:fcaa:ac99::/64 dev mikrom-br0
```

#### Firecracker process zombie after SIGKILL
If `kill_process` escalates to `SIGKILL` and the process becomes a zombie, the agent's GC loop will reap it automatically. If zombies persist, check the parent init system (systemd, Docker init, etc.).

#### Snapshot resume fails
If a VM was paused with `pause_vm` and `resume_vm` fails with "snapshot not found", verify the snapshot files exist:
```bash
ls /var/lib/mikrom-agent/<agent_id>/<vm_id>/snapshots/
```
If the files are missing, the VM will start fresh from the base rootfs instead of resuming.
