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
[`config/defaults.json`](./config/defaults.json): devnet as the active network,
devnet and mainnet URL profiles, plus the public Remilia example database. Your
local `~/.octra/sqlite.json` overlays it.

## Quickstart

You need Rust/Cargo and an Octra wallet. Reads use signed RPC calls but do not
need wallet funds. Creating databases and running writes need a funded wallet.
The audited Circle WASM is bundled, and the default config points to the public
Remilia database.

```sh
git clone https://github.com/tomismeta/octra-sqlite.git
cd octra-sqlite
cargo install --path . --locked

octra-sqlite setup
octra-sqlite status

octra-sqlite remilia ".tables"
octra-sqlite remilia "select name, launched_month from collection order by launched_month;"
octra-sqlite verify remilia
```

Create your own sample database:

```sh
octra-sqlite quickstart my_collections
octra-sqlite my_collections ".tables"
octra-sqlite my_collections "select name, launched_month from collection order by launched_month;"
```

Create your own schema:

```sh
octra-sqlite new organization "
create table person(first_name text not null, last_name text not null);
insert into person values ('Ada','Lovelace'),('Grace','Hopper');
"
octra-sqlite organization "select * from person;"
```

For non-interactive setup, use `init` instead of the wizard:

```sh
octra-sqlite init --wallet ./wallet.json
```

Advanced users can override the preloaded connection settings:

```sh
octra-sqlite init --wallet ./wallet.json --rpc http://YOUR_RPC/rpc --network devnet
```

Switch to the bundled mainnet profile:

```sh
octra-sqlite init --wallet ./wallet.json --network mainnet
```

More CRUD examples live in [`examples/`](./examples/).
The tiny read-only Remilia API example lives at
[`examples/remilia-read-api/`](./examples/remilia-read-api/).

## Ontology

- **Database**: the SQLite database you open and query.
- **Database name**: a saved local name in `~/.octra/sqlite.json`, like
  `remilia`.
- **Database URI**: an explicit `oct://NETWORK/CIRCLE_ID` pointer to a database.
- **Circle**: the Octra program and storage identity underneath a database.
- **Wallet**: the Octra key used to sign reads and writes.
- **RPC and network**: the Octra endpoint and network used by the CLI.
- **Explorer**: the OctraScan base URL for the active network.
- **OSR1 and OSW1**: small `octra-sqlite` wire formats for typed results and
  owner write authorization. They are project protocols, not SQLite or Octra
  standards.

## Commands

Commands manage setup, databases, verification, and Octra deployment:

| Command | Purpose |
| --- | --- |
| `octra-sqlite setup` | Configure wallet, RPC, network, and default database. |
| `octra-sqlite init ...` | Non-interactive config for scripts and advanced users. |
| `octra-sqlite config` | Show wallet, network, RPC, explorer, and saved databases. |
| `octra-sqlite status [DATABASE]` | Verify config, wallet, bundled WASM, manifest, and live database health. |
| `octra-sqlite quickstart DATABASE` | Create a new SQLite database with the built-in Remilia sample. |
| `octra-sqlite new DATABASE` | Create a fresh SQLite database and save `DATABASE` locally. |
| `octra-sqlite database list` | List saved database names. |
| `octra-sqlite database info [DATABASE]` | Show database URI, network, Circle ID, and RPC. |
| `octra-sqlite open DATABASE` | Open the SQLite shell explicitly. |
| `octra-sqlite deploy ...` | Update an existing Circle program with the bundled or rebuilt WASM. |
| `octra-sqlite verify [DATABASE]` | Print live program, storage, schema, and typed-query checks. |
| `octra-sqlite install` | Print local install commands. |
| `octra-sqlite help` | Show CLI help. |

SQLite-shaped commands run against a database name or advanced `oct://` URI:

```sh
octra-sqlite DATABASE
octra-sqlite open DATABASE
octra-sqlite DATABASE "SQL"
octra-sqlite DATABASE ".tables"
octra-sqlite DATABASE ".schema"
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
| `.open` | SQLite CLI | Switch to another database name, Circle ID, or `oct://` URI. |
| `.quit` / `.exit` | SQLite CLI | Exit the shell. |
| `.storage` | Octra | Show SQLite page storage info. |
| `.circle` | Octra | Show Circle program metadata. |
| `.wallet` | Octra | Show the active caller wallet. |
| `.proof` / `.verify` | Octra | Verify live Circle SQLite status. |

Low-level compatibility commands also exist (`query`, `exec`, `tables`,
`schema`, `storage`, `circle`, `proof`), but the reference path is the
SQLite-shaped `octra-sqlite DATABASE "SQL"` form above.

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
- Results use OSR1, the compact typed-result codec.
- Writes use OSW1, the owner write authorization frame.
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
- Circle ID or transaction hash when relevant

## Build And Verify

Builders who modify the Circle program can rebuild and audit it:

```sh
bash scripts/build-wasm.sh
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
```

Run the test suite:

```sh
cargo test --locked
OCTRA_SQLITE_WASM=circle/wasm/octra_sqlite_circle.wasm \
  cargo test --locked --features wasm-behavior --test wasm_host_harness
```

Architecture notes live in [`docs/`](./docs). The bundled public artifact is
recorded in
[`release/octra-sqlite-0.1.0.json`](./release/octra-sqlite-0.1.0.json).
The current Rust CLI/library release manifest is
[`release/octra-sqlite-0.2.0.json`](./release/octra-sqlite-0.2.0.json).
