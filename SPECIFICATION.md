# Distributed Rate Limiting Service - Technical Specification

## 1. Overview

This document specifies the design and implementation requirements for a distributed rate limiting service ("Hivemind") that integrates with Envoy Proxy's global rate limiting API. The service implements a peer-to-peer mesh architecture for state synchronization without relying on centralized storage.

## 2. Core Requirements

### 2.1 Mandatory Requirements

1. **Language**: MUST be implemented in Rust
2. **Protocol**: MUST implement gRPC for Envoy integration
3. **Architecture**: MUST use a decentralized peer mesh for state sharing (no centralized storage)
4. **Compatibility**: MUST support all current Envoy global rate limiting capabilities
5. **Deployment**: MUST be implemented to run as a sidecar alongside the application and Envoy proxy

### 2.2 Secondary Requirements

1. **Observability**: MUST emit and support collection of metrics via OpenTelemetry (OTEL)
   - Priority: Lower than core functionality
   - Implementation timeline: After core rate limiting features are stable

### 2.3 Design Goals

- High availability through distributed architecture
- Low latency rate limit decisions (sub-millisecond target)
- Horizontal scalability
- Eventual consistency with conflict resolution
- Fault tolerance and partition tolerance
- **Leverage existing libraries**: SHOULD use well-maintained third-party libraries where possible rather than reimplementing functionality

## 3. System Architecture

### 3.1 Components

#### 3.1.1 gRPC Server
- Implements Envoy's `RateLimitService` interface
- Handles incoming rate limit check requests from Envoy proxies
- Returns rate limit decisions based on current state

#### 3.1.2 State Manager
- Maintains local rate limit counters and windows
- Applies rate limit rules and policies
- Manages time-windowed counters (per-second, per-minute, per-hour, per-day)

#### 3.1.3 Peer Mesh Coordinator
- Discovers and maintains connections to peer nodes
- Synchronizes rate limit state across the mesh
- Implements gossip protocol or CRDT-based state replication
- Handles node joining, leaving, and failure detection

#### 3.1.4 Configuration Manager
- Loads and validates rate limit configuration
- Supports dynamic configuration updates
- Manages rate limit descriptors and actions

### 3.2 Architecture Diagram

#### Sidecar Deployment Model
```
┌─────────────────────────────────────────┐
│              Pod / Instance             │
│                                         │
│  ┌──────────────┐                       │
│  │ Application  │                       │
│  └──────────────┘                       │
│         │                               │
│         │ HTTP/gRPC                     │
│         ▼                               │
│  ┌──────────────┐                       │
│  │    Envoy     │                       │
│  │    Proxy     │                       │
│  └──────┬───────┘                       │
│         │                               │
│         │ gRPC (localhost)              │
│         ▼                               │
│  ┌──────────────┐                       │
│  │  Hivemind    │                       │
│  │  Rate Limit  │                       │
│  │   Service    │                       │
│  └──────┬───────┘                       │
│         │                               │
│         │ Peer Mesh (cross-pod)         │
└─────────┼───────────────────────────────┘
          │
          │ Gossip/CRDT Sync
          ▼
┌─────────────────────────────────────────┐
│         Other Pod Instances             │
│  (Application + Envoy + Hivemind)       │
└─────────────────────────────────────────┘
```

#### Distributed Communication
```
Pod 1                    Pod 2                    Pod N
┌──────────────┐        ┌──────────────┐        ┌──────────────┐
│ App + Envoy  │        │ App + Envoy  │        │ App + Envoy  │
│      +       │        │      +       │        │      +       │
│  Hivemind    │◄──────►│  Hivemind    │◄──────►│  Hivemind    │
└──────────────┘  Mesh  └──────────────┘  Mesh  └──────────────┘
       │                       │                       │
       │ Local gRPC            │ Local gRPC            │ Local gRPC
       │ (localhost)           │ (localhost)           │ (localhost)
       ▼                       ▼                       ▼
    Envoy                   Envoy                   Envoy
```

