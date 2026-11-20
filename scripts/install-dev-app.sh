#!/usr/bin/env bash
# Installs Socorin Dev.app as the single, authoritative instance on this machine:
# 1. Quits any running instances.
# 2. Removes duplicate instances from ~/Applications or other directories.
# 3. Unregisters stale paths from macOS LaunchServices.
# 4. Hides build artifacts from Spotlight.
# 5. Copies the release bundle cleanly to /Applications/Socorin Dev.app.
# 6. Signs and registers the single bundle with LaunchServices.
# 7. Launches /Applications/Socorin Dev.app.

set -euo pipefail
cd "$(dirname "$0")/.."

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
SOURCE_BUNDLE="src-tauri/target/release/bundle/macos/Socorin Dev.app"
if [ ! -d "$SOURCE_BUNDLE" ]; then
  SOURCE_BUNDLE="src-tauri/target/release/bundle/macos.noindex/Socorin Dev.app"
fi

if [ ! -d "$SOURCE_BUNDLE" ]; then
  echo "Error: Release bundle not found. Run a build first." >&2
  exit 1
fi

echo "Stopping running Socorin Dev instances..."
pkill -f "Socorin Dev.app" 2>/dev/null || true
pkill -f "/Contents/MacOS/socorin" 2>/dev/null || true

# Remove duplicate in user Applications if present
if [ -d "$HOME/Applications/Socorin Dev.app" ]; then
  echo "Removing duplicate instance from $HOME/Applications/Socorin Dev.app..."
  if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -u "$HOME/Applications/Socorin Dev.app" 2>/dev/null || true
  fi
  rm -rf "$HOME/Applications/Socorin Dev.app"
fi

# Hide target bundles from Spotlight & LaunchServices
./scripts/spotlight-hide-bundles.sh

# Install cleanly to /Applications
DEST="/Applications/Socorin Dev.app"
echo "Installing to $DEST..."
if [ -d "$DEST" ]; then
  if [ -x "$LSREGISTER" ]; then
    "$LSREGISTER" -u "$DEST" 2>/dev/null || true
  fi
  rm -rf "$DEST"
fi

cp -R "$SOURCE_BUNDLE" "$DEST"
codesign --force --deep --sign - "$DEST"

if [ -x "$LSREGISTER" ]; then
  "$LSREGISTER" -f "$DEST"
fi

echo "Successfully installed single instance to $DEST"
open "$DEST"
