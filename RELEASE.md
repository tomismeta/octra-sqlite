# Release Notes

## 0.6.3

This is a focused SQLite engine maintenance release over `0.6.2`. It rebuilds
the bundled Circle WASM with SQLite 3.53.4 and preserves the product and
automation surfaces.

### Changed

- Bundled SQLite engine: 3.53.3 -> 3.53.4.
- Bundled Circle WASM hash and byte length changed because of the SQLite engine
  rebuild.
- Release metadata now includes the previous `0.6.1-0.6.2` 3.53.3 WASM epoch
  in the metadata-only historical catalog.
- README, toolchain docs, release manifest, and roadmap now identify the
  current bundled engine as SQLite 3.53.4.

### Unchanged

- OSR1/OSW1, owner-write authorization, storage accounting, execution budgets,
  top-level commands, CLI JSON envelopes, root Rust types, and runtime
  dependencies are unchanged.
- The optional `wasm-behavior` harness dependency is patched to `wasmtime`
  36.0.13 to keep the release audit clean.

### Proof Gate

Satisfied before publish: Vitals rehearsed the engine upgrade on devnet, then
upgraded the mainnet Lab DB Circle to SQLite 3.53.4 with read readiness, write
readiness, current-engine status, gateway health, updater health, and mirror
writes confirmed.

## 0.6.2

This is a focused client architecture, automation, and documentation release
over `0.6.1`. It makes the CLI exercise the same routine SQL data plane as Rust
applications without adding commands or crate-root types.

### Changed

- Ordinary one-statement CLI reads and writes now flow through `Database`.
- CLI responsibilities are separated into named modules instead of one catch-all
  orchestrator.
- Precise source error codes survive into CLI automation; the public client
  exposes them through the additive `Error::code()` method, and
  `cli::error_code()` owns the binary's stable JSON classification.
- docs.rs now presents a deliberate start path, read/write model, API map,
  error contract, advanced boundaries, and build configuration.
- GitHub releases have a dedicated release environment and OIDC workflow for
  crates.io trusted publishing once the crates.io-side publisher registration is
  complete.
- Pasted and stdin-imported private-key text is zeroized on parse failures, and
  the standalone `wasm-behavior` test feature now compiles independently.

Keep the GitHub `release` environment approval-gated, non-bypassable, and
restricted to `v*` refs. Before the first OIDC publish, register its exact
repository, workflow, and environment as the crates.io trusted publisher.

### Unchanged

- SQLite remains 3.53.3.
- The bundled Circle WASM is byte-for-byte identical to `0.6.1`; existing
  Circles do not need an upgrade.
- OSR1/OSW1, owner-write authorization, top-level commands, CLI JSON envelopes,
  and crate-root type names are unchanged.

### Version Boundary

This work is patch-compatible and therefore belongs in `0.6.2`. A future
change to `ClientOptions` key ownership, clone semantics, or signer interfaces
would break the public Rust API and remains reserved for a justified `0.7.0`.

## 0.6.1

This is a focused security and reliability hardening release over `0.6.0`.
SQLite remains 3.53.3, but the Circle WASM is a new engine epoch because it now
bounds SQLite work and enforces the exact current stable-storage capacity.

### Added

- Deterministic query and exec VDBE work budgets with stable error codes.
- Exact VFS capacity facts in `storage_info`, `limits --json`, and the release
  manifest.
- Durable owner-sequence comparison from `storage_info` when the
  storage-independent `auth_info` response omits it.
- The 1,024-dirty-page transaction ceiling in human and JSON limits.
- Durable upgrade manifest states: `prepared`, `applied`, and `complete`.
- Machine-readable confidentiality facts.

### Changed

- Config, nonce, receipt, OSR1, public program-info, and read-mode handling now
  fail closed at their trust boundaries.
- Normal upgrades always use the integrity-checked embedded release WASM.
- Verify and upgrade smoke tables are unique. Upgrade cleanup is atomic;
  verify attempts a confirmed cleanup and reports any cleanup failure.
- Config, backups, upgrade metadata, and RPC traces use private, safer local
  file creation.

### Security Boundary

