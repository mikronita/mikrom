# mikrom-router-caddy

Custom Caddy build for Mikrom with a native Go plugin for dynamic routing and ACME challenge handling.

## Features

- **Dynamic Routing**: Automatically syncs routes from Mikrom PostgreSQL database and listens for real-time updates via NATS.
- **ACME Challenge Support**: Intercepts `/.well-known/acme-challenge/` requests and responds with tokens stored in the database.
- **TLS Management**: Receives TLS certificate updates via NATS and stores them in the database.
- **High Performance**: Native Go implementation integrated directly into Caddy's HTTP pipeline.

## Configuration

The plugin is configured via the `Caddyfile`.

### Global Options

```caddyfile
{
    order mikrom_router before reverse_proxy

    mikrom_app {
        nats_url "nats://localhost:4222"
        db_url "postgres://user:pass@localhost:5432/mikrom"
        master_key "your-master-key"
    }
}
```

### Site Configuration

```caddyfile
:80, :443 {
    mikrom_router
    reverse_proxy {vars.mikrom_target}
}
```

## Building

Use `xcaddy` to build Caddy with this plugin:

```bash
xcaddy build --with github.com/antpard/mikrom/mikrom-router=.
```

Or use the provided `Dockerfile`.

## Development

1. Generate Go Protobuf stubs:
   ```bash
   mkdir -p proto/router/v1
   protoc --proto_path=../mikrom-proto/proto --go_out=proto/router/v1 --go_opt=paths=source_relative router.proto
   ```
2. Run tests:
   ```bash
   go test ./...
   ```
