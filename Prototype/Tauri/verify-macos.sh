#!/bin/sh
set -eu
cd "$(dirname "$0")"
cargo build --locked
exe=./target/debug/queue-launcher-tauri-prototype
scratch=$(mktemp -d "${TMPDIR:-/tmp}/queue-tauri.XXXXXXXX")
export QUEUE_TAURI_PROTOTYPE_DATA="$scratch/baseline"
"$exe" --verify-initial
"$exe" --verify-reopen
for count in 12 50; do
    export QUEUE_TAURI_PROTOTYPE_DATA="$scratch/scale-$count"
    "$exe" "--verify-$count"
done
printf 'Scratch data: %s\n' "$scratch"
