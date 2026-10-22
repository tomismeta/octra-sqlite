# Policy And Wallet Roles

`v0.1.0` enforces owner-only writes for databases created with
`octra-sqlite new`.

## Current Model

Each new database deploys an owner-personalized copy of the bundled audited WASM:

- the CLI derives the creator wallet's Ed25519 public key
- the CLI patches that public key and a database id into the WASM before
  `deploy_circle`
- every `exec` or `exec_trace` call must include a signed OSW1 owner write
  intent
- the contract verifies the signature, database id, Circle method, SQL text,
  and monotonic sequence before SQLite runs
- the signer must match the embedded owner public key
- the accepted sequence is committed with the same metadata promotion as the
  SQLite pages

Under the current upstream RPC contract, ordinary sealed views require
authentication as the Circle owner; authenticating a non-owner wallet is not
sufficient. This is a source-level contract: pinned LiteNode commit
`7f9be4489870645cfcc95439638cde40f5da9f50`, `circle_auth.ml::view_gate`,
selects `Private_owner`, and `private_owner` rejects non-owner access to
non-public views. Separately, isolated devnet tests rejected non-owners on the
preceding `6108d174…` and current `69b9a16e…` artifacts. On the current
artifact, a correctly addressed, signed non-owner read returned RPC `-32000`,
`circle owner authorization required`. This does not establish enforcement on
every network. The current artifact also has a confirmed fresh devnet
deployment, owner-signed sealed reads, confirmed default/Unicode persisted
rows, and backup integrity. Normal devnet upgrade from 3.53.4 to 3.54.1,
clean rollback, and re-upgrade preserved the rows; post-upgrade write/read
and integrity checks passed, and rollback dry-run refused to cross that write.

Public mode permits walletless SQL reads and complete SQLite file export through
`backup_chunk`. Neither mode encrypts SQL, SQLite data, or query results. Write
SQL and values are included in the Octra transaction message and remain visible
in transaction history. These access facts describe the upstream RPC gate, not a
new ACL in the bundled program; OSW1 continues to govern writes.

`--read-only` is only a client-side write guard: it neither makes public reads
signed nor bypasses owner authentication for sealed reads.

OSW1 currently verifies owner-signed intent, not native caller-bound role
membership. Current LiteNode source exposes authenticated caller and Circle
identity through `host_caller_*` and `host_self_*`, but octra-sqlite has not yet
proven their availability and semantics across supported networks or designed
the migration and rollback path for existing Circles. OSW1 therefore remains
the production authorization boundary and a single-use write capability for
its database id, method, sequence, and SQL.

## Denied Writes

Auth denials are hard rejects with conventional auth return codes:

```text
auth_required -> wasm export returned 401
auth_denied   -> wasm export returned 403
```

The contract also emits a receipt event with the policy reason:

```text
event: octra.sqlite.auth
value: auth_not_authorized:auth_denied:signed exec signer is not the database owner
```

Missing or malformed OSW1 owner write intents use `auth_not_authenticated:*`
values.
The explorer shows the rejected transaction and numeric auth code; the CLI and
`contract_receipt` surface the richer `octra.sqlite.auth` value.

## SQL Events

Successful writes always emit:

```text
event: octra.sqlite.exec
value: sql_fnv1a64:<hash>
```

SQLite write failures roll back and emit:

```text
event: octra.sqlite.error
value: sqlite_exec_failed:<sqlite error>
```

Set `OCTRA_SQLITE_EMIT_SQL_ONCHAIN_EVENT=1` to use `exec_trace` and emit full
SQL text as an additional `octra.sqlite.sql` on-chain event. This is permanent,
and public-read databases make that event public. The default keeps SQL text
out of the additional event and emits only the SQL hash event; the containing
write transaction still carries the full SQL text and values.

## Native Policy Roadmap

The current OSW1 model is intentionally small and self-contained: the owner
can write. Other wallets can read only public databases; the upstream RPC
contract restricts sealed reads to the Circle owner as well.

The next policy layer should be Octra-native if the runtime caller imports pass
live network, migration, rollback, and security review:

- `admin`: deploy, reset, migrations, policy changes
- `writer`: `exec` for application tables under SQLite authorizer limits
- `reader`: `query`, `query_typed`, `schema`, and `storage_info`
- `auditor`: metadata, storage info, and proofs only

Do not trust wallet strings passed through SQL or client parameters. Wallet
authorization must happen before SQLite runs. Do not remove OSW1 merely because
the imports exist in source; a native transition must preserve owner-only
writes for already-deployed Circles and fail closed on older hosts.

### Native Caller Decision

Decision for the 2026-08-11 runtime-alignment pass: **hold OSW1; continue the
native-caller design as a future breaking security simplification.**

An isolated 1.6 KB probe executed successfully in a local host-harness test
against the current LiteNode WASM runtime source. `host_caller_*` returned the
authenticated owner and non-owner addresses, `host_self_*` returned the target
Circle, an owner update persisted one key-value entry, and a non-owner update
returned `403` without storage effects. A devnet deployment probe remains
required before this is network proof. The source, hashes, and reproduction
record live in the repository's
[native-caller proof](https://github.com/tomismeta/octra-sqlite/blob/main/docs/proofs/native-caller.md).
The probe did not modify the octra-sqlite Circle program.

The remaining design issue is expected-owner identity. The host exposes caller
and self, but not the Circle owner. A production contract must either derive the
owner address from the already-personalized public key or add an owner-address
personalization field. The former adds deterministic address code to WASM; the
latter expands deployment and upgrade metadata. The transition also changes
the write method contract, old-host compatibility, rollback behavior, and the
meaning of `auth_info`.

Proceed only when all of these are true:

- Octra documents caller/self as stable consensus host imports on every
  supported network.
- Each supported network exposes a protocol activation signal for those
  imports, and a preflight proves unsupported hosts reject the program update
  before commitment.
- Live owner and non-owner transactions reproduce the host test on devnet and
  mainnet preflight.
- The expected-owner representation is smaller and easier to audit than OSW1.
- Existing Circles have a hash-verified upgrade and rollback path.
- Older clients fail clearly rather than weakening or bypassing authorization.
- The security and C/WASM panel approves removal of in-contract signatures and
  replay sequencing.

## Current Limitations

- There is one owner writer key per database.
- OSW1 uses the configured Octra wallet key with a versioned
  domain-separated message. A derived owner-write subkey is a future hardening
  option, not a current user requirement.
- Grant/revoke for additional writer wallets is not implemented yet.
- `reset` is intentionally blocked on owner-personalized databases for
  `v0.1.0`.
- Native Octra key routes appear useful for encrypted resource lifecycle, but no
  documented binding currently proves they gate Circle program methods such as
  `exec`.
