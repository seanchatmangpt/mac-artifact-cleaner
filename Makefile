.PHONY: help build test lint fmt check clean release install clippy

help: ## Display this help message
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_\-]+:.*?## / {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the project in debug mode
	cargo build

release: ## Build the project in release mode
	cargo build --release

install: release ## Install the CLI globally (to ~/.cargo/bin)
	cargo install --path .

test: ## Run the test suite
	cargo test

lint: clippy ## Run linters (clippy)

clippy:
	cargo clippy -- -D warnings

fmt: ## Format the codebase
	cargo fmt --all

check: fmt lint test ## Run all checks (format, lint, tests)

clean: ## Clean the target directory
	cargo clean
