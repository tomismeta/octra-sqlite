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
wallet signs view requests. This authenticates reads; it does not encrypt data
or limit reads to the database owner. Writes use owner-signed OSW1 calls, and
their SQL and values remain visible in Octra transaction history.

Public-read databases are explicit:

```sh
octra-sqlite new public_art --read-mode public --schema examples/artists.sql
```

Public-read SQL queries use unauthenticated `octra_circleView`, so anyone can
read data intended to be public. Writes still use owner-signed OSW1 calls. For
public apps, prefer application-level rate limits or query allowlists at the app
edge; the database Circle is a public SQL read surface.

Public mode also includes the read-only `backup_chunk` method. A walletless
reader can reconstruct the complete SQLite file, so public mode must be treated
as a full database export surface.

Saved database metadata carries the read mode. Raw `oct://` targets detect the
Octra read surface from Circle metadata:

```sh
octra-sqlite 'oct://devnet/oct...' "select * from artist;"
```

Use `?read_mode=sealed` or `?read_mode=public` only when automation needs an
explicit override.

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

## Engine Upgrades

Use `upgrade` for normal in-place SQLite engine updates:

```sh
octra-sqlite upgrade
octra-sqlite upgrade DATABASE --dry-run
octra-sqlite upgrade DATABASE --dry-run --previous-wasm ./old-octra_sqlite_circle.wasm
octra-sqlite upgrade DATABASE
octra-sqlite upgrade rollback ~/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.3-20260801
```

Strict runbook for mainnet or high-value Circles:

1. Install and run the current octra-sqlite client for upgrade/status checks.
   Older clients may still query a Circle after an engine upgrade, but their
   status and upgrade expectations can be stale.
2. Run `octra-sqlite upgrade DATABASE --dry-run --json`.
3. If `status` is `already_current`, stop; rollback is not relevant because no
   program update is pending.
4. If an upgrade is needed, review `from.code_hash`, `to.code_hash`,
   `from.sqlite_version`, and `to.sqlite_version`.
5. Require `rollback.available: true` before applying. Do not use
   `--unsafe-no-rollback` on mainnet; without rollback bytes, the upgrade
   bundle cannot restore the previous Circle program.
6. Pause external writers for the database while applying the program update.
7. For service deployments, set `OCTRA_SQLITE_CONFIG` to a path writable by the
   upgrade operator and pass `--backup-dir` to a writable app-data directory.
8. Apply with `octra-sqlite upgrade DATABASE --yes --json --require-integrity`.
9. Run `octra-sqlite status DATABASE --ready --json`, confirm `write_ready:
   true`, `engine_current: true`, and `upgrade_needed: false`, then run an
   application query.
10. Resume writers and confirm a real application write lands. On busy or
   production-like paths, set `OCTRA_SQLITE_WRITE_OU` before the write and
   `OCTRA_SQLITE_VERIFY_WRITE_OU` before `verify --write-smoke`, or pass
   `--ou` / `--write-ou` explicitly.

Normal SQL writes default to `1000` OU. Circle program upgrades default to
`200000` OU; `upgrade --ou` controls the program-update transaction, while
`upgrade --write-smoke --write-ou` controls only the optional post-upgrade smoke
write.

`upgrade` without a database opens the guided terminal workflow. It uses the
saved default database when available, shows the preflight, prints the planned
bundle and backup paths, asks whether to keep the local backup, asks whether to
run write-smoke, then asks for final confirmation. Use `upgrade DATABASE --yes
--json` for automation.

`upgrade DATABASE --dry-run` reads live program, storage, auth, and
target-engine state without writing. A real upgrade:

- verifies that the active wallet is the Circle owner and the OSW1 database
  owner;
- verifies that the local config path is writable before submitting the program
  update, so post-upgrade metadata finalization does not fail late;
- patches the bundled WASM with the existing owner public key and database id;
- recovers the currently deployed personalized WASM from local metadata, chain
  transaction history, local old release artifacts, or an explicit
  `--previous-wasm` for rollback;
- writes a private local upgrade bundle with a named `.sqlite` backup,
  `previous.wasm`, and `upgrade.json`;
- persists `upgrade.json` as `prepared` before submitting the program update,
  changes it atomically to `applied` after on-chain verification, then to
  `complete` after smoke and local metadata work;
- aborts if storage generation or owner sequence changes before the program
  update is submitted;
- refuses a legacy database already larger than the target engine's writable
  page or file limit;
- submits one `circle_program_update`, then verifies `sqlite_version()` against
  the bundled engine.

