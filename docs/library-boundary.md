# Library Boundary

`octra-sqlite` has three maintained layers:

1. `protocol`: small, transport-independent wire formats.
   - OSR1 typed result decoding
   - OSW1 owner write intent framing
   - canonical transaction JSON
   - `oct://` database target parsing
2. `client`: the reusable Rust reference client.
   - `OctraSqlite::from_default_config()?`
   - `client.database("remilia")?`
   - `db.query("select ...")?`
   - `db.execute("insert ...")?`
3. `cli`: the SQLite-like user experience.
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

`client::low_level` exists for the CLI, deployment, and audit plumbing. New app,
agent, REST, MCP, or A2A integrations should build on the default `Database`
API first and only use `low_level` when they need to reproduce the CLI's signed
Octra transaction flow.

Do not add servers, frameworks, query builders, ORMs, or agent runtimes to the
core repo. Those belong in examples or downstream adapters.
