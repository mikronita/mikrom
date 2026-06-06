# mikrom-network

`mikrom-network` manages the WireGuard mesh and host identity coordination for Mikrom.

## Runtime Requirements

- `MIKROM_HOST_ID`
- `NATS_URL`

## Configuration

| Variable | Default | Description |
|---|---|---|
| `MIKROM_HOST_ID` | required | Host identity used by the network node |
| `NATS_URL` | `nats://[::1]:4222` | NATS server URL |
| `MIKROM_DATA_DIR` | `/var/lib/mikrom-network` | State directory for key material |
| `MIKROM_WG_PORT` | `51823` | WireGuard listen port |
| `MIKROM_ADVERTISE_ADDRESS` | - | Optional advertise address for the mesh |
| `MIKROM_NETWORK_NATS_CONNECT_TIMEOUT_SECS` | `5` | Timeout for the initial NATS connection |

## Development

```bash
cargo run -p mikrom-network
cargo nextest run -p mikrom-network
```
