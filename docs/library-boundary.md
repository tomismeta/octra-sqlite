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

`Database::execute(sql)` is the confirmed write path.
`Database::execute_no_wait(sql)` returns `SubmittedTransaction`; pass it to
`Database::wait(&submitted)` to complete the lifecycle.

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
or requires OSW1 owner write intent.

## Raw

`octra_sqlite::client::raw` is supported raw plumbing for the CLI, audits,
tests, and advanced adapters.

It exposes sessions and direct Octra RPC helpers such as `view`, `query_typed`,
`exec_sql`, `submit_tx`, and `wait_for_transaction`. New app, REST, MCP, A2A,
or service integrations should start with `Client` and `Database` and use
`raw` only when they need to reproduce the CLI's signed Octra transaction flow.

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
