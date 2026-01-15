# Contributing to luat

Thank you for your interest in contributing to luat! This document provides guidelines and information for contributors.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/luat
   cd luat
   ```
3. **Create a branch** for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites

- Rust 1.75 or later
- Git

### Quick Setup

We provide a setup script that installs all development dependencies:

```bash
# Run the setup script
./scripts/setup-dev.sh

# Or for WASM development specifically
./scripts/setup-dev.sh --wasm
```

### Building

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run clippy
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all
```

## Architecture Overview

Luat is split into a core engine and thin adapters (CLI, WASM). The engine is resolver-driven so host environments can swap resolution strategies without re-implementing routing or action logic.

### Core Concepts (Engine)

- **Resolver-first design**: `ResourceResolver` is the extension point. Different resolvers (filesystem, memory, bundle) provide the same behavior in dev/prod/WASM.
- **File-based routing**: `+page.luat`, `+page.server.lua`, `+layout.luat`, `+layout.server.lua`, `+server.lua`, `+error.luat` in `src/routes/`. Dynamic segments use `[param]`, `[[optional]]`, `[...rest]`.
- **Actions**: Defined in `+page.server.lua` as an `actions` table. Requests are actions when:
  - method is not `GET`, or
  - query contains `?/actionName` (e.g. `?/login`).
- **Action templates**: Optional `*.luat` siblings of `+page.luat` for post-action rendering:
  - `METHOD.action.luat` (e.g. `POST.login.luat`),
  - `method.action.luat`,
  - `action.luat` (fallback).
- **Fragments**: Action template HTML is returned as a fragment; adapters detect `x-luat-fragment` and skip wrapping in `app.html`.

### Bundling and Production

- **Bundle output**: Build generates `__routes`, `__server_sources`, and `__require_map` in the bundle.
- **Server sources**: `+page.server.lua` and `+server.lua` are loaded via `__server_sources` in production.
- **Require resolution**: Uses the same resolution rules in dev/prod with alias support (`$lib`, `lib/`).

### Adapter Responsibilities

- **CLI dev server**: Converts HTTP requests to `LuatRequest`, calls `engine.respond_async`, and wraps HTML in `app.html`.
- **Production server**: Loads bundle, sets up resolver, and delegates to the engine for routing/actions.
- **WASM**: Uses the same engine logic with a memory resolver.

## WASM Development

Luat supports WebAssembly compilation for running in browsers and edge runtimes. This requires the Emscripten SDK.

### Automatic Setup (Recommended)

The easiest way to set up WASM development is using our setup script:

```bash
./scripts/setup-dev.sh --wasm
```

This will:
1. Install the Emscripten SDK (if not present)
2. Add the `wasm32-unknown-emscripten` Rust target
3. Configure your environment

### Manual Setup

If you prefer manual installation:

#### 1. Install Emscripten SDK

**macOS (Homebrew):**
```bash
brew install emscripten
```

**Linux/macOS (Manual):**
```bash
# Clone the SDK
git clone https://github.com/emscripten-core/emsdk.git ~/emsdk

# Install and activate latest version
cd ~/emsdk
./emsdk install latest
./emsdk activate latest

# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)
echo 'source ~/emsdk/emsdk_env.sh' >> ~/.bashrc
source ~/.bashrc
```

**Windows:**
```powershell
# Clone the SDK
git clone https://github.com/emscripten-core/emsdk.git C:\emsdk

# Install and activate
cd C:\emsdk
.\emsdk.bat install latest
.\emsdk.bat activate latest

# Add to PATH (run as administrator)
.\emsdk_env.bat
```

#### 2. Add Rust WASM Target

```bash
rustup target add wasm32-unknown-emscripten
```

#### 3. Verify Installation

```bash
# Check em++ is available
em++ --version

# Check Rust target is installed
rustup target list | grep wasm32-unknown-emscripten
```

### Building for WASM

```bash
# Build the luat crate for WASM
cargo build --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm

# Check only (faster, no output artifacts)
cargo check --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm
```

### WASM Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `native` | Full native support with threading and filesystem | Yes |
| `wasm` | WASM-compatible build (single-threaded, no filesystem) | No |
| `send` | Enable `Send` trait for multi-threaded usage | Yes (with native) |
| `async-lua` | Enable async Lua methods | Yes (with native) |
| `filesystem` | Enable filesystem resolver and cache | Yes (with native) |

