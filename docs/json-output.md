# JSON Output

Use `--json` for stable machine-readable output. Every envelope has:

```json
{
  "ok": true,
  "type": "query",
  "schema": "octra-sqlite.cli.v1"
}
```

Errors use the same schema on stderr:

```json
{
  "ok": false,
  "type": "error",
  "schema": "octra-sqlite.cli.v1",
  "error": {
    "code": "database_error",
    "message": "database error (sqlite_prepare_failed): no such table: demo"
  }
}
```

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

## RPC Trace

For read proof/debugging, write exact JSON-RPC request/response envelopes to a
JSONL file:

```sh
octra-sqlite DATABASE --trace-rpc-json trace.jsonl "select * from artist;"
```

Each line is:

```json
{
  "schema": "octra-sqlite.rpc-trace.v1",
  "sequence": 1,
  "timestamp_ms": 1780000000000,
  "rpc": "https://devnet.octrascan.io/rpc",
  "method": "octra_circleViewAuth",
  "http_status": 200,
  "request": {},
  "response": {},
  "error": null
}
```

Trace files are opt-in. They may contain SQL text, Circle IDs, caller wallet,
public keys, read signatures, and response data. They never contain private
keys.