## 4. Envoy Rate Limit Service API

### 4.1 gRPC Service Definition

The service MUST implement the `envoy.service.ratelimit.v3.RateLimitService` interface:

```protobuf
service RateLimitService {
  rpc ShouldRateLimit(RateLimitRequest) returns (RateLimitResponse);
}
```

### 4.2 Rate Limit Request

Must handle:
- Domain: Identifies the rate limit configuration domain
- Descriptors: Array of rate limit descriptors (key-value pairs)
- Hits addend: Number of hits to add (default: 1)

### 4.3 Rate Limit Response

Must return:
- Overall code: OK, OVER_LIMIT, or UNKNOWN
- Per-descriptor status
- Headers to append to response
- Request headers to append
- Response headers to append
- Dynamic metadata

### 4.4 Rate Limit Capabilities

Must support all Envoy rate limiting features:

1. **Descriptor-based rate limiting**
   - Hierarchical descriptor matching
   - Multiple descriptor entries per request
   - Descriptor value matching (exact, prefix, suffix)

2. **Rate limit actions**
   - Request headers
   - Remote address
   - Generic key
   - Header value match
   - Dynamic metadata
   - Metadata
   - Extension

3. **Time windows**
   - Second
   - Minute
   - Hour
   - Day

4. **Limit responses**
   - Per-descriptor limits
   - Quota bucket ID
   - Duration until reset
   - Remaining quota

5. **Advanced features**
   - Shadow mode
   - Local rate limiting
   - Rate limit override
   - Custom response headers

## 5. Distributed State Management

### 5.1 Peer Mesh Protocol

#### 5.1.1 Node Discovery
- Bootstrap nodes configuration
- mDNS for local network discovery
- DNS-based service discovery
- Static peer configuration

#### 5.1.2 State Synchronization

**Option A: Gossip Protocol**
- Epidemic-style information dissemination
- Periodic state exchange with random peers
- Anti-entropy mechanism for convergence
- Configurable gossip interval and fanout

**Option B: CRDT (Conflict-free Replicated Data Types)**
- PN-Counter for rate limit counters
- G-Counter for monotonic counters
- OR-Set for descriptor sets
- Automatic conflict resolution

**Recommended**: Hybrid approach using CRDTs with gossip for propagation

#### 5.1.3 Consistency Model
- Eventual consistency
- Read-your-writes consistency per node
- Monotonic reads within time windows
- Bounded staleness (configurable)

### 5.2 Data Structures

#### 5.2.1 Rate Limit Counter
```rust
struct RateLimitCounter {
    descriptor: Vec<DescriptorEntry>,
    time_window: TimeWindow,
    current_count: AtomicU64,
    window_start: Instant,
    limit: u64,
    vector_clock: VectorClock,
}
```

#### 5.2.2 Peer Node
```rust
struct PeerNode {
    node_id: NodeId,
    address: SocketAddr,
    last_seen: Instant,
    state_version: u64,
    health: HealthStatus,
}
```

### 5.3 Partition Handling

- Graceful degradation during network partitions
- Local decision making with higher error margins
- Partition detection using heartbeats
- Automatic reconciliation on partition healing

## 6. Performance Requirements

### 6.1 Latency
- P50: < 1ms for rate limit decisions
- P95: < 5ms for rate limit decisions
- P99: < 10ms for rate limit decisions

### 6.2 Throughput
- Minimum: 10,000 requests/second per node
- Target: 100,000 requests/second per node

### 6.3 Scalability
- Support for 100+ peer nodes in mesh
- Horizontal scaling without downtime
- Linear performance scaling with node count

### 6.4 Resource Usage
- Memory: O(n) where n = unique descriptor combinations
- CPU: < 50% utilization at 50% target throughput
- Network: Configurable gossip bandwidth limits

## 7. Configuration

### 7.1 Rate Limit Configuration Format

Support Envoy's rate limit configuration format:

