# Toolchain

The reference user path does not require a WASM toolchain. The repo ships the
audited Circle WASM at `circle/wasm/octra_sqlite_circle.wasm`; `octra-sqlite new`
and `octra-sqlite deploy` use that artifact by default.

## User Requirements

- Rust stable with Cargo for the CLI.
- A funded Octra wallet for writes and deploy/update calls on the configured
  network.
- Network access to the configured Octra RPC.

Users do not need Docker, Python, WABT, WASI, or a C compiler.

## Builder Requirements

Only builders who modify `circle/source/octra_sqlite_circle.c` need:

- A WASI-capable `clang` that supports `--target=wasm32-wasi`.
- `wasm-objdump` from WABT for import/export auditing.

## Current Bundled Build

The bundled `v0.1.0` Circle WASM artifact is:

```text
compiler clang version 22.1.0-wasi-sdk
sqlite_sha256 d8cbe58389cb5b375e81fe9b456fe55098180975a7c06e9b934ce36906b75b21
code_bytes 607496
code_hash 0e28ecc233306fd59539a22209be633fa7e6ca7410c84ce7c940abfcfb372e7a
artifact circle/wasm/octra_sqlite_circle.wasm
```

The same values are recorded in `release/octra-sqlite-0.1.0.json` and checked by
`octra-sqlite status`.

## Optional Rebuild

If you change the contract source, rebuild locally with:

```sh
bash scripts/build-wasm.sh
```

`scripts/build-wasm.sh` still prints the compiler version, SQLite source hash,
WASM byte length, and WASM SHA-256 hash on every build.

Docker and Python are intentionally not part of this solution.
