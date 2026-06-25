# Contributing

This repo is trying to be a small, auditable reference architecture for SQLite
inside an Octra Circle. Please keep changes aligned with that shape.

## Before You Change Code

- Prefer the Rust CLI for user-facing workflows.
- Do not add Python to the reference path.
- Do not add Docker to the reference path.
- Treat `circle/source/octra_sqlite_circle.c` as consensus-critical code.
- Keep role and policy enforcement outside SQL unless Octra exposes
  authenticated caller identity to WASM.

## Checks

Run:

```sh
cargo fmt
cargo test --locked
find scripts -name '*.sh' -exec bash -n {} \;
octra-sqlite doctor --skip-network
```

If you changed the bundled WASM, also run:

```sh
bash scripts/build-wasm.sh
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
OCTRA_SQLITE_WASM=circle/wasm/octra_sqlite_circle.wasm \
  cargo test --locked --features wasm-behavior --test wasm_host_harness
```

Update `release/octra-sqlite-v12.json`, `docs/toolchain.md`, and
`CHANGELOG.md` when artifact bytes or hashes change.
