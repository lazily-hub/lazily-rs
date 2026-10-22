#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
work="$(mktemp -d "${TMPDIR:-/tmp}/lazily-rs-core-consumer.XXXXXX")"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/consumer/src"
cp "$root/tests/core_artifact_consumer/Cargo.toml" "$work/consumer/Cargo.toml"
cp "$root/tests/core_artifact_consumer/main.rs" "$work/consumer/src/main.rs"

if [[ -n "${LAZILY_CORE_ARTIFACT_VERSION:-}" ]]; then
  version="$LAZILY_CORE_ARTIFACT_VERSION"
  sed -i "s|{ path = \"__ARTIFACT__\" }|\"=$version\"|" "$work/consumer/Cargo.toml"
else
  CARGO_TARGET_DIR="$work/package-target" cargo package --allow-dirty --no-verify --package lazily
  archive="$(find "$work/package-target/package" -maxdepth 1 -type f -name 'lazily-*.crate' -print -quit)"
  test -n "$archive"
  mkdir -p "$work/artifact"
  tar -xzf "$archive" -C "$work/artifact"
  artifact="$(find "$work/artifact" -mindepth 1 -maxdepth 1 -type d -print -quit)"
  test -n "$artifact"
  sed -i "s|__ARTIFACT__|$artifact|g" "$work/consumer/Cargo.toml"
fi
cd "$work/consumer"
CARGO_TARGET_DIR="$work/consumer-target" cargo run --quiet

if grep -F "$root" Cargo.lock >/dev/null 2>&1; then
  echo "Core artifact consumer resolved the source checkout" >&2
  exit 1
fi

if [[ -n "${LAZILY_CORE_ARTIFACT_VERSION:-}" ]]; then
  CARGO_TARGET_DIR="$work/consumer-target" cargo metadata --format-version 1 |
    python3 -c '
import json, sys
metadata = json.load(sys.stdin)
version = sys.argv[1]
packages = [item for item in metadata["packages"] if item["name"] == "lazily"]
if len(packages) != 1 or packages[0]["version"] != version:
    raise SystemExit("consumer did not resolve the exact Lazily version")
if not (packages[0].get("source") or "").startswith("registry+"):
    raise SystemExit("consumer did not resolve Lazily from a registry")
' "$LAZILY_CORE_ARTIFACT_VERSION"
fi
