# Changelog

## 0.6.3

- Rebuilt the bundled Circle WASM with SQLite 3.53.4, taking SQLite's
  upstream fixes for problems in 3.53.0 through 3.53.3.
- Added the 0.6.1-0.6.2 Circle WASM as a metadata-only historical upgrade
  catalog entry so rollback recovery can identify the previous 3.53.3 epoch
  without bundling old WASM bytes.
- Added `--ou` for ordinary CLI SQL writes and restore batches, plus
  `OCTRA_SQLITE_WRITE_OU` as the CLI write budget default.
- Added `verify --write-ou`, `upgrade --write-smoke --write-ou`, and
  `OCTRA_SQLITE_VERIFY_WRITE_OU` for confirmed write-smoke rehearsals.
- Added `receipt TX_HASH [DATABASE] --json` for following an already-submitted
  Circle transaction without resubmitting SQL.
- Added `nonce` and signed `ou` metadata to submitted write results and
  introduced the stable `receipt_pending` CLI error code for accepted writes
  whose receipt is not available before the wait deadline.
- Updated the release manifest, toolchain record, README badge, and roadmap for
  the 3.53.4 engine epoch.
- Added a WASM harness smoke test for the 3.53.4 engine path: version reporting,
  expression-index reads, JSON extraction, and malformed JSONB failure.
- Updated the optional `wasm-behavior` harness dependency to `wasmtime` 36.0.13
  to keep `cargo audit` green.
- Kept OSR1/OSW1, owner-write authorization, storage accounting, execution
  budgets, root Rust entry points, and runtime dependencies unchanged from
  `0.6.2`.

## 0.6.2

- Routed ordinary one-statement CLI query and execute through the same
  `Database` data plane used by Rust applications, while keeping tracing,
  scripts, restore, deploy, verify, and upgrade on explicit control-plane
  plumbing.
- Split the CLI catch-all module into focused onboarding,
  database, SQL, inspection, deployment, catalog, and test modules without
  changing commands or output envelopes.
- Preserved precise RPC, Circle, receipt, and local policy error codes at their
  construction sites; added `Error::code()` while retaining the existing CLI
  JSON error vocabulary and broad `ErrorKind` categories.
- Curated the docs.rs crate landing page and advanced module boundaries while
  keeping the README focused on product use.
- Added the GitHub-side trusted-publishing workflow from exact release tags
  through a dedicated release environment; crates.io-side publisher
  registration remains the one-time setup before using OIDC publish.
- Zeroized pasted/imported private-key text on every exit path and made the
  standalone `wasm-behavior` test feature self-contained without changing the
  resolved dependency set.
- Kept SQLite 3.53.3, the bundled Circle WASM, OSR1/OSW1, owner-write
  authorization, top-level CLI commands, JSON envelopes, and root Rust types
  unchanged from `0.6.1`.

## 0.6.1

- Added deterministic SQLite progress-handler budgets: 5,000,000 VDBE steps
  per query and 25,000,000 per exec, with stable limit errors and rollback on
  exhausted writes.
- Corrected current generation-manifest capacity to 8,069 pages and 33.05 MB
  of SQLite file data under Octra's 33.55 MB key-plus-value storage cap, while
  retaining 8,192 legacy VFS read slots. Upgrade preflight refuses a legacy
  database that is already larger than the target engine can write.
- Exposed the 1,024-dirty-page per-exec limit in human and JSON capability
  output, with bulk-update guidance.
- Made upgrade preflight read durable owner sequence from `storage_info` when
  storage-independent `auth_info` omits it.
- Made config parsing, nonce decoding, receipt confirmation, query row counts,
  and OSR1 decoding fail closed; OSR1 now rejects oversized counts, invalid
  UTF-8, non-finite reals, and allocation/offset overflow.
- Routed public `ProgramInfo` reads through the unsigned Octra RPC and made
  explicit URI read modes take precedence over saved metadata.
- Removed the ambient WASM deployment override. Normal upgrades now use only
  the integrity-checked embedded release artifact; deliberate custom deploys
  require explicit command flags.
