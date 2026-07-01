# Changelog

## 0.4.0

- Added explicit public-read database creation with
  `new DATABASE --read-mode public`.
- Added public-read routing: public database reads use `octra_circleView`, while
  sealed database reads keep `octra_circleViewAuth`.
- Recorded a devnet public-read proof Circle with unsigned SQL reads and an
  on-chain non-owner write rejection.
- Kept writes unchanged: all state-changing SQL still uses owner-signed OSW1
  calls.
- Saved read-mode and Circle tuple metadata for new databases and exposed it in
  manifests, `database list`, `database info`, `status`, `limits --json`, and
  `commands --json`.
- Made raw `oct://` targets default to sealed reads unless saved metadata or an
  explicit `?read_mode=public`/`?read_mode=auto` marker says otherwise.
- Simplified interactive `new` to the product path: database name, read mode,
  confirmation. Wallet, network, default database, and manifest path are
  resolved from configuration and conventions.
- Removed redundant public command surfaces: `init`, `quickstart`, command
  aliases, option aliases, and legacy config aliases.
- Kept `setup --yes` as the scriptable setup path and `new --sample NAME` as
  the built-in sample path.
- Updated README and reference docs around one clean cold-start path and
  explicit sealed/public read modes.

## 0.3.4

- Added guided `octra-sqlite new` database creation for interactive first-run
  setup.
- Added `new --schema FILE --manifest FILE --json` for scriptable database
  creation with a machine-readable deployment manifest.
- Added `commands --json` for machine-readable command and JSON-envelope
  discovery.
- Refuse to create a new saved database when the local database name already
  exists, before any Circle creation or spend.
- Added `new` to the stable CLI JSON envelope documentation.
- Tightened public docs around database-first ontology and neutral headless
  setup examples.

## 0.3.3

- Added compact RPC trace modes: `full`, `summary`, `request_only`, and
  `response_meta`, with `full` preserved as the default exact trace.
- Expanded `limits --json` into the supported automation capability surface for
  versions, SQL/result limits, restore behavior, auth boundaries, and trace
  modes.
- Tightened JSON error output with `exit_code` and stable error classifications
  for SQL rejection, auth, result limits, RPC, wallet, target, and write
  failures.
- Added a binary-level JSON contract fixture for `limits --json` and JSON error
  envelopes.
- Rebuilt the bundled Circle WASM so `auth_info` no longer reads SQLite page
  metadata, allowing owner-signed first writes on empty sealed database Circles.
- Added `deploy --bootstrap-owner` for explicit owner-checked recovery of an
  empty Circle whose deployed program cannot expose `auth_info` before first
  storage pages exist.
- Added `restore --bootstrap-owner` for the exact empty-storage cache bootstrap
  case: first restore batch only, full `oct://` URI required, OSW1 signed, then
  normal `auth_info` verification resumes.
- Made `restore --bootstrap-owner` idempotent after bootstrap: if `auth_info`
  is already readable, restore continues through the normal owner-auth path.
- Added bounded retry/backoff for transient RPC read/view/receipt failures,
  including rate limits and non-JSON gateway responses, without replaying write
  submissions.
- Added `status --json` readiness booleans and `wallet status` for headless
  wallet path, permissions, caller, and target read/write checks.
- Reduced restore/backfill RPC pressure by reusing verified owner-auth metadata
  during a restore run while still signing every write.
- Made restore batch failures compact by default, with SQL hash and preview;
  full SQL text is available with `--verbose-sql`.
- Persisted local creation metadata for new saved databases: owner wallet,
  owner public key, database id, code hash, code bytes, create transaction, and
  bootstrap program update transaction.
- Published a refreshed devnet proof for the rebuilt 0.3.3 Circle WASM,
  including write-smoke, backup integrity, and non-owner write rejection
  evidence.
- Documented Rust/Cargo 1.87+, pinned source installs, read/write auth,
  restore/backfill happy path, result limits, and compact trace usage.
- Added local tool settings to `.gitignore` so machine-specific files cannot be
  committed accidentally.

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
