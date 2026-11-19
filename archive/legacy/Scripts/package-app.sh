#!/bin/sh
set -eu

cd "$(dirname "$0")/.."
swift build --configuration release "$@"

bundle="${QUEUE_LAUNCHER_BUNDLE:-$PWD/build/Queue Launcher.app}"
mkdir -p "$bundle/Contents/MacOS"
cp .build/release/QueueLauncher "$bundle/Contents/MacOS/QueueLauncher"
cp Scripts/Info.plist "$bundle/Contents/Info.plist"
# Finder can add metadata to an existing bundle that codesign rejects.
if xattr -p com.apple.FinderInfo "$bundle" >/dev/null 2>&1; then
    xattr -d com.apple.FinderInfo "$bundle"
fi
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
printf '%s\n' "$bundle"
