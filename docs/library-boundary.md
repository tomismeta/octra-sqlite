# Library Boundary

`octra-sqlite` has three maintained layers:

1. `protocol`: small, transport-independent wire formats.
   - OSR1 typed result decoding
   - OSW1 owner write intent framing
   - canonical transaction JSON
   - `oct://` database URI parsing
2. `client`: the reusable Rust reference client.
   - `OctraSqlite::from_default_config()?`
   - `client.database("remilia")?`
   - `db.query("select ...")? -> QueryResult`
   - `db.execute("insert ...")? -> ExecResult`
   - `db.prepare_write(...) -> sign_write(...) -> submit_signed_write_and_wait(...)`
   - `db.prepare_write_no_wait(...) -> sign_write(...) -> submit_signed_write(...)`
   - `Transport` for HTTP, mock tests, and future adapters
3. `cli`: the SQLite-shaped user experience.
   - `octra-sqlite remilia`
   - `.tables`, `.schema`, `.read`, `.open`
   - Octra-aware inspection through `.circle`, `.storage`, `.wallet`, `.proof`

The default public Rust path should stay small:

```rust
use octra_sqlite::client::OctraSqlite;

let client = OctraSqlite::from_default_config()?;
let db = client.database("remilia")?;
let rows = db.query("select * from collection where opensea_slug = 'milady';")?;
```

`Database` methods return typed Rust wrappers over OSR1/RPC data.
`QueryResult`, `ExecResult`, `SubmittedTx`, `ProgramInfo`, and `AuthInfo` are
the public client shapes; `serde_json::Value` remains available under
`client::low_level` for CLI and audit plumbing.

Every public database operation has safety metadata through
`operation_safety(DatabaseOperation::...)`. Adapters should surface that
metadata directly: reads are safe to preview, writes submit Octra transactions,
and write operations require owner write intent authorization.

`Transport` is the only network seam in the client layer. The default transport
is blocking HTTP RPC through `HttpTransport`, which is appropriate for the CLI,
native REST services, MCP servers, A2A agents, and tests. Browser and worker
clients are protocol-compatible through OSR1/OSW1, but they should use a future
async/WASM transport rather than the current blocking Rust transport. Adapters
should not reimplement OSR1, OSW1, or transaction signing.

`client::low_level` exists for the CLI, deployment, and audit plumbing. New app,
agent, REST, MCP, or A2A integrations should build on the default `Database`
API first and only use `low_level` when they need to reproduce the CLI's signed
Octra transaction flow.

Do not add servers, frameworks, query builders, ORMs, or agent runtimes to the
core repo. Those belong in examples or downstream adapters.
