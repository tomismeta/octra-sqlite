# JSON Output

Use `--json` for stable machine-readable output. Every envelope has:

```json
{
  "ok": true,
  "type": "query",
  "schema": "octra-sqlite.cli.v1"
}
```

The 0.3.x contract is additive: consumers should require `ok`, `type`, and
`schema`, then read command-specific fields. New fields may be added, but the
documented meanings below should not change inside the 0.3 line.

Errors use the same schema on stderr:

```json
{
  "ok": false,
  "type": "error",
  "schema": "octra-sqlite.cli.v1",
  "exit_code": 1,
  "error": {
    "code": "sql_rejected",
    "message": "database error (sqlite_prepare_failed): no such table: demo"
  }
}
```

Process exit codes are intentionally small for now:

| Exit | Meaning |
| --- | --- |
| `0` | Command succeeded. |
| `1` | Command failed; use `error.code` and `error.message` for detail. |

Stable error classifications:

| Code | Meaning |
| --- | --- |
| `sql_too_large` | SQL exceeded the Circle statement/payload byte limit. |
| `transactions_not_supported` | Restore saw unsupported transaction control SQL. |
| `read_only` | `--read-only` refused a write. |
| `result_limit_exceeded` | Query exceeded the Circle row limit. |
| `result_too_large` | Query response exceeded the Circle response buffer. |
| `sql_rejected` | SQLite rejected the SQL, such as syntax or missing table. |
| `auth_failed` | Wallet/signature/owner authorization failed. |
| `circle_write_failed` | A submitted Circle write was rejected or failed. |
| `bootstrap_unverified` | A bootstrap first write was submitted, but post-write `auth_info` still failed. |
| `wallet_error` | Wallet config or key loading failed. |
| `target_error` | Database name, URI, network, or Circle target failed. |
| `timeout` | Receipt or transaction wait timed out. |
| `decode_error` | RPC or contract response could not be decoded. |
| `rpc_unavailable` | HTTP transport failed. |
| `rpc_error` | Octra RPC returned an error envelope. |
| `config_error` | Local config could not be loaded or resolved. |
| `command_failed` | Fallback classification for other command failures. |

## Envelopes

### `query`

Produced by read SQL with `--json`.

```json
{
  "ok": true,
  "type": "query",
  "schema": "octra-sqlite.cli.v1",
  "database": {
    "uri": "oct://devnet/oct...",
    "network": "devnet",
    "circle": "oct...",
    "rpc": "https://devnet.octrascan.io/rpc",
    "wallet": "oct..."
  },
  "columns": ["id", "name"],
  "rows": [[1, "Monet"]],
  "row_count": 1,
  "result": {}
}
```

Queries include `columns` and `rows`.

### `write`

Produced by single-statement writes with `--json`.

```json
{
  "ok": true,
  "type": "write",
  "schema": "octra-sqlite.cli.v1",
  "status": "confirmed",
  "tx_hash": "abc...",
  "statements": null,
  "cost": {},
  "receipt": {},
  "result": {}
}
```

Writes do not include `columns` or `rows`.

### `write_script`

Produced by multi-statement SQL scripts with `--json`.

```json
{
  "ok": true,
  "type": "write_script",
  "schema": "octra-sqlite.cli.v1",
  "database": {},
  "plan": {},
  "statements": 3,
  "batches": 1,
  "progress": [],
  "writes": []
}
```

Script writes do not include SQL `columns` or `rows`.

### `restore`

Produced by `restore DATABASE --file dump.sql --json`.

```json
{
  "ok": true,
  "type": "restore",
  "schema": "octra-sqlite.cli.v1",
  "plan": {},
  "statements": 3279,
  "batches": 200,
  "progress": [],
  "writes": []
}
```

Full restore output includes per-batch progress and write summaries. It does
not include SQL result rows.

When `restore --bootstrap-owner` is used for an empty-storage recovery, the
envelope also includes:

```json
{
  "bootstrap_owner": true,
  "bootstrap": {
    "mode": "owner_first_write",
    "reason": "empty_storage_cache",
    "uri": "oct://mainnet/oct...",
    "owner": "oct...",
    "owner_pubkey": "hex...",
    "db_id": "hex...",
    "code_hash": "hex..."
  }
}
```

Use `--json-summary` for compact restore output:

```json
{
  "ok": true,
  "type": "restore",
  "schema": "octra-sqlite.cli.v1",
  "summary": true,
  "plan": {},
  "statements": 3279,
  "batches": 200,
  "writes": {
    "total": 200,
    "confirmed": 200,
    "submitted": 0,
    "rejected": 0,
    "first_tx_hash": "abc...",
    "last_tx_hash": "def...",
    "failed": []
  }
}
```

### `check`

Produced by `check DATABASE --sql-file dump.sql --json`.

```json
{
  "ok": true,
  "type": "check",
  "schema": "octra-sqlite.cli.v1",
  "syntax_checked": false,
  "target": {},
  "plan": {},
  "warnings": []
}
```

`check` plans and validates Octra SQLite script limits. SQLite syntax and
semantics are enforced by SQLite inside the Circle when executed.

### `status`, `verify`, `database_list`, `database_info`, `limits`

Inspection commands return `ok`, `type`, `schema`, and command-specific fields.
They do not include SQL `columns` or `rows` unless they are returning an
embedded typed SQLite query result.

`limits --json` is the compact capability surface for automation. It includes
CLI/SQLite/schema versions, SQL byte limits, result row/response limits, restore
behavior, read/write auth facts, and available trace modes.

## RPC Trace

For read proof/debugging, write JSON-RPC trace envelopes to a JSONL file:

```sh
octra-sqlite DATABASE --trace-rpc-json trace.jsonl "select * from artist;"
octra-sqlite DATABASE --trace-rpc-json trace.jsonl --trace-rpc-json-mode summary "select * from artist;"
```

Trace mode defaults to `full`. Available modes:

| Mode | Contents |
| --- | --- |
| `full` | Exact JSON-RPC request and response bodies plus metadata. |
| `summary` | Method, status, hashes, byte counts, and error only. |
| `request_only` | Exact request body plus response metadata. |
| `response_meta` | Request and response hashes/byte counts only. |

Each full-trace line is:

```json
{
  "schema": "octra-sqlite.rpc-trace.v1",
  "mode": "full",
  "sequence": 1,
  "timestamp_ms": 1780000000000,
  "rpc": "https://devnet.octrascan.io/rpc",
  "method": "octra_circleViewAuth",
  "http_status": 200,
  "ok": true,
  "request": {},
  "response": {},
  "request_meta": {},
  "response_meta": {},
  "error": null
}
```

Trace files are opt-in. They may contain SQL text, Circle IDs, caller wallet,
public keys, read signatures, and response data. They never contain private
keys, but treat them as sensitive operational logs: keep them out of git and
use restrictive file permissions when storing them on shared systems.