```yaml
domain: default
descriptors:
  - key: source_cluster
    value: cluster_a
    rate_limit:
      unit: second
      requests_per_unit: 100
    descriptors:
      - key: destination_cluster
        value: cluster_b
        rate_limit:
          unit: minute
          requests_per_unit: 1000
```

### 7.2 Service Configuration

```yaml
server:
  grpc_port: 8081
  metrics_port: 9090
  admin_port: 8080

mesh:
  node_id: "auto"  # or explicit ID
  bootstrap_peers:
    - "peer1.example.com:7946"
    - "peer2.example.com:7946"
  gossip_interval_ms: 100
  sync_interval_ms: 1000
  max_peers: 100

rate_limiting:
  config_path: "/etc/ratelimit/config.yaml"
  config_reload_interval: 60s
  local_cache_size: 10000
  staleness_threshold_ms: 500

observability:
  metrics_enabled: true
  tracing_enabled: true
  log_level: "info"

  # OpenTelemetry Configuration
  otel:
    metrics:
      enabled: true
      exporter: "otlp"  # otlp, prometheus, or both
      endpoint: "http://otel-collector:4317"
      protocol: "grpc"  # grpc or http
      interval: 10s
    tracing:
      enabled: true
      exporter: "otlp"
      endpoint: "http://otel-collector:4317"
      protocol: "grpc"
      sample_rate: 1.0  # 0.0 to 1.0
    resource:
      service_name: "hivemind"
      service_version: "auto"  # or explicit version

  # Prometheus scrape endpoint (compatibility)
  prometheus:
    enabled: true
    port: 9090
    path: "/metrics"
```

## 8. Observability

### 8.1 Metrics

**OpenTelemetry (OTEL) - Primary Method**

The service MUST emit metrics via OpenTelemetry, supporting:
- OTLP (OpenTelemetry Protocol) export
- Configurable exporters (OTLP/gRPC, OTLP/HTTP)
- Metric aggregation and batching
- Resource attributes (service name, version, instance ID)

**Prometheus Compatibility - Secondary Method**

MUST also expose Prometheus-compatible metrics endpoint for backward compatibility.

**Required Metrics:**

- `ratelimit.requests.total` (Counter): Total rate limit requests
  - Attributes: `domain`, `response_code`, `descriptor_key`
- `ratelimit.over_limit.total` (Counter): Total over-limit responses
  - Attributes: `domain`, `descriptor_key`
- `ratelimit.request.duration` (Histogram): Request latency in milliseconds
  - Attributes: `domain`, `response_code`
  - Buckets: 0.1, 0.5, 1, 2, 5, 10, 25, 50, 100ms
- `ratelimit.counter.value` (Gauge): Current counter values (sampled)
  - Attributes: `domain`, `descriptor_key`, `time_window`
- `ratelimit.mesh.peers.active` (Gauge): Number of active peers
- `ratelimit.mesh.sync.latency` (Histogram): Mesh sync latency in milliseconds
  - Attributes: `peer_id`, `operation`
- `ratelimit.mesh.state_updates.total` (Counter): State update events
  - Attributes: `operation`, `peer_id`
- `ratelimit.mesh.bytes_sent.total` (Counter): Total bytes sent to peers
  - Attributes: `peer_id`
- `ratelimit.mesh.bytes_received.total` (Counter): Total bytes received from peers
  - Attributes: `peer_id`

### 8.2 Tracing

**OpenTelemetry Integration**

- MUST support distributed tracing via OpenTelemetry
- OTLP trace export (gRPC and HTTP)
- Trace sampling configuration
- Context propagation via W3C Trace Context

**Trace Spans:**

- `ratelimit.check`: Rate limit decision span
  - Attributes: domain, descriptor keys, response code, hit count
- `ratelimit.state.read`: Local state read operations
- `ratelimit.state.write`: Local state write operations
- `ratelimit.mesh.sync`: Mesh synchronization operations
  - Attributes: peer count, bytes transferred, duration
- `ratelimit.mesh.gossip`: Gossip protocol operations

