# Library Boundary

`octra-sqlite` has four maintained Rust layers.

## Root

The crate root is the first-story application API:

```rust
use octra_sqlite::Client;

let client = Client::from_default_config()?;
let db = client.database("organization")?;
let rows = db.query("select * from person order by first_name;")?;
```

Root exports are intentionally small:

- `Client`, `ClientOptions`, and `Database`
- `QueryResult`, `ExecuteResult`, and `SubmittedTransaction`
- `AuthInfo`, `ProgramInfo`, and `ReadMode`
- `Value`, `Error`, `ErrorKind`, and `Result`

`Client` is the control plane: configuration, transport ownership, and database
selection. `Database` is the data plane: SQL reads, writes, and inspection.

The CLI's ordinary one-statement query and write paths use this same
`Database` data plane. CLI-only workflows stay lower level where they need
capabilities the application API deliberately does not expose: RPC tracing,
script batching, restore, deployment, verification, and engine upgrades. This
keeps one implementation for routine SQL without inflating the root API to fit
operator concerns.

`Database::execute(sql)` is the confirmed write path.
`Database::execute_no_wait(sql)` returns `SubmittedTransaction`; pass it to
`Database::wait(&submitted)` to complete the lifecycle.

`Error::kind()` supplies a stable broad category. `Error::code()` preserves a
precise machine-readable code supplied by a remote source or assigned at a
local protocol boundary. Callers should not classify human error text.

## Client

`octra_sqlite::client` is the advanced application integration layer.

It exposes local config types and helpers:

- `Config`, `NetworkConfig`, and `DatabaseMetadata`
- `config_path`, `load_config`, and `write_config`

It exposes the supported transport seam:

- `Transport`
- `HttpTransport`
- `RpcTraceMode`

It exposes the advanced write/signer lifecycle:

- `PreparedWrite`
- `PreparedOwnerWrite`
- `SignedWrite`
- `Operation`
- `OperationSafety`

Use `Operation::Execute.safety()` when an adapter needs to surface whether an
operation reads SQL, mutates state, submits a transaction, waits for a receipt,
or requires OSW1 owner write intent. The metadata is target-independent and
conservative; public reads can use unsigned Circle views.

## Raw

`octra_sqlite::client::raw` is supported raw plumbing for the CLI, audits,
tests, and advanced adapters.

It exposes sessions and direct Octra RPC helpers such as `view`, `query_typed`,
`exec_sql`, `submit_tx`, and `wait_for_transaction`. New app, REST, MCP, A2A,
or service integrations should start with `Client` and `Database` and use
`raw` only when they need direct RPC control or an operator workflow that is
not part of the application data plane.

## Protocol

`octra_sqlite::protocol` is transport-independent wire format support:

- `osr1`: typed SQL result decoding
- `osw1`: owner write intent framing
- `target`: `oct://` database URI parsing and read modes
- `tx`: canonical Octra transaction JSON

Adapters should not reimplement OSR1, OSW1, target parsing, or transaction
canonicalization.

## Boundaries

The CLI remains the primary product surface for humans and automation. Rust
applications use the root `Client`/`Database` API first.

Do not add servers, frameworks, query builders, ORMs, agent runtimes,
compatibility aliases, or duplicated command surfaces to the core repo. Those
belong in examples or downstream adapters if they earn their weight.
