# Integration Tests

This directory contains integration tests for Hivemind with Envoy Proxy.

## Prerequisites

- Docker and Docker Compose
- `curl` command-line tool

## Running the Tests

### Single Node Tests

```bash
# Run all single-node integration tests
make test-integration

# Or manually:
docker-compose -f docker-compose.yaml up -d
./run_tests.sh
docker-compose -f docker-compose.yaml down
```

### Distributed (Multi-Node) Tests

```bash
# Run all distributed integration tests
make test-distributed

# Or manually:
docker-compose -f docker-compose.distributed.yaml up -d
sleep 10  # Wait for cluster to form
./run_distributed_tests.sh
docker-compose -f docker-compose.distributed.yaml down
```

### All Tests

```bash
# Run unit tests, single-node integration, and distributed tests
make test-all
```

## Test Architecture

### Single Node Setup

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Test      │────▶│   Envoy     │────▶│  Backend    │
│   Script    │     │   Proxy     │     │  (httpbin)  │
└─────────────┘     └──────┬──────┘     └─────────────┘
                          │
                          │ gRPC (rate limit check)
                          ▼
                   ┌─────────────┐
                   │  Hivemind   │
                   └─────────────┘
```

### Distributed Setup (3 Nodes)

```
                          ┌─────────────────────────────────────────────┐
                          │           Cluster (Chitchat Gossip)         │
                          │                                             │
┌────────┐    ┌────────┐  │  ┌────────────┐                            │
│ Envoy  │───▶│Hivemind│◀─┼──│            │                            │
│   1    │    │   1    │  │  │            │                            │
└────────┘    └────────┘  │  │            │                            │
                          │  │   Gossip   │                            │
┌────────┐    ┌────────┐  │  │   State    │                            │
│ Envoy  │───▶│Hivemind│◀─┼──│   Sync     │                            │
│   2    │    │   2    │  │  │            │                            │
└────────┘    └────────┘  │  │            │                            │
                          │  │            │                            │
┌────────┐    ┌────────┐  │  │            │                            │
│ Envoy  │───▶│Hivemind│◀─┼──│            │                            │
│   3    │    │   3    │  │  └────────────┘                            │
└────────┘    └────────┘  │                                             │
                          └─────────────────────────────────────────────┘
                                            │
                                            ▼
                                     ┌─────────────┐
                                     │   Backend   │
                                     │  (httpbin)  │
                                     └─────────────┘
```

## Test Cases

### Single Node Tests

1. **Basic Connectivity**: Verify Envoy can route requests
2. **Rate Limiting Enforced**: Verify requests are limited after exceeding threshold
3. **Rate Limit Headers**: Verify X-RateLimit headers are returned
4. **Rate Limit Reset**: Verify rate limits reset after the time window

### Distributed Tests

1. **All Nodes Accessible**: Verify all 3 nodes are reachable
2. **State Synchronization**: Verify rate limit state propagates between nodes via gossip
3. **Single Node Enforcement**: Verify each node enforces its own rate limits
4. **Distributed Enforcement**: Verify combined counts across nodes trigger rate limiting
5. **Rate Limit Headers**: Verify headers are present on all nodes
6. **Cluster Membership**: Verify nodes discover each other and form a cluster

## Configuration

- `ratelimit.yaml`: Rate limit rules (5 requests per second for test_key=limited)
- `envoy.yaml`: Envoy proxy configuration (single node)
- `envoy.distributed.node*.yaml`: Envoy configs for each distributed node
- `docker-compose.yaml`: Single node test environment
- `docker-compose.distributed.yaml`: 3-node distributed test environment
