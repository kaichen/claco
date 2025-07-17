#!/usr/bin/env bash

set -euo pipefail

# Configuration
REPO_OWNER="kaichen"
REPO_NAME="claco"
BINARY_NAME="claco"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
error() {
    echo -e "${RED}Error: $1${NC}" >&2
    exit 1
}

info() {
    echo -e "${GREEN}$1${NC}"
}

warn() {
    echo -e "${YELLOW}$1${NC}"
}

# Detect OS and architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)
            os="linux"
            ;;
        Darwin*)
            os="darwin"
            ;;
        *)
            error "Unsupported operating system: $(uname -s)"
            ;;
    esac

    case "$(uname -m)" in
        x86_64)
            arch="x86_64"
            ;;
        aarch64|arm64)
            arch="aarch64"
            ;;
        *)
            error "Unsupported architecture: $(uname -m)"
            ;;
    esac

    echo "${os}-${arch}"
}

# Get the latest release version from GitHub
get_latest_version() {
    local api_url="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest"
    
    if command -v curl &> /dev/null; then
        curl -s "$api_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    elif command -v wget &> /dev/null; then
        wget -qO- "$api_url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
    else
        error "Neither curl nor wget is available. Please install one of them."
    fi
}


# Install binary to target directory
install_binary() {
    local binary_path="$1"
    local target_path="${INSTALL_DIR}/${BINARY_NAME}"
    
    # Create directory if it doesn't exist
    if [[ ! -d "$INSTALL_DIR" ]]; then
        if [[ -w "$(dirname "$INSTALL_DIR")" ]]; then
            info "Creating directory ${INSTALL_DIR}..."
            mkdir -p "$INSTALL_DIR"
        else
            warn "Root access required to create ${INSTALL_DIR}"
            sudo mkdir -p "$INSTALL_DIR"
        fi
    fi
    
    # Check if we need sudo
    if [[ -w "$INSTALL_DIR" ]]; then
        info "Installing ${BINARY_NAME} to ${INSTALL_DIR}..."
        cp "$binary_path" "$target_path"
        chmod +x "$target_path"
    else
        warn "Root access required to install to ${INSTALL_DIR}"
        info "Installing ${BINARY_NAME} to ${INSTALL_DIR}..."
        sudo cp "$binary_path" "$target_path"
        sudo chmod +x "$target_path"
    fi
    
    info "Installation complete!"
}

# Main installation flow
main() {
    info "Installing ${BINARY_NAME}..."
    
    # Check if binary already exists
    if command -v "$BINARY_NAME" &> /dev/null; then
        warn "${BINARY_NAME} is already installed at $(which ${BINARY_NAME})"
        read -p "Do you want to continue and overwrite? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            info "Installation cancelled"
            exit 0
        fi
    fi
    
    # Detect platform
    local platform
    platform=$(detect_platform)
    info "Detected platform: ${platform}"
    
    # Get latest version
    local version
    version=$(get_latest_version)
    if [[ -z "$version" ]]; then
        error "Failed to get latest version"
    fi
    info "Latest version: ${version}"
    
    # Download and install binary
    local temp_dir
    temp_dir=$(mktemp -d)
    trap "rm -rf $temp_dir" EXIT
    
    # Map platform to release asset naming
    local release_platform
    case "$platform" in
        darwin-x86_64)
            release_platform="x86_64-apple-darwin"
            ;;
        darwin-aarch64)
            release_platform="aarch64-apple-darwin"
            ;;
        linux-x86_64)
            release_platform="x86_64-unknown-linux-musl"
            ;;
        linux-aarch64)
            release_platform="aarch64-unknown-linux-musl"
            ;;
        *)
            error "Unsupported platform: $platform"
            ;;
    esac
    
    # Construct download URL
    local download_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${version}/${BINARY_NAME}-${version}-${release_platform}.tar.gz"
    local archive_path="${temp_dir}/${BINARY_NAME}.tar.gz"
    
    info "Downloading ${BINARY_NAME} ${version} for ${platform}..."
    
    if command -v curl &> /dev/null; then
        curl -L -o "$archive_path" "$download_url" || error "Failed to download binary"
    elif command -v wget &> /dev/null; then
        wget -O "$archive_path" "$download_url" || error "Failed to download binary"
    fi
    
    # Extract binary
    info "Extracting binary..."
    tar -xzf "$archive_path" -C "$temp_dir" || error "Failed to extract archive"
    
    # Find the binary
    local binary_path="${temp_dir}/${BINARY_NAME}"
    if [[ ! -f "$binary_path" ]]; then
        binary_path=$(find "$temp_dir" -name "$BINARY_NAME" -type f | head -1)
        if [[ -z "$binary_path" ]]; then
            error "Binary not found in archive"
        fi
    fi
    
    # Install binary
    install_binary "$binary_path"
    
    # Verify installation
    local installed_path="${INSTALL_DIR}/${BINARY_NAME}"
    if [[ -f "$installed_path" && -x "$installed_path" ]]; then
        info "${BINARY_NAME} has been successfully installed!"
        info "Location: ${installed_path}"
        info "Version: $("$installed_path" --version 2>/dev/null || echo 'version info not available')"
        
        # Check if install dir is in PATH
        if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
            warn "Note: ${INSTALL_DIR} is not in your PATH"
            info "To use ${BINARY_NAME}, either:"
            info "  1. Add to PATH: export PATH=\"${INSTALL_DIR}:\$PATH\""
            info "  2. Use full path: ${installed_path}"
        fi
    else
        error "Installation verification failed"
    fi
}

# Run main function
main "$@"