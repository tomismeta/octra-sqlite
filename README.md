# octra-sqlite

**Real SQLite inside an Octra Circle.**

[![crates.io](https://img.shields.io/crates/v/octra-sqlite.svg)](https://crates.io/crates/octra-sqlite)
[![docs.rs](https://docs.rs/octra-sqlite/badge.svg)](https://docs.rs/octra-sqlite)
[![ci](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-6f42c1)](./LICENSE)
[![sqlite](https://img.shields.io/badge/sqlite-3.53.3-0f766e)](https://sqlite.org/)

`octra-sqlite` runs the SQLite C engine inside an Octra `wasm_v1` Circle.
It gives you a SQLite-shaped CLI and a small Rust client for live Circle state:
query public databases without a wallet, create sealed or public-read databases
with an Octra wallet, and verify what was deployed.

If you're new to Octra, start with the official
[Octra docs](https://docs.octra.org/) and
[Circles guide](https://docs.octra.org/user-docs/circles). In this project, a
Circle is the Octra-hosted WASM environment that owns the SQLite pages and
executes the SQLite engine.

- Real SQLite: SQL is executed by the bundled SQLite C amalgamation.
- Octra-native writes: state-changing SQL is owner-signed through OSW1 owner
  write intent.
- Public when you choose: public-read databases can be queried without a
  wallet, while sealed databases remain the default.
- Verifiable deployment: the bundled WASM, rebuild inputs, audit script, hashes,
  and devnet proof metadata are published with each release.

## CLI Quick Start

You need Rust/Cargo 1.96+ from a current Rust toolchain. If `cargo` is missing,
install Rust with [rustup](https://rustup.rs/); distro packages such as
`apt install cargo` may be too old. The Circle WASM is bundled; no local WASM
toolchain is required.

```sh
cargo +stable install octra-sqlite --locked
```

Read a public database immediately, no wallet required:

```sh
octra-sqlite 'oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ' \
  "select id, name from artist order by id;"
```

Open the same database in the interactive `sqlite>` shell:

```sh
octra-sqlite open 'oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ'
```

Create a database when you have a funded Octra wallet:

```sh
octra-sqlite setup
octra-sqlite new art < examples/artists.sql
octra-sqlite status art --ready
octra-sqlite art "select * from artist order by name;"
```

`setup` walks you through wallet and network defaults; see [Wallets](#wallets)
for wallet options and [docs/headless.md](./docs/headless.md) for scripted
setup.

## Rust Client Quick Start

```toml
[dependencies]
octra-sqlite = "0.6"
```

```rust,no_run
use octra_sqlite::{Client, Result};

fn main() -> Result<()> {
    let client = Client::default();
    let db = client.database(
        "oct://devnet/octQfYK2fE9RvR9kfj8FJfMBQw1e4EzfHB8Q5Z9J2DCnRBQ",
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
`Client`, `ClientOptions`, `Database`, `QueryResult`, `ExecuteResult`,
`SubmittedTransaction`, `AuthInfo`, `ProgramInfo`, `ReadMode`, `Value`,
`Error`, `ErrorKind`, and `Result`.

Use `Client::default()` for config-free public reads,
`Client::from_default_config()` for local CLI config, and
`Client::with_options(...)` when code needs explicit target, wallet, RPC,
caller, or key-material overrides. Lower-level transport/session helpers live
under `octra_sqlite::client` and `octra_sqlite::client::raw`. See
[docs.rs](https://docs.rs/octra-sqlite) for the authoritative Rust API
reference.

## Writes And Read Modes

Databases are `sealed` by default. Sealed databases use signed Octra view auth
for reads and owner-signed OSW1 calls for writes. Sealed authenticates the
reader; it does not encrypt the database or make reads owner-only. SQL and
values submitted in writes are visible in Octra transaction history.

Public-read databases are explicit:

```sh
octra-sqlite new public_art --read-mode public --schema examples/artists.sql
```

Public-read SQL queries use unsigned `octra_circleView`; anyone can query the
public data. Writes remain owner-signed OSW1 calls in both read modes. Raw
Circle targets detect the Octra read surface automatically; use
`?read_mode=sealed` or `?read_mode=public` only when you need an explicit
override.

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

Supported wallet inputs are an official Octra wallet-generator `wallet.json`
from [wallet.octra.org](https://wallet.octra.org/), an existing plaintext
wallet JSON, or a private key pasted/imported through the CLI. WebCLI `.oct`
files are encrypted/PIN-protected and are not imported directly; use the
official `wallet.json`, attach plaintext wallet JSON, or import the private key.

## Verifiability

The crate ships `circle/wasm/octra_sqlite_circle.wasm` so users do not need a
local WASM toolchain. Released binaries embed the bundled WASM and release
manifest; source and operator overrides are explicit. `scripts/audit-wasm.sh`
checks the Circle import/export surface,
[docs/toolchain.md](./docs/toolchain.md) records the rebuild inputs, and
release manifests publish the bundled WASM hash and record live devnet proofs
when completed.

The `0.6.2` crate keeps SQLite 3.53.3 and the audited `0.6.1` Circle WASM while
converging the routine CLI and Rust client paths. The current release manifest
is [release/octra-sqlite-0.6.2.json](./release/octra-sqlite-0.6.2.json).
Existing Circles keep the engine they were deployed with until their owner runs
`upgrade`; historical release manifests retain the prior deployment proofs.

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

MSRV is Rust 1.96. While the crate is `0.x`, the Rust API may change in minor
versions. CLI JSON envelopes, `commands --json`, release manifests, and the
OSR1/OSW1 wire formats are treated as stable automation surfaces and changed
carefully.

`octra-sqlite` is still alpha software for Octra testing. Do not store secrets,
production records, financial records, or irreplaceable data in alpha
databases.

## Upgrades

Use `upgrade` when a new octra-sqlite release ships a new bundled SQLite
engine:

```sh
octra-sqlite upgrade
octra-sqlite upgrade art --dry-run
octra-sqlite upgrade art
octra-sqlite upgrade rollback ~/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.2-20260707
```

An upgrade preserves the Circle ID, SQLite pages, read mode, owner-write
identity, and local database name. Before updating the Circle program, the CLI
checks Circle ownership, verifies the OSW1 owner wallet, writes a local SQLite
backup by default, and stores a private upgrade bundle with rollback WASM and
an `upgrade.json` manifest.

Default bundles are named with the network, Circle ID, previous SQLite version,
and date, for example `devnet-oct...-sqlite-3.53.2-20260707`.

For known historical octra-sqlite release engines, the release manifest JSON is
the catalog source of truth: base WASM SHA-256, byte length, and GitHub source
URL. Rollback still needs actual old bytes from chain history, local artifacts,
or `--previous-wasm`; the CLI accepts them only after they reproduce the live
program hash exactly.
`--unsafe-no-rollback` is an emergency escape hatch only; without rollback
bytes, the upgrade bundle cannot restore the previous Circle program.

```sh
octra-sqlite upgrade art --dry-run --previous-wasm ./old-octra_sqlite_circle.wasm
```

Rollback is clean only if no database writes happened after the upgrade and the
live counters needed to prove that are available. The
optional `--write-smoke` check performs a create/insert/drop write cycle
against the new engine. It leaves no smoke table behind, but it still dirties
the database and makes rollback require `--force-after-writes`.

Rollback availability matters only when `from.code_hash` differs from
`to.code_hash`. If `upgrade --dry-run` reports `status: "already_current"`,
there is no upgrade to apply and rollback is not relevant.

For `upgrade`, `rollback` is reserved for `upgrade rollback BUNDLE`. If a saved
database is literally named `rollback`, pass its raw `oct://` URI instead.

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
| `octra-sqlite status [DATABASE] --ready` | Exit nonzero unless live read/query readiness checks pass. |
| `octra-sqlite verify [DATABASE]` | Verify live Circle SQLite status and optional integrity/write checks. |
| `octra-sqlite upgrade` | Guided backup, upgrade, and verify workflow. |
| `octra-sqlite upgrade DATABASE` | Backup, upgrade, and verify a database Circle against the bundled SQLite engine. |
| `octra-sqlite upgrade DATABASE --dry-run` | Run upgrade preflight without writing. |
| `octra-sqlite upgrade rollback BUNDLE` | Restore the previous verified Circle program from an upgrade bundle. |
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

## Reference

- [API docs](https://docs.rs/octra-sqlite)
- [Examples](./examples/)
- [Release manifests](./release/)
- [Public surface](./docs/public-surface.md)
- [Headless setup](./docs/headless.md)
- [JSON output](./docs/json-output.md)
- [Operations](./docs/operations.md)
- [Roadmap](./docs/roadmap.md)
- [Storage model](./docs/storage-model.md)
- [Toolchain and builds](./docs/toolchain.md)
- [OSR1 typed results](./docs/spec/osr1.md)
- [OSW1 owner write intent](./docs/spec/osw1.md)

## License, Contributing, Security

`octra-sqlite` is licensed under the [MIT license](./LICENSE). See
[CONTRIBUTING.md](./CONTRIBUTING.md) for contribution guidance and
[SECURITY.md](./SECURITY.md) for the current security policy.
