#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
work="$(mktemp -d "${TMPDIR:-/tmp}/lazily-rs-core-consumer.XXXXXX")"
trap 'rm -rf "$work"' EXIT

CARGO_TARGET_DIR="$work/package-target" cargo package --allow-dirty --no-verify --package lazily
archive="$(find "$work/package-target/package" -maxdepth 1 -type f -name 'lazily-*.crate' -print -quit)"
test -n "$archive"
mkdir -p "$work/artifact" "$work/consumer/src"
tar -xzf "$archive" -C "$work/artifact"
artifact="$(find "$work/artifact" -mindepth 1 -maxdepth 1 -type d -print -quit)"
test -n "$artifact"

cp "$root/tests/core_artifact_consumer/Cargo.toml" "$work/consumer/Cargo.toml"
cp "$root/tests/core_artifact_consumer/main.rs" "$work/consumer/src/main.rs"
sed -i "s|__ARTIFACT__|$artifact|g" "$work/consumer/Cargo.toml"
cd "$work/consumer"
CARGO_TARGET_DIR="$work/consumer-target" cargo run --quiet

if grep -F "$root" Cargo.lock >/dev/null 2>&1; then
  echo "Core artifact consumer resolved the source checkout" >&2
  exit 1
fi
