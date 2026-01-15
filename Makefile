# Luat Development Makefile
# Run `make help` to see available commands

.PHONY: help build test check fmt clippy clean wasm wasm-check setup setup-wasm dev install

# Default target
help:
	@echo "Luat Development Commands"
	@echo "========================="
	@echo ""
	@echo "Build Commands:"
	@echo "  make build        Build all crates (native)"
	@echo "  make install      Install luat CLI locally"
	@echo "  make test         Run all tests"
	@echo "  make check        Type-check all crates"
	@echo "  make clippy       Run clippy lints"
	@echo "  make fmt          Format code"
	@echo "  make clean        Clean build artifacts"
	@echo ""
	@echo "WASM Commands:"
	@echo "  make wasm         Build luat-wasm for WASM (debug)"
	@echo "  make wasm-release Build luat-wasm for WASM (release)"
	@echo "  make wasm-check   Type-check WASM build (faster)"
	@echo "  make wasm-test    Build and run WASM tests in Node.js"
	@echo ""
	@echo "Setup Commands:"
	@echo "  make setup        Setup development environment"
	@echo "  make setup-wasm   Setup WASM development environment"
	@echo ""
	@echo "Development:"
	@echo "  make dev          Start CLI dev server"
	@echo ""

# Native build commands
build:
	cargo build --workspace

install:
	cargo install --path crates/luat-cli

test:
	cargo test --workspace

check:
	cargo check --workspace

clippy:
	cargo clippy --workspace -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clean:
	cargo clean

# WASM build commands
# Helper to source emsdk if emcc is not found
EMSDK_ENV := $(HOME)/emsdk/emsdk_env.sh
define ensure_emcc
	@if ! command -v emcc >/dev/null 2>&1; then \
		if [ -f "$(EMSDK_ENV)" ]; then \
			echo "Sourcing emsdk environment..."; \
			. "$(EMSDK_ENV)" && $(1); \
		else \
			echo "Error: emcc not found. Install emsdk: https://emscripten.org/docs/getting_started/downloads.html"; \
			echo "Then run: ./scripts/setup-dev.sh --wasm"; \
			exit 1; \
		fi; \
	else \
		$(1); \
	fi
endef

wasm:
	$(call ensure_emcc,cargo build --package luat-wasm --target wasm32-unknown-emscripten)

wasm-check:
	$(call ensure_emcc,cargo check --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm)

wasm-release:
	$(call ensure_emcc,cargo build --package luat-wasm --target wasm32-unknown-emscripten --release)

wasm-test:
	$(call ensure_emcc,cargo build --package luat-wasm --target wasm32-unknown-emscripten --release && node target/wasm32-unknown-emscripten/release/luat-wasm.js)

# Setup commands
setup:
	./scripts/setup-dev.sh

setup-wasm:
	./scripts/setup-dev.sh --wasm

# Development
dev:
	cargo run -p luat-cli -- dev

# CI commands (used by GitHub Actions)
ci: fmt-check clippy test
	@echo "CI checks passed!"

ci-wasm: wasm-check
	@echo "WASM CI check passed!"
