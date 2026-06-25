#!/usr/bin/env bash
set -euo pipefail

WASM="${1:?usage: scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm}"

if ! command -v wasm-objdump >/dev/null 2>&1; then
  echo "wasm-objdump is required for WASM import/export auditing" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

wasm-objdump -x "$WASM" > "$tmp_dir/objdump.txt"

sed -n '/Import\[/,/Function\[/p' "$tmp_dir/objdump.txt" \
  | grep ' <- ' \
  | sed 's/.*<- //' \
  | sort > "$tmp_dir/imports.actual"

cat > "$tmp_dir/imports.expected" <<'EOF'
octra.host_emit_event
octra.host_kv_del
octra.host_kv_get
octra.host_kv_get_len
octra.host_kv_put
octra.host_response_finish
octra.host_response_reset
octra.host_response_write
EOF
sort "$tmp_dir/imports.expected" -o "$tmp_dir/imports.expected"

if ! diff -u "$tmp_dir/imports.expected" "$tmp_dir/imports.actual"; then
  echo "unexpected WASM imports" >&2
  exit 1
fi

sed -n '/Export\[/,/Elem\[/p' "$tmp_dir/objdump.txt" \
  | grep ' -> ' \
  | sed 's/.*-> "//; s/"$//' \
  | sort > "$tmp_dir/exports.actual"

cat > "$tmp_dir/exports.expected" <<'EOF'
memory
octra_alloc
octra_manifest
octra_query
octra_update
EOF
sort "$tmp_dir/exports.expected" -o "$tmp_dir/exports.expected"

if ! diff -u "$tmp_dir/exports.expected" "$tmp_dir/exports.actual"; then
  echo "unexpected WASM exports" >&2
  exit 1
fi

printf 'audit ok %s\n' "$WASM"
