#!/usr/bin/env bash
#
# Quick install script for codex-local
# Installs the debug binary (faster for development)
# For production use, run: ./deploy.sh
#

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Determine which binary to install (debug or release)
if [ -f "target/release/codex-local" ]; then
    BINARY_PATH="target/release/codex-local"
    BUILD_TYPE="release"
elif [ -f "target/debug/codex-local" ]; then
    BINARY_PATH="target/debug/codex-local"
    BUILD_TYPE="debug"
else
    echo -e "${RED}==> Error: No codex-local binary found${NC}"
    echo "Run 'cargo build' or 'cargo build --release' first"
    exit 1
fi

# Determine install directory
INSTALL_DIR="${HOME}/.local/bin"

# Create install directory if it doesn't exist
if [ ! -d "$INSTALL_DIR" ]; then
    echo -e "${YELLOW}==> Creating install directory: ${INSTALL_DIR}${NC}"
    mkdir -p "$INSTALL_DIR"
fi

echo -e "${GREEN}==> Installing codex-local (${BUILD_TYPE}) to ${INSTALL_DIR}${NC}"
cp "$BINARY_PATH" "$INSTALL_DIR/codex-local"
chmod +x "$INSTALL_DIR/codex-local"

echo -e "${GREEN}==> Successfully installed codex-local!${NC}"
echo ""
echo -e "Binary location: ${INSTALL_DIR}/codex-local"
echo -e "Build type: ${BUILD_TYPE}"
echo ""

# Check if ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo -e "${YELLOW}WARNING: ${INSTALL_DIR} is not in your PATH${NC}"
    echo ""
    echo "Add the following to your ~/.bashrc, ~/.zshrc, or equivalent:"
    echo ""
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo "Then reload your shell or run:"
    echo "    source ~/.bashrc  # or ~/.zshrc"
    echo ""
else
    echo -e "${GREEN}✓ ${INSTALL_DIR} is in your PATH${NC}"
    echo ""
    echo "You can now run: codex-local"
fi
