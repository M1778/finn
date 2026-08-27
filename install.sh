#!/bin/sh
set -e

# --dry-run reports the plan and exits without touching the network or the filesystem.
#
# It exists because the interesting part of this script is the *name* it resolves -- one wrong
# architecture token silently installs a binary that cannot run -- and that decision is
# otherwise only observable by performing the install. An unknown argument is refused rather
# than ignored: `curl | sh` passes none, so anything here was typed on purpose.
DRY_RUN=0
for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            echo "usage: install.sh [--dry-run]"
            echo "  --dry-run   print the resolved platform, asset and URL, then exit"
            exit 0
            ;;
        *)
            echo "Error: unknown argument '$arg'. Try --help." >&2
            exit 2
            ;;
    esac
done

# 1. Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected OS: $OS"
echo "Detected architecture: $ARCH"

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
        echo "Unsupported OS: $OS" >&2
        exit 1
        ;;
esac

# 3. Normalise the architecture, and refuse one with no build
#
# `ARCH` used to be read here and then never used again, while every asset was named by OS
# alone -- so an arm64 machine was served the x86_64 build and found out when the kernel
# refused to exec it, from a file already sitting on its PATH. The asset name now carries the
# architecture, which is what ADR-0010 requires: "named with both OS and architecture ... a bug
# that exists today and that the naming scheme has to make impossible rather than merely
# discouraged."
#
# The tokens are the ones the release workflow publishes, and the two spellings of each are
# both real: Linux says `x86_64`/`aarch64`, macOS says `x86_64`/`arm64`, and some
# Linux-on-Windows layers say `amd64`.
#
# An unrecognised architecture is refused by name and downloads nothing. Falling back to
# x86_64 is precisely the defect above, and guessing here would cost the user a broken binary
# on their PATH instead of a message they can act on.
case "$ARCH" in
    x86_64|amd64|x64)  ARCH_TOKEN="x86_64" ;;
    aarch64|arm64)     ARCH_TOKEN="aarch64" ;;
    *)
        echo "Error: no finn build is published for architecture '$ARCH' on $PLATFORM." >&2
        echo "Published architectures: x86_64, aarch64." >&2
        echo "Nothing has been downloaded or installed. Build from source with" >&2
        echo "'cargo install --path .' on that machine instead." >&2
        exit 1
        ;;
esac

ASSET="finn-${PLATFORM}-${ARCH_TOKEN}.${EXT}"

# 4. Construct Download URL
#
# `VERSION` is load-bearing rather than decorative: `latest` uses GitHub's floating endpoint,
# and any other value is taken as a release tag, so `FINN_VERSION=0.4.0` installs that release
# rather than whatever is newest. The leading `v` is added if the caller left it off, because a
# version and a tag differ by one character and users type either.
if [ -n "${FINN_VERSION:-}" ]; then
    VERSION="$FINN_VERSION"
fi

#
# $FINN_RELEASE_BASE serves the assets from somewhere else -- a mirror, or a test. finn already
# takes exactly this shape of override for the finc version index ($FINN_FINC_INDEX,
# src/commands/download.rs), and the reason is the same: the alternative to an override is
# patching a constant, and a constant nothing can point elsewhere is a constant nothing can
# exercise.
if [ -n "${FINN_RELEASE_BASE:-}" ]; then
    # A trailing slash would produce a double slash, which some servers 404 on.
    BASE="$(printf '%s' "$FINN_RELEASE_BASE" | sed 's|/*$||')"
    DOWNLOAD_URL="$BASE/$ASSET"
elif [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/$ASSET"
else
    case "$VERSION" in
        v*) TAG="$VERSION" ;;
        *)  TAG="v$VERSION" ;;
    esac
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"
fi

# The protocols curl may use, derived from the URL rather than hardcoded.
#
# `--proto` pins the initial transfer and `--proto-redir` pins where a redirect may take it;
# without the second, a redirect is free to move the transfer to a protocol the first one
# refused. The default base is https and stays pinned to https. A base someone pointed at plain
# http gets http -- they asked for it -- but it is said out loud, because "the bytes arrived
# unencrypted" is not something to discover later.
case "$DOWNLOAD_URL" in
    https://*)
        PROTOS='=https'
        ;;
    http://*)
        PROTOS='=http,https'
        echo "Warning: $DOWNLOAD_URL is plain HTTP. The transfer is not encrypted and" >&2
        echo "         anything on the path can rewrite it; only the checksum below stands" >&2
        echo "         between that and your PATH." >&2
        ;;
    *)
        echo "Error: refusing to download from '$DOWNLOAD_URL' -- not an http(s) URL." >&2
        exit 1
        ;;
esac

echo "Installing Finn for ${PLATFORM}-${ARCH_TOKEN}..."
echo "Source: $DOWNLOAD_URL"

if [ "$DRY_RUN" = "1" ]; then
    echo ""
    echo "--dry-run: nothing was downloaded and nothing was installed."
    echo "  platform:    ${PLATFORM}-${ARCH_TOKEN}"
    echo "  asset:       $ASSET"
    echo "  checksum:    ${ASSET}.sha256"
    echo "  binary:      $BINARY_NAME"
    echo "  install dir: $INSTALL_DIR"
    exit 0
