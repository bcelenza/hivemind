# Integration Tests

This directory contains integration tests for Hivemind with Envoy Proxy.

## Prerequisites

- Docker and Docker Compose
- `curl` command-line tool

## Running the Tests

```bash
# Start the test environment
docker-compose -f test/docker-compose.yaml up -d

# Wait for services to be ready
sleep 5

# Run the integration tests
./test/run_tests.sh

# Clean up
docker-compose -f test/docker-compose.yaml down
```

## Test Architecture

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

- **Test Script**: Makes HTTP requests to Envoy and verifies rate limiting behavior
- **Envoy**: Routes requests and calls Hivemind for rate limit decisions
- **Hivemind**: Enforces rate limits based on configuration
- **Backend**: Simple HTTP server (httpbin) to return responses

## Test Cases

1. **Basic Rate Limiting**: Verify requests are limited after exceeding threshold
2. **Different Descriptors**: Verify different rate limits apply to different descriptors
3. **Rate Limit Headers**: Verify X-RateLimit headers are returned
