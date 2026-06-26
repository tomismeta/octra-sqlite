# octra-sqlite

**Real SQLite inside an Octra Circle.**

[![license](https://img.shields.io/badge/license-MIT-6f42c1)](./LICENSE)
[![ci](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/tomismeta/octra-sqlite/actions/workflows/ci.yml)
[![network](https://img.shields.io/badge/network-configurable-2563eb)](https://docs.octra.org/)
[![sqlite](https://img.shields.io/badge/sqlite-3.53.2-0f766e)](https://sqlite.org/)

`octra-sqlite` runs the SQLite C engine inside an Octra `wasm_v1` Circle. The
Rust CLI deploys the bundled Circle WASM, signs Octra RPC calls with your
wallet, and gives you SQLite-shaped commands over live Circle state.

The CLI ships with a small default config at
[`config/defaults.json`](./config/defaults.json): devnet RPC plus the public
Remilia example database. Your local `~/.octra/sqlite.json` overlays it.

## Quickstart

You need Rust/Cargo and a funded Octra wallet for deploys and writes.
Everything else needed for the default path is bundled in this repo, including
the precompiled Circle WASM.

```sh
git clone https://github.com/tomismeta/octra-sqlite.git
cd octra-sqlite
cargo install --path . --locked

octra-sqlite setup
octra-sqlite status

octra-sqlite new remilia < examples/remilia-collections.sql
octra-sqlite remilia "select name, launched_month, relationship from collection order by launched_month;"
octra-sqlite verify remilia
```

For a one-command sample database:

```sh
octra-sqlite quickstart remilia
octra-sqlite remilia ".tables"
octra-sqlite remilia "select name, launched_month from collection order by launched_month;"
```

For non-interactive setup, use `init` instead of the wizard:

```sh
octra-sqlite init --wallet ./wallet.json
```

Advanced users can override the preloaded connection settings:

```sh
octra-sqlite init --wallet ./wallet.json --rpc http://YOUR_RPC/rpc --network devnet
```

More CRUD examples live in [`examples/`](./examples/).

## Ontology

- **Database**: the SQLite database you open and query.
- **Database name**: a local name saved in `~/.octra/sqlite.json`, like
  `remilia`.
- **Database URI**: an advanced `oct://NETWORK/CIRCLE_ID` pointer to a database.
- **Circle**: the Octra program and storage identity underneath a database.
- **Wallet, RPC, network**: Octra connection and signing configuration.

## Commands

Commands manage databases, wallets, verification, and release artifacts:

| Command | Purpose |
| --- | --- |
| `octra-sqlite setup` | Configure wallet, RPC, network, and default database. |
| `octra-sqlite init ...` | Non-interactive config for scripts and advanced users. |
| `octra-sqlite config` | Show wallet, RPC, network, and default database. |
| `octra-sqlite status [DB_NAME]` | Verify config, wallet, bundled WASM, manifest, and live database health. |
| `octra-sqlite quickstart DB_NAME` | Create a new SQLite database with the built-in sample. |
| `octra-sqlite new DB_NAME` | Create a fresh SQLite database and save `DB_NAME` locally. |
| `octra-sqlite database list` | List saved database names. |
| `octra-sqlite database info [DB_NAME]` | Show database URI, network, Circle id, and RPC. |
| `octra-sqlite open DB_NAME` | Open the SQLite shell explicitly. |
| `octra-sqlite deploy ...` | Update an existing Circle program with the bundled or rebuilt WASM. |
| `octra-sqlite verify [DB_NAME]` | Print live program, storage, schema, and typed-query proof. |
| `octra-sqlite install` | Print local install commands. |
| `octra-sqlite help` | Show CLI help. |

SQLite-shaped commands run against a database name or advanced `oct://` URI:

```sh
octra-sqlite DB_NAME
octra-sqlite open DB_NAME
octra-sqlite DB_NAME "SQL"
octra-sqlite DB_NAME ".tables"
octra-sqlite DB_NAME ".schema"
```

## SQLite Shell

Open a database without a SQL argument to enter the interactive shell:

```sh
octra-sqlite remilia
```

The prompt is intentionally familiar:

```sql
sqlite> select name, launched_month from collection limit 3;
sqlite> insert into collection(name,opensea_slug,chain,relationship,launched_month,date_precision)
   ...> values ('Example','example','Ethereum','Remilia adjacent','2026-06-01','month');
sqlite> .tables
sqlite> .quit
```

`sqlite>` means the shell is ready for a new SQL statement or dot command.
`...>` means the shell is waiting for the rest of a multiline SQL statement.
SQL runs when the statement ends with `;`. Dot commands run immediately and must
start at a fresh `sqlite>` prompt.

Inside the shell, SQL statements are SQLite. Dot commands are client commands:

| Dot command | Origin | Purpose |
| --- | --- | --- |
| `.help` | SQLite CLI | Show shell commands. |
| `.tables` | SQLite CLI | List tables. |
| `.schema` | SQLite CLI | Show schema. |
| `.mode` | SQLite CLI | Set output mode: `box`, `table`, `list`, `json`, `line`, or `csv`. |
| `.headers` | SQLite CLI | Show or hide column headers. |
| `.timer` | SQLite CLI | Show query timing. |
| `.output` | SQLite CLI | Redirect command output. |
| `.read` | SQLite CLI | Execute SQL from a file. |
| `.show` | SQLite CLI | Show shell settings. |
| `.databases` | SQLite CLI | Show the current `main` database URI. |
| `.open` | SQLite CLI | Switch to another database name, Circle id, or `oct://` URI. |
| `.quit` / `.exit` | SQLite CLI | Exit the shell. |
| `.storage` | Octra | Show SQLite page storage info. |
| `.circle` | Octra | Show Circle program metadata. |
| `.wallet` | Octra | Show the active caller wallet. |
| `.proof` / `.verify` | Octra | Prove live Circle SQLite status. |

Low-level compatibility commands also exist (`query`, `exec`, `tables`,
`schema`, `storage`, `circle`, `proof`), but the reference path is the
SQLite-shaped `octra-sqlite DB_NAME "SQL"` form above.

## Architecture

```text
Rust CLI
  -> signed Octra RPC
    -> wasm_v1 Circle
      -> SQLite C engine
        -> SQLite VFS
          -> SQLite database pages stored in Octra key-value storage
```

The consensus-critical surface is intentionally small:

- SQLite is the real SQLite C engine.
- The storage adapter is SQLite's VFS hook: it is how SQLite reads and writes
  database pages.
- Octra key-value storage is the durable storage for those SQLite
  database pages.
- The generation manifest is the commit record that says which page versions
  make up the current database.
- Results use the compact OSR1 typed codec.
- Rendering happens in Rust, outside the Circle program.
- Vendored SQLite source lives at `vendor/sqlite/3.53.2/`.

## Limits And Policy

- Alpha is intended for Octra devnet testing. The CLI has configurable network
  and RPC settings, but do not use this for production data yet.
- Do not store secrets, production records, or financial records in alpha
  databases.
- The bundled SQLite Circle stores up to 8,192 SQLite pages: 32 MiB at the
  current 4 KiB page size.
- A single write can dirty up to 1,024 SQLite pages, about 4 MiB of database
  growth. Larger imports should be chunked.
- SQL frames are capped at 8 KiB. Practically, SQL text up to 8,191 bytes is
  accepted by the current frame parser.
- Query responses are capped at 512 rows and about 64 KiB. Use `limit` and
  pagination-style queries for larger tables.
- New databases are owner-bound by default. The CLI patches the bundled WASM
  with the creator wallet's public key, and writes must carry a signed owner
  write intent bound to the exact Circle method, database id, sequence, and SQL
  before SQLite runs.
- Other authenticated wallets can read through the signed view path, but cannot
  write unless the database owner key is used.
- Future multi-wallet role grants should still become Octra-native method
  access control when the runtime exposes that policy surface.
- `OCTRA_SQLITE_TRACE_SQL_EVENT=1` makes writes use `exec_trace`, emitting an
  opt-in `octra.sqlite.sql` event with SQL text. The default write event stores
  only a SQL hash.

## Requirements

- Rust stable with Cargo.
- A funded Octra wallet for deploys and writes on the configured network.

The audited Circle WASM is bundled at `circle/wasm/octra_sqlite_circle.wasm`.
The Circle program source lives at `circle/source/octra_sqlite_circle.c`.

## Alpha Feedback

Please use the alpha feedback issue template and include:

- OS and shell
- install path
- wallet setup path, without private keys
- exact command
- expected result
- actual result
- Circle id or transaction hash when relevant

## Build And Test

Builders who modify the Circle program can rebuild and audit it:

```sh
bash scripts/build-wasm.sh
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
```

## Source

```sh
cargo test --locked
OCTRA_SQLITE_WASM=circle/wasm/octra_sqlite_circle.wasm \
  cargo test --locked --features wasm-behavior --test wasm_host_harness
```

Architecture notes live in [`docs/`](./docs). The bundled public artifact is
recorded in
[`release/octra-sqlite-0.1.0.json`](./release/octra-sqlite-0.1.0.json).
