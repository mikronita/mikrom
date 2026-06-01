# mikrom-dns

`mikrom-dns` is the internal DNS service for Mikrom. It resolves platform services, worker nodes, and tenant resources under the `*.mikrom.internal.` namespace.

## Zones

- `s.mikrom.internal.` for core control-plane services.
- `n.mikrom.internal.` for network and worker identities.
- `u.mikrom.internal.` for customer resources and microVMs.

## Stack

- Rust 2024
- Hickory DNS
- DashMap
- NATS
- OpenTelemetry

## Runtime Behavior

- Answers are populated reactively from NATS events.
- Upstream lookups are forwarded to the comma-separated resolvers configured in `UPSTREAM_DNS`.
- The service supports optional `NATS_SYS_IP` for system-zone exposure.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | `nats://localhost:4222` | NATS server URL |
| `UPSTREAM_DNS` | `2606:4700:4700::1111,2001:4860:4860::8888` | Ordered upstream resolver list |
| `NATS_SYS_IP` | - | Optional IPv6 address for the system zone |
| `ENABLE_TELEMETRY` | `true` | Enable OTLP export |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://192.168.122.128:4317` | OTLP endpoint |

## Development

```bash
cargo run -p mikrom-dns
cargo nextest run -p mikrom-dns
make ci-smoke
make ci-fast
```

## Notes

- The service uses `dashmap` to keep zone state hot in memory.
- The current implementation is IPv6-first and models customer resources under the tenant-specific `u.mikrom.internal.` zone.
- Integration tests expect NATS to be reachable.
