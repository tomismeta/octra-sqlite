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
octra-sqlite restore art --file dump.sql --json
```

The JSON summary includes statement counts, batch counts, transaction hashes,
and per-batch statement ranges.

## Limits

```sh
octra-sqlite limits art
octra-sqlite limits art --json
```

Current operational limits:

- One SQL statement or payload must fit within the Circle SQL byte limit.
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

This is a client-side safety guard, not an Octra policy layer.