fi

# 5. Prepare Install Directory
mkdir -p "$INSTALL_DIR"

# 6. Download
#
# Everything is fetched into a temp directory and checked BEFORE anything is written
# into $INSTALL_DIR, which is on the user's PATH. The previous version piped curl
# straight into tar; a pipeline reports the exit status of its LAST command, so curl's
# failure was invisible and an HTTP error page could be fed to tar unnoticed.
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM

ARCHIVE="$TEMP_DIR/$ASSET"

# -f turns an HTTP error status into a non-zero exit instead of a saved error page.
# --proto/--proto-redir stop a redirect from downgrading the transfer to plaintext.
#
# --max-time is generous rather than absent: a server that accepts the connection and then
# never sends the body would otherwise hang the install forever with no output to explain it.
# 30 minutes is long enough for a slow link and short enough to be a failure rather than a
# hang.
if ! curl -fL --proto "$PROTOS" --proto-redir "$PROTOS" --tlsv1.2 \
        --connect-timeout 20 --max-time 1800 -o "$ARCHIVE" "$DOWNLOAD_URL"; then
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

# 7. Verify integrity
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
if CHECKSUM_HTTP="$(curl -sS -L --proto "$PROTOS" --proto-redir "$PROTOS" --tlsv1.2 \
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
        # Both sides are lowercased before comparing. Hex is case-insensitive and the tools
        # disagree: `sha256sum` and `shasum` emit lowercase, PowerShell's `Get-FileHash` emits
        # uppercase. A plain string `!=` between them refuses a correct archive, and it refuses
        # it with the word "mismatch" -- which reads exactly like tampering.
        EXPECTED="$(tr -d '\r' < "$CHECKSUM_FILE" | awk 'NR==1 {print $1}' | tr 'ABCDEF' 'abcdef')"

        if command -v sha256sum >/dev/null 2>&1; then
            ACTUAL="$(sha256sum "$ARCHIVE" | awk '{print $1}' | tr 'ABCDEF' 'abcdef')"
        elif command -v shasum >/dev/null 2>&1; then
            ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}' | tr 'ABCDEF' 'abcdef')"
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

# 8. Extract
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

# 9. Check the archive contained what it promised
#
# `BINARY_NAME` used to be assigned per-platform and never read, so extraction succeeding was
# the only evidence anything was installed -- and extraction succeeding only means the archive
# unpacked, not that finn was in it. finn does the same check on the way in for finc toolchains
# ("finn asserts it on the way in, because a toolchain missing its standard library fails later
# and less clearly"), and the reason is the same one.
if [ ! -f "$INSTALL_DIR/$BINARY_NAME" ]; then
    echo "Error: $ASSET unpacked without a '$BINARY_NAME' in it." >&2
    echo "The archive layout is not what this installer expects, so there is no finn to run." >&2
    echo "Contents of $INSTALL_DIR:" >&2
    ls -A "$INSTALL_DIR" >&2 || true
    exit 1
fi

chmod +x "$INSTALL_DIR/$BINARY_NAME"

# The temp directory is removed by the EXIT trap on every path, success or failure.

# 10. Finalize
echo ""
echo "------------------------------------------------"
echo "Finn installed successfully to: $INSTALL_DIR"
echo "------------------------------------------------"
echo ""
echo "To use 'finn' in your terminal, it has to be on your PATH."
echo ""

# Both branches used to print the same `export PATH=...`. On Windows that is wrong in the way
# that costs the most time: this script only ever runs from MSYS, Git Bash or Cygwin, where
# `export` really does work -- for that one session -- so it looks like it worked and then finn
# is missing from PowerShell, from cmd, and from the next session, with nothing connecting the
# two observations.
if [ "$PLATFORM" = "windows" ]; then
    echo "This is a Windows install, so the entry belongs to Windows rather than to this"
    echo "shell. An 'export' here lasts only until this session closes."
    echo ""
    echo "   Settings > System > About > Advanced system settings > Environment Variables"
    echo "   then add this to your user 'Path':"
    echo ""
    echo "      %USERPROFILE%\\.finn\\bin"
    echo ""
    echo "Or from cmd.exe. Note that setx truncates a Path longer than 1024 characters, so"
    echo "use the dialog above if yours is already long:"
    echo ""
    echo "      setx Path \"%Path%;%USERPROFILE%\\.finn\\bin\""
    echo ""
    echo "For this MSYS or Git Bash session only:"
    echo ""
    echo "      export PATH=\"\$HOME/.finn/bin:\$PATH\""
    echo ""
    echo "The Windows installer, finn-setup-windows.exe, does this step for you."
else
    echo "   export PATH=\"\$HOME/.finn/bin:\$PATH\""
    echo ""
    echo "You can add this line to your ~/.bashrc, ~/.zshrc, or ~/.profile"
fi
