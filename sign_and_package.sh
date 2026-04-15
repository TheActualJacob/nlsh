#!/bin/bash
# Local code signing and notarization script for nlsh
# Run from the repo root after: cd nlsh && cargo build --release && cd ..
#
# Prerequisites:
#   - "Developer ID Application: ..." cert in Keychain
#   - "Developer ID Installer: ..." cert in Keychain
#   - xcrun notarytool available (Xcode 13+)

set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────
APP_CERT="Developer ID Application: Robert Ryan (JQTCAT85GP)"
INSTALLER_CERT="Developer ID Installer: Robert Ryan (JQTCAT85GP)"
BUNDLE_ID="com.robertryan.nlsh"

APPLE_ID="${APPLE_ID:-ask.notivize@gmail.com}"
TEAM_ID="${TEAM_ID:-19222613261}"
# Set APPLE_APP_SPECIFIC_PASSWORD in your environment — never hardcode it here

if [[ -z "${APPLE_APP_SPECIFIC_PASSWORD:-}" ]]; then
  echo "Error: APPLE_APP_SPECIFIC_PASSWORD env var is not set."
  echo "Generate one at https://appleid.apple.com → App-Specific Passwords"
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$REPO_ROOT/nlsh/target/release"
PKG_ROOT="$REPO_ROOT/nlsh/pkgroot"
INSTALL_PREFIX="$PKG_ROOT/usr/local/bin"
OUT_DIR="$REPO_ROOT/dist"

VERSION="${1:-$(date +%Y%m%d)}"
PKG_NAME="nlsh-${VERSION}.pkg"

# ── Step 1: Sign binaries ─────────────────────────────────────────────────────
echo "==> Signing binaries..."
mkdir -p "$INSTALL_PREFIX"
cp "$BUILD_DIR/nlsh" "$INSTALL_PREFIX/nlsh"
cp "$REPO_ROOT/nlsh-model/.build/release/nlsh-model" "$INSTALL_PREFIX/nlsh-model"

for bin in nlsh nlsh-model; do
  codesign \
    --sign "$APP_CERT" \
    --options runtime \
    --timestamp \
    --force \
    "$INSTALL_PREFIX/$bin"
  echo "    Signed: $bin"
done

# ── Step 2: Build .pkg ────────────────────────────────────────────────────────
echo "==> Building .pkg..."
mkdir -p "$OUT_DIR"
UNSIGNED_PKG="$OUT_DIR/nlsh-unsigned.pkg"

pkgbuild \
  --root "$PKG_ROOT" \
  --identifier "$BUNDLE_ID" \
  --version "$VERSION" \
  --install-location "/" \
  "$UNSIGNED_PKG"

# ── Step 3: Sign .pkg ─────────────────────────────────────────────────────────
echo "==> Signing .pkg..."
SIGNED_PKG="$OUT_DIR/$PKG_NAME"
productsign \
  --sign "$INSTALLER_CERT" \
  --timestamp \
  "$UNSIGNED_PKG" \
  "$SIGNED_PKG"

rm "$UNSIGNED_PKG"
echo "    Signed pkg: $SIGNED_PKG"

# ── Step 4: Notarize ──────────────────────────────────────────────────────────
echo "==> Submitting for notarization (this takes ~1-2 minutes)..."
xcrun notarytool submit "$SIGNED_PKG" \
  --apple-id "$APPLE_ID" \
  --team-id "$TEAM_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" \
  --wait

# ── Step 5: Staple ────────────────────────────────────────────────────────────
echo "==> Stapling notarization ticket..."
xcrun stapler staple "$SIGNED_PKG"

echo ""
echo "Done! Distribution-ready package: $SIGNED_PKG"
echo "Verify with: spctl --assess --type install --verbose \"$SIGNED_PKG\""
