# mikrom-telemetry

The observability service for the Mikrom PaaS. it collects, aggregates, and exports metrics and logs from the entire cluster using NATS as the backbone.

**Port:** `9090` (Prometheus Metrics)

## Key Responsibilities

- **Metrics Collection**: Subscribes to NATS subjects to receive real-time CPU, RAM, and Disk usage from both host nodes and individual microVMs.
- **Prometheus Exporter**: Serves a `/metrics` endpoint that exposes all collected data in a format compatible with Prometheus.
- **Log Aggregation**: Receives console logs from microVMs via NATS and forwards them to [Grafana Loki](https://grafana.com/oss/loki/) for long-term storage and querying.
- **Real-time Processing**: Efficiently handles high-volume telemetry streams using Tokio and asynchronous NATS.

## Metrics Architecture

The service exports several categories of metrics:
- **`mikrom_vm_*`**: Per-VM resource utilization (CPU, RAM).
- **`mikrom_sys_*`**: Host-level metrics (Load average, Disk usage, total memory).

## NATS Integration

| Subject | Source | Description |
|---|---|---|
| `mikrom.metrics.>` | Agent | Per-VM resource metrics |
| `mikrom.agent.*.metrics` | Agent | Host-level system metrics |
| `mikrom.logs.>` | Agent | Console logs from microVMs |

## Configuration

| Variable | Default | Description |
|---|---|---|
| `NATS_URL` | `nats://localhost:4222` | URL of the NATS server |
| `LOKI_URL` | `http://localhost:3100` | Target Grafana Loki API |
| `METRICS_PORT` | `9090` | Port for the Prometheus exporter |

## Development

```bash
# Run the telemetry service
cargo run -p mikrom-telemetry

# Check metrics locally
curl http://localhost:9090/metrics
```

## Internal Architecture

```
src/
  main.rs      # Configuration and service entrypoint
  service.rs   # Core logic: NATS subscribers and Prometheus registry
  loki.rs      # Loki client and payload formatting
```
