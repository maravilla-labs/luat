#!/bin/bash
# Development environment setup script for Luat
# Usage: ./scripts/setup-dev.sh [--wasm]

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}!${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Darwin*)    echo "macos";;
        Linux*)     echo "linux";;
        CYGWIN*|MINGW*|MSYS*) echo "windows";;
        *)          echo "unknown";;
    esac
}

# Install Rust if not present
setup_rust() {
    print_step "Checking Rust installation..."

    if command_exists rustc; then
        local rust_version=$(rustc --version | cut -d' ' -f2)
        print_success "Rust $rust_version is installed"
    else
        print_warning "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        print_success "Rust installed successfully"
    fi
}

# Setup WASM development environment
setup_wasm() {
    print_step "Setting up WASM development environment..."

    local os=$(detect_os)
    local emsdk_dir="$HOME/emsdk"

    # Check if Emscripten is already available
    if command_exists emcc; then
        local em_version=$(emcc --version 2>/dev/null | head -1)
        print_success "Emscripten is already installed: $em_version"
    else
        print_step "Installing Emscripten SDK..."

        if [ "$os" = "macos" ] && command_exists brew; then
            # Try homebrew first on macOS
            print_step "Installing via Homebrew..."
            brew install emscripten
            print_success "Emscripten installed via Homebrew"
        else
            # Manual installation
            if [ -d "$emsdk_dir" ]; then
                print_warning "emsdk directory exists at $emsdk_dir, updating..."
                cd "$emsdk_dir"
                git pull
            else
                print_step "Cloning emsdk repository..."
                git clone https://github.com/emscripten-core/emsdk.git "$emsdk_dir"
                cd "$emsdk_dir"
            fi

            print_step "Installing latest Emscripten..."
            ./emsdk install latest
            ./emsdk activate latest

            # Add to shell profile
            local shell_rc=""
            if [ -f "$HOME/.zshrc" ]; then
                shell_rc="$HOME/.zshrc"
            elif [ -f "$HOME/.bashrc" ]; then
                shell_rc="$HOME/.bashrc"
            fi

            if [ -n "$shell_rc" ]; then
                if ! grep -q "emsdk_env.sh" "$shell_rc" 2>/dev/null; then
                    echo "" >> "$shell_rc"
                    echo "# Emscripten SDK" >> "$shell_rc"
                    echo "source \"$emsdk_dir/emsdk_env.sh\" 2>/dev/null" >> "$shell_rc"
                    print_success "Added emsdk_env.sh to $shell_rc"
                fi
            fi

            # Source for current session
            source "$emsdk_dir/emsdk_env.sh"
            print_success "Emscripten SDK installed"
        fi
    fi

    # Add Rust WASM target
    print_step "Adding wasm32-unknown-emscripten Rust target..."
    if rustup target list | grep -q "wasm32-unknown-emscripten (installed)"; then
        print_success "wasm32-unknown-emscripten target already installed"
    else
        rustup target add wasm32-unknown-emscripten
        print_success "wasm32-unknown-emscripten target installed"
    fi

    # Verify setup
    print_step "Verifying WASM setup..."
    if command_exists emcc; then
        print_success "emcc is available"
    else
        print_error "emcc not found in PATH. You may need to restart your shell or run:"
        echo "    source ~/emsdk/emsdk_env.sh"
    fi
}

# Verify the development environment
verify_setup() {
    print_step "Verifying development environment..."

    local all_good=true

    if command_exists rustc; then
        print_success "rustc: $(rustc --version | cut -d' ' -f2)"
    else
        print_error "rustc not found"
        all_good=false
    fi

    if command_exists cargo; then
        print_success "cargo: $(cargo --version | cut -d' ' -f2)"
    else
        print_error "cargo not found"
        all_good=false
    fi

    if command_exists git; then
        print_success "git: $(git --version | cut -d' ' -f3)"
    else
        print_error "git not found"
        all_good=false
    fi

    if [ "$all_good" = true ]; then
        print_success "All prerequisites are installed!"
    else
        print_error "Some prerequisites are missing"
        exit 1
    fi
}

# Test building the project
test_build() {
    local wasm_mode=$1

    print_step "Testing project build..."

    # Native build
    cargo check --workspace
    print_success "Native build check passed"

    # WASM build (if requested)
    if [ "$wasm_mode" = true ]; then
        print_step "Testing WASM build..."
        cargo check --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm
        print_success "WASM build check passed"
    fi
}

# Main script
main() {
    echo ""
    echo "Luat Development Environment Setup"
    echo "==================================="
    echo ""

    local wasm_mode=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --wasm)
                wasm_mode=true
                shift
                ;;
            --help|-h)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  --wasm    Setup WASM development environment (includes Emscripten)"
                echo "  --help    Show this help message"
                exit 0
                ;;
            *)
                print_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done

    # Run setup steps
    setup_rust
    verify_setup

    if [ "$wasm_mode" = true ]; then
        setup_wasm
    fi

    test_build $wasm_mode

    echo ""
    print_success "Development environment setup complete!"
    echo ""

    if [ "$wasm_mode" = true ]; then
        echo "WASM development is ready. Build commands:"
        echo "  cargo build --package luat --target wasm32-unknown-emscripten --no-default-features --features wasm"
        echo ""
        print_warning "Note: You may need to restart your shell or run 'source ~/emsdk/emsdk_env.sh' for Emscripten to be available."
    fi

    echo ""
    echo "Next steps:"
    echo "  cargo build --workspace    # Build all crates"
    echo "  cargo test --workspace     # Run all tests"
    echo ""
}

main "$@"
