.PHONY: test-integration test-integration-up test-integration-down test-integration-logs

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

# Run unit tests
test:
	cargo test

# Run all tests (unit + integration)
test-all: test test-integration
