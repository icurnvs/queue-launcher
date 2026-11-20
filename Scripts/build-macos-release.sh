#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

tag=${RELEASE_TAG:-}
if [[ ${PUBLISH:-false} == true && -z "$tag" ]]; then
  echo 'Uploading requires an existing release tag.' >&2
  exit 1
fi
if [[ -n "$tag" ]]; then
  if [[ ! "$tag" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
    echo 'Unsupported release tag.' >&2
    exit 1
  fi
  [[ $(git rev-parse "refs/tags/$tag^{commit}") == "$(git rev-parse HEAD)" ]] || {
    echo 'The checkout must match the release tag.' >&2
    exit 1
  }
fi
version=$(python3 -c 'import json; print(json.load(open("Tauri/tauri.conf.json"))["version"])')
if [[ "$tag" == v* && "$tag" != "v$version" ]]; then
  echo "Tag $tag does not match application version $version." >&2
  exit 1
fi
[[ $(uname -m) == arm64 ]] || { echo 'An Apple Silicon runner is required.' >&2; exit 1; }
label=${tag:-build-$(git rev-parse --short HEAD)}

cargo test --locked --release --manifest-path Tauri/Cargo.toml --bin queue-launcher
sh Tauri/verify-macos-app.sh
(
  cd Tauri
  npx --yes --package @tauri-apps/cli@2.12.4 tauri build --bundles app --target aarch64-apple-darwin --ci -- --locked
)
app='Tauri/target/aarch64-apple-darwin/release/bundle/macos/Queue Launcher.app'
codesign --verify --deep --strict "$app"
[[ $(lipo -archs "$app/Contents/MacOS/queue-launcher") == arm64 ]]
output="$PWD/build/release"
[[ ! -e "$output" ]] || { echo 'Release output already exists; use a clean checkout.' >&2; exit 1; }
mkdir -p "$output"
zip="QueueLauncher-$label-macos-arm64.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$output/$zip"
(cd "$output" && shasum -a 256 "$zip" > "$zip.sha256")
ls -lh "$output"
if [[ -n ${GITHUB_OUTPUT:-} ]]; then
  echo "commit=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"
fi
