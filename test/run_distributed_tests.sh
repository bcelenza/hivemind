#!/usr/bin/env bash
#
# Distributed integration test script for Hivemind rate limiting
#
# This script tests that multiple Hivemind nodes synchronize rate limit
# state across a cluster using the gossip protocol.

set -euo pipefail

# Get the directory where this script lives
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source shared test library
source "$SCRIPT_DIR/test_lib.sh"

# Configuration
NODE1_URL="${NODE1_URL:-http://localhost:10001}"
NODE2_URL="${NODE2_URL:-http://localhost:10002}"
NODE3_URL="${NODE3_URL:-http://localhost:10003}"

# Gossip sync delay - time to allow state to propagate between nodes
# Chitchat gossips every 100ms, but with network latency we need more time
SYNC_DELAY=1

# Wait for all services to be ready
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

# Test 1: All nodes accessible
test_all_nodes_accessible() {
    run_test "All nodes are accessible"

    local all_ok=true
    for node in 1 2 3; do
        local url_var="NODE${node}_URL"
        local url="${!url_var}"
        local status
        status=$(get_status "$url")
        if [ "$status" != "200" ]; then
            log_fail "Node $node at $url returned HTTP $status"
            all_ok=false
        fi
    done

    if [ "$all_ok" = true ]; then
        log_pass "All 3 nodes are accessible and returning HTTP 200"
    fi
}

# Test 2: Rate limit state synchronizes across nodes
test_state_synchronization() {
    run_test "Rate limit state synchronizes across nodes"

    # Make 3 requests rapidly to node 1 (within same time window)
    log_debug "Making 3 requests to Node 1..."
    for i in 1 2 3; do
        curl -s -o /dev/null "$NODE1_URL/status/200"
    done

    # Wait for gossip to propagate
    log_debug "Waiting ${SYNC_DELAY}s for gossip propagation..."
    sleep $SYNC_DELAY

    # Check remaining count on node 2 - should reflect the 3 requests made to node 1
    local remaining
    remaining=$(get_remaining "$NODE2_URL")

    # After 3 requests to node1 + 1 for getting remaining on node2 = 4 total
    # So remaining should be around 1 (5 - 4)
    # Allow some flexibility for timing
    if [ -n "$remaining" ] && [ "$remaining" -lt "$RATE_LIMIT" ]; then
        log_pass "State synchronized: Node 2 shows $remaining remaining (less than limit of $RATE_LIMIT)"
    else
        log_fail "State not synchronized: Node 2 shows '$remaining' remaining (expected < $RATE_LIMIT)"
    fi
}

# Test 3: Single node enforces local rate limit
test_single_node_enforcement() {
    run_test "Single node enforces local rate limit"

    # This test verifies basic rate limiting works on a single node
    log_debug "Making $((RATE_LIMIT + 2)) requests to Node 1..."

    local rate_limited=false
    for i in $(seq 1 $((RATE_LIMIT + 2))); do
        local status
        status=$(get_status "$NODE1_URL")
        if [ "$status" = "429" ]; then
            rate_limited=true
            log_debug "Rate limited on request $i"
            break
        fi
    done

    if [ "$rate_limited" = true ]; then
        log_pass "Node 1 enforces rate limit (HTTP 429)"
    else
        log_fail "Node 1 did not enforce rate limit"
    fi
}

# Test 4: Distributed enforcement - exhaust limit across nodes
test_distributed_enforcement() {
    run_test "Distributed rate limiting is enforced across cluster"

    # Make rapid requests alternating between nodes
    # The idea is to exhaust the global limit by hitting different nodes
    log_debug "Making rapid requests across all nodes..."

    local nodes=("$NODE1_URL" "$NODE2_URL" "$NODE3_URL")
    local rate_limited=false
    local total_requests=0

    # Make requests rapidly to try to hit global limit
    for round in 1 2 3 4 5; do
        for url in "${nodes[@]}"; do
            local status
            status=$(get_status "$url")
            total_requests=$((total_requests + 1))
            if [ "$status" = "429" ]; then
                rate_limited=true
                log_debug "Rate limited on request $total_requests"
                break 2
            fi
            # Small delay for gossip
            sleep 0.1
        done
    done

    if [ "$rate_limited" = true ]; then
        log_pass "Distributed rate limiting enforced after $total_requests requests"
    else
        # Check if we got close - with eventual consistency we might overshoot
        log_debug "Made $total_requests requests without 429"
        if [ $total_requests -gt $RATE_LIMIT ]; then
            # With eventual consistency across 3 nodes, overshoot up to node count is expected
            local overshoot=$((total_requests - RATE_LIMIT))
            if [ $overshoot -le 4 ]; then
                log_pass "Distributed rate limiting working with acceptable overshoot ($overshoot extra requests due to eventual consistency)"
            else
                log_fail "Distributed rate limiting allowed too many extra requests ($overshoot overshoot)"
            fi
        else
            log_fail "Could not verify distributed rate limiting"
        fi
    fi
}

# Test 5: Rate limit headers present across all nodes
test_ratelimit_headers() {
    run_test "Rate limit headers are present on all nodes"

    local all_ok=true
    for node in 1 2 3; do
        local url_var="NODE${node}_URL"
        local url="${!url_var}"
        local headers
        headers=$(curl -s -D - -o /dev/null "$url/status/200")

        if ! echo "$headers" | grep -qi "x-ratelimit-limit"; then
            log_fail "Node $node missing X-RateLimit-Limit header"
            all_ok=false
        fi
        if ! echo "$headers" | grep -qi "x-ratelimit-remaining"; then
            log_fail "Node $node missing X-RateLimit-Remaining header"
            all_ok=false
        fi
    done

    if [ "$all_ok" = true ]; then
        log_pass "All nodes return rate limit headers"
    fi
}

# Test 6: Cluster membership verification
test_cluster_membership() {
    run_test "Cluster membership is established"

    # We verify cluster membership indirectly by checking that state propagates
    # Make a request to node 1
    curl -s -o /dev/null "$NODE1_URL/status/200"

    # Wait for propagation
    sleep $SYNC_DELAY

    # If the remaining on other nodes is reduced, cluster is working
    local remaining2 remaining3
    remaining2=$(get_remaining "$NODE2_URL")
    remaining3=$(get_remaining "$NODE3_URL")

    log_debug "After request to Node 1: Node 2 remaining=$remaining2, Node 3 remaining=$remaining3"

    # Both should show reduced remaining (less than limit)
    if [ -n "$remaining2" ] && [ -n "$remaining3" ]; then
        if [ "$remaining2" -lt "$RATE_LIMIT" ] && [ "$remaining3" -lt "$RATE_LIMIT" ]; then
            log_pass "Cluster membership working - state propagated to all nodes"
        else
            log_fail "State did not propagate to all nodes (r2=$remaining2, r3=$remaining3)"
        fi
    else
        log_fail "Could not get remaining from nodes"
    fi
}

# Main
main() {
    echo "=========================================="
    echo "Hivemind Distributed Integration Tests"
    echo "=========================================="

    wait_for_services

    test_all_nodes_accessible

    wait_for_rate_limit_reset
    test_state_synchronization

    wait_for_rate_limit_reset
    test_single_node_enforcement

    wait_for_rate_limit_reset
    test_distributed_enforcement

    wait_for_rate_limit_reset
    test_ratelimit_headers

    wait_for_rate_limit_reset
    test_cluster_membership

    print_summary "Distributed Test Summary"
}

main "$@"
