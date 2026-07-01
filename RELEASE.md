# Release Notes

## 0.4.0

This is a productization release over the deployed `0.3.3` Circle WASM proof.
The contract, wire formats, and bundled SQLite engine are unchanged. A separate
devnet proof Circle records the explicit public-read Octra deployment tuple.

## Added

- Explicit public-read database creation with `new DATABASE --read-mode public`.
  Public-read databases deploy the Octra
  `public / gateway_allowed / public_resources` Circle tuple, use
  `octra_circleView` for SQL reads, and keep writes owner-signed through OSW1.
- Public-read metadata in saved database records, deployment manifests,
  `database list`, `database info`, `status`, `limits --json`, and
  `commands --json`.
- Unsigned public-read client routing for saved public database metadata and
  URIs explicitly marked with `?read_mode=public`.
- Explicit `?read_mode=auto` probing for callers that want the client to inspect
  Circle info before choosing signed or unsigned reads.

## Changed

- Removed redundant command and compatibility surfaces: `init`, `quickstart`,
  `database` command aliases, old option aliases, and legacy config aliases.
- `setup --yes` is the scriptable setup path; `new --sample NAME` is the
  sample database path.
- `setup` now configures wallet and network only. The first `new` establishes
  the default database.
- Raw `oct://` targets without saved metadata or `?read_mode=public` now default
  to sealed reads. This avoids hidden Circle-info probing on the shortest URI.
- Interactive `new` now asks for an explicit database name and uses configured
  defaults instead of prompting for wallet path, schema file, default selection,
  and manifest path. It saves the new database as the default and writes
  `DATABASE.octra-sqlite.json`.
- `setup` and interactive `new` both use an explicit `devnet/mainnet` network
  selector.
- Human `new` output is shorter and action-oriented. Public-read manifests and
  create output include a shareable `?read_mode=public` read URI.
- README, public surface docs, and operations docs now present one clean
  product path: setup, new, query, status, restore.
- README and `install` guidance now lead with a walletless public-read command
  so a cold-start user can query a live database before importing a wallet.

## Notes

- No Circle redeploy is required for `0.4.0`.
- Sealed remains the default read mode. Public-read is explicit and intended
  only for datasets that should be public.
- Public-read changes read authentication only. Write authorization is still
  OSW1 owner write intent.

## 0.3.4

This is a CLI productization release over the deployed `0.3.3` Circle WASM
proof. The contract, wire formats, bundled SQLite engine, and devnet proof
Circle are unchanged.

## Added

- `commands --json` for machine-readable command and JSON-envelope discovery.
- Guided `octra-sqlite new` database creation for interactive first-run setup.
- `new DATABASE --schema FILE --manifest FILE --json` for scriptable database
  creation with a machine-readable deployment manifest.
- Release manifest `release/octra-sqlite-0.3.4.json`, explicitly marked as a
  client-only release over the deployed `0.3.3` Circle WASM proof.

## Changed

- `new` refuses to overwrite an existing local database name before reading SQL,
  touching RPC, creating a Circle, or spending.
- `commands --json` is covered by a completeness test against the public Clap
  command surface.

## Notes

- No Circle redeploy is required for `0.3.4`.
- The database manifest emitted by `new --manifest` contains public deployment
  data only. It does not contain private keys or raw wallet JSON.

## 0.3.3

This is an automation contract hardening release over `0.3.2`. The bundled
Circle WASM is rebuilt so empty sealed database Circles can expose `auth_info`
before the first SQLite storage pages exist on runtimes that can invoke empty
storage views. It also includes an explicit first-write bootstrap path for
mainnet RPCs that fail authenticated views below the contract while storage is
still empty.

## Added

- `--trace-rpc-json-mode` for compact read trace files: `summary`,
  `request_only`, and `response_meta`, while keeping `full` as the default.
- Expanded `limits --json` output for SQL/result limits, restore behavior,
  read/write auth boundaries, trace modes, and schema versions.
- Stable JSON error `exit_code` plus clearer machine-readable classifications
  such as `sql_rejected`, `result_limit_exceeded`, `result_too_large`,
  `auth_failed`, `rpc_unavailable`, and `circle_write_failed`.
- A small CLI JSON contract fixture covering real binary `limits --json` and
  JSON error output.
- Storage-independent `auth_info` in the Circle WASM for safe first-write
  bootstrap on empty database Circles.
