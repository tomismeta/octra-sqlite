# octra-sqlite

**Real SQLite inside an Octra Circle.**

[![crates.io](https://img.shields.io/crates/v/octra-sqlite.svg)](https://crates.io/crates/octra-sqlite)
[![docs.rs](https://docs.rs/octra-sqlite/badge.svg)](https://docs.rs/octra-sqlite)
[![ci](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-6f42c1)](./LICENSE)
[![sqlite](https://img.shields.io/badge/sqlite-3.53.2-0f766e)](https://sqlite.org/)

`octra-sqlite` runs the SQLite C engine inside an Octra `wasm_v1` Circle.
It is not an ORM, a SQL dialect, or a SQLite reimplementation. It is SQLite
running in an Octra execution environment, with a Rust CLI and client library
around deployment, reads, writes, proofs, and developer ergonomics.

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

## Core Commands

In commands below, `DATABASE` can be a saved database name or a raw `oct://`
URI.

| Command | Purpose |
| --- | --- |
| `octra-sqlite setup` | Configure wallet and network defaults. |
| `octra-sqlite new DATABASE [SQL]` | Create a Circle-backed SQLite database. |
| `octra-sqlite DATABASE "SQL"` | Run SQL against a database. |
| `octra-sqlite open DATABASE` | Open the interactive `sqlite>` shell. |
| `octra-sqlite status DATABASE --ready` | Exit nonzero unless the database is operational. |
| `octra-sqlite restore DATABASE --file dump.sql` | Restore large SQL text with chunked execution. |
| `octra-sqlite limits DATABASE --json` | Show SQL, restore, transaction, auth, and trace limits. |
| `octra-sqlite commands --json` | Show the supported CLI and JSON envelopes. |

The full public command surface is documented in
[docs/public-surface.md](./docs/public-surface.md) and discoverable through
`octra-sqlite commands --json`.

## `sqlite>` Shell

Run `octra-sqlite DATABASE` or `octra-sqlite open DATABASE` to enter a
SQLite-shaped shell. SQL runs when it ends with `;`; dot commands run
immediately.

Common dot commands include `.tables`, `.schema`, `.mode`, `.backup`, `.dump`,
`.read`, `.import`, and `.open`. Octra-aware inspection commands include
`.circle`, `.storage`, `.wallet`, and `.verify`.

```sh
octra-sqlite art ".backup main art.sqlite"
sqlite3 art.sqlite "pragma integrity_check;"
```

Local `sqlite3` is optional. It is used only for exported-file integrity checks
and local snapshot rendering commands such as `.dump` and `.fullschema`. See
[docs/operations.md](./docs/operations.md) for restore and backfill guidance.

## Verifiability

The crate ships `circle/wasm/octra_sqlite_circle.wasm` so users do not need a
local WASM toolchain. `scripts/audit-wasm.sh` checks the Circle import/export
surface, [docs/toolchain.md](./docs/toolchain.md) records the rebuild inputs,
and release manifests publish the bundled WASM hash plus live devnet proof
metadata.

The `0.5.1` crate uses the same bundled Circle WASM as `0.5.0`; it is a
README/package polish release for the crates.io debut. The current release
manifest is [release/octra-sqlite-0.5.1.json](./release/octra-sqlite-0.5.1.json).

```text
Rust CLI/client -> Octra RPC -> Circle wasm_v1
                                  |
                                  v
                   SQLite C engine -> VFS -> Octra page storage
```

The consensus surface is intentionally small: SQLite runs SQL, the VFS stores
SQLite pages in Octra storage, and the Rust client handles signing, rendering,
backup, restore, and local developer experience.

## Feature Flags

| Feature | Purpose |
| --- | --- |
| `default` | Enables `cli` and `http`. |
| `cli` | Builds the `octra-sqlite` command line interface. |
| `http` | Enables the default blocking HTTP RPC transport. |
| `wasm-behavior` | Enables host-harness tests for the bundled Circle WASM. |

`cargo build --no-default-features --lib` builds the protocol/client core
without the CLI or HTTP transport. docs.rs builds with `http` and without the
CLI so library documentation stays focused.

## Stability

MSRV is Rust 1.87. While the crate is `0.x`, the Rust API may change in minor
versions. CLI JSON envelopes, `commands --json`, release manifests, and the
OSR1/OSW1 wire formats are treated as stable automation surfaces and changed
carefully.

`octra-sqlite` is still alpha software for Octra testing. Do not store secrets,
production records, financial records, or irreplaceable data in alpha
databases.

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
