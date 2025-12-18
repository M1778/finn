#!/bin/sh
set -e

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected OS: $OS"

# Default variables
INSTALL_DIR="$HOME/.finn/bin"
REPO="M1778M/finn"
VERSION="latest" # Can be changed to a specific tag if needed

# 2. Determine Platform specific variables
case "$OS" in
    Linux)
        PLATFORM="linux"
        EXT="tar.gz"
        FORMAT="tar"
        BINARY_NAME="finn"
        ;;
    Darwin)
        PLATFORM="macos"
        EXT="tar.gz"
        FORMAT="tar"
        BINARY_NAME="finn"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="windows"
        EXT="zip"
        FORMAT="zip"
        BINARY_NAME="finn.exe"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

# 3. Construct Download URL
# Uses the 'latest' release endpoint from GitHub
DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/finn-${PLATFORM}.${EXT}"

echo "Installing Finn for $PLATFORM..."
echo "Source: $DOWNLOAD_URL"

# 4. Prepare Install Directory
mkdir -p "$INSTALL_DIR"

# 5. Download and Extract
if [ "$FORMAT" = "zip" ]; then
    # Windows/Zip logic
    # We use curl to download to a temp file, then unzip
    TEMP_FILE=$(mktemp).zip
    curl -L "$DOWNLOAD_URL" -o "$TEMP_FILE"
    
    # Check if unzip is available
    if command -v unzip >/dev/null 2>&1; then
        unzip -o "$TEMP_FILE" -d "$INSTALL_DIR"
        # Move files out of subfolder if zip structure requires it, 
        # but our workflow zips files directly, so they should be in root or one folder deep.
        # Cleanup
        rm "$TEMP_FILE"
    else
        echo "Error: 'unzip' command not found. Please install unzip or use the Windows Installer (.exe)."
        rm "$TEMP_FILE"
        exit 1
    fi
else
    # Unix/Tar logic
    # Download and pipe directly to tar
    curl -L "$DOWNLOAD_URL" | tar xz -C "$INSTALL_DIR"
fi

# 6. Finalize
echo ""
echo "------------------------------------------------"
echo "¿? Finn installed successfully to: $INSTALL_DIR"
echo "------------------------------------------------"
echo ""
echo "To use 'finn' in your terminal, add this to your PATH:"
echo ""

if [ "$PLATFORM" = "windows" ]; then
    echo "   export PATH=\"\$HOME/.finn/bin:\$PATH\""
    echo "   (Or add $INSTALL_DIR to your Windows Environment Variables)"
else
    echo "   export PATH=\"\$HOME/.finn/bin:\$PATH\""
    echo ""
    echo "You can add this line to your ~/.bashrc, ~/.zshrc, or ~/.profile"
fi