### 8.3 Logging

- Structured logging (JSON format)
- Configurable log levels
- Rate limit decision logging (optional, for debugging)
- Mesh topology changes

## 9. Security

### 9.1 Transport Security

- TLS support for gRPC server
- Mutual TLS (mTLS) for peer mesh communication
- Certificate rotation support

### 9.2 Authentication & Authorization

- Optional API key authentication for gRPC
- Peer authentication using certificates
- Configuration access control

## 10. Deployment

### 10.1 Deployment Model

**Primary Model: Sidecar Deployment**

The service MUST be deployed as a sidecar container alongside the application and Envoy proxy. This deployment model provides:

- **Locality**: Rate limit service runs on localhost, minimizing network latency
- **Isolation**: Each application instance has its own rate limit service instance
- **Simplicity**: No separate infrastructure required
- **Scalability**: Rate limit capacity scales automatically with application instances
- **Resource Sharing**: Shares pod/instance resources with application

#### Kubernetes Deployment
```yaml
apiVersion: v1
kind: Pod
metadata:
  name: app-with-ratelimit
spec:
  containers:
  - name: application
    image: myapp:latest
    ports:
    - containerPort: 8080

  - name: envoy
    image: envoyproxy/envoy:v1.28-latest
    ports:
    - containerPort: 10000
    volumeMounts:
    - name: envoy-config
      mountPath: /etc/envoy

  - name: hivemind
    image: hivemind:latest
    ports:
    - containerPort: 8081  # gRPC for Envoy
    - containerPort: 7946  # Peer mesh
    - containerPort: 9090  # Metrics
    env:
    - name: POD_NAME
      valueFrom:
        fieldRef:
          fieldPath: metadata.name
    - name: POD_NAMESPACE
      valueFrom:
        fieldRef:
          fieldPath: metadata.namespace
```

#### Docker Compose (Development)
```yaml
version: '3.8'
services:
  app:
    image: myapp:latest

  envoy:
    image: envoyproxy/envoy:v1.28-latest
    volumes:
      - ./envoy.yaml:/etc/envoy/envoy.yaml
    depends_on:
      - hivemind

  hivemind:
    image: hivemind:latest
    environment:
      - MESH_NODE_ID=node1
    ports:
      - "8081:8081"  # gRPC
      - "7946:7946"  # Peer mesh
```

#### Systemd Service (Bare Metal)
For non-containerized deployments, run as a systemd service on the same host as Envoy.

### 10.2 Networking Considerations

**Intra-Pod Communication (Envoy ↔ Hivemind)**
- Communication over localhost/127.0.0.1
- No encryption required (same network namespace)
- Ultra-low latency (microseconds)
- Use Unix domain sockets for even lower latency (optional)

**Inter-Pod Communication (Hivemind ↔ Hivemind)**
- Peer mesh communication across pod network
- MUST use mTLS for security
- Service discovery via Kubernetes DNS or headless service
- UDP for gossip protocol (with optional DTLS)
- TCP/QUIC for bulk state transfer

### 10.3 Resource Requirements

**Minimum (per sidecar)**
- CPU: 100m (0.1 cores)
- Memory: 128Mi

**Recommended (per sidecar)**
- CPU: 250m - 500m (0.25 - 0.5 cores)
- Memory: 256Mi - 512Mi

**High-Traffic Workloads**
- CPU: 1000m - 2000m (1-2 cores)
- Memory: 1Gi - 2Gi

### 10.4 Service Discovery

For peer mesh formation in Kubernetes:

1. **Headless Service**: Create a headless service to expose all pod IPs
```yaml
apiVersion: v1
kind: Service
metadata:
  name: hivemind-mesh
spec:
  clusterIP: None
  selector:
    app: myapp
  ports:
  - name: mesh
    port: 7946
```

2. **DNS Discovery**: Peers discover each other via DNS SRV records
3. **Pod Labels**: Use labels for selective mesh membership
4. **StatefulSet Support**: Optional for stable network identities

