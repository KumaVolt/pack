#!/usr/bin/env bash
# Build the Pack desktop app with the pack CLI/daemon bundled inside.
# Usage: scripts/build-app.sh [--target <rust-triple>] [extra `cargo tauri build` args]
set -euo pipefail
cd "$(dirname "$0")/.."

target=$(rustc -vV | sed -n 's/^host: //p')
if [ "${1:-}" = "--target" ]; then target=$2; shift 2; fi
ext=""; case "$target" in *windows*) ext=".exe";; esac

cargo build --release -p pack --target "$target"
mkdir -p app/binaries
cp "target/$target/release/pack$ext" "app/binaries/pack-$target$ext"
(cd app && cargo tauri build --target "$target" "$@")
