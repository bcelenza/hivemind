#!/usr/bin/env bash
#
# Sustained Distributed Rate Limiting Test
#
# This test verifies that distributed rate limiting remains consistent
# (within tolerable limits given eventually consistent state) over a
# longer time period.
#
# The test sends a consistent stream of requests across all nodes and
# asserts that the allowed requests per second stay within acceptable
# bounds for the majority of time windows.

set -euo pipefail

# Get the directory where this script lives
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source shared test library
source "$SCRIPT_DIR/test_lib.sh"

# =============================================================================
# Configuration (all configurable via environment variables)
# =============================================================================

# Test duration in seconds
TEST_DURATION="${TEST_DURATION:-60}"

# Rate limit from config (requests per second allowed by the rate limiter)
RATE_LIMIT="${RATE_LIMIT:-5}"

# Request rate to send (should be higher than RATE_LIMIT to trigger limiting)
# This is the rate at which we send requests, not the rate we expect to be allowed
REQUEST_RATE="${REQUEST_RATE:-15}"

# Percentage tolerance above the rate limit
# e.g., 20 means we allow up to 20% more than RATE_LIMIT per second
TOLERANCE_PERCENT="${TOLERANCE_PERCENT:-40}"

# Percentage of windows allowed to fail before the test fails
WINDOW_FAILURE_THRESHOLD_PERCENT="${WINDOW_FAILURE_THRESHOLD_PERCENT:-5}"

# Catastrophic failure multiplier - fail immediately if any window exceeds this
CATASTROPHIC_MULTIPLIER="${CATASTROPHIC_MULTIPLIER:-3}"

# Number of nodes in the cluster
NODE_COUNT="${NODE_COUNT:-3}"

# Node URLs
NODE1_URL="${NODE1_URL:-http://localhost:10001}"
NODE2_URL="${NODE2_URL:-http://localhost:10002}"
NODE3_URL="${NODE3_URL:-http://localhost:10003}"

# Output directory for vegeta results
OUTPUT_DIR="${OUTPUT_DIR:-/tmp/hivemind-sustained-test}"

# =============================================================================
# Derived values
# =============================================================================

# Maximum allowed requests per second (rate limit + tolerance)
MAX_ALLOWED_PER_SECOND=$(echo "$RATE_LIMIT * (1 + $TOLERANCE_PERCENT / 100)" | bc -l | xargs printf "%.0f")

# Catastrophic threshold - if any window exceeds this, fail immediately
CATASTROPHIC_THRESHOLD=$(echo "$RATE_LIMIT * $CATASTROPHIC_MULTIPLIER" | bc -l | xargs printf "%.0f")

# Maximum number of windows that can fail
MAX_FAILED_WINDOWS=$(echo "$TEST_DURATION * $WINDOW_FAILURE_THRESHOLD_PERCENT / 100" | bc -l | xargs printf "%.0f")

# =============================================================================
# Helper functions
# =============================================================================

check_vegeta_installed() {
    if ! command -v vegeta &> /dev/null; then
        log_fail "vegeta is not installed. Please install it:"
        echo "  - macOS: brew install vegeta"
        echo "  - Linux: go install github.com/tsenart/vegeta@latest"
        echo "  - Or download from: https://github.com/tsenart/vegeta/releases"
        exit 1
    fi
}

check_jq_installed() {
    if ! command -v jq &> /dev/null; then
        log_fail "jq is not installed. Please install it:"
        echo "  - macOS: brew install jq"
        echo "  - Linux: apt-get install jq / yum install jq"
        exit 1
    fi
}

create_targets_file() {
    # Create targets that round-robin across all nodes
    # Each line is a target URL
    cat > "$TARGETS_FILE" << EOF
GET ${NODE1_URL}/status/200
GET ${NODE2_URL}/status/200
GET ${NODE3_URL}/status/200
EOF
}

wait_for_services() {
    log_info "Waiting for all nodes to be ready..."

    for url in "$NODE1_URL" "$NODE2_URL" "$NODE3_URL"; do
        if ! wait_for_url "$url"; then
            log_fail "Node at $url did not become ready in time"
            exit 1
        fi
    done
    echo ""
    log_info "All nodes are ready!"

    # Additional wait for gossip to establish cluster membership
    log_info "Waiting for cluster membership to stabilize..."
    sleep 5
}

print_test_config() {
    echo ""
    echo "=========================================="
    echo "Sustained Distributed Rate Limit Test"
    echo "=========================================="
    echo "Configuration:"
    echo "  Test duration:          ${TEST_DURATION}s"
    echo "  Rate limit:             ${RATE_LIMIT} req/s"
    echo "  Request rate:           ${REQUEST_RATE} req/s"
    echo "  Tolerance:              ${TOLERANCE_PERCENT}%"
    echo "  Max allowed/second:     ${MAX_ALLOWED_PER_SECOND} req/s"
    echo "  Catastrophic threshold: ${CATASTROPHIC_THRESHOLD} req/s"
    echo "  Window failure limit:   ${WINDOW_FAILURE_THRESHOLD_PERCENT}% (${MAX_FAILED_WINDOWS} windows)"
    echo "  Node count:             ${NODE_COUNT}"
    echo "  Nodes:                  ${NODE1_URL}, ${NODE2_URL}, ${NODE3_URL}"
    echo "=========================================="
    echo ""
}

