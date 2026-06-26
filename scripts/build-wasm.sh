#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SQLITE_DIR="${SQLITE_DIR:-"$ROOT_DIR/vendor/sqlite/3.53.2"}"
OUT="${OUT:-"$ROOT_DIR/circle/wasm/octra_sqlite_circle.wasm"}"
CC="${CC:-clang}"

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

if [ ! -f "$SQLITE_DIR/sqlite3.c" ] || [ ! -f "$SQLITE_DIR/sqlite3.h" ]; then
  echo "SQLite amalgamation not found at $SQLITE_DIR" >&2
  echo "Set SQLITE_DIR or vendor sqlite3.c/sqlite3.h under vendor/sqlite/3.53.2." >&2
  exit 1
fi

if ! "$CC" --print-targets 2>/dev/null | grep -Eq 'wasm32|WebAssembly'; then
  echo "$CC does not advertise a wasm32 target." >&2
  echo "Install a WASI-capable toolchain, then rerun with CC=/path/to/wasi-sdk/bin/clang." >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"

EXTRA_CFLAGS=()
if [ -n "${WASI_SYSROOT:-}" ]; then
  EXTRA_CFLAGS+=(--sysroot "$WASI_SYSROOT")
fi

"$CC" \
  --target=wasm32-wasi \
  "${EXTRA_CFLAGS[@]}" \
  -Oz \
  -flto \
  -nostdlib \
  -ffreestanding \
  -fno-builtin \
  -fno-stack-protector \
  -ffunction-sections \
  -fdata-sections \
  -DSQLITE_OS_OTHER=1 \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_DEFAULT_MEMSTATUS=0 \
  -DSQLITE_DQS=0 \
  -DSQLITE_OMIT_COMPILEOPTION_DIAGS \
  -DSQLITE_OMIT_DEPRECATED \
  -DSQLITE_OMIT_INTROSPECTION_PRAGMAS \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_OMIT_TRACE \
  -DSQLITE_OMIT_WAL \
  -DSQLITE_OMIT_UTF16 \
  -DSQLITE_OMIT_LOCALTIME \
  -DSQLITE_OMIT_STDIO \
  -I "$SQLITE_DIR" \
  -I "$ROOT_DIR/circle/crypto" \
  "$ROOT_DIR/circle/source/octra_sqlite_circle.c" \
  "$ROOT_DIR/circle/crypto/tweetnacl.c" \
  "$SQLITE_DIR/sqlite3.c" \
  -Wl,--no-entry \
  -Wl,--allow-undefined \
  -Wl,--gc-sections \
  -Wl,--strip-all \
  -Wl,-z,stack-size=1048576 \
  -Wl,--export=octra_alloc \
  -Wl,--export=octra_manifest \
  -Wl,--export=octra_query \
  -Wl,--export=octra_update \
  -o "$OUT"

if [ "${SKIP_WASM_AUDIT:-0}" != "1" ]; then
  "$ROOT_DIR/scripts/audit-wasm.sh" "$OUT"
fi

bytes="$(wc -c < "$OUT" | tr -d ' ')"
hash="$(sha256_stream < "$OUT")"

printf 'built %s\nbytes %s\nsha256 %s\n' "$OUT" "$bytes" "$hash"
sqlite_hash="$(
  {
    printf 'sqlite3.c\0'
    cat "$SQLITE_DIR/sqlite3.c"
    printf '\0sqlite3.h\0'
    cat "$SQLITE_DIR/sqlite3.h"
  } | sha256_stream
)"
printf 'sqlite_sha256 %s\n' "$sqlite_hash"
printf 'compiler %s\n' "$("$CC" --version | sed -n '1p')"
