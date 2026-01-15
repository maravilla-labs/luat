#!/bin/sh
# Install luat CLI
# Usage: curl -fsSL https://raw.githubusercontent.com/maravilla-labs/luat/main/scripts/install.sh | sh

set -e

REPO="maravilla-labs/luat"
INSTALL_DIR="${LUAT_INSTALL_DIR:-$HOME/.luat/bin}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    printf "${GREEN}info${NC}: %s\n" "$1"
}

warn() {
    printf "${YELLOW}warn${NC}: %s\n" "$1"
}

error() {
    printf "${RED}error${NC}: %s\n" "$1"
    exit 1
}

detect_platform() {
    local os arch

    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        linux)
            case "$arch" in
                x86_64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
                *) error "Unsupported architecture: $arch" ;;
            esac
            ;;
        darwin)
            case "$arch" in
                x86_64) echo "x86_64-apple-darwin" ;;
                arm64) echo "aarch64-apple-darwin" ;;
                *) error "Unsupported architecture: $arch" ;;
            esac
            ;;
        *)
            error "Unsupported operating system: $os"
            ;;
    esac
}

get_latest_version() {
    curl -s "https://api.github.com/repos/$REPO/releases/latest" | \
        grep '"tag_name"' | \
        sed -E 's/.*"([^"]+)".*/\1/'
}

install_luat() {
    info "Detecting platform..."
    local platform
    platform=$(detect_platform)
    info "Platform: $platform"

    info "Fetching latest version..."
    local version
    version=$(get_latest_version)
    if [ -z "$version" ]; then
        error "Failed to get latest version"
    fi
    info "Version: $version"

    local archive="luat-${version}-${platform}.tar.gz"
    local url="https://github.com/$REPO/releases/download/${version}/${archive}"

    info "Downloading $url..."

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download and extract
    local temp_dir
    temp_dir=$(mktemp -d)
    trap "rm -rf $temp_dir" EXIT

    if ! curl -fsSL "$url" -o "$temp_dir/$archive"; then
        error "Failed to download luat"
    fi

    info "Extracting..."
    tar -xzf "$temp_dir/$archive" -C "$temp_dir"

    # Install binary
    mv "$temp_dir/luat" "$INSTALL_DIR/luat"
    chmod +x "$INSTALL_DIR/luat"

    info "Installed luat to $INSTALL_DIR/luat"
    echo ""

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        warn "Add the following to your shell profile:"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""

        # Detect shell and suggest specific file
        local shell_name
        shell_name=$(basename "$SHELL")
        case "$shell_name" in
            bash)
                if [ -f "$HOME/.bashrc" ]; then
                    warn "For bash, add to ~/.bashrc"
                elif [ -f "$HOME/.bash_profile" ]; then
                    warn "For bash, add to ~/.bash_profile"
                fi
                ;;
            zsh)
                warn "For zsh, add to ~/.zshrc"
                ;;
            fish)
                warn "For fish, run: set -U fish_user_paths $INSTALL_DIR \$fish_user_paths"
                ;;
        esac
    fi

    echo ""
    info "Installation complete!"
    echo ""
    echo "Run 'luat --help' to get started."
}

main() {
    echo ""
    echo "  luat installer"
    echo "  =============="
    echo ""

    # Check for required tools
    if ! command -v curl >/dev/null 2>&1; then
        error "curl is required but not installed"
    fi

    if ! command -v tar >/dev/null 2>&1; then
        error "tar is required but not installed"
    fi

    install_luat
}

main