### Troubleshooting WASM Builds

**"em++" not found:**
- Ensure Emscripten is installed and `emsdk_env.sh` is sourced
- Run `source ~/emsdk/emsdk_env.sh` in your current terminal

**Missing WASM target:**
- Run `rustup target add wasm32-unknown-emscripten`

**Build errors with async/threading:**
- Ensure you're using `--no-default-features --features wasm`
- WASM builds cannot use the `native` feature

### Running the CLI in Development

```bash
# Run the CLI
cargo run -p luat-cli -- --help

# Start dev server
cargo run -p luat-cli -- dev

# Initialize a new project
cargo run -p luat-cli -- init test-project
```

## Code Style

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` for formatting
- All public APIs must have documentation
- Add tests for new functionality
- Keep commits focused and atomic

## Pull Request Process

1. **Update documentation** for any changed APIs
2. **Add tests** for new features or bug fixes
3. **Update CHANGELOG.md** with a description of your changes
4. **Run the test suite** and ensure all tests pass:
   ```bash
   cargo test --workspace
   cargo clippy --workspace -- -D warnings
   cargo fmt --all -- --check
   ```
5. **Submit your PR** with a clear description of the changes

### PR Title Format

Use a clear, descriptive title:
- `feat: add new template directive`
- `fix: resolve parsing issue with nested components`
- `docs: update README examples`
- `refactor: simplify cache implementation`

## Reporting Issues

When reporting issues, please include:

1. **luat version** (`luat --version` or check Cargo.toml)
2. **Rust version** (`rustc --version`)
3. **Operating system**
4. **Steps to reproduce** the issue
5. **Expected behavior** vs **actual behavior**
6. **Relevant template code** (if applicable)

## Feature Requests

Feature requests are welcome! Please:

1. Check existing issues to avoid duplicates
2. Clearly describe the use case
3. Explain why existing features don't meet your needs
4. Consider if you'd be willing to implement it

## Development Guidelines

### Adding New Features

1. Discuss significant changes in an issue first
2. Keep the API surface minimal
3. Maintain backward compatibility when possible
4. Add comprehensive tests

### Modifying the Parser

The parser uses PEST grammar files located in `crates/luat/src/`:
- `grammar.pest` - Main template grammar
- `lua53.pest` - Lua expression grammar
- `luat_extensions.pest` - Custom extensions

When modifying the grammar:
1. Update relevant `.pest` files
2. Run `cargo build` to regenerate parser
3. Update tests in `parser.rs`
4. Update documentation if syntax changes

## WebAssembly (WASM) Support

Luat can be compiled to WebAssembly for running in browsers, edge runtimes, and other WASM environments.

### Building for WASM

```bash
# 1. Install Emscripten SDK (required for mlua WASM support)
git clone https://github.com/emscripten-core/emsdk.git
cd emsdk
./emsdk install latest
./emsdk activate latest
source ./emsdk_env.sh

# 2. Add the WASM target
rustup target add wasm32-unknown-emscripten

# 3. Build the luat crate for WASM
cargo build --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm
```

### Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `native` | Full native support with threading and filesystem | Yes |
| `wasm` | WASM-compatible build (single-threaded, no filesystem) | No |
| `send` | Enable `Send` trait for multi-threaded usage | Yes (with native) |
| `async-lua` | Enable async Lua methods | Yes (with native) |
| `filesystem` | Enable filesystem resolver and cache | Yes (with native) |

### WASM Limitations

When building for WASM, the following features are not available:
- `FileSystemResolver` and `FileSystemCache` (use `MemoryResourceResolver` and `MemoryCache` instead)
- Async Lua methods (`render_from_bundle` async)
- Thread-safe types (`Send` trait)


### Testing

- Unit tests go in the same file as the code (`#[cfg(test)]` module)
- Integration tests go in `tests/` directory
- Use `tempfile` for tests that need filesystem access
- Test both success and error cases

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT OR Apache-2.0).

## Questions?

If you have questions, feel free to:
- Open a discussion on GitHub
- Ask in the issue comments
- Reach out to the maintainers

Thank you for contributing!
