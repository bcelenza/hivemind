#!/usr/bin/env bash
#
# Shared test library for Hivemind integration tests
#
# This file contains common functions used by both single-node and distributed tests.

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test counters (initialized by sourcing script)
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Rate limit configuration (matches test/ratelimit.yaml)
RATE_LIMIT=5

# Time to wait for rate limit window to reset (seconds)
RATE_LIMIT_WINDOW_RESET=2

# Logging functions
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

log_debug() {
    echo -e "${BLUE}[DEBUG]${NC} $1"
}

# Run a test with proper setup
run_test() {
    local test_name="$1"
    TESTS_RUN=$((TESTS_RUN + 1))
    echo ""
    log_info "Running test: $test_name"
}

# Wait for rate limit window to reset between tests
wait_for_rate_limit_reset() {
    log_debug "Waiting for rate limit window to reset..."
    sleep $RATE_LIMIT_WINDOW_RESET
}

# Wait for a single URL to be ready
wait_for_url() {
    local url="$1"
    local max_attempts="${2:-30}"
    local attempt=1

    while ! curl -s -o /dev/null -w "%{http_code}" "$url/status/200" | grep -q "200"; do
        if [ $attempt -ge $max_attempts ]; then
            return 1
        fi
        echo -n "."
        sleep 1
        ((attempt++))
    done
    return 0
}

# Get HTTP status code from a URL
get_status() {
    local url="$1"
    curl -s -o /dev/null -w "%{http_code}" "$url/status/200"
}

# Get remaining rate limit from response headers
get_remaining() {
    local url="$1"
    local headers
    headers=$(curl -s -D - -o /dev/null "$url/status/200")
    echo "$headers" | grep -i "x-ratelimit-remaining" | awk '{print $2}' | tr -d '\r'
}

# Check if rate limit headers are present in response
check_ratelimit_headers() {
    local url="$1"
    local headers
    headers=$(curl -s -D - -o /dev/null "$url/status/200")

    local has_limit=false
    local has_remaining=false

    if echo "$headers" | grep -qi "x-ratelimit-limit"; then
        has_limit=true
    fi

    if echo "$headers" | grep -qi "x-ratelimit-remaining"; then
        has_remaining=true
    fi

    if [ "$has_limit" = true ] && [ "$has_remaining" = true ]; then
        return 0
    else
        return 1
    fi
}

# Print test summary
# Arguments: $1 = summary title (optional, defaults to "Test Summary")
print_summary() {
    local title="${1:-Test Summary}"
    echo ""
    echo "=========================================="
    echo "$title"
    echo "=========================================="
    echo "Tests run:    $TESTS_RUN"
    echo "Tests passed: $TESTS_PASSED"
    echo "Tests failed: $TESTS_FAILED"
    echo "=========================================="

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "${GREEN}All tests passed!${NC}"
        return 0
    else
        echo -e "${RED}Some tests failed!${NC}"
        return 1
    fi
}
