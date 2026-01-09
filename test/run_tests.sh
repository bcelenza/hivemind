#!/usr/bin/env bash
#
# Integration test script for Hivemind rate limiting
#
# This script tests that Envoy correctly enforces rate limits via Hivemind.

set -euo pipefail

# Configuration
ENVOY_URL="${ENVOY_URL:-http://localhost:10000}"
RATE_LIMIT=5  # Matches test/ratelimit.yaml

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

run_test() {
    local test_name="$1"
    TESTS_RUN=$((TESTS_RUN + 1))
    echo ""
    log_info "Running test: $test_name"
}

# Wait for services to be ready
wait_for_services() {
    log_info "Waiting for services to be ready..."
    local max_attempts=30
    local attempt=1

    while ! curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200" | grep -q "200"; do
        if [ $attempt -ge $max_attempts ]; then
            log_fail "Services did not become ready in time"
            exit 1
        fi
        echo -n "."
        sleep 1
        ((attempt++))
    done
    echo ""
    log_info "Services are ready!"
}

# Test 1: Basic connectivity
test_basic_connectivity() {
    run_test "Basic connectivity"

    local status_code
    status_code=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200")

    if [ "$status_code" = "200" ]; then
        log_pass "Basic connectivity works (HTTP $status_code)"
    else
        log_fail "Basic connectivity failed (HTTP $status_code)"
    fi
}

# Test 2: Rate limiting enforced
test_rate_limiting_enforced() {
    run_test "Rate limiting is enforced after threshold"

    # Make requests up to the limit (should all succeed)
    local success_count=0
    for i in $(seq 1 $RATE_LIMIT); do
        local status_code
        status_code=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200")
        if [ "$status_code" = "200" ]; then
            ((success_count++))
        fi
    done

    log_info "Made $success_count successful requests within limit"

    # The next request should be rate limited (429)
    local limited_status
    limited_status=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200")

    if [ "$limited_status" = "429" ]; then
        log_pass "Request was rate limited (HTTP 429) after exceeding threshold"
    else
        log_fail "Request was NOT rate limited (HTTP $limited_status), expected 429"
    fi
}

# Test 3: Rate limit headers present
test_ratelimit_headers() {
    run_test "Rate limit headers are present"

    # Wait for rate limit window to reset
    sleep 2

    local headers
    headers=$(curl -s -D - -o /dev/null "$ENVOY_URL/status/200")

    local has_limit=false
    local has_remaining=false

    if echo "$headers" | grep -qi "x-ratelimit-limit"; then
        has_limit=true
    fi

    if echo "$headers" | grep -qi "x-ratelimit-remaining"; then
        has_remaining=true
    fi

    if [ "$has_limit" = true ] && [ "$has_remaining" = true ]; then
        log_pass "X-RateLimit-Limit and X-RateLimit-Remaining headers are present"
    else
        if [ "$has_limit" = false ]; then
            log_fail "X-RateLimit-Limit header is missing"
        fi
        if [ "$has_remaining" = false ]; then
            log_fail "X-RateLimit-Remaining header is missing"
        fi
    fi
}

# Test 4: Rate limit resets after window
test_rate_limit_reset() {
    run_test "Rate limit resets after time window"

    # Exhaust the rate limit
    for i in $(seq 1 $((RATE_LIMIT + 2))); do
        curl -s -o /dev/null "$ENVOY_URL/status/200"
    done

    # Verify we're rate limited
    local limited_status
    limited_status=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200")

    if [ "$limited_status" != "429" ]; then
        log_fail "Expected to be rate limited before reset"
        return
    fi

    log_info "Confirmed rate limited, waiting for window reset..."

    # Wait for the rate limit window to reset (1 second window + buffer)
    sleep 2

    # Should be able to make requests again
    local reset_status
    reset_status=$(curl -s -o /dev/null -w "%{http_code}" "$ENVOY_URL/status/200")

    if [ "$reset_status" = "200" ]; then
        log_pass "Rate limit reset after time window (HTTP $reset_status)"
    else
        log_fail "Rate limit did not reset (HTTP $reset_status)"
    fi
}

# Print summary
print_summary() {
    echo ""
    echo "=========================================="
    echo "Test Summary"
    echo "=========================================="
    echo "Tests run:    $TESTS_RUN"
    echo -e "Tests passed: ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Tests failed: ${RED}$TESTS_FAILED${NC}"
    echo "=========================================="

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "${GREEN}All tests passed!${NC}"
        return 0
    else
        echo -e "${RED}Some tests failed!${NC}"
        return 1
    fi
}

# Main
main() {
    echo "=========================================="
    echo "Hivemind Integration Tests"
    echo "=========================================="

    wait_for_services

    test_basic_connectivity
    test_rate_limiting_enforced
    test_ratelimit_headers
    test_rate_limit_reset

    print_summary
}

main "$@"
