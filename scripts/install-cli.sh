#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required. Install Rust first: https://rustup.rs" >&2
  exit 1
fi

cd "$ROOT_DIR"
cargo install --path . --locked
echo "installed octra-sqlite"
