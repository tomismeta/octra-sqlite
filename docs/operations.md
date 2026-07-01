# Operations

This page is for scripts, mirrors, migrations, and headless jobs.

## Database Identity

Use local database names for humans:

```sh
octra-sqlite database set art oct://devnet/oct...
octra-sqlite art ".tables"
```

Use full `oct://NETWORK/<circle>` URIs for automation. A URI carries the
network boundary; a local name depends on `~/.octra/sqlite.json`.

```sh
octra-sqlite database info art --json
octra-sqlite status oct://devnet/oct... --json
```

## Read Modes

Databases are sealed by default. Sealed reads use `octra_circleViewAuth`, so a
wallet signs view requests. Writes use owner-signed OSW1 calls.

Public-read databases are explicit:

```sh
octra-sqlite new public_art --read-mode public --schema examples/artists.sql
```

Public-read SQL queries use unauthenticated `octra_circleView`, so anyone can
read data intended to be public. Writes still use owner-signed OSW1 calls. For
public apps, prefer application-level rate limits or query allowlists at the app
edge; the database Circle is a public SQL read surface.

Saved database metadata carries the read mode. If automation uses a raw
`oct://` URI, keep the mode explicit:

```sh
octra-sqlite 'oct://devnet/oct...?read_mode=public' "select * from artist;"
```

Raw URIs without a read-mode marker default to sealed reads. Use
`?read_mode=auto` only when you intentionally want the client to probe Circle
info before choosing signed or unsigned reads.

## Empty Circle Bootstrap Recovery

New `0.3.3+` databases expose `auth_info` before any SQLite pages exist, so the
first owner-signed initializer write can run normally.

If an older empty database Circle was created but cannot expose `auth_info`
because the RPC reports a missing storage cache, redeploy the bundled
owner-personalized WASM with the Circle owner wallet, then run the first schema
or restore batch through the explicit bootstrap path:

```sh
octra-sqlite deploy \
  --circle oct://mainnet/oct... \
  --rpc https://octra.network/rpc \
  --bootstrap-owner

octra-sqlite restore \
  oct://mainnet/oct... \
  --file schema.sql \
  --bootstrap-owner \
  --json-summary
```

`deploy --bootstrap-owner` does not submit SQL. It records local bootstrap
metadata after confirming the active wallet is the Circle owner and deploying
the owner-personalized bundled WASM.

`restore --bootstrap-owner` is narrower still: it requires a full
`oct://NETWORK/CIRCLE` URI, requires that `auth_info` fails with the exact empty
storage-cache error, verifies the Circle owner and deployed code hash, submits
only the first restore batch as an OSW1 owner-signed write using the saved
metadata, then immediately returns to normal `auth_info` verification for any
remaining batches.

If `auth_info` is already readable, `restore --bootstrap-owner` reports
`already_bootstrapped` and runs the normal restore path. That makes retries safe
after a successful bootstrap.

If the first write is submitted but post-write `auth_info` still fails,
`restore --bootstrap-owner --json-summary` emits `ok:false`, the first write
transaction summary, and `post_auth_info.error`, then exits nonzero. Do not
publish or backfill the database until normal `status` passes.

## Large Restore

Prefer `restore` for SQL dumps, mirrors, and backfills:

```sh
octra-sqlite check art --sql-file dump.sql
octra-sqlite restore art --file dump.sql
cat dump.sql | octra-sqlite restore art
```

`restore` splits SQL into statements, skips simple SQLite dump wrappers such as
`BEGIN TRANSACTION`, `COMMIT`, and `PRAGMA foreign_keys`, then submits safe
batches under the Circle SQL byte limit. `ROLLBACK`, savepoints, and other
transaction-control statements are rejected because silently changing their
meaning would violate SQLite expectations.

Use JSON for automation:

```sh
octra-sqlite restore art --file dump.sql --json-summary
```

The JSON summary includes statement counts, batch counts, transaction hashes,
and failed batches only. Use `--json` when a caller needs every batch receipt.
Restore errors are compact by default: batch number, statement range, SQL hash,
and a short SQL preview. Use `--verbose-sql` only when full SQL text is needed
in local debugging logs.

Happy path for a mirror/backfill:

1. Generate idempotent SQL with stable primary keys.
2. Run `octra-sqlite check DATABASE --sql-file dump.sql --json`.
3. Run `octra-sqlite restore DATABASE --file dump.sql --json-summary`.
4. Run an application count/range query, then `octra-sqlite verify DATABASE`.

If restore fails, inspect the reported batch or statement range. A multi-batch
restore can partially apply, so retry by rerunning idempotent SQL after fixing
the cause. There is no persisted resume checkpoint in the current release line.

On slower or rate-limited RPCs, the CLI retries read/view/receipt polling for
transient `429`, `503`, timeout, and non-JSON gateway responses. It does not
silently replay accepted write submissions.

## Limits

```sh
octra-sqlite limits art
octra-sqlite limits art --json
```

Current operational limits:

- One SQL statement or payload must fit within the Circle SQL byte limit.
- One read query returns at most 512 rows.
- Large result payloads can fail with `result_too_large`; select fewer columns
  or add a narrower `where` / `limit`.
- Large scripts are split into multiple signed writes.
- Each accepted write is atomic.
- A multi-batch restore is not globally atomic.
- User-managed `BEGIN`, `COMMIT`, `ROLLBACK`, and savepoints are not the Octra
  transaction boundary.

## Idempotent Imports

Make backfills safe to retry:

```sql
create table if not exists schema_migrations(
  name text primary key,
  applied_at text not null
);

insert or ignore into schema_migrations(name, applied_at)
values ('001_initial', datetime('now'));
```

For data loads, prefer stable keys plus `insert or replace`, `insert or ignore`,
or deterministic `delete where ...; insert ...;` chunks. Back up before large
changes:

```sh
octra-sqlite art ".backup main art-before.sqlite"
octra-sqlite restore art --file migration.sql
octra-sqlite verify art
```

## Concurrency

Use one writer at a time for now. Concurrent writers submit independent Octra
transactions, and the repo does not ship a multi-writer locking protocol.

## Read-Only Guard

Use `--read-only` in scripts that must never submit writes:

```sh
octra-sqlite art --read-only "select * from artist;"
```

This is a client-side safety guard, not an Octra policy layer. Reads still use
signed Octra view auth with the active wallet. Writes use OSW1 owner write
intent and are owner-gated by the Circle program.