- `deploy --bootstrap-owner` for explicit owner-checked recovery of empty
  Circles created by older builds that cannot expose `auth_info` before first
  storage pages exist.
- `restore --bootstrap-owner` for the exact empty-storage cache case: the first
  restore batch is OSW1 owner-signed from saved bootstrap metadata, then the CLI
  immediately returns to normal `auth_info` verification.
- Idempotent bootstrap restore retries: once `auth_info` is readable,
  `restore --bootstrap-owner` reports `already_bootstrapped` and continues
  normally.
- Bounded RPC retry/backoff for read/view/receipt polling under rate limits,
  transient gateway failures, timeouts, and non-JSON RPC bodies. Write
  submissions are not silently replayed.
- `status --json` readiness booleans for Circle reachability, auth, owner
  writes, storage, SQLite, and query readiness.
- `wallet status` for headless wallet path, file permissions, caller, and
  target read/write relationship without printing wallet secrets.
- Cached owner-auth metadata during restore/backfill runs to reduce repeated
  `auth_info` calls while keeping each write OSW1-signed.
- Compact restore batch failure output by default, with full SQL text available
  only through `--verbose-sql`.
- Local creation metadata for newly saved databases, including owner wallet,
  owner public key, database id, bundled code hash, code bytes, and create
  transaction.
- Rust/Cargo 1.87+ and pinned release install documentation.

## Notes

- JSON shapes remain additive. Existing `query`, `write`, `write_script`,
  `restore`, `check`, `status`, `verify`, and `limits` envelopes keep their
  command-specific fields.
- `--read-only` is a client guard. Reads still use signed Octra view auth;
  writes remain OSW1 owner-gated by the Circle program.
- Multi-batch restore is practical for backfills but not globally atomic. Make
  restore SQL idempotent before retrying a failed load.
- `deploy --bootstrap-owner` does not bypass OSW1 and does not submit SQL. It
  only deploys owner-personalized bundled WASM after confirming the current
  wallet is the Circle owner.
- `restore --bootstrap-owner` is deliberately narrow: full `oct://` URI only,
  exact missing empty-storage cache failure only, bundled owner-personalized
  WASM hash match required, and first batch only.
- Normal command output and JSON do not print private keys or raw wallet JSON.
  Opt-in RPC trace files can include request signatures and should be handled as
  sensitive proof/debug logs.

## 0.3.2

This is an automation polish release over the deployed `0.3.1` Circle WASM.

## Added

- `--trace-rpc-json FILE` for one-shot read SQL, writing exact JSON-RPC
  request/response envelopes to JSONL.
- `restore DATABASE --file dump.sql --json-summary` for compact restore output
  with totals and first/last transaction hashes.
- `docs/json-output.md` documenting stable CLI JSON envelopes for `query`,
  `write`, `restore`, `check`, `status`, `verify`, and `error`.
- Clearer headless install guidance for `rustup stable`, lockfile-compatible
  Cargo, and service-user executable permissions.

## Notes

- RPC trace is read-only and opt-in. It may contain SQL text, Circle IDs, caller
  wallet, public keys, read signatures, and response data. It never contains
  private keys.
- The bundled Circle WASM and live devnet proof are unchanged from `0.3.1`.

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

The `0.3.3` Circle WASM is deployed to the public devnet proof Circle.

```text
circle: octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
version: 3
personalized_code_hash: 195bbc6dde80edc764041c45faa55f67db16a3352f7a83dd50f86f5882393f6f
bundled_wasm_hash: 36664d04fd0457c4c7da200328c753984746769cec479fd93f799665c66f8c5d
code_bytes: 609354
circle_create_tx: 318ca1a98df95bedb87d1042d0555eecc94660bbf828813a148bf11393ed73ed
initializer_tx: 971b50d434226e7892bb3e5f926a1dced9dd35df1df4bfe4266351116c3bc5f0
program_update_tx: efe65e5ff4c23668703f1e51b6af74f99442355f0cb4a753aff25e74b3388ebb
write_smoke_tx: aae4bb0f4cc4506c80f37ce1045958398cdb1a6d70183baab274b73e056ec28b
non_owner_denied_tx: fe13f345435efce21a362fddab3ac9234dfd2ba747581a65709b03b915c44148
backup_sha256: 7eac9d276c6ddefeb72ae112635a82262e5a62c969ba0394c4cfaa7e502e2ab7
backup_integrity: ok
manifest: release/octra-sqlite-0.3.4.json
proof: docs/proofs/devnet.md
```
