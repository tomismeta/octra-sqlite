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

Authenticated non-owner wallets can still use the signed view path for reads,
but their writes are denied before SQLite execution.

OSW1 currently verifies owner-signed intent, not native
caller-bound role membership. Until Octra exposes native method access control
or trusted caller identity inside `wasm_v1`, OSW1 should be treated as a
single-use write capability for its database id, method, sequence, and SQL.

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

Set `OCTRA_SQLITE_TRACE_SQL_EVENT=1` to use `exec_trace` and emit the full SQL
text as an additional `octra.sqlite.sql` event. This is useful for demos and
proofs, but the default keeps SQL text out of events.

## Native Policy Roadmap

The current OSW1 model is intentionally small and self-contained.
It solves the default go-live requirement: creator can write, other wallets can
read but not write.

The next policy layer should be Octra-native when the runtime exposes
documented method access control or an authenticated caller import:

- `admin`: deploy, reset, migrations, policy changes
- `writer`: `exec` for application tables under SQLite authorizer limits
- `reader`: `query`, `query_typed`, `schema`, and `storage_info`
- `auditor`: metadata, storage info, and proofs only

Until that native surface exists, do not trust wallet strings passed through SQL
or client parameters. Wallet authorization must happen before SQLite runs.

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
