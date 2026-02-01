.PHONY: test-integration test-integration-up test-integration-down test-integration-logs \
       test-distributed test-distributed-up test-distributed-down test-distributed-logs \
       test-sustained test-sustained-quick

# Get the directory where this Makefile lives
TEST_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
ROOT_DIR := $(abspath $(TEST_DIR)/..)

# Run the full integration test suite
test-integration: test-integration-up
	@echo "Running integration tests..."
	@$(TEST_DIR)/run_tests.sh || ($(MAKE) test-integration-down && exit 1)
	@$(MAKE) test-integration-down

# Start the test environment
test-integration-up:
	@echo "Starting test environment..."
	@docker compose -f $(TEST_DIR)/docker-compose.yaml up -d --build
	@echo "Waiting for services to initialize..."
	@sleep 5

# Stop the test environment
test-integration-down:
	@echo "Stopping test environment..."
	@docker compose -f $(TEST_DIR)/docker-compose.yaml down

# View logs from test environment
test-integration-logs:
	@docker compose -f $(TEST_DIR)/docker-compose.yaml logs -f

# Run the distributed integration test suite
test-distributed: test-distributed-up
	@echo "Running distributed integration tests..."
	@$(TEST_DIR)/run_distributed_tests.sh || ($(MAKE) test-distributed-down && exit 1)
	@$(MAKE) test-distributed-down

# Start the distributed test environment
test-distributed-up:
	@echo "Starting distributed test environment (3 nodes)..."
	@docker compose -f $(TEST_DIR)/docker-compose.distributed.yaml up -d --build
	@echo "Waiting for cluster to form and stabilize..."
	@sleep 10

# Stop the distributed test environment
test-distributed-down:
	@echo "Stopping distributed test environment..."
	@docker compose -f $(TEST_DIR)/docker-compose.distributed.yaml down

# View logs from distributed test environment
test-distributed-logs:
	@docker compose -f $(TEST_DIR)/docker-compose.distributed.yaml logs -f

# Run the sustained distributed rate limiting test (default: 60 seconds)
# This test verifies consistency over a longer time period
# Requires: vegeta, jq
test-sustained: test-distributed-up
	@echo "Running sustained distributed rate limiting test..."
	@$(TEST_DIR)/run_sustained_distributed_test.sh || ($(MAKE) test-distributed-down && exit 1)
	@$(MAKE) test-distributed-down

# Run a quick version of the sustained test (10 seconds) for faster iteration
test-sustained-quick: test-distributed-up
	@echo "Running quick sustained distributed rate limiting test (10s)..."
	@TEST_DURATION=10 $(TEST_DIR)/run_sustained_distributed_test.sh || ($(MAKE) test-distributed-down && exit 1)
	@$(MAKE) test-distributed-down

# Run unit tests
test:
	cargo test

# Run all tests (unit + integration + distributed + sustained)
test-all: test test-integration test-distributed test-sustained
