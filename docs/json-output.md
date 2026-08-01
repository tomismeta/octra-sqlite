# JSON Output

Use `--json` for stable machine-readable output. Every envelope has:

```json
{
  "ok": true,
  "type": "query",
  "schema": "octra-sqlite.cli.v1"
}
```

The CLI JSON contract is additive: consumers should require `ok`, `type`, and
`schema`, then read command-specific fields. New fields may be added, but the
documented meanings below should not change inside a stable release line.

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

Some recoverable errors add `error.details`. For `receipt_pending`, details can
include `tx_hash`, `nonce`, `ou`, `circle`, `database`, and `next_command`.
`nonce` and `ou` are present for writes submitted by the current command and may
be `null` when polling an already-submitted transaction.

Some errors also include `error.hint` for operator guidance. Budget errors keep
their stable code and may add a hint to reduce query work, lower result limits,
or use an index-backed access pattern.

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
| `query_budget_exceeded` | Query exceeded the deterministic SQLite work limit. |
| `exec_budget_exceeded` | Write execution exceeded the deterministic SQLite work limit. |
| `receipt_pending` | A write was submitted, but its Circle receipt was not available before the wait deadline. |
| `result_too_large` | Query response exceeded the Circle response buffer. |
| `sql_rejected` | SQLite rejected the SQL, such as syntax or missing table. |
| `auth_failed` | Wallet/signature/owner authorization failed. |
| `circle_write_failed` | A submitted Circle write was rejected or failed. |
| `bootstrap_unverified` | A bootstrap first write was submitted, but post-write `auth_info` still failed. |
| `bootstrap_already_done` | Bootstrap was requested after `auth_info` was already readable. |
| `auth_uninitialized` | Auth inspection is not yet readable. |
| `storage_uninitialized` | Circle storage is not yet readable or initialized. |
| `wallet_error` | Wallet config or key loading failed. |
| `target_error` | Database name, URI, network, or Circle target failed. |
| `timeout` | Receipt or transaction wait timed out. |
| `decode_error` | RPC or contract response could not be decoded. |
| `rpc_rate_limited` | Octra RPC returned or implied rate limiting. |
| `rpc_non_json` | Octra RPC returned a non-JSON response body. |
| `rpc_unavailable` | HTTP transport failed. |
| `rpc_error` | Octra RPC returned an error envelope. |
| `config_error` | Local config could not be loaded or resolved. |
| `command_failed` | Fallback classification for other command failures. |

## Envelopes

### `new`

Produced by `new DATABASE --json`.

```json
{
  "ok": true,
  "type": "new",
  "schema": "octra-sqlite.cli.v1",
  "manifest_version": "octra-sqlite.database.v1",
  "database": {
    "name": "art",
    "uri": "oct://devnet/oct...",
    "read_uri": "oct://devnet/oct...",
    "network": "devnet",
    "circle": "oct...",
    "rpc": "https://devnet.octrascan.io/rpc",
    "read": {
      "mode": "sealed",
      "privacy_class": "sealed",
      "browser_mode": "native_sealed",
      "resource_mode": "sealed_read"
    }
  },
  "confidentiality": {
    "encrypted": false,
    "read_access": "authenticated_wallet",
    "read_owner_only": false,
    "write_sql_visible_in_transaction_history": true
  },
  "program": {
    "runtime": "wasm_v1",
    "wasm_hash": "hex...",
    "wasm_bytes": 611677
  },
  "initializer": {
    "present": true,
    "sha256": "hex...",
    "statements": 2,
    "batches": 1,
    "writes": []
  },
  "readiness": {},
  "next": {}
}
```

