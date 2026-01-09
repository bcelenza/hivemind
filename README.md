# Hivemind

> ⚠️ **Notice**: This project is a prototype/proof-of-concept and is not intended for production use. It is provided as-is for experimentation and learning purposes.

Distributed rate limiting service for Envoy Proxy written in Rust.

## Overview

Hivemind is a high-performance, distributed rate limiting service that integrates with Envoy Proxy's global rate limiting API. It uses a peer-to-peer mesh architecture for state synchronization to avoid requiring a centralized storage or other potential single point of failure.

## Distributed vs. Centralized Rate Limiting

Rate limiting architectures generally fall into two categories, each with distinct trade-offs:

### Centralized Rate Limiting

A centralized approach uses a single source of truth (typically Redis or a similar data store) for all rate limit counters.

**Pros:**
- **Exact enforcement**: All nodes see the same counter values with strong consistency
- **Simple mental model**: One counter per rate limit key, no synchronization complexity
- **Immediate propagation**: Counter updates are instantly visible to all nodes

**Cons:**
- **Single point of failure**: If the central store goes down, rate limiting fails (or must fail-open)
- **Network latency**: Every rate limit check requires a round-trip to the central store
- **Scalability limits**: Central store can become a bottleneck under high load
- **Operational complexity**: Requires provisioning, monitoring, and maintaining additional infrastructure

**Best for:** Scenarios requiring exact rate limit enforcement, such as financial/billing use cases.

### Distributed Rate Limiting (Hivemind's Approach)

Hivemind uses gossip-based eventual consistency where each node maintains its own counters and shares state with peers.

**Pros:**
- **No single point of failure**: Cluster continues operating if nodes fail
- **Lower latency**: Rate limit decisions are made locally without network round-trips
- **Horizontal scalability**: Adding nodes increases capacity without bottlenecks
- **Simpler deployment**: No external dependencies; runs as a sidecar alongside your application

**Cons:**
- **Eventual consistency**: Counter values may temporarily diverge across nodes
- **Potential overshoot**: During propagation delay, the cluster may allow slightly more requests than the configured limit
- **Consistency window**: The gossip interval (default 100ms) determines how quickly state converges

**Best for:** High-throughput APIs where approximate enforcement is acceptable, DDoS protection, preventing abuse, or environments where operational simplicity is valued over exact precision.

Hivemind also supports **standalone mode** (no mesh) for single-instance deployments or when running behind a load balancer that routes consistently to the same instance.

## Features

- **Rust Implementation**: High performance and memory safety
- **gRPC API**: Compatible with Envoy Proxy's rate limit service v3
- **Distributed Architecture**: Peer mesh for state sharing without centralized storage
- **Sidecar Deployment**: Runs alongside your application and Envoy proxy
- **Low Latency**: Sub-millisecond rate limit decisions

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

### Command Line Options

```bash
hivemind [OPTIONS]

Options:
  -c, --config <PATH>       Path to the rate limit configuration file
  -a, --addr <ADDR>         gRPC server address [default: 127.0.0.1:8081]
      --mesh                Enable mesh networking for distributed rate limiting
      --node-id <ID>        Mesh node ID (auto-generated if not specified)
      --mesh-addr <ADDR>    Mesh bind address [default: 0.0.0.0:7946]
      --peers <ADDRS>       Bootstrap peer addresses (comma-separated)
  -h, --help                Print help
  -V, --version             Print version
```

### Standalone Mode

By default, Hivemind runs in standalone mode with local rate limiting:

```bash
hivemind -c config/ratelimit.yaml -a 0.0.0.0:8081
```

### Distributed Mode (Mesh)

For distributed rate limiting across multiple instances, enable mesh mode with the `--mesh` flag. Nodes use a gossip protocol (Chitchat) to synchronize rate limit counters.

**First node (seed):**
```bash
hivemind -c config/ratelimit.yaml -a 0.0.0.0:8081 \
  --mesh --node-id node-1 --mesh-addr 0.0.0.0:7946
```

**Additional nodes:**
```bash
hivemind -c config/ratelimit.yaml -a 0.0.0.0:8081 \
  --mesh --node-id node-2 --mesh-addr 0.0.0.0:7946 \
  --peers node-1:7946
```

Nodes automatically discover each other through gossip, so you only need to specify one seed peer to join the cluster.

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

**Running distributed integration tests:**

```bash
cd test
make test-distributed
```

This runs a 3-node Hivemind cluster to verify distributed rate limiting and gossip-based state synchronization.

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

## References

- [Envoy Rate Limit Service](https://www.envoyproxy.io/docs/envoy/latest/api-v3/service/ratelimit/v3/rls.proto)
- [Envoy Global Rate Limiting](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/other_features/global_rate_limiting)
