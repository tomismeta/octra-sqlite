#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM="$ROOT_DIR/circle/wasm/octra_sqlite_circle.wasm"
JSON=0
SKIP_FUEL=0
MAX_WASM_BYTES=""
MAX_PACKAGE_BYTES=""

usage() {
  cat <<'EOF'
usage: scripts/measure-wasm.sh [options]

Measure octra-sqlite's bundled Circle WASM, packaged crate size, and optional
Wasmtime fuel baseline.

Options:
  --wasm PATH                 WASM artifact to measure
  --json                      Print machine-readable JSON
  --skip-fuel                 Skip the Wasmtime fuel harness
  --max-wasm-bytes BYTES      Fail if the WASM exceeds this size
  --max-package-bytes BYTES   Fail if the packaged crate exceeds this size
  -h, --help                  Show this help
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --wasm)
      WASM="${2:?--wasm requires a path}"
      shift 2
      ;;
    --json)
      JSON=1
      shift
      ;;
    --skip-fuel)
      SKIP_FUEL=1
      shift
      ;;
    --max-wasm-bytes)
      MAX_WASM_BYTES="${2:?--max-wasm-bytes requires a value}"
      shift 2
      ;;
    --max-package-bytes)
      MAX_PACKAGE_BYTES="${2:?--max-package-bytes requires a value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

bytes_of() {
  wc -c < "$1" | tr -d ' '
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

check_max() {
  local label="$1"
  local actual="$2"
  local max="$3"
  if [ -n "$max" ] && [ "$actual" -gt "$max" ]; then
    echo "$label $actual exceeds budget $max" >&2
    exit 1
  fi
}

if [ ! -f "$WASM" ]; then
  echo "WASM not found: $WASM" >&2
  exit 1
fi

cd "$ROOT_DIR"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

VERSION="$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)"
if [ -z "$VERSION" ]; then
  echo "could not read package version from Cargo.toml" >&2
  exit 1
fi

WASM_BYTES="$(bytes_of "$WASM")"
WASM_SHA256="$(sha256_of "$WASM")"

if ! cargo package --locked --allow-dirty --no-verify > "$TMP_DIR/package.log" 2>&1; then
  cat "$TMP_DIR/package.log" >&2
  exit 1
fi
PACKAGE="$ROOT_DIR/target/package/octra-sqlite-$VERSION.crate"
if [ ! -f "$PACKAGE" ]; then
  echo "packaged crate not found: $PACKAGE" >&2
  exit 1
fi
PACKAGE_BYTES="$(bytes_of "$PACKAGE")"

check_max "WASM bytes" "$WASM_BYTES" "$MAX_WASM_BYTES"
check_max "package bytes" "$PACKAGE_BYTES" "$MAX_PACKAGE_BYTES"

public_read_query_typed=null
sealed_read_auth_wasm_delta=null
unsigned_exec_select=null
auth_denied_before_verify=null
auth_bad_signature_verify=null
signed_exec_select=null
signed_tiny_write=null
signed_restore_batch=null
representative_vitals_query=null

if [ "$SKIP_FUEL" -eq 0 ]; then
  fuel_output="$(
    OCTRA_SQLITE_WASM="$WASM" cargo test --locked --features wasm-behavior \
      --test wasm_host_harness owner_signed_exec_has_measurable_fuel_cost -- --nocapture 2>&1
  )" || {
    printf '%s\n' "$fuel_output" >&2
    exit 1
  }
  fuel_line="$(printf '%s\n' "$fuel_output" | grep 'octra-sqlite fuel baseline:' | tail -n 1 || true)"
  if [ -z "$fuel_line" ]; then
    printf '%s\n' "$fuel_output" >&2
    echo "fuel baseline line not found" >&2
    exit 1
  fi
  fuel_pairs="${fuel_line#*octra-sqlite fuel baseline: }"
  for pair in $fuel_pairs; do
    key="${pair%%=*}"
    value="${pair#*=}"
    case "$key" in
      public_read_query_typed) public_read_query_typed="$value" ;;
      sealed_read_auth_wasm_delta) sealed_read_auth_wasm_delta="$value" ;;
      unsigned_exec_select) unsigned_exec_select="$value" ;;
      auth_denied_before_verify) auth_denied_before_verify="$value" ;;
      auth_bad_signature_verify) auth_bad_signature_verify="$value" ;;
      signed_exec_select) signed_exec_select="$value" ;;
      signed_tiny_write) signed_tiny_write="$value" ;;
      signed_restore_batch) signed_restore_batch="$value" ;;
      representative_vitals_query) representative_vitals_query="$value" ;;
    esac
  done
fi

if [ "$JSON" -eq 1 ]; then
  cat <<EOF
{
  "schema": "octra-sqlite.fuel-size.v1",
  "package": {
    "name": "octra-sqlite",
    "version": "$VERSION",
    "crate_bytes": $PACKAGE_BYTES
  },
  "wasm": {
    "path": "$WASM",
    "bytes": $WASM_BYTES,
    "sha256": "$WASM_SHA256"
  },
  "fuel": {
    "public_read_query_typed": $public_read_query_typed,
    "sealed_read_auth_wasm_delta": $sealed_read_auth_wasm_delta,
    "unsigned_exec_select": $unsigned_exec_select,
    "auth_denied_before_verify": $auth_denied_before_verify,
    "auth_bad_signature_verify": $auth_bad_signature_verify,
    "signed_exec_select": $signed_exec_select,
    "signed_tiny_write": $signed_tiny_write,
    "signed_restore_batch": $signed_restore_batch,
    "representative_vitals_query": $representative_vitals_query
  }
}
EOF
else
  cat <<EOF
octra-sqlite fuel/size baseline
version: $VERSION
crate_bytes: $PACKAGE_BYTES
wasm_bytes: $WASM_BYTES
wasm_sha256: $WASM_SHA256
public_read_query_typed: $public_read_query_typed
sealed_read_auth_wasm_delta: $sealed_read_auth_wasm_delta
unsigned_exec_select: $unsigned_exec_select
auth_denied_before_verify: $auth_denied_before_verify
auth_bad_signature_verify: $auth_bad_signature_verify
signed_exec_select: $signed_exec_select
signed_tiny_write: $signed_tiny_write
signed_restore_batch: $signed_restore_batch
representative_vitals_query: $representative_vitals_query
EOF
fi