If `--manifest FILE` is supplied, the same database manifest is written to
disk and the JSON envelope includes `manifest_path`.

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
    "wallet": "oct...",
    "read_mode": "sealed"
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
  "nonce": 42,
  "ou": "200000",
  "statements": null,
  "cost": {},
  "receipt": {},
  "result": {}
}
```

Writes do not include `columns` or `rows`.
`nonce` and `ou` report the submitted Octra account nonce and signed write
budget when known.

If submission succeeds but the receipt is still pending, the command exits with
`error.code: "receipt_pending"`. `error.message` identifies the submitted
transaction, and `error.details.next_command` gives the
`octra-sqlite receipt TX_HASH DATABASE --json` follow-up when the database is
known.

### `receipt`

Produced by `receipt TX_HASH [DATABASE] --json`.

```json
{
  "ok": true,
  "type": "receipt",
  "schema": "octra-sqlite.cli.v1",
  "database": {},
  "status": "confirmed",
  "tx_hash": "abc...",
  "tx_url": "https://...",
  "receipt": {},
  "result": {}
}
```

`receipt` waits for an already-submitted transaction. It does not resubmit SQL.

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

### `upgrade` and `upgrade_rollback`

Produced by `upgrade DATABASE --json`, `upgrade DATABASE --dry-run --json`,
and `upgrade rollback BUNDLE --json`.

```json
{
  "ok": true,
  "type": "upgrade",
  "schema": "octra-sqlite.cli.v1",
  "mode": "applied",
  "status": "applied",
  "upgrade_required": true,
  "dry_run": false,
  "database": {},
  "from": {
    "sqlite_version": "3.53.3",
    "code_hash": "hex..."
  },
  "to": {
    "sqlite_version": "3.53.4",
    "code_hash": "hex..."
  },
  "target": {
    "sqlite_version": "3.53.4",
    "code_hash": "hex...",
    "wasm": "embedded:circle/wasm/octra_sqlite_circle.wasm"
  },
  "backup": {
    "skipped": false,
    "path": "/home/user/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.3-20260801/devnet-oct...-sqlite-3.53.3-20260801.sqlite",
    "sha256": "hex..."
  },
  "rollback": {
    "available": true,
    "clean": true,
    "clean_reason": null,
    "wasm": "previous.wasm",
    "guard": {
      "storage_generation": 2,
      "owner_sequence": 41
    }
  },
  "transaction": {
    "tx_hash": "abc...",
    "tx_url": "https://..."
  },
  "verification": {
    "sqlite_version": "3.53.4",
    "storage_generation_unchanged": true,
    "owner_sequence_unchanged": true
  },
  "bundle": {
    "path": "/home/user/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.3-20260801",
    "manifest": "/home/user/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.3-20260801/upgrade.json"
  }
}
```

`upgrade` without a database argument opens the guided terminal workflow.
`upgrade DATABASE --yes --json` is the non-interactive automation path. `mode`
is `dry_run`, `planned`, `already_current`, or `applied`. `status` is
`already_current` when no program update is pending; in that case
`upgrade_required` is `false` and rollback is not relevant. The upgrade bundle
manifest uses schema `octra-sqlite.upgrade.bundle.v1` and records the engine
epoch boundary: previous code hash, new code hash, update transaction, backup
metadata, and rollback guard. The JSON output never includes private keys or
raw wallet JSON.

The on-disk bundle manifest is written before chain submission with
`status: "prepared"`, atomically replaced with `status: "applied"` after the
new program is verified, and finalized as `status: "complete"` after optional
smoke and local metadata work. A `prepared` manifest already contains the
target hash and rollback guard, so it remains usable if local finalization is
interrupted after the chain changes.

`verification.storage_generation_unchanged` and
`verification.owner_sequence_unchanged` are `true`, `false`, or `null`. `null`
means the live status surface did not return one side of the comparison, so the
CLI does not turn an unknown counter into a false claim.

Upgrade preflight reads `storage_info.owner_sequence` when the
storage-independent `auth_info` response omits it, so supported historical and
current engines can prove the comparison without rewriting the old Circle
first.

`rollback.clean` is also `true`, `false`, or `null`. `null` means rollback bytes
are available but clean rollback could not be proven from live counters;
rollback remains fail-closed unless the operator explicitly reviews and uses
`--force-after-writes`.

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

### `status`, `wallet_status`, `wallet_attach`, `wallet_import`, `verify`, `database_list`, `database_info`, `limits`, `commands`, `receipt`

Inspection commands return `ok`, `type`, `schema`, and command-specific fields.
They do not include SQL `columns` or `rows` unless they are returning an
embedded typed SQLite query result.

`status --json` includes top-level fields for automation: `ready`,
`read_ready`, `write_ready`, `sqlite_version`, `program_version`,
`engine_current`, and `upgrade_needed`. Version fields are `null` when the live
check is skipped or cannot complete.

The nested `readiness` object reports the underlying checks:
`circle_reachable`, `auth_readable`, `owner_write_valid`,
`storage_initialized`, `sqlite_ready`, and `query_ready`. Values are `null` when
live checks are skipped or not reached.

A known historical octra-sqlite engine can be read/write healthy while
`engine_current` is `false` and `upgrade_needed` is `true`; that is an upgrade
signal, not a generic readiness failure.

Use `status DATABASE --ready` as the read/query operational gate. With `--json`,
it prints the same single status envelope and exits nonzero when `read_ready` is
not `true`. `write_ready` is separate so walletless public-read databases can be
healthy for reads while still reporting that owner writes are unavailable.

`wallet status --json` reports wallet path, file permissions, caller
address, active target, and read/write relationship to the target Circle. It
does not print private keys or raw wallet JSON.

`wallet attach --json` and `wallet import --json` report the active wallet path
and derived Octra address. They do not print private keys, signatures, or raw
wallet JSON.

`limits --json` is the compact capability surface for automation. It includes
CLI/SQLite/schema versions, SQL byte and VDBE work limits, result row/response
limits, exact VFS capacity, restore behavior, read/write auth facts,
confidentiality facts, and available trace modes. In particular,
`confidentiality.encrypted` and `confidentiality.sealed_owner_only` are `false`,
write SQL is marked visible in transaction history, and
`storage.max_dirty_pages_per_exec` distinguishes per-write capacity from total
database capacity. Contract `response_too_large` errors are reported to CLI
automation as `result_too_large`.

`commands --json` lists the supported CLI command surface and the stable JSON
envelopes each command can emit. Use it when a caller needs command discovery
without parsing human help text.

## RPC Trace

For read proof/debugging, write JSON-RPC trace envelopes to a JSONL file:

```sh
octra-sqlite DATABASE --trace-rpc-json trace.jsonl "select * from artist;"
octra-sqlite DATABASE --trace-rpc-json trace.jsonl --trace-rpc-json-mode summary "select * from artist;"
```

Trace mode defaults to `full`. Available modes:

Trace files are created privately on Unix and the path must not already exist;
the CLI refuses to truncate an existing file.

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
