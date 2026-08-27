#!/bin/sh
set -e

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected OS: $OS"

# Default variables
INSTALL_DIR="$HOME/.finn/bin"
REPO="M1778/finn"
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

# 5. Download
#
# Everything is fetched into a temp directory and checked BEFORE anything is written
# into $INSTALL_DIR, which is on the user's PATH. The previous version piped curl
# straight into tar; a pipeline reports the exit status of its LAST command, so curl's
# failure was invisible and an HTTP error page could be fed to tar unnoticed.
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM

ARCHIVE="$TEMP_DIR/finn-${PLATFORM}.${EXT}"

# -f turns an HTTP error status into a non-zero exit instead of a saved error page.
# --proto '=https' stops a redirect from downgrading the transfer to plaintext.
if ! curl -fL --proto '=https' --tlsv1.2 -o "$ARCHIVE" "$DOWNLOAD_URL"; then
    echo "" >&2
    echo "Error: failed to download $DOWNLOAD_URL" >&2
    echo "No archive was retrieved, so nothing has been installed." >&2
    echo "If a release for this platform has not been published yet, there is" >&2
    echo "nothing to install; build from source with 'cargo install --path .'." >&2
    exit 1
fi

if [ ! -s "$ARCHIVE" ]; then
    echo "Error: the downloaded archive is empty: $DOWNLOAD_URL" >&2
    echo "Refusing to install from it." >&2
    exit 1
fi

# 6. Verify integrity
#
# A '<asset>.sha256' beside the asset is enforced when one is published. Note the limit
# of this check -- a checksum served from the same release as the archive detects
# corruption and truncation, not a tampered-with release. It is not a signature.
#
# A failed request is NOT evidence of absence. Only the server actually answering 404
# means "no checksum is published"; a DNS failure, a timeout, a 5xx or a blocking proxy
# means we could not find out, and those refuse rather than quietly downgrading to an
# unverified install. Otherwise anyone able to stall or block this single request would
# turn verification off silently -- and this binary lands on the user's PATH.
#
# No -f here, deliberately: -f collapses "404, definitively absent" (exit 22) and
# "never got an answer" (exit 6, 7, 28...) into one indistinguishable failure. The HTTP
# status is what separates them, so it is captured explicitly. curl's own stderr is kept
# rather than discarded, because it carries the reason reported below.
CHECKSUM_URL="${DOWNLOAD_URL}.sha256"
CHECKSUM_FILE="$TEMP_DIR/archive.sha256"
CHECKSUM_ERR="$TEMP_DIR/checksum.err"

# Run as an `if` condition so `set -e` does not abort before the status can be read.
# The timeouts matter: without them a server that accepts the connection and then
# never answers would stall the install forever. Bounded, a stall becomes exit 28 and
# is refused below like any other "could not find out".
if CHECKSUM_HTTP="$(curl -sS -L --proto '=https' --tlsv1.2 \
        --connect-timeout 10 --max-time 60 \
        -o "$CHECKSUM_FILE" -w '%{http_code}' "$CHECKSUM_URL" 2>"$CHECKSUM_ERR")"; then
    CHECKSUM_RC=0
else
    CHECKSUM_RC=$?
fi

if [ "$CHECKSUM_RC" -ne 0 ]; then
    case "$CHECKSUM_RC" in
        6)  CHECKSUM_WHY="could not resolve the host" ;;
        7)  CHECKSUM_WHY="could not connect to the host" ;;
        28) CHECKSUM_WHY="the request timed out" ;;
        35) CHECKSUM_WHY="the TLS handshake failed" ;;
        52) CHECKSUM_WHY="the server sent no reply" ;;
        56) CHECKSUM_WHY="the connection broke while receiving" ;;
        *)  CHECKSUM_WHY="the request failed" ;;
    esac
    echo "" >&2
    echo "Error: could not find out whether this release publishes a checksum." >&2
    echo "  url:    $CHECKSUM_URL" >&2
    echo "  reason: $CHECKSUM_WHY (curl exit $CHECKSUM_RC)" >&2
    if [ -s "$CHECKSUM_ERR" ]; then
        sed 's/^/  curl:   /' "$CHECKSUM_ERR" >&2
    fi
    echo "That is not the same as the release having no checksum, so this archive is" >&2
    echo "unverified. Refusing to install it into $INSTALL_DIR." >&2
    echo "Retry once the network is reachable, or build from source with" >&2
    echo "'cargo install --path .'." >&2
    exit 1
fi

case "$CHECKSUM_HTTP" in
    200)
        EXPECTED="$(tr -d '\r' < "$CHECKSUM_FILE" | awk 'NR==1 {print $1}')"

        if command -v sha256sum >/dev/null 2>&1; then
            ACTUAL="$(sha256sum "$ARCHIVE" | awk '{print $1}')"
        elif command -v shasum >/dev/null 2>&1; then
            ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
        else
            ACTUAL=""
        fi

        if [ -z "$ACTUAL" ]; then
            echo "Error: a checksum is published for this release but neither 'sha256sum'" >&2
            echo "nor 'shasum' is available to check it. Refusing to install unverified." >&2
            exit 1
        elif [ -z "$EXPECTED" ]; then
            echo "Error: the published checksum file is unreadable: $CHECKSUM_URL" >&2
            echo "Refusing to install an archive that cannot be verified." >&2
            exit 1
        elif [ "$EXPECTED" != "$ACTUAL" ]; then
            echo "Error: checksum mismatch -- refusing to install." >&2
            echo "  expected: $EXPECTED" >&2
            echo "  actual:   $ACTUAL" >&2
            exit 1
        fi

        echo "Checksum verified."
        ;;
    404)
        # The server answered, and the answer is that there is no such file.
        echo "Note: this release publishes no checksum file (the server returned 404 for"
        echo "      ${CHECKSUM_URL##*/}), so the download could not be verified against"
        echo "      one. Only the transfer itself was checked."
        ;;
    *)
        echo "" >&2
        echo "Error: could not read the published checksum." >&2
        echo "  url:    $CHECKSUM_URL" >&2
        echo "  reason: the server returned HTTP $CHECKSUM_HTTP" >&2
        echo "Only a 404 would mean no checksum is published; this does not, so the" >&2
        echo "archive is unverified. Refusing to install it into $INSTALL_DIR." >&2
        exit 1
        ;;
esac

# 7. Extract
#
# Extraction is a plain command, not the tail of a pipeline, so a corrupt or
# unexpected archive fails here loudly instead of half-populating $INSTALL_DIR.
if [ "$FORMAT" = "zip" ]; then
    if ! command -v unzip >/dev/null 2>&1; then
        echo "Error: 'unzip' command not found. Please install unzip or use the Windows Installer (.exe)." >&2
        exit 1
    fi
    if ! unzip -o "$ARCHIVE" -d "$INSTALL_DIR"; then
        echo "Error: could not extract $ARCHIVE" >&2
        echo "The archive may be corrupt or truncated; nothing reliable was installed." >&2
        exit 1
    fi
else
    if ! tar xzf "$ARCHIVE" -C "$INSTALL_DIR"; then
        echo "Error: could not extract $ARCHIVE" >&2
        echo "The archive may be corrupt or truncated; nothing reliable was installed." >&2
        exit 1
    fi
fi

# The temp directory is removed by the EXIT trap on every path, success or failure.

# 8. Finalize
echo ""
echo "------------------------------------------------"
echo "Finn installed successfully to: $INSTALL_DIR"
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
