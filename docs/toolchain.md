# Toolchain

The reference user path does not require a WASM toolchain. The repo ships the
audited Circle WASM at `circle/wasm/octra_sqlite_circle.wasm`; `octra-sqlite new`
and `octra-sqlite deploy` use that artifact by default.

## User Requirements

- Rust/Cargo 1.96 or newer for the CLI. `rustup stable` is recommended; distro
  packages can lag behind the lockfile. Use `cargo +stable ...` or
  `rustup default stable` when a server still defaults to an older toolchain.
  Cargo must support lockfile version 4.
- The stock `sqlite3` CLI only for local export/integrity workflows: `.dump`,
  `.fullschema`, and `verify --integrity`.
- A funded Octra wallet for writes and deploy/update calls on the configured
  network.
- Network access to the configured Octra RPC.

Users do not need Docker, Python, WABT, WASI, a C compiler, or local `sqlite3`
for the cold start path.

When copying source to a server, prefer a Git-native archive so platform
sidecar files are not included:

```sh
git archive --format=tar HEAD | tar -x -C /opt/octra-sqlite/source
```

If you must create a tarball on macOS, disable AppleDouble files:

```sh
COPYFILE_DISABLE=1 tar -cf octra-sqlite.tar .
```

## Builder Requirements

Only builders who modify `circle/source/octra_sqlite_circle.c` need:

- A WASI-capable `clang` that supports `--target=wasm32-wasip1`.
- `wasm-objdump` from WABT for import/export auditing.

Homebrew LLVM builders can set `WASI_SYSROOT` to the `wasi-libc` sysroot:

```sh
WASI_SYSROOT=/opt/homebrew/opt/wasi-libc/share/wasi-sysroot \
CC=/opt/homebrew/opt/llvm/bin/clang \
bash scripts/build-wasm.sh
```

## Current Bundled Build

The current bundled Circle WASM artifact is:

```text
compiler Homebrew clang version 22.1.8
sqlite_sha256 ba6dbb8b81fe3f40bf45ff5b427137ae62ff4639838115be8dbc3c0866d18235
sqlite3_c_upstream_sha3_256 67f423e9ebbbdc473cbc4772c872ee6b89f31fde4ed0279a5c25d5f65c043a16
code_bytes 611677
code_hash 8fe0dad1a4bb4fcfc7afab626a58eda45edeac3b25607f130b201997698d8bcf
artifact circle/wasm/octra_sqlite_circle.wasm
```

The same values are recorded in `release/octra-sqlite-0.6.3.json` and checked by
`octra-sqlite status`.

The release manifest JSON is also the source of truth for the small historical
metadata catalog of blessed previous Circle WASM epochs: release range, byte
length, base WASM SHA-256, and GitHub source URL. The crate does not bundle
those old WASM bytes. `upgrade` uses the catalog only to guide operators toward
the correct previous artifact; actual rollback bytes still come from chain
history, local artifacts, or `--previous-wasm`, and are accepted only after the
reconstructed personalized hash matches the live program hash exactly.

## Optional Rebuild

If you change the contract source, rebuild locally with:

```sh
bash scripts/build-wasm.sh
```

`scripts/build-wasm.sh` still prints the compiler version, SQLite source hash,
WASM byte length, and WASM SHA-256 hash on every build.

Docker and Python are intentionally not part of this solution.
