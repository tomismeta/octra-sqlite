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
database URI and recovery commands so the Circle can still be opened, inspected,
and retried.
