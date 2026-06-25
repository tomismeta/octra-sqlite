# Owner Write Intent

Owner write intent is the project-local signed frame used by `octra-sqlite`
before a state-changing SQL statement is sent to the Circle `exec` method.

It is not a SQLite standard, an Octra standard, or a general-purpose protocol.
It is deliberately smaller than SQL policy. Octra authenticates the wallet that
sends the Circle call. The owner write intent authenticates the exact SQLite
write inside that call.

## Frame

All integers are unsigned big-endian.

```text
domain        21 bytes   "octra-sqlite.osw1.v1\0"
database_id   32 bytes   owner-patched database id
sequence       8 bytes   monotonic positive u64
method_len     2 bytes   length of method
method         N bytes   "exec" or "exec_trace"
sql_len        4 bytes   length of SQL bytes
sql            M bytes   UTF-8 SQL text
```

The `osw1` text in the domain is a v0.1 compatibility label from the first
implementation. It is not intended to name a public standard.

For the same compatibility reason, low-level Circle JSON from the v0.1 WASM may
still include `"auth":"osw1"`. Clients should present that as owner write intent.

The Ed25519 signature is over the exact frame bytes. The contract receives four
string parameters:

```text
sql
owner_pubkey_hex
sequence_decimal
signature_hex
```

The owner public key must match the public key patched into the Circle WASM. The sequence must be greater than the last accepted owner sequence recorded in Circle key-value metadata.

## Security Notes

`octra-sqlite` signs owner write intents with the configured Octra wallet key.
That key is also used for Octra transactions, so the frame relies on the
versioned domain prefix above to keep SQLite write intents distinct from every
other signature made by the wallet.

Until Octra exposes native method access control or a trusted caller identity to
the WASM runtime, an owner write intent signature is a caller-independent write
capability for one database id, one method, one sequence, and one SQL string. Do
not log, publish, or reuse owner write intent parameters before they are
submitted.

Owner write intent authorization state is keyed by owner public key and
sequence, not by raw signature bytes. Do not use Ed25519 signature bytes for
deduplication, idempotency, or authorization decisions.

## Commit Rule

The owner sequence is committed in the same v4 metadata promotion as SQLite page state. A failed SQL statement, failed page write, failed manifest write, or failed metadata write must not consume the sequence.

## Golden Vector

Input:

```text
db_id:    1fce55ad53f355909514a6a349e2afb2a22cf3bca124d239a9ace46a4108c482
sequence: 42
method:   exec
sql:      select 1;
```

Frame hex:

```text
6f637472612d73716c6974652e6f7377312e7631001fce55ad53f355909514a6a349e2afb2a22cf3bca124d239a9ace46a4108c482000000000000002a0004657865630000000973656c65637420313b
```