- Made upgrade bundles durable before chain submission with atomic
  `prepared -> applied -> complete` manifest transitions.
- Replaced fixed smoke tables with unique collision-safe tables; verify always
  attempts a confirmed cleanup and reports cleanup failure.
- Made config, backup, upgrade-manifest, and RPC-trace files private and atomic
  where applicable; RPC tracing now refuses to overwrite an existing file.
- Made sealed-read and write-transaction visibility explicit in human output,
  manifests, `limits --json`, README, security guidance, and operations docs.
- Documented that public-read access permits full database backup and that
  foreign-key enforcement is currently off.
- Kept SQLite 3.53.3, OSR1/OSW1 wire formats, owner-write authorization, root
  Rust API, and the eight-import/five-export WASM surface unchanged.

## 0.6.0

- Rebuilt the bundled Circle WASM with SQLite 3.53.3.
- Added `octra-sqlite upgrade` as a setup-style guided workflow and
  `octra-sqlite upgrade DATABASE` as the direct safe in-place engine upgrade
  path for existing database Circles.
- Added `upgrade --dry-run` for owner, storage, rollback, and target-engine
  preflight without writing.
- Added private local upgrade bundles containing the SQLite backup, recovered
  previous personalized WASM when available, and an `upgrade.json` manifest
  with the engine epoch boundary.
- Named default upgrade bundles with network, Circle ID, previous SQLite
  version, and date.
- Added `octra-sqlite upgrade rollback BUNDLE` to restore the previous verified
  Circle program from an upgrade bundle.
- Made rollback refuse to cross post-upgrade writes unless
  `--force-after-writes` is supplied; forced rollback takes a fresh backup
  first.
- Made `--write-smoke` clean up its smoke table after the write cycle while
  still recording that clean rollback now requires `--force-after-writes`.
- Restored public deployment/read proof sections and personalization fields in
  the release manifest so the upgraded live WASM hash remains reproducible.
- Embedded the bundled Circle WASM and release manifest in the binary so
  installed builds do not depend on the source/build directory at runtime.
- Added a release-manifest-backed, metadata-only historical WASM catalog plus
  `upgrade --previous-wasm PATH` for rollback recovery from older
  owner-personalized deployments without bundling old WASM bytes in the crate.
- Made the rollback-byte bypass explicit as `upgrade --unsafe-no-rollback`;
  the normal upgrade path still refuses to apply without rollback bytes.
- Renamed the full-SQL event opt-in to
  `OCTRA_SQLITE_EMIT_SQL_ONCHAIN_EVENT` so the permanence of SQL event tracing
  is visible at the call site.
- Made `rollback.clean` nullable when live counters are unavailable, while
  keeping rollback fail-closed.
- Added `engine_current` and `upgrade_needed` status JSON fields so healthy old
  engines are reported as upgrade candidates instead of generic failures.
- Made no-op upgrade preflight report `status: "already_current"` and mark
  rollback as not relevant.
- Raised the documented MSRV to Rust/Cargo 1.96+ after downstream staging
  exposed 1.88 install incompatibility with the resolved CLI dependency set.
- Kept OSR1/OSW1 wire formats, read modes, owner-write authorization, JSON
  envelope schema, and the Rust root API unchanged from `0.5.2`.

## 0.5.2

- Made raw Circle targets detect the Octra read surface from Circle metadata, so
  public-read databases can be opened with `oct://NETWORK/CIRCLE` without
  adding `?read_mode=public`.
- Split `status --json` readiness into `read_ready` and `write_ready`. The
  top-level `ready` and `status --ready` gate now track read/query readiness,
  while owner-write capability is reported separately.
- Reduced setup/wallet warning noise for explicit public-capable targets:
  walletless public reads are reported as a supported mode, not a broken owner
  setup.
- Preserved malformed wallet-load errors on public-read sessions, so public
  reads can continue without hiding why signed operations are unavailable.
- Added masked feedback for interactive private-key paste while keeping input
  out of shell history.
