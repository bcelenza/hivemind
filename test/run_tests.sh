#!/usr/bin/env bash
#
# Integration test script for Hivemind rate limiting (single node)
#
# This script tests that Envoy correctly enforces rate limits via Hivemind.

set -euo pipefail

# Get the directory where this script lives
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source shared test library
source "$SCRIPT_DIR/test_lib.sh"

# Configuration
ENVOY_URL="${ENVOY_URL:-http://localhost:10000}"

# Wait for services to be ready
wait_for_services() {
    log_info "Waiting for services to be ready..."

    if ! wait_for_url "$ENVOY_URL"; then
        log_fail "Services did not become ready in time"
        exit 1
    fi
    echo ""
    log_info "Services are ready!"
}

# Test 1: Basic connectivity
test_basic_connectivity() {
    run_test "Basic connectivity"

    local status_code
    status_code=$(get_status "$ENVOY_URL")

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
        status_code=$(get_status "$ENVOY_URL")
        if [ "$status_code" = "200" ]; then
            ((success_count++))
        fi
    done

    log_info "Made $success_count successful requests within limit"

    # The next request should be rate limited (429)
    local limited_status
    limited_status=$(get_status "$ENVOY_URL")

    if [ "$limited_status" = "429" ]; then
        log_pass "Request was rate limited (HTTP 429) after exceeding threshold"
    else
        log_fail "Request was NOT rate limited (HTTP $limited_status), expected 429"
    fi
}

# Test 3: Rate limit headers present
test_ratelimit_headers() {
    run_test "Rate limit headers are present"

    if check_ratelimit_headers "$ENVOY_URL"; then
        log_pass "X-RateLimit-Limit and X-RateLimit-Remaining headers are present"
    else
        log_fail "Rate limit headers are missing"
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
    limited_status=$(get_status "$ENVOY_URL")

    if [ "$limited_status" != "429" ]; then
        log_fail "Expected to be rate limited before reset"
        return
    fi

    log_info "Confirmed rate limited, waiting for window reset..."

    # Wait for the rate limit window to reset
    wait_for_rate_limit_reset

    # Should be able to make requests again
    local reset_status
    reset_status=$(get_status "$ENVOY_URL")

    if [ "$reset_status" = "200" ]; then
        log_pass "Rate limit reset after time window (HTTP $reset_status)"
    else
        log_fail "Rate limit did not reset (HTTP $reset_status)"
    fi
}

# Main
main() {
    echo "=========================================="
    echo "Hivemind Integration Tests"
    echo "=========================================="

    wait_for_services

    test_basic_connectivity

    wait_for_rate_limit_reset
    test_rate_limiting_enforced

    wait_for_rate_limit_reset
    test_ratelimit_headers

    wait_for_rate_limit_reset
    test_rate_limit_reset

    print_summary "Test Summary"
}

main "$@"
