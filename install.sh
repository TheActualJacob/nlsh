#!/bin/sh
# nlsh installer
# Usage: curl -fsSL https://github.com/rjacobryan/langcmdline/releases/latest/download/install.sh | sh

set -e

REPO="rjacobryan/langcmdline"
NLSH_BIN="/usr/local/bin/nlsh"
SHELLS_FILE="/etc/shells"

# ── Detect latest version ────────────────────────────────────────────────────
if command -v curl >/dev/null 2>&1; then
    FETCH="curl -fsSL"
elif command -v wget >/dev/null 2>&1; then
    FETCH="wget -qO-"
else
    echo "error: curl or wget is required" >&2
    exit 1
fi

echo "Fetching latest nlsh release..."
VERSION=$($FETCH "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' \
    | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$VERSION" ]; then
    echo "error: could not determine latest version" >&2
    exit 1
fi

echo "Installing nlsh $VERSION..."

# ── Download .pkg ─────────────────────────────────────────────────────────────
PKG_URL="https://github.com/$REPO/releases/download/$VERSION/nlsh-${VERSION}.pkg"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

$FETCH "$PKG_URL" -o "$TMP/nlsh.pkg" 2>/dev/null || {
    # curl needs -o flag explicitly; retry with output flag
    curl -fsSL "$PKG_URL" -o "$TMP/nlsh.pkg"
}

# ── Install .pkg ──────────────────────────────────────────────────────────────
echo "Installing package (your password may be required)..."
sudo installer -pkg "$TMP/nlsh.pkg" -target / -quiet

# ── Register in /etc/shells ───────────────────────────────────────────────────
if ! grep -qxF "$NLSH_BIN" "$SHELLS_FILE" 2>/dev/null; then
    echo "Adding $NLSH_BIN to $SHELLS_FILE..."
    echo "$NLSH_BIN" | sudo tee -a "$SHELLS_FILE" >/dev/null
fi

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo "  nlsh $VERSION installed to $NLSH_BIN"
echo ""
echo "  Run nlsh to configure your AI backend:"
echo "    nlsh"
echo ""
echo "  To set as your default shell:"
echo "    chsh -s $NLSH_BIN"
echo ""
