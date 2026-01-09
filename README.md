# Hivemind

Distributed rate limiting service for Envoy Proxy written in Rust.

## Overview

Hivemind is a high-performance, distributed rate limiting service that integrates with Envoy Proxy's global rate limiting API. It uses a peer-to-peer mesh architecture for state synchronization to avoid requiring acentralized storage or other potential single point of failure.

## Features

- **Rust Implementation**: High performance and memory safety
- **gRPC API**: Compatible with Envoy Proxy's rate limit service v3
- **Distributed Architecture**: Peer mesh for state sharing without centralized storage
- **Sidecar Deployment**: Runs alongside your application and Envoy proxy
- **Low Latency**: Sub-millisecond rate limit decisions
- **Observability**: OpenTelemetry metrics and tracing support

## Documentation

See [SPECIFICATION.md](SPECIFICATION.md) for the complete technical specification.

## Quick Start

### Prerequisites

- Rust 1.70 or later
- Docker (optional, for containerized deployment)
- Kubernetes (optional, for production deployment)

### Building

```bash
cargo build --release
```

### Running

```bash
cargo run
```

### Configuration

Edit `config.yaml` to configure the service. Key settings:

- `server.grpc_port`: Port for Envoy to connect to (default: 8081)
- `mesh.bootstrap_peers`: List of peer nodes to connect to
- `rate_limiting.config_path`: Path to rate limit rules configuration

See `config/ratelimit.yaml` for rate limit rule examples.

## Development

```

### Building from Source

```bash
# Development build
cargo build

# Release build with optimizations
cargo build --release

# Run unit tests
cargo test

# Run with logging
RUST_LOG=info cargo run
```

### Integration Tests

The project includes integration tests that verify Hivemind works correctly with Envoy Proxy. The tests use Docker Compose to spin up a complete environment with Hivemind, Envoy, and a backend service.

**Prerequisites:**
- Docker and Docker Compose

**Running integration tests:**

```bash
cd test
make test-integration
```

This will:
1. Build the Hivemind Docker image
2. Start Hivemind, Envoy, and a backend service
3. Run tests that verify rate limiting behavior
4. Clean up all containers

**Manual testing:**

```bash
# Start the test environment
cd test
make test-integration-up

# Make requests (rate limit is 5/sec)
curl http://localhost:10000/status/200

# View logs
make test-integration-logs

# Stop the environment
make test-integration-down
```

## Deployment

### Kubernetes Sidecar

See the specification document for detailed Kubernetes deployment examples.

### Docker

```bash
# Build image
docker build -t hivemind:latest .

# Run
docker run -p 8081:8081 -v $(pwd)/config.yaml:/etc/hivemind/config.yaml hivemind:latest
```

## License

Apache License 2.0 - See LICENSE file for details.

## References

- [Envoy Rate Limit Service](https://www.envoyproxy.io/docs/envoy/latest/api-v3/service/ratelimit/v3/rls.proto)
- [Envoy Global Rate Limiting](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/other_features/global_rate_limiting)
