# Changelog

## 0.1.0

- Added `octra-sqlite new NAME`, a SQLite-style database creation flow that
  creates a Circle with native signed RPC, deploys the SQLite WASM,
  saves an `oct://` database URI, and can initialize schema/data with sqlite-style
  positional SQL, stdin, `--sql`, or `--read`.
- Added `octra-sqlite setup`, `octra-sqlite quickstart NAME`, wallet
  auto-discovery, and `new --sample remilia` for the beginner path while keeping
  flag-driven `init`, `new`, and `deploy` for advanced users.
- Added `octra-sqlite status` and `release/octra-sqlite-0.1.0.json` so a clean
  checkout can validate config, wallet discovery, the bundled WASM artifact,
  manifest metadata, and live database health.
- Added `octra-sqlite config`, `octra-sqlite database info`, and shell `.show`
  so the CLI exposes wallet, RPC, database, and shell state directly.
- Changed the page VFS from full-generation snapshots to sparse
  `OSQLVFS3` manifest commits: successful writes now persist dirty pages, one
  manifest, and metadata, then garbage-collect replaced page versions.
- Collapsed the public CLI toward the stock `sqlite3` feel: `octra-sqlite DB
  "SQL"` and one-shot dot commands are the primary path, with CSV mode, timer,
  and output redirection in the shell.
- Split the Rust CLI into focused modules for command orchestration, output
  rendering, and OSR1 typed-result decoding.
- Deployed a clean `v0.1.0` reference program to a public devnet Circle.
- Proved live SQLite writes with receipt-confirmed create, insert, update, and
  delete transactions.
- Added typed result methods for `REAL` and `BLOB` support outside legacy JSON.
- Pinned SQLite `now` to a deterministic VFS timestamp.
- Added allocator overflow guards and VFS-level read-only enforcement.
- Documented current policy, wallet-role, and key-value atomicity boundaries.
- Replaced the stale Docker-based CI step with Rust tests, shell syntax checks,
  bundled WASM size/hash checks, import/export audit, and the Wasmtime behavior
  harness.
- Removed archived Python/proof tooling from the public reference surface and
  rewrote the README around the minimal user journey.
- Moved concrete SQL walkthroughs into `examples/` so the README stays generic
  and minimal.
