# Public Surface

The reference repo has one primary user-facing entrypoint:

```sh
octra-sqlite
```

Keep routine workflows behind the Rust CLI first. The Rust client library is
the reusable reference client for code and agents. Lower-level scripts are only
for building and auditing the bundled WASM.

Runtime defaults live in `config/defaults.json`. The bundled defaults preload
devnet RPC and the public Remilia example database; user config overlays them.

The reference first-run path is:

```sh
octra-sqlite setup
octra-sqlite status
octra-sqlite quickstart NAME
```

The reference configurable database creation path is:

```sh
octra-sqlite new NAME
octra-sqlite new NAME < schema.sql
octra-sqlite new NAME "create table ..."
```

`quickstart` is a thin convenience layer over `new`: it chooses the built-in
`remilia` sample by default, saves a database name, and makes the new database
the default database. `new` submits a native signed `deploy_circle` transaction
whose payload includes the bundled audited SQLite WASM, saves an `oct://`
database URI, and then runs optional initializer SQL through the same signed
`exec` path as later writes.

`deploy` updates existing Circle programs through the same Rust-native signed
RPC path. The Octra webcli helper is not required for the maintained SQLite
workflow.

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

## Public

- `src/main.rs`: Rust CLI orchestration.
- `src/output.rs`: CLI table/json/csv rendering.
- `src/client/`: reusable Rust client.
- `src/client/mod.rs`: intentional client exports. The intended path is
  `OctraSqlite -> Database -> query/execute`; raw deploy/RPC helpers live under
  `client::low_level`.
- `src/protocol/`: transport-independent wire formats and target parsing.
- `src/protocol/osr1.rs`: OSR1 typed-result decoding.
- `src/protocol/osw1.rs`: OSW1 owner write intent framing.
- `config/defaults.json`: preloaded devnet RPC and public example database.
- `circle/source/octra_sqlite_circle.c`: Octra Circle program source.
- `circle/wasm/octra_sqlite_circle.wasm`: bundled audited Circle WASM.
- `docs/spec/osr1.md`: typed result codec.
- `docs/spec/osw1.md`: OSW1 owner write intent frame.
- `release/octra-sqlite-0.1.0.json`: release manifest for the bundled Circle
  WASM and published network-specific deployment.
- `examples/`: concrete runnable walkthroughs kept out of the README.
- `scripts/install-cli.sh`: local installer for `cargo install --path .`.
- `scripts/build-wasm.sh`: optional local WASM rebuild for contract changes.
- `scripts/audit-wasm.sh`: import/export audit.

## Rule

If a workflow is part of the reference experience, implement it in Rust and
expose it through `octra-sqlite`. Do not grow the supported surface by adding
new first-class scripts.
