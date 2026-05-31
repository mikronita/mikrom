# Architecture and Zone Specification: `mikrom-dns`

This document details the zone structure and technical design of the native **Mikrom** DNS server (`mikrom-dns`), utilizing the private `.internal` TLD for dynamic discovery and internal routing of MicroVMs and control plane components.

---

## 1. TLD and Zone Hierarchy

To avoid network collisions and ensure strict isolation between platform traffic, host infrastructure, and ephemeral customer resources, the private root domain **`mikrom.internal.`** is established.

Name resolution is segmented into three independent subzones:

                              [ mikrom.internal ]
                                       │
     ┌─────────────────────────────────┼─────────────────────────────────┐
     ▼                                 ▼                                 ▼

[ s.mikrom.internal ]           [ n.mikrom.internal ]          [ u.mikrom.internal ]
(Control Plane/Core)            (WireGuard Mesh/Hosts)         (Customer Resources/VMs)


### A. Core Infrastructure Zone (`s.mikrom.internal.`)
Reserved exclusively for Mikrom's global control plane components. Enables inter-service communication agnostic to the host's physical IP.
* `nats.s.mikrom.internal` &rarr; Asynchronous messaging cluster.
* `api.s.mikrom.internal` &rarr; API Gateway and central management plane.
* `scheduler.s.mikrom.internal` &rarr; Dynamic load orchestrator.

### B. Overlay Network Zone (`n.mikrom.internal.`)
Maps fixed addresses of the WireGuard mesh topology (`fd00::/64`). Facilita resolution and identification of physical servers (*metal workers*) and edge proxies.
* `prod-neon.n.mikrom.internal` &rarr; `fd00::deed:1d1c`
* `worker-01.n.mikrom.internal` &rarr; Internal IP of the secondary physical compute node.

### C. User Resource Zone (`u.mikrom.internal.`)
High-performance zone responsible for resolving ephemeral IPv6 IPs (`fdac:5111:...`) assigned to MicroVMs. To guarantee a secure multitenant environment and avoid name collisions between customers, the hierarchy injects a unique Tenant identifier (or its short hash):

* **Resolution Pattern:** `<resource-name>.<tenant-short-id>.u.mikrom.internal.`
* **Practical Example:** `customer-db.43afce.u.mikrom.internal` &rarr; `AAAA fdac:5111:a310:e0bd::1`

---

## 2. Technology Stack in Rust

The server is built on the Rust asynchronous ecosystem, using production-grade components to ensure resolution latencies in the microsecond range:

* **`tokio`**: Asynchronous runtime for handling non-blocking I/O over UDP/TCP sockets.
* **`hickory-server`**: Modular engine for implementing the `RequestHandler` trait.
* **`dashmap`**: Concurrent hash map for storing in-memory route tables with cell-level optimized locking (avoiding thread contention).

### `Cargo.toml` Configuration (Reference)
```toml
[package]
name = "mikrom-dns"
version = "0.1.0"
edition = "2024"

[dependencies]
tokio = { workspace = true }
hickory-server = "0.26"
hickory-proto = "0.26"
dashmap = { workspace = true }
async-trait = "0.1"
async-nats = { workspace = true }
mikrom-proto = { path = "../mikrom-proto" }
prost = "0.13"
futures = "0.3"
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = { workspace = true }
```

---

## 3. DNS Server Implementation (Model)

The server implements a handler (`MikromDnsHandler`) that classifies incoming traffic using a subzone discriminator:
- **System**: `s.mikrom.internal.`
- **Network**: `n.mikrom.internal.`
- **User**: `u.mikrom.internal.`

Record storage is reactive, populated via NATS event subscriptions regarding MicroVM state.

External lookups are forwarded to the upstream resolvers configured in `UPSTREAM_DNS`.
The value accepts a comma-separated list and is tried in order, so the default fallback
chain can be `2606:4700:4700::1111,2001:4860:4860::8888`.

---

## 4. Ephemeral Operation Strategy

1. **Reactive NATS Subscription**: The `mikrom-dns` binary spawns a background thread (`tokio::spawn`) subscribed to the worker state subject. Each successful initialization message injects the IP mapping into the `DashMap`. Each MicroVM destruction event removes the key immediately.
2. **Critical Response TTL**: Since the lifecycle of containers and MicroVMs in Mikrom can be less than a minute, the TTL (Time to Live) configured in AAAA responses must remain static between 2 and 5 seconds. This mitigates the impact of client DNS caches during rapid data load rescheduling.
