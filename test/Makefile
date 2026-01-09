.PHONY: test-integration test-integration-up test-integration-down test-integration-logs \
       test-distributed test-distributed-up test-distributed-down test-distributed-logs

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
	@docker-compose -f $(TEST_DIR)/docker-compose.yaml up -d --build
	@echo "Waiting for services to initialize..."
	@sleep 5

# Stop the test environment
test-integration-down:
	@echo "Stopping test environment..."
	@docker-compose -f $(TEST_DIR)/docker-compose.yaml down

# View logs from test environment
test-integration-logs:
	@docker-compose -f $(TEST_DIR)/docker-compose.yaml logs -f

# Run the distributed integration test suite
test-distributed: test-distributed-up
	@echo "Running distributed integration tests..."
	@$(TEST_DIR)/run_distributed_tests.sh || ($(MAKE) test-distributed-down && exit 1)
	@$(MAKE) test-distributed-down

# Start the distributed test environment
test-distributed-up:
	@echo "Starting distributed test environment (3 nodes)..."
	@docker-compose -f $(TEST_DIR)/docker-compose.distributed.yaml up -d --build
	@echo "Waiting for cluster to form and stabilize..."
	@sleep 10

# Stop the distributed test environment
test-distributed-down:
	@echo "Stopping distributed test environment..."
	@docker-compose -f $(TEST_DIR)/docker-compose.distributed.yaml down

# View logs from distributed test environment
test-distributed-logs:
	@docker-compose -f $(TEST_DIR)/docker-compose.distributed.yaml logs -f

# Run unit tests
test:
	cargo test

# Run all tests (unit + integration + distributed)
test-all: test test-integration test-distributed