For older owner-personalized deployments, rollback recovery can use either the
local release artifacts, an already-personalized old WASM, or the previous
release's base `circle/wasm/octra_sqlite_circle.wasm`. The release manifest
JSON is the catalog source of truth for historical base WASM SHA-256 values,
byte lengths, and GitHub source URLs so the CLI can identify likely old epochs
without bundling their bytes. The CLI patches provided base WASM bytes with
live `auth_info` and accepts them only if the resulting hash matches the
currently deployed program. Use
`--previous-wasm PATH` for one run or `OCTRA_SQLITE_PREVIOUS_WASM=PATH` for
automation.

Rollback redeploys the `previous.wasm` from the bundle. It refuses to cross
post-upgrade writes unless `--force-after-writes` is supplied, and forced
rollback writes a fresh backup first. `--write-smoke` is intentionally opt-in:
it performs a create/insert/drop write cycle on the new engine. It leaves no
smoke table behind, but it still dirties production data and makes clean
rollback unavailable. When the chain does not expose the counters needed to
prove clean rollback, `rollback.clean` is `null`; rollback remains fail-closed.
Rollback availability matters only when `from.code_hash` differs from
`to.code_hash`; for `status: "already_current"`, rollback is irrelevant.

For the `upgrade` command, `rollback` is reserved for `upgrade rollback
BUNDLE`. If a saved database is literally named `rollback`, pass its raw
`oct://` URI instead.

For high-value migrations, the conservative alternative is blue-green: back up
the old Circle, create a new Circle with the new release, restore into it, run
application checks, then update the local database name or app configuration to
the new Circle.

Upgrade manifests define an engine epoch boundary. Replay byte identity is
per-engine-version, so keep the `from`/`to` code hashes and update transaction
with backups and traces.

Released binaries use the integrity-checked embedded WASM for normal creation,
bootstrap, verification, and upgrades. Use an explicit `--wasm PATH` only on
commands that support deliberate custom deployment; ambient environment cannot
replace the release upgrade target. `OCTRA_SQLITE_MANIFEST=PATH` is only for
checking a specific external release manifest. Builders using `--build` can point
`OCTRA_SQLITE_ROOT` at a source checkout when the current directory is not the
repo.

Default bundle names use the environment, Circle ID, previous SQLite version,
and date only:

```text
~/.octra/sqlite/upgrades/devnet-oct...-sqlite-3.53.3-20260801/
  devnet-oct...-sqlite-3.53.3-20260801.sqlite
  previous.wasm
  upgrade.json
```

## Foreign Keys

SQLite foreign-key enforcement is currently off. The Circle authorizer rejects
user `PRAGMA` statements, and restore skips dump wrappers such as
`PRAGMA foreign_keys=OFF`; applications must not assume declared foreign-key
constraints are enforced. Changing this requires a deliberate engine policy,
not a restore-time toggle.

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

Owner-signed SQL writes default to `1000` OU. Operators can raise the signed
budget without changing SQL:

```sh
export OCTRA_SQLITE_WRITE_OU=200000
octra-sqlite DATABASE "insert into events(id, body) values (1, 'ok');"
octra-sqlite restore DATABASE --file dump.sql --ou 200000
octra-sqlite verify DATABASE --write-smoke --write-ou 200000
octra-sqlite upgrade DATABASE --write-smoke --write-ou 200000
```

If a write was submitted but the receipt did not arrive before the wait
deadline, the CLI returns `receipt_pending`. With `--json` or
`--json-summary`, `error.details` includes the transaction hash, nonce, OU,
Circle, and recovery command when known. Follow the submitted transaction; do
not retry the write blindly:

```sh
octra-sqlite receipt TX_HASH DATABASE --json
```

## Limits

```sh
octra-sqlite limits art
octra-sqlite limits art --json
```

Current operational limits:

- One SQL statement or payload must fit within the Circle SQL byte limit.
- One read query returns at most 512 rows.
- Query execution is capped at 5,000,000 SQLite VDBE steps.
- Write execution is capped at 25,000,000 SQLite VDBE steps.
- The current generation-manifest VFS supports 8,069 pages of 4,096 bytes, or
  33.05 MB of SQLite file data, within Octra's 33.55 MB stable-storage cap when
  the Circle storage is dedicated to the SQLite VFS.
- One `exec` can dirty at most 1,024 distinct pages. Chunk broad updates by a
  stable primary-key range and verify each accepted transaction.
- Large result payloads can fail with `response_too_large`; select fewer columns
  or add a narrower `where` / `limit`. CLI JSON reports this class as
  `result_too_large`.
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