# =============================================================================
# Main test execution
# =============================================================================

run_load_test() {
    log_info "Starting load test: ${REQUEST_RATE} req/s for ${TEST_DURATION}s..."
    log_info "Sending requests across ${NODE_COUNT} nodes..."

    # Run vegeta attack
    # -rate: requests per second
    # -duration: how long to run
    # -targets: file with target URLs (round-robins through them)
    # -output: binary results file
    if ! vegeta attack \
        -rate="${REQUEST_RATE}/s" \
        -duration="${TEST_DURATION}s" \
        -targets="$TARGETS_FILE" \
        -output="$OUTPUT_DIR/results.bin" \
        2>/dev/null; then
        log_fail "Vegeta attack failed"
        return 1
    fi

    # Check if results file was created
    if [ ! -f "$OUTPUT_DIR/results.bin" ]; then
        log_fail "Results file not created: $OUTPUT_DIR/results.bin"
        return 1
    fi

    log_info "Load test complete. Analyzing results..."

    # Convert to JSON for analysis
    if ! vegeta encode --to=json < "$OUTPUT_DIR/results.bin" > "$RESULTS_FILE" 2>/dev/null; then
        log_fail "Failed to encode vegeta results to JSON"
        return 1
    fi

    # Check if JSON file was created and has content
    if [ ! -s "$RESULTS_FILE" ]; then
        log_fail "Results JSON file is empty: $RESULTS_FILE"
        return 1
    fi

    return 0
}

analyze_results() {
    # Use jq to bucket requests by second and count successes (non-429) per bucket
    # The vegeta JSON output is JSONL (one JSON object per line) with:
    #   - timestamp: ISO 8601 timestamp string
    #   - code: HTTP status code
    #   - error: error string if any

    log_info "Bucketing requests by second..."

    # Process results: group by second, count allowed (non-429) requests
    # We consider a request "allowed" if it didn't get rate limited (not 429)
    # Note: vegeta outputs JSONL format, so we use -s to slurp into an array
    # The timestamp is ISO format, so we parse it to get the epoch second
    if ! jq -s '
        # Parse timestamp and extract epoch second
        map(. + {epoch_second: (.timestamp | split(".")[0] | strptime("%Y-%m-%dT%H:%M:%S") | mktime)})
        # Group by epoch second
        | group_by(.epoch_second)
        # Aggregate each group
        | map({
            second: .[0].epoch_second,
            total: length,
            allowed: map(select(.code != 429)) | length,
            rate_limited: map(select(.code == 429)) | length,
            errors: map(select(.code == 0 or .code >= 500)) | length
        })
        | sort_by(.second)
    ' "$RESULTS_FILE" > "$ANALYSIS_FILE" 2>/dev/null; then
        log_fail "Failed to analyze results with jq"
        return 1
    fi

    return 0
}