### 10.5 High Availability

- No single point of failure
- Automatic peer failover
- Split-brain prevention
- Rolling updates without downtime

## 11. Testing Requirements

### 11.1 Unit Tests
- Rate limit logic
- CRDT operations
- Configuration parsing

### 11.2 Integration Tests
- gRPC API compatibility with Envoy
- Peer mesh synchronization
- Failure scenarios

### 11.3 Performance Tests
- Load testing at target throughput
- Latency benchmarking
- Scalability testing with varying peer counts

### 11.4 Chaos Engineering
- Network partition scenarios
- Node failure and recovery
- Clock skew handling

## 12. Dependencies

### 12.0 Dependency Philosophy

The implementation SHOULD prefer using well-established, actively maintained third-party libraries over custom implementations where appropriate. This approach:

- Reduces development time and maintenance burden
- Leverages battle-tested code from the Rust ecosystem
- Benefits from community security audits and bug fixes
- Allows focus on core business logic and unique features

**Guidelines for library selection:**
- Prefer libraries with active maintenance and community support
- Evaluate security track record and audit status
- Consider performance characteristics and benchmarks
- Assess API stability and breaking change history
- Check licensing compatibility (prefer Apache-2.0, MIT, BSD)

**Custom implementation should be considered when:**
- No suitable library exists for the specific use case
- Existing libraries have unacceptable performance characteristics
- Dependencies would introduce significant complexity or bloat
- Core functionality requires tight integration with custom logic

### 12.1 Core Dependencies

- **tonic**: gRPC implementation
- **tokio**: Async runtime
- **serde**: Serialization
- **rustls**: TLS implementation
- **config**: Configuration management

### 12.2 Observability Dependencies

- **opentelemetry**: OpenTelemetry API and SDK
- **opentelemetry-otlp**: OTLP exporter for metrics and traces
- **opentelemetry-prometheus**: Prometheus exporter (compatibility)
- **tracing**: Structured logging and tracing
- **tracing-opentelemetry**: Bridge between tracing and OpenTelemetry
- **tracing-subscriber**: Tracing subscriber implementations

### 12.3 Mesh Dependencies

- **libp2p** or **quinn**: P2P networking (QUIC)
- CRDT library (e.g., `crdts` crate or custom implementation)
- **memberlist** or custom gossip implementation

## 13. Milestones

### Phase 1: Core Implementation
- [x] Project setup and structure
- [x] gRPC server implementation
- [x] Basic rate limiting logic
- [x] Configuration loading
- [x] Unit tests

### Phase 2: Distributed State
- [x] CRDT implementation for counters
- [x] Gossip protocol implementation
- [x] Peer discovery and management
- [x] State synchronization
- [x] Integration tests

### Phase 3: Production Readiness
- [ ] Basic metrics (Prometheus endpoint)
- [ ] Security (TLS/mTLS)
- [ ] Performance optimization
- [ ] Documentation
- [ ] Load testing

### Phase 4: Advanced Features
- [ ] OpenTelemetry metrics and tracing (OTEL integration)
- [ ] Dynamic configuration updates
- [ ] Advanced Envoy features
- [ ] Admin API
- [ ] Deployment tooling

## 14. References

- [Envoy Rate Limit Service](https://www.envoyproxy.io/docs/envoy/latest/api-v3/service/ratelimit/v3/rls.proto)
- [Envoy Global Rate Limiting](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/other_features/global_rate_limiting)
- [CRDTs: Consistency without concurrency control](https://hal.inria.fr/inria-00609399/document)
- [Gossip Protocols](https://www.cs.cornell.edu/home/rvr/papers/flowgossip.pdf)

## 15. Glossary

- **CRDT**: Conflict-free Replicated Data Type
- **Descriptor**: Key-value pair used for rate limit matching
- **Mesh**: Peer-to-peer network topology
- **Vector Clock**: Logical clock for tracking causality in distributed systems
- **Window**: Time period for rate limit enforcement (second, minute, hour, day)
