#!/usr/bin/env bash
# Installs Socorin Dev.app as the single, authoritative instance on this machine:
# 1. Quits any running instances.
# 2. Removes duplicate instances from ~/Applications or other directories.
# 3. Unregisters stale paths from macOS LaunchServices.
# 4. Hides build artifacts from Spotlight.
# 5. Copies the release bundle cleanly to /Applications/Socorin Dev.app.
# 6. Signs with entitlements and registers the single bundle with LaunchServices.
# 7. Launches /Applications/Socorin Dev.app.

set -euo pipefail
cd "$(dirname "$0")/.."

# 1. Quit running instances
pkill -f "/Applications/Socorin Dev.app/Contents/MacOS/socorin" 2>/dev/null || true
pkill -f "Socorin Dev.app" 2>/dev/null || true
sleep 1

# 2. Remove duplicate instances from ~/Applications
rm -rf "$HOME/Applications/Socorin Dev.app" "$HOME/Applications/Socorin.app"

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
# 3. Hide target bundles from Spotlight & LaunchServices
./scripts/spotlight-hide-bundles.sh

SOURCE_BUNDLE="src-tauri/target/release/bundle/macos.noindex/Socorin Dev.app"
if [ ! -d "$SOURCE_BUNDLE" ]; then
  SOURCE_BUNDLE="src-tauri/target/release/bundle/macos/Socorin Dev.app"
fi

if [ ! -d "$SOURCE_BUNDLE" ]; then
  echo "Error: Release bundle not found. Run a build first." >&2
  exit 1
fi

# 4. Install cleanly to /Applications
DEST="/Applications/Socorin Dev.app"
echo "Installing to $DEST..."
if [ -d "$DEST" ]; then
  if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -u "$DEST" 2>/dev/null || true
  fi
  rm -rf "$DEST"
fi

cp -R "$SOURCE_BUNDLE" "$DEST"

# 5. Sign with entitlements
ENTITLEMENTS_FLAG=()
if [ -f "src-tauri/Entitlements.plist" ]; then
  ENTITLEMENTS_FLAG=(--entitlements "src-tauri/Entitlements.plist")
fi

codesign --force --deep --sign - "${ENTITLEMENTS_FLAG[@]}" "$DEST"

if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -f "$DEST"
fi

echo "Successfully installed single instance to $DEST"
open "$DEST"