evaluate_results() {
    echo ""
    echo "=========================================="
    echo "Per-Second Window Analysis"
    echo "=========================================="

    # Check if analysis file exists and has content
    if [ ! -f "$ANALYSIS_FILE" ]; then
        log_fail "Analysis file not found: $ANALYSIS_FILE"
        return 1
    fi

    local window_count
    window_count=$(jq 'length' "$ANALYSIS_FILE" 2>/dev/null || echo "0")

    if [ "$window_count" -eq 0 ]; then
        log_fail "No data collected - analysis file is empty"
        return 1
    fi

    local failed_windows=0
    local catastrophic_failure=false
    local total_windows=0
    local total_allowed=0
    local max_allowed_in_window=0
    local min_allowed_in_window=999999
    local windows_over_limit=0

    # Arrays to track failed windows for reporting
    local failed_window_details=()

    # Read and analyze each second
    while IFS= read -r window; do
        local second=$(echo "$window" | jq -r '.second')
        local allowed=$(echo "$window" | jq -r '.allowed')
        local total=$(echo "$window" | jq -r '.total')
        local rate_limited=$(echo "$window" | jq -r '.rate_limited')

        total_windows=$((total_windows + 1))
        total_allowed=$((total_allowed + allowed))

        # Track min/max
        if [ "$allowed" -gt "$max_allowed_in_window" ]; then
            max_allowed_in_window=$allowed
        fi
        if [ "$allowed" -lt "$min_allowed_in_window" ]; then
            min_allowed_in_window=$allowed
        fi

        # Check if over the rate limit (for informational purposes)
        if [ "$allowed" -gt "$RATE_LIMIT" ]; then
            windows_over_limit=$((windows_over_limit + 1))
        fi

        # Check for catastrophic failure
        if [ "$allowed" -gt "$CATASTROPHIC_THRESHOLD" ]; then
            log_fail "CATASTROPHIC: Window at second $second allowed $allowed requests (threshold: $CATASTROPHIC_THRESHOLD)"
            catastrophic_failure=true
        fi

        # Check if window failed (exceeded tolerance)
        if [ "$allowed" -gt "$MAX_ALLOWED_PER_SECOND" ]; then
            failed_windows=$((failed_windows + 1))
            local overshoot_percent=$(echo "scale=1; (($allowed - $RATE_LIMIT) * 100) / $RATE_LIMIT" | bc)
            failed_window_details+=("  Second $second: $allowed allowed (+${overshoot_percent}% over limit)")
        fi

    done < <(jq -c '.[]' "$ANALYSIS_FILE")

    # Handle edge case of no windows
    if [ "$total_windows" -eq 0 ]; then
        log_fail "No time windows were recorded"
        return 1
    fi

    # Calculate statistics
    local avg_allowed=$(echo "scale=2; $total_allowed / $total_windows" | bc)
    local overall_overshoot_percent=$(echo "scale=2; (($avg_allowed - $RATE_LIMIT) * 100) / $RATE_LIMIT" | bc)
    local failed_percent=$(echo "scale=2; ($failed_windows * 100) / $total_windows" | bc)
    local over_limit_percent=$(echo "scale=2; ($windows_over_limit * 100) / $total_windows" | bc)

    # Print summary
    echo ""
    echo "Results Summary:"
    echo "  Total windows analyzed:     $total_windows"
    echo "  Total requests allowed:     $total_allowed"
    echo "  Average allowed/second:     $avg_allowed (limit: $RATE_LIMIT)"
    echo "  Overall overshoot:          ${overall_overshoot_percent}%"
    echo "  Min allowed in a window:    $min_allowed_in_window"
    echo "  Max allowed in a window:    $max_allowed_in_window"
    echo ""
    echo "  Windows over rate limit:    $windows_over_limit (${over_limit_percent}%)"
    echo "  Windows over tolerance:     $failed_windows (${failed_percent}%)"
    echo "  Max failed windows allowed: $MAX_FAILED_WINDOWS (${WINDOW_FAILURE_THRESHOLD_PERCENT}%)"
    echo ""

    # Print failed window details if any
    if [ ${#failed_window_details[@]} -gt 0 ]; then
        echo "Failed Windows (exceeded ${TOLERANCE_PERCENT}% tolerance):"
        for detail in "${failed_window_details[@]}"; do
            echo "$detail"
        done
        echo ""
    fi

    # Determine pass/fail
    echo "=========================================="
    echo "Test Result"
    echo "=========================================="

    if [ "$catastrophic_failure" = true ]; then
        log_fail "TEST FAILED: Catastrophic failure detected (window exceeded ${CATASTROPHIC_MULTIPLIER}x rate limit)"
        return 1
    fi

    if [ "$failed_windows" -gt "$MAX_FAILED_WINDOWS" ]; then
        log_fail "TEST FAILED: Too many windows exceeded tolerance"
        log_fail "  Failed: $failed_windows windows (${failed_percent}%)"
        log_fail "  Allowed: $MAX_FAILED_WINDOWS windows (${WINDOW_FAILURE_THRESHOLD_PERCENT}%)"
        return 1
    fi

    log_pass "TEST PASSED: Distributed rate limiting maintained consistency"
    log_info "  ${failed_windows}/${total_windows} windows exceeded tolerance (${failed_percent}%, limit: ${WINDOW_FAILURE_THRESHOLD_PERCENT}%)"
    log_info "  Average overshoot: ${overall_overshoot_percent}% (tolerance: ${TOLERANCE_PERCENT}%)"

    return 0
}

# =============================================================================
# Main
# =============================================================================

main() {
    # Check dependencies
    check_vegeta_installed
    check_jq_installed

    # Setup
    mkdir -p "$OUTPUT_DIR"

    # Define file paths (used by functions)
    TARGETS_FILE="$OUTPUT_DIR/targets.txt"
    RESULTS_FILE="$OUTPUT_DIR/results.json"
    ANALYSIS_FILE="$OUTPUT_DIR/analysis.json"

    # Print configuration
    print_test_config

    # Wait for services
    wait_for_services

    # Wait for rate limit window to reset before starting
    log_info "Waiting for rate limit windows to reset..."
    sleep 3

    # Create targets file
    create_targets_file

    # Run the load test
    if ! run_load_test; then
        log_fail "Load test failed"
        exit 1
    fi

    # Analyze results
    if ! analyze_results; then
        log_fail "Results analysis failed"
        exit 1
    fi

    # Evaluate and report
    if evaluate_results; then
        exit 0
    else
        exit 1
    fi
}

main "$@"
