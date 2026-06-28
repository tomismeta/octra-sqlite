# Release Notes

## 0.3.1

This is a DevEx hardening release for automation, large SQL restore, and
headless use. The bundled Circle WASM artifact is rebuilt so query tail
validation delegates to SQLite instead of a contract-owned SQL comment parser.

## Added

- `restore DATABASE --file dump.sql` for large SQL restores with internal
  batching, progress output, and stable JSON output.
- `DATABASE --sql-file FILE` and stdin execution for one-shot SQL without shell
  argument-size pressure.
- `check DATABASE --sql-file dump.sql` for local restore planning: statement
  count, executable batches, skipped SQLite dump wrappers, and size warnings.
- `limits [DATABASE]` for the current SQL frame limit, restore behavior,
  transaction boundary, owner-write model, and read-only guard.
- `--json` output for `status`, `verify`, `database list`, `database info`,
  `restore`, `check`, and `limits`.
- Structured JSON error envelopes for automation.
- `--read-only` for one-shot SQL execution.
- `docs/operations.md` for large restore, idempotent imports, migration,
  concurrency, and operational limits.
- SQLite-native query tail validation in the Circle WASM.

## Notes

- `check` is a planner, not a chain-state dry-run. SQLite syntax and semantics
  are still ultimately enforced by SQLite inside the Circle.
- One accepted write is atomic. A multi-batch restore can partially apply; make
  restore scripts idempotent or restore into a fresh database.
- `restore` and `.read` strip common SQLite dump wrappers such as
  `BEGIN TRANSACTION`, `COMMIT`, and `PRAGMA foreign_keys`. Other
  transaction-control statements such as `ROLLBACK` and savepoints are rejected
  because user-managed transactions are not supported across Octra writes.
- Each SQL statement must fit inside the Circle SQL frame limit. The CLI checks
  this before submitting restore batches.

## 0.3.0

This is a portability release for moving SQLite data into and out of an Octra
Circle while keeping the CLI familiar to `sqlite3` users.

## Added

- `.backup ?main? FILE` and `.save FILE` for exporting a Circle-backed database
  to a local `.sqlite` file.
- `verify --integrity` for exporting a pinned backup and running local
  `sqlite3` `pragma integrity_check;`.
- `.dump ?OBJECTS?`, `.read FILE`, `.output`, `.once`, `.import --csv`,
  `.indexes`, and `.fullschema` in the interactive shell.
- Crates.io package metadata and an intentional package include list.

## Changed

- `.dump` and `.fullschema` now render from a pinned local backup through stock
  `sqlite3`, keeping the SQL text path SQLite-native.
- Bundled defaults now include network profiles only; Remilia remains an
  explicit example, not a preloaded database.
- The Circle WASM adds a read-only `backup_chunk` view for generation-pinned
  page streaming.

## Notes

- `.read` restores SQL text into a Circle through signed writes. SQLite shell
  transaction/foreign-key wrappers are stripped before submission, and large
  files are applied in batches because Octra SQL frames are capped. Restart from
  a fresh database if an interrupted restore must be retried.
- `.read` and `.import` now fail before submission when a single SQL statement
  exceeds the 8,191-byte Circle SQL statement limit.
- `.import --csv` is positional and imports fields as SQLite string literals;
  empty fields remain empty strings.
- Binary `.restore` from a `.sqlite` file remains deferred until it can be kept
  as small and auditable as `.backup`.

## 0.2.1

This is a hardening release for the Rust CLI/client and bundled Circle WASM.

## Added

- `cargo build --no-default-features --lib` coverage for the protocol/client
  core without HTTP or CLI dependencies.
- Plain `circle_url` and `tx_url` fields in write output when the active network
  has an explorer profile.

## Changed

- CLI SQL routing now lets SQLite inside the Circle classify single statements
  and only submits a signed write when SQLite returns `sqlite_readonly_required`.
