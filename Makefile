SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

.PHONY: help
help: ## Show available targets.
	@awk 'BEGIN {FS = ":.*##"; printf "\nFerrous Frog commands:\n\n"} /^[a-zA-Z0-9_.-]+:.*##/ {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.PHONY: install
install: ## Install frontend dependencies from the lockfile.
	npm ci

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
build: ## Build the release desktop executable without installers.
	npm run tauri:build -- --no-bundle -- --locked

.PHONY: release
release: ## Build release installers for the current platform.
	npm run tauri:build -- -- --locked

.PHONY: release-linux
release-linux: ## Build Linux deb and AppImage installers without requiring host FUSE.
	@pkg-config --exists librsvg-2.0 || (echo "Linux AppImage packaging needs librsvg-2.0 pkg-config metadata; install/extract the development package and set PKG_CONFIG_PATH." >&2; exit 1)
	APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri:build -- --bundles deb,appimage -- --locked

.PHONY: build-web
build-web: ## Build the React/Vite frontend.
	npm run build

.PHONY: check
check: check-rust check-tauri ## Run Rust checks for workspace crates and the Tauri app.

.PHONY: check-rust
check-rust: ## Check Rust workspace crates except the Tauri app.
	cargo check --workspace --exclude ferrous-frog-app --locked

.PHONY: check-tauri
check-tauri: ## Check the Tauri app crate.
	cargo check -p ferrous-frog-app --locked

.PHONY: check-js-rendering
check-js-rendering: ## Check the optional Chrome CDP backend and test browser discovery.
	cargo check -p ferrous-frog-app --features js-rendering --locked
	cargo test -p ferrous-frog-crawler-core --features js-rendering rendering::tests --locked

.PHONY: test-rendering
test-rendering: check-js-rendering ## Verify browser HTTP politeness, nested requests and pause/stop with Chrome.
	cargo test -p ferrous-frog-crawler-core --features js-rendering chrome_rendering_ --locked -- --ignored --test-threads=1

.PHONY: check-versions
check-versions: ## Verify that Rust, npm, Tauri and release versions agree.
	node scripts/check-versions.mjs

.PHONY: test-release
test-release: ## Check release publication guards and checksum generation without GitHub writes.
	node scripts/test-release-workflow.mjs

.PHONY: lint
lint: ## Run Clippy on all default-feature Rust targets.
	cargo clippy --workspace --all-targets --locked -- -D warnings

.PHONY: test
test: ## Run all Rust tests.
	cargo test --workspace --locked

.PHONY: test-ui
test-ui: build-web ## Exercise the React workspace and production shell in headless Chrome.
	node scripts/check-crawl-graph.mjs
	node scripts/check-schedule.mjs
	node scripts/smoke-ui.mjs

NATIVE_APP ?= target/release/ferrous-frog
NATIVE_DEB ?=
NATIVE_APPIMAGE ?=
export NATIVE_DEB NATIVE_APPIMAGE

.PHONY: test-native
test-native: ## Run the Linux native desktop smoke (built app and WebDriver tools required).
	node scripts/smoke-native.mjs --app "$(NATIVE_APP)"

.PHONY: test-native-installers
test-native-installers: ## Privately extract Linux deb/AppImage installers and run the native smoke from each payload.
	@set -eu; deb="$${NATIVE_DEB}"; appimage="$${NATIVE_APPIMAGE}"; \
	if test -z "$$deb"; then deb=$$(find target/release/bundle/deb -name '*.deb' -type f -print -quit 2>/dev/null || true); fi; \
	if test -z "$$appimage"; then appimage=$$(find target/release/bundle/appimage -name '*.AppImage' -type f -print -quit 2>/dev/null || true); fi; \
	test -n "$$deb" && test -n "$$appimage" || (echo "Build deb and AppImage installers first, or set NATIVE_DEB and NATIVE_APPIMAGE." >&2; exit 1); \
	node scripts/verify-linux-installers.mjs --deb "$$deb" --appimage "$$appimage"

.PHONY: test-audit-reports
test-audit-reports: ## Verify complete report and comparison packages directly from disk in Chrome.
	@set -eu; report_check_dir=$$(mktemp -d); trap 'rm -rf "$$report_check_dir"' EXIT; \
	FF_AUDIT_OFFLINE_FIXTURE_DIR="$$report_check_dir/report" cargo test -p ferrous-frog-export --locked portable_report_exports_every; \
	node scripts/check-offline-audit-report.mjs "$$report_check_dir/report"; \
	FF_AUDIT_COMPARISON_OFFLINE_FIXTURE_DIR="$$report_check_dir/comparison" cargo test -p ferrous-frog-export --locked comparison_package_streams_1205; \
	node scripts/check-offline-audit-report.mjs "$$report_check_dir/comparison"

BENCH_URLS ?= 1000000

.PHONY: bench-synthetic
bench-synthetic: ## Run the release-mode SQLite synthetic benchmark. Override with BENCH_URLS=10000.
	BENCH_URLS=$(BENCH_URLS) cargo test -p ferrous-frog-storage --release --locked sqlite_large_synthetic_storage_benchmark -- --ignored --nocapture

.PHONY: fmt
fmt: ## Format Rust source.
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check Rust formatting.
	cargo fmt --all -- --check

.PHONY: verify ci
verify: ci ## Run the same checks as GitHub Actions (Chrome required).
ci: check-versions test-release fmt-check lint test build-web test-ui test-audit-reports test-rendering ## Run all CI checks.

.PHONY: clean
clean: ## Remove generated build outputs.
	cargo clean
	rm -rf dist
