#!/usr/bin/env bash

set -euo pipefail

# Configuration
REPO_OWNER="kaichen"
REPO_NAME="claco"
BINARY_NAME="claco"

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

# Detect install directory
detect_install_dir() {
    # If INSTALL_DIR is already set, use it
    if [[ -n "${INSTALL_DIR:-}" ]]; then
        echo "$INSTALL_DIR"
        return
    fi
    
    # Check for user-writable directories in PATH
    local path_dirs=()
    IFS=':' read -ra path_dirs <<< "$PATH"
    
    # Preferred user directories
    local user_dirs=("$HOME/.local/bin" "$HOME/bin")
    
    # Check if preferred directories exist and are in PATH
    for dir in "${user_dirs[@]}"; do
        if [[ -d "$dir" ]] && [[ -w "$dir" ]]; then
            # Check if this directory is in PATH
            for path_dir in "${path_dirs[@]}"; do
                if [[ "$path_dir" == "$dir" ]]; then
                    info "Found user-writable directory in PATH: $dir"
                    echo "$dir"
                    return
                fi
            done
        fi
    done
    
    # Check if preferred directories exist but not in PATH
    for dir in "${user_dirs[@]}"; do
        if [[ -d "$dir" ]] && [[ -w "$dir" ]]; then
            warn "Found user-writable directory not in PATH: $dir"
            echo "$dir"
            return
        fi
    done
    
    # Default to /usr/local/bin
    echo "/usr/local/bin"
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
    local install_dir="$2"
    local target_path="${install_dir}/${BINARY_NAME}"
    
    # Create directory if it doesn't exist
    if [[ ! -d "$install_dir" ]]; then
        if [[ -w "$(dirname "$install_dir")" ]]; then
            info "Creating directory ${install_dir}..."
            mkdir -p "$install_dir"
        else
            warn "Root access required to create ${install_dir}"
            sudo mkdir -p "$install_dir"
        fi
    fi
    
    # Check if we need sudo
    if [[ -w "$install_dir" ]]; then
        info "Installing ${BINARY_NAME} to ${install_dir}..."
        cp "$binary_path" "$target_path"
        chmod +x "$target_path"
    else
        warn "Root access required to install to ${install_dir}"
        info "Installing ${BINARY_NAME} to ${install_dir}..."
        sudo cp "$binary_path" "$target_path"
        sudo chmod +x "$target_path"
    fi
    
    info "Installation complete!"
}

# Main installation flow
main() {
    info "Installing ${BINARY_NAME}..."
    
    # Detect install directory
    local install_dir
    install_dir=$(detect_install_dir)
    info "Install directory: ${install_dir}"
    
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
    install_binary "$binary_path" "$install_dir"
    
    # Verify installation
    local installed_path="${install_dir}/${BINARY_NAME}"
    if [[ -f "$installed_path" && -x "$installed_path" ]]; then
        info "${BINARY_NAME} has been successfully installed!"
        info "Location: ${installed_path}"
        info "Version: $("$installed_path" --version 2>/dev/null || echo 'version info not available')"
        
        # Check if install dir is in PATH
        if ! echo "$PATH" | grep -q "$install_dir"; then
            warn "Note: ${install_dir} is not in your PATH"
            info "To use ${BINARY_NAME}, either:"
            info "  1. Add to PATH: export PATH=\"${install_dir}:\$PATH\""
            info "  2. Use full path: ${installed_path}"
        fi
    else
        error "Installation verification failed"
    fi
}

# Run main function
main "$@"