- Sealed reads authenticate a wallet; they are not encrypted or owner-only.
- Public reads permit complete SQLite-file export through `backup_chunk`.
- Write SQL and values remain visible in Octra transaction history.
- Foreign-key enforcement is currently off and cannot be enabled through user
  SQL.
- SQLite work is bounded inside the contract. Runtime-wide WASM fuel remains
  an Octra protocol responsibility.

### Proof

- Fresh public-read devnet proof:
  `oct://devnet/oct3mZCqnNpQLi3TaogPA6prcunUcNNjDMhAB3j6byLg52x`.
- The proof Circle is on SQLite 3.53.3, is `engine_current: true`, answers
  walletless public reads, passed owner write-smoke plus cleanup, and passed
  local `sqlite3` integrity check from a page backup.
- Capacity evidence remains 8,069 pages / 33,050,624 SQLite file bytes. The
  higher 33,554,432-byte number is the Octra Circle stable-storage cap for
  key-plus-value bytes; VFS page keys, the generation manifest, and metadata
  consume the difference.

## 0.6.0

This is the first post-debut engine upgrade release. It rebuilds the bundled
Circle WASM with SQLite 3.53.3 and adds a first-class upgrade workflow for
existing database Circles.

## Added

- `octra-sqlite upgrade` for the guided terminal upgrade workflow.
- `octra-sqlite upgrade DATABASE` for safe in-place Circle program upgrades.
- `octra-sqlite upgrade DATABASE --dry-run` for preflight without writes.
- `octra-sqlite upgrade rollback BUNDLE` for restoring the previous verified
  personalized Circle program from an upgrade bundle.
- Private upgrade bundles containing backup metadata, rollback WASM when
  recovered, transaction proof, and the from/to code-hash epoch boundary.
- Operator-readable default bundle names containing network, Circle ID,
  previous SQLite version, and date.
- A release-manifest-backed, metadata-only historical WASM catalog for
  identifying known pre-0.6.0 release engines without bundling old engine bytes
  in the crate.
- `upgrade --previous-wasm PATH` for explicit rollback recovery with custom or
  unknown old program bytes.

## Changed

- Bundled SQLite engine: 3.53.2 -> 3.53.3.
- Bundled Circle WASM hash and byte length changed because of the SQLite engine
  rebuild.
- `--write-smoke` now removes its smoke table after the write cycle; it still
  counts as a post-upgrade write and makes clean rollback unavailable.
- Bundled Circle WASM and release manifest are embedded in the binary, with
  file/env overrides still available for source builds and operators.
- `rollback.clean` is nullable when live counters are unavailable; rollback
  remains fail-closed until reviewed.
- The rollback-byte bypass is named `--unsafe-no-rollback`; it is an emergency
  operator escape hatch and leaves the upgrade bundle unable to restore the
  previous Circle program.
- Full SQL event tracing now uses
  `OCTRA_SQLITE_EMIT_SQL_ONCHAIN_EVENT=1`, making the permanent on-chain event
  behavior explicit in the opt-in name.
- `status --json` reports `engine_current` and `upgrade_needed` separately from
  read/write readiness.
- No-op upgrade preflight reports `status: "already_current"` and marks
  rollback as not relevant.
- MSRV is now Rust/Cargo 1.96+.

## Notes

- Existing Circles are not upgraded automatically. Run `upgrade` or
  `upgrade DATABASE` with the owner wallet when you choose to move a Circle to
  the new engine.
- Upgrade can reconstruct rollback bytes from chain history, local old
  octra-sqlite release artifacts, or an explicit previous WASM plus live
  `auth_info`. Historical catalog metadata points operators to the expected
  tagged artifact and base WASM SHA-256 when manual recovery is needed.
- Rollback is clean only before post-upgrade writes. `--write-smoke` proves
  writes on the new engine with a create/insert/drop cycle, but intentionally
  makes clean rollback unavailable.
- Rollback availability matters only when `from.code_hash` differs from
  `to.code_hash`.
- The release manifest records a devnet proof that creates a 0.5.2 public-read
  database, upgrades it to 3.53.3, rolls it back to 3.53.2, then upgrades it to
  3.53.3 again.
- The README quick-start public Circle is recorded separately and remains on
  SQLite 3.53.2 until its owner upgrades it.
