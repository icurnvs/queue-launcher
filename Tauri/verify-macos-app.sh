#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo build --locked --bin queue-launcher

scratch=$(mktemp -d "${TMPDIR:-/tmp}/queue-tauri-app.XXXXXXXX")
printf 'Scratch data: %s\n' "$scratch"
for phase in initial reopen cleanup; do
    QUEUE_LAUNCHER_DATA="$scratch/persistence" ./target/debug/queue-launcher --verify-macos-offline "$phase"
done
QUEUE_LAUNCHER_DATA="$scratch/scale" ./target/debug/queue-launcher --verify-macos-offline 50
