# Release Notes

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
circle: oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
version: 4
personalized_code_hash: 37e377095b33437ad3ebbda0cd67766e005cfe0b82967d6abdcfabb5427f2f46
bundled_wasm_hash: 29861d38ddad25f5cd2b153bb70cfa6b1b54ebd2532fe931fa1f012b7f39ca9c
code_bytes: 607800
circle_create_tx: 1a1817d310278a3814d5446b1869a098ce4055be2421aa31694d3bb4a51312cb
initializer_tx: da10d2af72c3b4be2053fe47cc65b9e4073bd31f52fcdc85451a0cefbbbdbf43
program_update_tx: 98ce68ef74d9c4ef50bdf0654201d67cd74822da0231f6ce4cd5c30e1f0311f1
non_owner_denied_tx: 08ea0b734025ed87d5694af6f2800bcec55411815e6b7724a325159ac6b6d3b3
sample: remilia collection
manifest: release/octra-sqlite-0.2.1.json
proof: docs/proofs/devnet.md
```
