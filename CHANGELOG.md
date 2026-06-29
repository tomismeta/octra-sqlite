# Changelog

## 0.3.2

- Added `--trace-rpc-json FILE` for one-shot read SQL JSON-RPC trace files.
- Added `restore --json-summary` for compact restore automation output.
- Documented stable CLI JSON envelope shapes in `docs/json-output.md`.
- Documented rustup/Cargo lockfile expectations and service-user install
  permissions for headless deployments.
- Kept the bundled Circle WASM unchanged from 0.3.1.

## 0.3.1

- Added `restore DATABASE --file dump.sql` for large SQL restores with internal
  batching, progress, and stable JSON output.
- Added `DATABASE --sql-file FILE` and stdin execution so automation does not
  need to pass large SQL through shell arguments.
- Added `check DATABASE --sql-file dump.sql` to validate script size, batching,
  and known restore limits without writing.
- Added `limits [DATABASE]` to expose statement-size, restore, transaction,
  owner-write, and read-only guard behavior.
- Added `--json` output for `status`, `verify`, `database list`,
  `database info`, `restore`, `check`, and `limits`.
- Added structured JSON errors for automation, including `sql_too_large`,
  `transactions_not_supported`, `read_only`, `database_error`, `wallet_error`,
  `target_error`, and `rpc_error`.
- Added `--read-only` for one-shot SQL execution.
- Documented headless/server use, large restore, idempotent imports, concurrency,
  and migration guidance.
- Rebuilt the bundled Circle WASM so query tail validation delegates to SQLite
  instead of a contract-owned SQL comment parser.

## 0.3.0

- Added `.backup ?main? FILE` and `.save FILE` to export Circle-backed SQLite
  pages as a normal local `.sqlite` file.
- Added `verify --integrity`, which exports a pinned backup and runs local
  `sqlite3` `pragma integrity_check;`.
- Added SQLite-shaped portability commands: `.dump`, `.read`, `.output`,
  `.once`, `.import --csv`, `.indexes`, and `.fullschema`.
- Changed `.dump` and `.fullschema` to render from a pinned local SQLite
  snapshot using stock `sqlite3`, instead of a project-specific SQL renderer.
- Added backup chunk streaming to the Circle view API, pinned to a generation so
  backups fail if storage changes mid-stream.
- Removed the public Remilia database from bundled defaults; examples remain
  explicit under `examples/`.
- Added crates.io package metadata and an intentional package include list.
- Rebuilt the bundled Circle WASM for the backup view surface.

## 0.2.1

- Made the protocol/client core build without HTTP or CLI dependencies.
- Kept normal `cargo install --path . --locked` behavior unchanged through
  default features.
- Hardened wallet signing state so sessions keep a signer instead of cloned
  private-key strings.
- Verified supplied public keys match the private key and tightened supported
  private-key forms.
- Removed the client-side SQL read/write prefix heuristic; the CLI now defers
  single-statement classification to SQLite inside the Circle.
- Preserved script-style `.read` and multi-statement execution through the
  signed write path.
- Removed undocumented legacy top-level aliases in favor of the SQLite-shaped
  `octra-sqlite DATABASE "SQL"` path, `database`/`db`, `status`, and `verify`.
- Retired `.proof` as a synonym until a real proof artifact exists.
- Added plain explorer links for writes and live status when a network explorer
  profile is configured.
- Fixed `new --no-name` follow-up instructions so status uses the `oct://` URI.
- Rebuilt the bundled Circle WASM so single-statement reads accept SQLite
  trailing comments.

## 0.2.0

- Refactored the Rust code around a reusable protocol/client boundary while
  keeping the SQLite-shaped CLI as the primary user experience.
- Added the public `OctraSqlite -> Database -> query/execute` API shape for
  native Rust callers.
- Added devnet and mainnet network profiles, with devnet defaulting to
  `https://devnet.octrascan.io/rpc` and mainnet preloaded as
  `https://octra.network/rpc`.
- Added the public Remilia example database to bundled config.
- Added a tiny read-only Remilia API example under `examples/remilia-read-api/`.
- Improved SQLite error expressiveness for both read and write failures.
- Preserved owner-only write intent enforcement for state-changing SQL.
- Refactored the code so REST APIs, MCP servers, A2A agents, web apps, and
  other transports can build on the same protocol/client core.
- Added protocol/client tests and configuration hygiene checks.
- Kept the bundled Circle WASM artifact unchanged from the audited devnet proof.

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
