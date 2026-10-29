#!/usr/bin/env bash
# Build the Pack desktop app and its installers, with the pack CLI/daemon
# bundled inside. Needs cargo-packager (`cargo install cargo-packager --locked`).
# Usage: scripts/build-app.sh [--target <rust-triple>] [--formats app,dmg|nsis,wix|deb,appimage]
# Installers land in target/<triple>/packager.
set -euo pipefail
cd "$(dirname "$0")/.."

target=$(rustc -vV | sed -n 's/^host: //p')
formats=""
while [ $# -gt 0 ]; do
  case "$1" in
    --target) target=$2; shift 2 ;;
    --formats) formats=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
ext=""; case "$target" in *windows*) ext=".exe";; esac

cargo build --release -p pack --target "$target"
mkdir -p app/binaries
cp "target/$target/release/pack$ext" "app/binaries/pack-$target$ext"
cargo build --release -p pack-app --target "$target"
cargo packager -p pack-app --release --target "$target" --out-dir "$PWD/target/$target/packager" ${formats:+--formats "$formats"}
