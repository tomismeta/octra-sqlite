# Public Surface

The reference repo has one primary user-facing entrypoint:

```sh
octra-sqlite
```

Keep routine workflows behind the Rust CLI first. The Rust client library is
the reusable reference client for code and agents. Lower-level scripts are only
for building and auditing the bundled WASM.

There is no separate agent command set. Humans use the readable CLI and
`sqlite>` shell. Automation, agents, and services use the same commands with
`--json`, `--json-summary`, `status --ready`, `commands --json`, and
`limits --json`. Rust applications can use the client library directly.

Runtime defaults live in `config/defaults.json`. The bundled defaults keep
devnet active and preload devnet and mainnet URL profiles; user config overlays
them. No database names are bundled by default.

The reference first-run path is:

```sh
octra-sqlite setup
octra-sqlite status
octra-sqlite new art < examples/artists.sql
octra-sqlite status art --ready
octra-sqlite art ".tables"
octra-sqlite art "select * from artist;"
```

The reference configurable database creation path is:

```sh
octra-sqlite new
octra-sqlite new DATABASE
octra-sqlite new DATABASE --sample artists
octra-sqlite new DATABASE --schema schema.sql --manifest database.json --json
octra-sqlite new DATABASE --read-mode public --schema public-schema.sql
octra-sqlite new DATABASE < schema.sql
octra-sqlite new DATABASE "create table ..."
octra-sqlite restore DATABASE --file dump.sql
octra-sqlite restore DATABASE --file dump.sql --json-summary
octra-sqlite check DATABASE --sql-file dump.sql
octra-sqlite limits DATABASE
octra-sqlite commands --json
octra-sqlite install --json
```

`new` submits a native signed `deploy_circle` transaction whose payload includes
the bundled audited SQLite WASM, saves an `oct://` database URI, and then runs
optional initializer SQL through the same signed `exec` path as later writes.
`new --sample NAME` is the built-in sample path; it is not a separate command.
Interactive `new` and setup's optional sample creation both ask for read mode
as `sealed` or `public`, with `sealed` as the default.
`new --read-mode public` creates an explicit public-read Circle tuple
(`public / gateway_allowed / public_resources`). Public-read SQL queries use
`octra_circleView`; sealed databases keep `octra_circleViewAuth`. Writes stay
owner-signed through OSW1 in both modes.

`deploy` updates existing Circle programs through the same Rust-native signed
RPC path. The Octra webcli helper is not required for the maintained SQLite
workflow. `deploy --bootstrap-owner` is a narrow recovery path for empty
owner-owned Circles whose older program cannot expose `auth_info` before first
storage pages exist; it verifies Circle ownership, deploys owner-personalized
bundled WASM, and saves local bootstrap metadata without submitting SQL. Pair it
with `restore --bootstrap-owner` to submit only the first storage-creating batch
as an OSW1 owner-signed write, then return to the normal `auth_info` path.

State-changing SQL uses the Circle `exec` method through a signed `circle_call`.
For owner-personalized databases, the CLI also includes an OSW1 owner write
intent that binds the Circle method, database id, sequence, and SQL before
SQLite runs.
Set
`OCTRA_SQLITE_TRACE_SQL_EVENT=1` to use `exec_trace` and emit SQL text in
addition to the default SQL hash event.

`status`, `config`, and `database info` are the primary inspection commands.
They should stay expressive enough that users do not need to inspect
`~/.octra/sqlite.json`, transaction JSON, or explorer pages for the common path.
`config` shows the active RPC/explorer plus all bundled network profiles.
`wallet status` shows wallet path, permissions, derived caller, and target
read/write relationship without printing wallet secrets.
For automation, use `--json` on non-interactive commands and prefer full
`oct://NETWORK/<circle>` URIs over local database names.
Use `--trace-rpc-json FILE` on one-shot read SQL when an app or agent needs the
Octra JSON-RPC request/response envelope for proof or debugging. Use
`--trace-rpc-json-mode summary` when hashes and sizes are enough.
`limits --json` is the supported place to discover SQL limits, result limits,
auth behavior, restore behavior, trace modes, and JSON schema names.
`commands --json` lists the supported command surface and JSON envelopes without
requiring callers to parse human help text.

## Public

- `src/main.rs`: tiny binary entrypoint.
- `src/cli/mod.rs`: top-level Rust CLI command orchestration.
- `src/cli/output.rs`: CLI table/json/csv rendering.
- `src/cli/shell.rs`: interactive SQLite-style shell and dot commands.
- `src/cli/portability.rs`: backup, dump, SQL restore, and CSV import helpers.
- `src/client/`: reusable Rust client.
- `src/client/mod.rs`: intentional client exports. The intended path is
  `OctraSqlite -> Database -> query/execute` with typed results and operation
  safety metadata. Raw deploy/RPC helpers live under `client::low_level`.
- `src/client/database.rs`: small database handle.
- `src/client/rpc.rs`: signed RPC/view/query plumbing.
- `src/client/results.rs`: typed client result wrappers and receipt validation.
- `src/client/safety.rs`: operation safety metadata.
- `src/client/write.rs`: owner-write prepare, sign, and submit lifecycle.
- `src/client/transport.rs`: the client transport seam. The default is HTTP
  RPC; tests and future adapters can provide their own `Transport`.
- `src/client/error.rs`: typed client error kinds for adapters that need stable
  authorization, receipt, timeout, RPC, transport, and protocol categories.
- `src/protocol/`: transport-independent wire formats and database URI parsing.
- `src/protocol/osr1.rs`: OSR1 typed-result decoding.
- `src/protocol/osw1.rs`: OSW1 owner write intent framing.
- `config/defaults.json`: active devnet config and devnet/mainnet URL profiles.
- `circle/source/octra_sqlite_circle.c`: Octra Circle program source.
- `circle/wasm/octra_sqlite_circle.wasm`: bundled audited Circle WASM.
- `docs/spec/osr1.md`: typed result codec.
- `docs/spec/osw1.md`: OSW1 owner write intent frame.
- `docs/operations.md`: large restore, limits, atomicity, and migration
  guidance.
- `docs/json-output.md`: stable CLI JSON envelopes and read RPC trace format.
- `release/octra-sqlite-0.4.0.json`: release manifest for the bundled Circle
  WASM and network deployment metadata.
- `examples/`: concrete runnable walkthroughs kept out of the README, including
  a tiny read-only Remilia API example.
- `scripts/install-cli.sh`: local installer for `cargo install --path .`.
- `scripts/build-wasm.sh`: optional local WASM rebuild for contract changes.
- `scripts/audit-wasm.sh`: import/export audit.

## Rule

If a workflow is part of the reference experience, implement it in Rust and
expose it through `octra-sqlite`. Do not grow the supported surface by adding
new first-class scripts.
