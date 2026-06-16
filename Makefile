SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show available targets.
	@awk 'BEGIN {FS = ":.*##"; printf "\nFerrous Frog commands:\n\n"} /^[a-zA-Z0-9_.-]+:.*##/ {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.PHONY: install
install: ## Install frontend dependencies.
	npm install

.PHONY: dev
dev: tauri-dev ## Start the Tauri desktop app in development mode.

.PHONY: dev-web
dev-web: ## Start the Vite web dev server only.
	npm run dev -- --host 127.0.0.1

.PHONY: stop-dev
stop-dev: ## Stop local Ferrous Frog Vite dev servers on ports 1420 and 1421.
	@for port in 1420 1421; do \
		pids=$$(lsof -tiTCP:$$port -sTCP:LISTEN 2>/dev/null || true); \
		if [ -n "$$pids" ]; then \
			echo "Stopping port $$port: $$pids"; \
			kill $$pids; \
		fi; \
	done

.PHONY: tauri-dev
tauri-dev: ## Start the Tauri desktop app in development mode.
	npm run tauri:dev

.PHONY: build
build: build-web check-tauri ## Build frontend and check the Tauri app.

.PHONY: build-web
build-web: ## Build the React/Vite frontend.
	npm run build

.PHONY: check
check: check-rust check-tauri ## Run Rust checks for workspace crates and the Tauri app.

.PHONY: check-rust
check-rust: ## Check Rust workspace crates except the Tauri app.
	cargo check --workspace --exclude ferrous-frog-app

.PHONY: check-tauri
check-tauri: ## Check the Tauri app crate.
	cargo check -p ferrous-frog-app

.PHONY: test
test: ## Run all Rust tests.
	cargo test --workspace

.PHONY: fmt
fmt: ## Format Rust source.
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check Rust formatting.
	cargo fmt --all -- --check

.PHONY: verify
verify: fmt-check test build-web ## Run formatting, tests, and frontend build.

.PHONY: clean
clean: ## Remove generated build outputs.
	cargo clean
	rm -rf dist