- OSR1/OSW1 wire formats, read modes, owner-write authorization, CLI JSON
  schema, and the Rust root API are unchanged from `0.5.2`.

## 0.5.2

This is a CLI readiness, raw-target, and install polish release over the
`0.5.1` crates.io debut. The contract, wire formats, bundled SQLite engine,
bundled Circle WASM, deployed event strings, and on-chain behavior are unchanged.

## Changed

- Raw Circle targets now detect the Octra read surface from Circle metadata, so
  public-read databases can be opened with `oct://NETWORK/CIRCLE` without
  adding `?read_mode=public`.
- `status --json` now reports `read_ready` and `write_ready` separately. The
  top-level `ready` field and `status --ready` gate track read/query readiness;
  owner-write readiness is reported separately for public-read deployments.
- Status output treats walletless public reads as a supported mode when Circle
  metadata confirms the target is public-readable.
- Public-read sessions now preserve malformed wallet-load errors and surface
  them when signed operations need a valid wallet.
- Interactive private-key paste now shows masked `*` feedback while keeping the
  secret out of shell history.
- CI now runs `cargo audit` for published-crate supply-chain checks.
- MSRV is now Rust/Cargo 1.88+, allowing patched HTTP cookie/time dependencies
  without a supply-chain audit ignore.

## Notes

- No Circle redeploy is required for `0.5.2`.
- The `0.5.2` release manifest records the same bundled Circle WASM hash as
  `0.5.0` and `0.5.1`.

## 0.5.1

This is a README, dependency, and packaging polish release. The contract, wire
formats, bundled SQLite engine, CLI command surface, JSON envelopes, Rust API,
and public-read behavior are unchanged from `0.5.0`.

## Changed

- Refactored the README around the package reader: quick CLI query, quick Rust
  client query, Octra context, root Rust API surface, read modes, wallet setup,
  verifiability, MSRV, stability, CLI command reference, shell dot-command
  reference, and security/contribution links.
- Replaced the GitHub-only Mermaid architecture diagram with a plain text
  diagram that renders on crates.io.
- Added `Client::default()` for config-free public reads, mirrored the README's
  Rust example in crate docs, and guarded the constructor in the public-surface
  test.
- Swapped the default HTTP transport dependency from `reqwest` to `ureq` and
  moved the crate to Rust edition 2024.
- Refreshed `CONTRIBUTING.md` and included it with `SECURITY.md` in the crate
  package.
- Added CI coverage for clippy and the `http`-without-`cli` feature
  configuration used by docs.rs.

## Notes

- No Circle redeploy is required for `0.5.1`.
- `0.5.1` is ready to be the first crates.io-published version.

## 0.5.0

This is a Rust API ontology and crates.io debut preparation release over the
deployed `0.3.3` Circle WASM proof. The contract, wire formats, bundled SQLite
engine, CLI command surface, JSON envelopes, and public-read behavior are
unchanged.

## Added

- Root Rust API exports for the admired path:
  `Client`, `ClientOptions`, `Database`, `QueryResult`, `ExecuteResult`,
  `SubmittedTransaction`, `AuthInfo`, `ProgramInfo`, `ReadMode`, `Value`,
  `Error`, `ErrorKind`, and `Result`.
- `Database::wait(&SubmittedTransaction)` to complete the `execute_no_wait`
  lifecycle without exposing raw sessions.
- `client::raw` for supported raw session/RPC plumbing.
- Public-surface compile tripwire tests and config alias-cycle tests.

## Changed

- Renamed the Rust public API around product nouns and removed the
  implementation-derived pre-debut names instead of carrying aliases.
- Removed old Rust aliases instead of carrying compatibility debt.
- Moved CLI entrypoints under `octra_sqlite::cli`.
- Replaced the old free operation-safety helper with `Operation::safety()`.
- Settled saved-database CLI naming on `database default NAME`.
- Updated README, library-boundary docs, and shipped Rust examples to use the
  root API.

## Notes

- No Circle redeploy is required for `0.5.0`.
- External signer work is deferred to `0.6.0`.

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
- README now leads with a walletless public-read command so a cold-start user
  can query a live database before importing a wallet.

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

- Public client-to-database Rust API shape.
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
