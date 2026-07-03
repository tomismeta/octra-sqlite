# octra-sqlite

**Real SQLite inside an Octra Circle.**

[![crates.io](https://img.shields.io/crates/v/octra-sqlite.svg)](https://crates.io/crates/octra-sqlite)
[![docs.rs](https://docs.rs/octra-sqlite/badge.svg)](https://docs.rs/octra-sqlite)
[![ci](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-6f42c1)](./LICENSE)
[![sqlite](https://img.shields.io/badge/sqlite-3.53.2-0f766e)](https://sqlite.org/)

`octra-sqlite` runs the SQLite C engine inside an Octra `wasm_v1` Circle.
It gives you a SQLite-shaped CLI and a small Rust client for live Circle state:
query public databases without a wallet, create sealed or public-read databases
with an Octra wallet, and verify what was deployed.

New to Octra? Start with the official [Octra docs](https://docs.octra.org/)
and [Circles guide](https://docs.octra.org/user-docs/circles). In this project,
a Circle is the Octra-hosted WASM environment that owns the SQLite pages and
executes the SQLite engine.

- Real SQLite: SQL is executed by the bundled SQLite C amalgamation.
- Octra-native writes: state-changing SQL is owner-signed through OSW1 owner
  write intent.
- Public when you choose: public-read databases can be queried without a
  wallet, while sealed databases remain the default.
- Verifiable deployment: the bundled WASM, rebuild inputs, audit script, hashes,
  and devnet proof metadata are published with each release.

## Quick Start: CLI

You need Rust/Cargo 1.87+. The Circle WASM is bundled; no local WASM toolchain
is required.

```sh
cargo install octra-sqlite --locked
```

Read a public database immediately, no wallet required:

```sh
octra-sqlite 'oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ?read_mode=public' \
  "select id, name from artist order by id;"
```

Open the same database in the interactive `sqlite>` shell:

```sh
octra-sqlite open 'oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ?read_mode=public'
```

Create a database when you have a funded Octra wallet:

```sh
octra-sqlite setup
octra-sqlite new art < examples/artists.sql
octra-sqlite status art --ready
octra-sqlite art "select * from artist order by name;"
```

`setup` is the first door for wallet and network defaults. It can import the
official Octra wallet-generator `wallet.json`, attach an existing plaintext
wallet JSON, accept a hidden private-key paste, or continue walletless for
public-read queries. For scripted wallet setup, see
[docs/headless.md](./docs/headless.md).

## Wallets

Public-read queries do not need a wallet. Sealed reads and all writes require a
configured Octra wallet because reads use signed Octra view auth and writes are
owner-signed.

```sh
octra-sqlite setup
octra-sqlite wallet status
octra-sqlite wallet attach ./wallet.json
printf '%s' "$OCTRA_PRIVATE_KEY_B64" | octra-sqlite wallet import --stdin --output ./wallet.json
```

Supported wallet inputs are the official Octra wallet-generator `wallet.json`,
an existing plaintext wallet JSON, or a private key pasted/imported through the
CLI. WebCLI `.oct` files are encrypted/PIN-protected and are not imported
directly; use the official `wallet.json`, attach plaintext wallet JSON, or
import the private key.

## Quick Start: Rust

```toml
[dependencies]
octra-sqlite = "0.5"
```

```rust,no_run
use octra_sqlite::{Client, Result};

fn main() -> Result<()> {
    let client = Client::default();
    let db = client.database(
        "oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ?read_mode=public",
    )?;
    let rows = db.query("select id, name from artist order by id;")?;
    println!("{} rows", rows.row_count);
    Ok(())
}
```

The high-level Rust path is deliberately small:
`Client -> Database -> query/execute`. Use `client::raw` only for lower-level
adapter plumbing that needs to reproduce CLI-style signed Octra RPC flows.

## Rust API Surface

The crate root exports the first-story API:

| Type | Purpose |
| --- | --- |
| `Client` | Configured entrypoint for opening saved database names, Circle IDs, or `oct://` URIs. |
| `ClientOptions` | Explicit target, wallet, RPC, network, and tracing options. |
| `Database` | Open database handle with `query`, `execute`, `execute_no_wait`, `wait`, `auth_info`, and `program_info`. |
| `QueryResult` | Typed read result: columns, rows, row count, and raw JSON. |
| `ExecuteResult` | Confirmed write result with submitted transaction metadata and receipt. |
| `SubmittedTransaction` | Submitted write returned by no-wait paths; may be pending, confirmed later, or stopped by RPC/chain behavior. |
| `AuthInfo` | Owner-write authorization metadata for the target database. |
| `ProgramInfo` | Deployed Circle program metadata. |
| `ReadMode` | `Sealed` or `Public` read routing. |
| `Error`, `ErrorKind`, `Result` | Error and result types for client operations. |
| `Value` | Re-exported `serde_json::Value` for raw fields and lower-level adapters. |

Lower-level transport/session helpers live under `octra_sqlite::client` and
`octra_sqlite::client::raw`.

## Writes And Read Modes

Databases are `sealed` by default. Sealed databases use signed Octra view auth
for reads and owner-signed OSW1 calls for writes.

Public-read databases are explicit:

```sh
octra-sqlite new public_art --read-mode public --schema examples/artists.sql
```

Public-read SQL queries use unsigned `octra_circleView`; anyone can query the
public data. Writes remain owner-signed OSW1 calls in both read modes. When
using a raw URI instead of a saved database name, mark public reads explicitly
with `?read_mode=public`.

## CLI Commands

In commands below, `DATABASE` can be a saved database name or a raw `oct://`
URI.

| Command | Purpose |
| --- | --- |
| `octra-sqlite setup` | Interactive wallet and network setup. |
| `octra-sqlite new [DATABASE] [SQL]` | Create a Circle-backed SQLite database. |
| `octra-sqlite new DATABASE --sample NAME` | Create a database from an explicit built-in sample. |
| `octra-sqlite new DATABASE --read-mode public` | Create a public-read database; writes remain owner-signed. |
| `octra-sqlite DATABASE "SQL"` | Run one SQL statement or script against a database. |
| `octra-sqlite DATABASE --read-only "SQL"` | Run SQL while refusing state-changing statements. |
| `octra-sqlite DATABASE --sql-file FILE` | Run SQL from a file. |
| `octra-sqlite open DATABASE` | Open the interactive `sqlite>` shell. |
| `octra-sqlite restore DATABASE --file dump.sql` | Restore large SQL text with chunked execution. |
| `octra-sqlite check DATABASE --sql-file dump.sql` | Check script size and batching without writing. |
| `octra-sqlite limits [DATABASE]` | Show SQL, restore, transaction, auth, and trace limits. |
| `octra-sqlite commands` | Show supported CLI commands and JSON envelopes. |
| `octra-sqlite status [DATABASE]` | Check config, wallet, WASM, Circle, auth, storage, and SQLite health. |
| `octra-sqlite status [DATABASE] --ready` | Exit nonzero unless live database readiness checks pass. |
| `octra-sqlite verify [DATABASE]` | Verify live Circle SQLite status and optional integrity/write checks. |
| `octra-sqlite config` | Show local config, networks, RPC, explorer, and saved databases. |
| `octra-sqlite database list` | List saved database names. |
| `octra-sqlite database info [DATABASE]` | Show database URI, Circle ID, network, and RPC. |
| `octra-sqlite database set NAME URI` | Save an `oct://` database URI locally. |
| `octra-sqlite database default NAME` | Set the default local database. |
| `octra-sqlite wallet status [DATABASE]` | Show wallet path, permissions, caller, and target read/write status. |
| `octra-sqlite wallet attach PATH` | Make an existing plaintext wallet JSON the active wallet. |
| `octra-sqlite wallet import PATH\|--stdin` | Normalize a plaintext wallet JSON or stdin private key into a local wallet JSON. |
| `octra-sqlite deploy [OPTIONS]` | Update an existing Circle with Circle WASM. |

The same public command surface is documented in
[docs/public-surface.md](./docs/public-surface.md) and emitted in machine form
by `octra-sqlite commands --json`.

## `sqlite>` Shell

Run `octra-sqlite DATABASE` or `octra-sqlite open DATABASE` to enter the shell.

```sql
sqlite> select id, name
   ...> from artist
   ...> order by name;
sqlite> .tables
sqlite> .quit
```

`sqlite>` is ready for a new SQL statement or dot command. `...>` is waiting
for the rest of a multiline SQL statement. SQL runs when it ends with `;`.
Dot commands run immediately.

| Dot command | Origin | Purpose |
| --- | --- | --- |
| `.help` | SQLite | Show shell commands. |
| `.tables` | SQLite | List tables. |
| `.schema [TABLE]` | SQLite | Show schema. |
| `.indexes [TABLE]` | SQLite | List indexes. |
| `.mode MODE` | SQLite | Set output mode: `box`, `table`, `list`, `json`, `line`, or `csv`. |
| `.headers on\|off` | SQLite | Show or hide column headers. |
| `.backup main FILE` | SQLite | Save a local `.sqlite` backup. |
| `.save FILE` | SQLite | Save a local `.sqlite` backup. |
| `.dump [TABLE]` | SQLite | Print SQL text for restore or inspection. |
| `.read FILE` | SQLite | Execute SQL from a file. |
| `.import --csv FILE TABLE` | SQLite | Import CSV rows. |
| `.output FILE` | SQLite | Redirect output. |
| `.once FILE` | SQLite | Redirect one command. |
| `.fullschema` | SQLite | Show schema plus SQLite metadata. |
| `.databases` | SQLite | Show the current database URI. |
| `.open DATABASE` | SQLite | Switch database. |
| `.timer on\|off` | SQLite | Show query timing. |
| `.show` | SQLite | Show shell settings. |
| `.quit` / `.exit` | SQLite | Exit the shell. |
| `.circle` | Octra | Show Circle metadata. |
| `.wallet` | Octra | Show active wallet. |
| `.storage` | Octra | Show SQLite page storage info. |
| `.verify` | Octra | Verify live Circle SQLite status. |

## Backup And Restore

```sh
octra-sqlite art ".backup main art.sqlite"
sqlite3 art.sqlite "pragma integrity_check;"

octra-sqlite art ".dump" > art.sql
octra-sqlite new art_copy
octra-sqlite restore art_copy --file art.sql
```

Local `sqlite3` is optional. It is used only for exported-file integrity checks
and local snapshot rendering commands such as `.dump` and `.fullschema`. The
`octra-sqlite` commands talk to the Octra Circle. See
[docs/operations.md](./docs/operations.md) for restore and backfill guidance.

## Verifiability

The crate ships `circle/wasm/octra_sqlite_circle.wasm` so users do not need a
local WASM toolchain. `scripts/audit-wasm.sh` checks the Circle import/export
surface, [docs/toolchain.md](./docs/toolchain.md) records the rebuild inputs,
and release manifests publish the bundled WASM hash plus live devnet proof
metadata.

The `0.5.1` crate uses the same bundled Circle WASM as `0.5.0`. The current
release manifest is
[release/octra-sqlite-0.5.1.json](./release/octra-sqlite-0.5.1.json).

```text
Rust CLI/client -> Octra RPC -> Circle wasm_v1
                                  |
                                  v
                   SQLite C engine -> VFS -> Octra page storage
```

The consensus surface is intentionally small: SQLite runs SQL, the VFS stores
SQLite pages in Octra storage, and the Rust client handles signing, rendering,
backup, restore, and local developer experience.

## Stability

MSRV is Rust 1.87. While the crate is `0.x`, the Rust API may change in minor
versions. CLI JSON envelopes, `commands --json`, release manifests, and the
OSR1/OSW1 wire formats are treated as stable automation surfaces and changed
carefully.

`octra-sqlite` is still alpha software for Octra testing. Do not store secrets,
production records, financial records, or irreplaceable data in alpha
databases.

## Cargo Features

| Feature | Purpose |
| --- | --- |
| `default` | Enables `cli` and `http`. |
| `cli` | Builds the `octra-sqlite` command line interface. |
| `http` | Enables the default blocking HTTP RPC transport. |
| `wasm-behavior` | Enables host-harness tests for the bundled Circle WASM. |

`cargo build --no-default-features --lib` builds the protocol/client core
without the CLI or HTTP transport. docs.rs builds with `http` and without the
CLI so library documentation stays focused.

## Reference

- [API docs](https://docs.rs/octra-sqlite)
- [Examples](./examples/)
- [Release manifests](./release/)
- [Public surface](./docs/public-surface.md)
- [Headless setup](./docs/headless.md)
- [JSON output](./docs/json-output.md)
- [Operations](./docs/operations.md)
- [Storage model](./docs/storage-model.md)
- [Toolchain and builds](./docs/toolchain.md)
- [OSR1 typed results](./docs/spec/osr1.md)
- [OSW1 owner write intent](./docs/spec/osw1.md)

## License, Contributing, Security

`octra-sqlite` is licensed under the [MIT license](./LICENSE). See
[CONTRIBUTING.md](./CONTRIBUTING.md) for contribution guidance and
[SECURITY.md](./SECURITY.md) for the current security policy.
