# mikrom-router

A high-performance dynamic ingress router for the Mikrom PaaS. It automatically maps incoming HTTP traffic to the correct microVM based on the `Host` header.

**Port:** `8080` (External)

## Key Responsibilities

- **Dynamic Routing**: Resolves hostnames (e.g., `whoami.apps.mikrom.spluca.org`) to internal microVM IP addresses in real-time.
- **Zero-Latency Lookups**: Utilizes an in-memory [Moka](https://github.com/moka-rs/moka) cache to avoid database bottlenecks on every request.
- **Reverse Proxying**: Proxies full HTTP requests (including headers, paths, and bodies) to backend applications using [Hyper](https://github.com/hyperium/hyper).
- **Auto-Discovery**: Queries the `apps` and `deployments` tables to find the latest `RUNNING` instance for any given host.

## How it works

1.  **Request**: An HTTP request arrives at the router.
2.  **Normalization**: The router extracts the `Host` header and removes any port number.
3.  **Cache Check**: Looks for a mapping in the local cache (60s TTL).
4.  **Database Lookup**: If missing, it queries PostgreSQL:
    ```sql
    SELECT a.port, d.ip_address 
    FROM apps a JOIN deployments d ON a.id = d.app_id 
    WHERE a.hostname = $1 AND d.status = 'RUNNING' 
    ORDER BY d.created_at DESC LIMIT 1
    ```
5.  **Proxy**: Forwards the request to `http://<vm_ip>:<app_port>` and returns the response.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `ROUTER_PORT` | `8080` | Port the router listens on |
| `DATABASE_URL` | — | PostgreSQL connection string |
| `LOG_LEVEL` | `info` | Filter for system logs |

## Development

```bash
# Run the router
cargo run -p mikrom-router

# Test a route (assuming app is running)
curl -H "Host: my-app.apps.mikrom.spluca.org" http://localhost:8080
```

## Internal Architecture

```
src/
  main.rs      # Axum router, proxy handler, and DB resolution logic
  config.rs    # Configuration management
```
