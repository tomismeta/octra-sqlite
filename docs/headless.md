# Headless Setup

Use the same CLI on a server or VPS. Keep secrets in files or environment
variables, not shell history.

## Wallet File

The preferred wallet file shape is:

```json
{
  "address": "oct...",
  "private_key_b64": "BASE64_PRIVATE_KEY",
  "public_key_b64": "BASE64_PUBLIC_KEY"
}
```

`public_key_b64` is optional, but when supplied the CLI verifies that it matches
the private key.

Lock down the file before use:

```sh
chmod 600 /secure/path/wallet.json
octra-sqlite init --wallet /secure/path/wallet.json --network devnet
```

## Config Path

By default, config lives at `~/.octra/sqlite.json`. Override it per process:

```sh
OCTRA_SQLITE_CONFIG=/secure/path/sqlite.json octra-sqlite status
```

## Server Checklist

Install with a current Rust stable toolchain:

```sh
rustup toolchain install stable --profile minimal
cargo install --path . --locked
```

If installing into a shared path, make the binary executable by the service
user or group:

```sh
sudo install -m 0755 ~/.cargo/bin/octra-sqlite /opt/octra-sqlite/bin/octra-sqlite
```

```sh
octra-sqlite config
octra-sqlite database list
octra-sqlite database info DATABASE
octra-sqlite verify DATABASE
```

For schema deploys:

```sh
octra-sqlite new DATABASE < schema.sql
```

If Circle creation succeeds but initializer SQL fails, the CLI prints the saved
database URI and recovery commands so the Circle can still be opened and
inspected. Initializer scripts can be partially applied, so inspect before
retrying.

For large imports and mirrors, avoid shell argument-sized SQL blobs:

```sh
octra-sqlite check DATABASE --sql-file dump.sql
octra-sqlite restore DATABASE --file dump.sql
```

Use `--json` or `--json-summary` for machine-readable output, and prefer full
`oct://NETWORK/<circle>` URIs in automation. For read proof/debugging, write
RPC traces to a file:

```sh
octra-sqlite DATABASE --trace-rpc-json trace.jsonl "select 1;"
```

Trace files can include SQL text, Circle IDs, wallet addresses, signatures, and
query responses. They do not include private keys, but store them like sensitive
logs: use restrictive permissions and do not commit them.
