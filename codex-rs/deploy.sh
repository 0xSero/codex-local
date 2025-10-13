#!/usr/bin/env bash
#
# Deploy script for codex-local
# Builds the project in release mode and installs the binary to ~/.local/bin
#

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}==> Building codex-local in release mode...${NC}"
cargo build --release --bin codex-local

# Determine install directory
INSTALL_DIR="${HOME}/.local/bin"

# Create install directory if it doesn't exist
if [ ! -d "$INSTALL_DIR" ]; then
    echo -e "${YELLOW}==> Creating install directory: ${INSTALL_DIR}${NC}"
    mkdir -p "$INSTALL_DIR"
fi

# Find the built binary
BINARY_PATH="target/release/codex-local"

if [ ! -f "$BINARY_PATH" ]; then
    echo -e "${RED}==> Error: Binary not found at ${BINARY_PATH}${NC}"
    exit 1
fi

echo -e "${GREEN}==> Installing codex-local to ${INSTALL_DIR}${NC}"
cp "$BINARY_PATH" "$INSTALL_DIR/codex-local"
chmod +x "$INSTALL_DIR/codex-local"

echo -e "${GREEN}==> Successfully installed codex-local!${NC}"
echo ""
echo -e "Binary location: ${INSTALL_DIR}/codex-local"
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
