# Octra Native Requirements

These are the protocol-level requirements that decide how far `octra-sqlite`
can move policy into native Octra enforcement.

Checked public docs on 2026-06-25:

- https://docs.octra.org/
- https://docs.octra.org/user-docs/circles
- https://docs.octra.org/developer-docs/rpc-scheme

Those docs establish Circles, `oct://` resource addressing, and the JSON-RPC
read/write shape, but they do not answer the protocol requirements below.

## Key-Value Atomicity

When a `wasm_v1` Circle update returns success after calling `host_kv_put` or
`host_kv_del` multiple times, does Octra commit all key-value mutations from that
update atomically as one unit?

The exact guarantee needed:

- either every successful host key-value mutation from the update is durable, or
  none are durable
- failed updates do not expose partial key-value writes
- validators observe the same committed key-value set for the update

This repository no longer depends on that answer for correctness. The contract
uses generation-scoped page keys and promotes a single metadata key last. A
documented all-or-nothing guarantee would let us simplify that design later.

## Wallet Roles And Write Policy

Implemented go-live baseline: a Circle created by `octra-sqlite new NAME`
enforces creator-only writes with an owner-patched WASM and signed owner write
intents.

`exec`, `query`, `query_typed`, `schema`, `storage_info`, and `reset` are not
Octra protocol commands, and they are not SQLite CLI dot commands. They are the
exported methods of the `octra-sqlite` Circle program. Octra sees them as the
method string in a `circle_call`; the SQLite engine only runs after the Circle
runtime dispatches into one of those methods.

Earlier live devnet evidence from 2026-06-25 showed that default Circle
ownership alone was not enough: a funded non-owner wallet could authenticate,
call `exec`, and mutate SQLite state. Native `circle_slot_policy_put` was
owner-only, but slot/state policy refs are 64-character hex refs and testing
`sha256("exec")` did not gate the SQLite `exec` method.

The remaining native policy goal:

- owner/admin wallets can call `exec` and `reset`
- non-owner wallets cannot call `exec` or `reset` unless explicitly granted
- reader wallets can call `query`, `query_typed`, `schema`, and `storage_info`
  according to the Circle's read policy
- enforcement happens before SQL execution
- failed unauthorized writes produce a deterministic policy result or native
  method error

The preferred native shape is therefore Circle-level program-method access control:

```text
circle program method policy:
  exec        -> owner/admin/writer
  reset       -> owner/admin
  query       -> owner/admin/writer/reader
  query_typed -> owner/admin/writer/reader
  schema      -> owner/admin/writer/reader
  storage_info -> owner/admin/writer/reader
```

An equivalent API shape is acceptable if the policy is validator-enforced,
bound to the authenticated transaction caller, and applied before the WASM
method runs.

The public RPC docs checked on 2026-06-25 do not document `program_exec` as the
Circle WASM write path. For this repo, writes remain native signed transactions
with `op_type: "circle_call"` and the Circle method name in `encrypted_data`.

If method access control does not exist, the second acceptable shape is an authenticated
`wasm_v1` caller import, for example:

```c
int32_t host_caller_addr(uint8_t *out, uint32_t cap);
```

The required properties for that import:

- the returned caller is the authenticated transaction signer or view signer
- the value cannot be supplied or modified by SQL parameters
- the value is deterministic across validators
- the import is available before `exec` or `reset` invokes SQLite

With that import, `octra-sqlite` can store a tiny role table in Circle key-value
storage outside SQLite and gate `exec`/`reset` before SQL runs.

## Are Circle Key Routes Enough?

Not unless Octra confirms an additional enforcement hook that is not visible in
the current public client source.

The current key routes appear to control HFHE/sealed-data key lifecycle:

- `circle_key_grant`
- `circle_key_extend`
- `circle_key_revoke`
- `circle_key_erase`
- `circle_key_policy_put`
- `octra_circleKeyPolicy`
- `octra_circleKeyPolicyAuth`

Those payloads are keyed by `key_id` plus lifecycle fields such as activation,
expiry, `revoked`, and `erased`. The observed `circle_call` path for program
methods carries `method` and `params`; it does not carry a `key_id`, and the
client-side HFHE authorization helper is used by FHE/sealed-resource routes, not
by program calls.

A key-based solution would be acceptable only if Octra documents and enforces
all of these:

- a `circle_call` can include a `key_id` or equivalent capability reference
- validators check that key policy before dispatching to the WASM method
- key policy can bind the capability to caller wallets and Circle program
  method names such as `exec`
- a caller without a live authorized key cannot execute `exec`
- revoking the key immediately prevents future writes

Without those properties, key grants are useful for encrypted resources but are
not SQL write authorization.

## Required Acceptance Test

For a fresh Circle created by the CLI:

1. owner creates a table with `exec`
2. owner grants a second funded wallet reader access
3. reader can run `query`
4. reader cannot run `exec`
5. owner can still run `exec`
6. if writer access is granted, the writer can run `exec`
7. if writer access is revoked, the writer can no longer run `exec`

The reference must not trust wallet strings supplied through SQL or client
parameters. The enforcement point must be native Octra policy or a
host-authenticated caller visible to the WASM program.

## TAPE Watch Item

TAPE should not be added to the default user journey until Octra documents how
it applies to Circle program execution, storage, or verifier hooks.

If TAPE becomes the native proof format for encrypted state or delegated
execution, the likely `octra-sqlite` use is not replacing SQLite-in-WASM. The
likely use is a future proof or privacy layer around:

- encrypted query inputs or outputs
- private table or column policies
- off-chain execution proofs for a state root

Those are separate from the current reference goal: a minimal, live SQLite
primitive in an Octra Circle with owner-signed writes.
