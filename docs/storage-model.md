# Storage Model

This project runs the real SQLite engine in a `wasm_v1` Circle and implements a
small SQLite VFS whose durable file is Circle key-value storage.

## Layout

- Meta key: `octra.sqlite.vfs.v1.meta`
- Legacy page keys: `octra.sqlite.vfs.v1.page.<8-hex-page-number>`
- Generation page keys:
  `octra.sqlite.vfs.v1.gen.<16-hex-generation>.page.<8-hex-page-number>`
- Generation manifest keys:
  `octra.sqlite.vfs.v1.gen.<16-hex-generation>.manifest`
- Page size: `4096`
- Storage label: `circle_key_value_page_vfs`
- Max database pages in `v0.1.0`: `8192`
- Max dirty pages per `exec` in `v0.1.0`: `1024`

The metadata record stores the durable main-file size, active generation, and
last accepted owner write sequence. Pages are always stored as fixed 4096-byte
values. SQLite temp and journal files are memory-backed inside a single
invocation; only the main database file is durable.

## Write Path

`exec` opens SQLite through the page VFS, starts `begin immediate`, runs the user
SQL under the SQLite authorizer, commits inside SQLite, then flushes dirty pages
to Circle key-value storage. The response reports whether pages were persisted,
the dirty-page count, the file size, and SQLite's change count.

The dirty-page buffer stays in the contract. It keeps writes staged inside the
invocation until SQLite has accepted the transaction. After SQLite commits, the
contract writes a new generation of page keys and promotes the metadata key
last:

- Write dirty pages under the next generation.
- Write a compact manifest mapping each logical page to its physical generation.
- Write metadata version `OSQLVFS4` with file size, generation, and owner
  sequence last.
- Readers use only the manifest named by metadata.

The manifest value is an array of big-endian `u64` generation numbers, one per
logical page in the durable SQLite file. This keeps successful commits sparse:
an update writes dirty pages, one manifest, and metadata. After metadata
promotion, replaced physical page versions and the previous manifest are deleted
best-effort, so steady-state storage stays bounded to one live physical value per
logical page plus one manifest. Existing `OSQLVFS3` databases remain readable;
the next successful write promotes them to `OSQLVFS4`.

## Read Path

`query` opens an existing database with `SQLITE_OPEN_READONLY` and enables
SQLite `query_only`. It accepts one read-only statement, rejects dangerous
surface area with `sqlite3_set_authorizer`, and returns bounded JSON rows.

The legacy JSON codec intentionally supports only `NULL`, `INTEGER`, and
`TEXT`. `REAL` and `BLOB` results fail closed there.

The deployed `v0.1.0` program also provides `query_typed` and `schema_typed`.
These methods return an `OSR1:<base64>` string. The decoded payload is:

```text
OSR1
u32 column_count
u32 row_count
repeat column_count:
  u32 utf8_name_len
  utf8_name_bytes
repeat row_count * column_count:
  u8 tag
  value bytes
```

Cell tags are:

- `0`: NULL
- `1`: signed 64-bit integer
- `2`: IEEE-754 double as canonical big-endian bits
- `3`: UTF-8 text with a 32-bit byte length
- `4`: blob with a 32-bit byte length

This keeps JSON escaping and presentation out of the contract for clients that
use the typed path. JSON methods remain as a compatibility layer.

## Persistence Semantics Pass

The current public web client and docs describe Circle deployment, resources,
policy hashes, member roots, and local APIs, but they do not yet document an
explicit all-or-nothing guarantee for every individual WASM key-value write made
during an update. The generation commit protocol means the reference no longer
needs that answer for correctness, though a documented guarantee could simplify
a future version.

The live harness can run a write smoke test when supplied with a funded wallet.
Without writer credentials it still proves live reads, schema, storage, and
SQLite-client access.

## Minimalism Decisions

The page VFS is the right seam for "real SQLite semantics on Octra": it uses
SQLite's official storage extension point and avoids rewriting the SQL engine.
The contract should stay small around that seam:

- Keep SQLite heavily omitted, but do not enable flags that introduce unsupported
  imports or change semantics silently.
- Audit final WASM imports and exports after every build.
- Keep client rendering and convenience tooling outside the contract.
- Keep policy separate from the VFS until the host exposes caller identity or
  method access control.
