# mikrom-proto

The shared interface definitions for the Mikrom ecosystem. It contains the Protocol Buffer files and the generated Rust code for NATS communication between services.

## Architecture

Mikrom uses [async-nats](https://github.com/nats-io/nats.rs) and [Prost](https://github.com/tokio-rs/prost) for high-performance NATS communication.

```
mikrom-api ---[Publish]---> mikrom-scheduler ---[Request]---> mikrom-agent
```

## Protocol Definitions

| Service | File | Description |
|---|---|---|
| `Builder` | `builder.proto` | Methods for cloning and building Git repositories. |
| `Scheduler` | `scheduler.proto` | Worker registration, metrics reporting, and app scheduling. |
| `Agent` | `agent.proto` | MicroVM lifecycle management on worker nodes. |

## Code Generation

The Rust code is automatically generated during the build process using `build.rs`.

```bash
# Force regeneration
cargo build -p mikrom-proto
```

## Security

The proto crate includes a `tls` module with utilities for loading and configuring **mutual TLS (mTLS)** certificates, ensuring that all internal NATS traffic is encrypted and authenticated.
