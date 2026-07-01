# Handoff

Last updated: 2026-07-01

## Current State

`octra-sqlite` is a Rust CLI and client library for running the real SQLite C
engine inside an Octra `wasm_v1` Circle. The bundled Circle WASM is included in
the repo, so a new user does not need to compile WASM to start.

Latest published release:

- Tag: `v0.4.0`
- Release: https://github.com/tomismeta/octra-sqlite/releases/tag/v0.4.0
- Tagged commit: `369ab25`
- Current `main`: ahead of the tag with README/handoff documentation polish

The `v0.4.0` release is a productization release over the deployed `0.3.3`
Circle WASM proof. The contract, wire formats, and bundled SQLite engine did
not change in `0.4.0`.

## Product Shape

The intended first-run path is now:

```sh
git clone https://github.com/tomismeta/octra-sqlite.git
cd octra-sqlite
cargo install --path . --locked
octra-sqlite setup
octra-sqlite new
```

Fast public-read path, no wallet required:

```sh
octra-sqlite 'oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ?read_mode=public' \
  "select id, name from artist order by id;"
```

Interactive SQLite-shaped use:

```sh
octra-sqlite open DATABASE
sqlite> .tables
sqlite> select * from artist;
sqlite> .quit
```

Machine and service use:

```sh
octra-sqlite status DATABASE --ready --json
octra-sqlite commands --json
octra-sqlite limits DATABASE --json
octra-sqlite restore DATABASE --file dump.sql --json-summary
```

## What Shipped Recently

### 0.4.0

- Public-read databases: `new DATABASE --read-mode public`.
- Sealed remains the default read mode.
- Public-read SQL uses unsigned `octra_circleView`.
- Sealed SQL reads keep signed `octra_circleViewAuth`.
- Writes remain owner-signed OSW1 calls in both modes.
- `setup` became the clean first door for wallet and network defaults.
- `new` became an opinionated database creation flow.
- Redundant legacy command surfaces were removed.
- Wallet onboarding was upgraded:
  - import official Octra wallet-generator `wallet.json`
  - attach existing plaintext `wallet.json`
  - paste private key through a hidden prompt
  - continue walletless for public-read queries only
- WebCLI `.oct` files are recognized as encrypted/PIN-protected and are not
  imported directly.

### 0.3.4

- Guided `octra-sqlite new`.
- Machine-readable `commands --json`.
- Scriptable database creation with `--schema`, `--manifest`, and `--json`.

### 0.3.3

- Empty sealed Circle bootstrap recovery.
- Mainnet-friendly restore/backfill retry improvements.
- `wallet status`.
- Readiness booleans in `status --json`.
- Compact RPC trace modes and stronger JSON error taxonomy.

### 0.3.1 and 0.3.2

- Large SQL restore/check paths.
- File/stdin SQL input.
- Stable JSON output docs.
- Optional read RPC trace files.

## Wallet Model

The repo needs a signer, not a wallet product.

Today it supports:

- official Octra wallet-generator JSON
- octra-sqlite normalized plaintext wallet JSON
- hidden interactive private-key paste
- headless stdin private-key import
- explicit wallet attach by path

It does not directly import encrypted WebCLI `.oct` files. The clean future path
is an external signer abstraction, not decrypting `.oct` inside octra-sqlite.
That future signer should be paired/authenticated, user-confirming, domain
separated, and non-exporting.

## Read Modes

Sealed databases:

- Default.
- Reads require signed Octra view auth.
- Writes require owner-signed OSW1.
- Best for private or operator-controlled data.

Public-read databases:

- Explicit opt-in with `--read-mode public`.
- Reads use unsigned `octra_circleView`.
- Writes still require owner-signed OSW1.
- Best for public datasets, public mirrors, and read-only app views.

Raw `oct://` URIs default to sealed unless they include
`?read_mode=public` or saved local metadata says the database is public.

## Verification And Hygiene

Current release hygiene:

- GitHub release for `v0.4.0` exists.
- Release manifest exists at `release/octra-sqlite-0.4.0.json`.
- `CHANGELOG.md` and `RELEASE.md` describe `0.4.0`.
- No Python/Docker dependency is part of the user path.
- The bundled Circle WASM is committed.
- Core Rust library remains separate from CLI concerns.
- `commands --json` is the machine-readable command surface.

Before the next release:

```sh
cargo test --locked --quiet
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --no-default-features --lib
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
git diff --check
```

## Immediate Follow-Ups

Recommended small polish before the next tag:

- Align the README command table with the new wallet flow.
- Add a tiny public-read example to `examples/README.md`.
- Keep `docs/headless.md` as the deeper server/operator wallet document.
- Keep the top-level README thin: cold start, wallet setup, CLI commands,
  read modes, `sqlite>` shell, architecture.

Do not add a separate agent command set. Agents, scripts, and services should
use the same CLI with `--json`, `--json-summary`, `commands --json`,
`limits --json`, full `oct://` URIs, or the Rust client library.

## Later Roadmap

High-value future work:

- Crates.io publish once the API surface has soaked.
- Prebuilt binaries if source install becomes friction.
- External signer abstraction for WebCLI/browser/HSM-style signing.
- Public-read operational soak and docs based on real users.
- More release automation around SQLite upgrades.
- Optional web/admin console later, only if it does not pollute the core.

Deferred deliberately:

- Direct encrypted `.oct` decryption inside octra-sqlite.
- REST/MCP/server framework inside the core repo.
- Agent-specific command surfaces.
- Multi-writer grants until Octra-native policy support is clearer.
- Complex migration framework.

## Design Guardrails

- Stay true to SQLite: use SQLite semantics and SQLite-shaped UX.
- Stay Octra-native: Circle deployment, OSW1 writes, Octra read modes.
- Keep the footprint small.
- Keep the CLI first-class for both humans and automation.
- Keep public API stability around wire formats and JSON envelopes.
- Prefer deletion and consolidation over new surface area.
- No old aliases or compatibility clutter during 0.x.