- Wallet sessions now keep signing state instead of cloned private-key strings,
  verify supplied public keys, and zeroize decoded/intermediate key material
  where practical.
- `new --no-name` status follow-up now uses the generated `oct://` URI.
- The Circle query path accepts SQLite trailing comments on single-statement
  reads.

## Removed

- Undocumented hidden top-level aliases: `query`, `exec`, `tables`, `schema`,
  `storage`, `circle`, `proof`, `doctor`, and `alias`.
- `.proof` as a shell synonym. Use `.verify`; reserve “proof” for a future
  durable proof artifact.

## 0.2.0

This release refactors the Rust client and protocol boundary while keeping the
CLI SQLite-like and primary. The Circle WASM artifact is unchanged from the
audited devnet proof; `0.2.0` is a client/library/devex release.

## Added

- Public `OctraSqlite -> Database -> query/execute` Rust API shape.
- Devnet and mainnet network profiles in bundled config.
- Public Remilia example database in bundled config.
- Tiny read-only Remilia API example under `examples/remilia-read-api/`.
- Clearer SQLite read and write error messages.

## Architecture

- Refactored the code around reusable protocol and client layers.
- Positioned REST APIs, MCP servers, A2A agents, web apps, and other transports
  to build on the same protocol/client core.
- Kept the core repo a primitive: no server framework, ORM, query builder, or
  agent runtime was added.

## Still True

- SQLite 3.53.2 runs inside the deployed Circle program.
- New databases are owner-bound by default with method-bound owner write
  intents.
- Other authenticated wallets can read through the signed view path, but cannot
  write unless they hold the owner key.
- The published live proof remains on devnet.

## 0.1.0

This release is a reference architecture for running real SQLite inside an
Octra `wasm_v1` Circle with page-backed Circle key-value storage. The software
version is network-neutral; the published live proof for this release is on
devnet.

## Proven

- SQLite 3.53.2 runs inside the deployed Circle program.
- SQLite pages persist in Circle key-value storage across calls.
- Receipt-confirmed `exec` calls can create tables, insert rows, update rows,
  delete rows, and read the final state back with SQL.
- New databases are owner-bound by default with method-bound owner write
  intents.
- A non-owner wallet can read, but cannot write unless it holds the owner key.
- The WASM import/export surface is audited by script.
- Local tests cover the typed result codec, owner write intent vectors, and core
  contract behavior.
- `octra-sqlite status` checks local config, wallet discovery, release manifest,
  bundled WASM bytes/hash, and live database health when credentials are present.
- The public proof Circle contains one intentional sample table, `collection`.

## Not Claimed Yet

- A published mainnet deployment proof.
- Multi-wallet writer grants and revocation.
- Native Octra method access control for SQL roles.
- Bit-for-bit reproducible builds across arbitrary host toolchains.
- A stable public package API beyond the starter CLI and documented scripts.

## Live Devnet Proof

```text
circle: octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
version: 2
personalized_code_hash: 2e8fae91e2372293f4554fed164ff31c07df3e423bd36eba31e1b8e40a760e9f
bundled_wasm_hash: 39635962bffb470daced92396ee27e206e6b3ea000b4ec7a954d3bcd05ba662b
code_bytes: 609404
circle_create_tx: 318ca1a98df95bedb87d1042d0555eecc94660bbf828813a148bf11393ed73ed
initializer_tx: 971b50d434226e7892bb3e5f926a1dced9dd35df1df4bfe4266351116c3bc5f0
program_update_tx: 3d1a3e308f2a29b4c7748745b269841b4025ebb777fe51629e066139c6446fd7
non_owner_denied_tx: 567559d31f4c8fa3a0f5eff42f8ea8b417ee2269ab1a0b5c404241de5ff6b6a1
backup_sha256: 5134da2b7c0e03c99a139e165469f35d824f0ede7a5a4f3433625b0d1021cb42
backup_integrity: ok
manifest: release/octra-sqlite-0.3.1.json
proof: docs/proofs/devnet.md
```