- Added `cargo audit` to CI for published-crate supply-chain checks.
- Raised the documented MSRV to Rust/Cargo 1.88+ so the lockfile can use
  patched HTTP cookie/time dependencies without a supply-chain audit ignore.
- Kept the bundled Circle WASM, OSR1/OSW1 wire formats, deployed event strings,
  and on-chain contract behavior unchanged from `0.5.1`.

## 0.5.1

- Refactored the README for public package readers: CLI and Rust client quick
  starts now appear above the fold, Octra context, the root Rust API, read
  modes, wallet setup, verifiability, stability, CLI commands, and shell
  dot-command reference stay visible.
- Added crates.io/docs.rs badges, MSRV/stability notes,
  license/contributing/security links, and a crates.io-safe ASCII architecture
  diagram.
- Added `Client::default()` for config-free public reads, mirrored the README's
  Rust example in crate docs, and guarded the constructor in the public-surface
  test.
- Swapped the default HTTP transport dependency from `reqwest` to `ureq` and
  moved the crate to Rust edition 2024.
- Refreshed `CONTRIBUTING.md` and included it with `SECURITY.md` in the crate
  package so README links resolve from crates.io.
- Added CI coverage for clippy and the `http`-without-`cli` feature
  configuration used by docs.rs.
- Kept CLI commands, JSON envelopes, Rust API, OSR1/OSW1 wire formats, bundled
  Circle WASM, and public-read behavior unchanged from `0.5.0`.

## 0.5.0

- Reshaped the Rust public API for the first crates.io-ready debut:
  `Client -> Database -> query/execute/inspect`.
- Promoted the first-story Rust types to the crate root:
  `Client`, `ClientOptions`, `Database`, `QueryResult`, `ExecuteResult`,
  `SubmittedTransaction`, `AuthInfo`, `ProgramInfo`, `ReadMode`, `Value`,
  `Error`, `ErrorKind`, and `Result`.
- Moved raw session/RPC helpers under `client::raw`.
- Renamed public Rust API types for consistent product vocabulary and removed
  old aliases before any public Rust production users existed.
- Added `Database::wait(&SubmittedTransaction)` so `execute_no_wait` has a
  complete high-level confirmation path.
- Replaced the old free operation-safety helper with `Operation::safety()`.
- Settled saved-database CLI naming on `database default NAME`.
- Added a public-surface compile tripwire and a config alias-cycle guard.
- Updated Rust examples and library-boundary docs to use root imports.
- Kept the bundled Circle WASM, OSR1/OSW1 wire formats, CLI commands, JSON
  envelopes, and public-read behavior unchanged from `0.4.0`.

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
- Simplified interactive `new` to the product path: explicit database name,
  read mode, confirmation. Wallet, network, default database, and manifest path
  are resolved from configuration and conventions.
- Simplified `setup` to wallet and network configuration. Database defaults are
  established by `new`.
- Added a shared wallet onboarding flow for `setup` and guided `new`: use
  `wallet.json` from the official Octra wallet generator, attach existing
  plaintext wallet JSON, paste a private key through a hidden terminal prompt,
  or continue without a wallet for public-read queries only.
- Accepted the official Octra wallet-generator JSON shape
  (`keyPair.publicKey` / `keyPair.secretKey`) and preserved that file shape when
  importing through the generator-guided setup path.
- Added `rpassword` as a CLI-only dependency for no-echo interactive private-key
  import; the protocol/client core and `--no-default-features` build stay
  dependency-light.
- Kept `.oct` WebCLI wallets out of the direct import path: they are explained
  as encrypted/PIN-protected and reserved for a future paired, confirming
  external signer flow.
- Shortened human `new` output and added a manifest `read_uri` field for
  shareable public-read database URIs.
- Removed redundant public command surfaces: `init`, `quickstart`, command
  aliases, option aliases, and legacy config aliases.
- Kept `setup --yes` as the scriptable setup path and `new --sample NAME` as
  the built-in sample path.
- Updated README and reference docs around one clean cold-start path and
  explicit sealed/public read modes.
- Made walletless public-read the first cold-start path in README.

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
- Added the reusable client-to-database API shape for
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
