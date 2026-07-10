# Security

## Scope

This is alpha software for Octra devnet testing. Do not store secrets,
production records, financial records, or irreplaceable data in alpha
databases.

Supported security-sensitive surfaces:

- Rust CLI signing, database resolution, deployment, query, and exec paths.
- The bundled `wasm_v1` SQLite Circle program.
- OSR1 typed-result decoding.
- Generation-manifest Circle key-value storage.

Out of scope for supported security reports:

- Local machine compromise or leaked wallet files.
- Deployment claims outside the published release manifests and proofs.

## Wallets

Never commit wallet JSON, private keys, seed phrases, `.env` files, or generated
secrets. The `.gitignore` excludes common wallet/key names, but contributors are
responsible for checking their diffs before sharing them.

## Data Visibility

`sealed` is an Octra authenticated-read mode, not confidentiality. It does not
encrypt SQLite pages, query results, SQL, or values, and any wallet accepted by
the Octra sealed-view surface can read. State-changing SQL and its values are
part of the signed Octra transaction message and remain visible in transaction
history. `exec` keeps full SQL out of the optional SQL event, but it cannot make
the containing transaction private.

Do not put secrets or confidential records in a Circle database unless Octra
adds a documented encryption and access-control layer that covers this exact
execution path.

## Resource Limits

The Circle program applies deterministic SQLite progress-handler budgets to
queries and writes. These bound SQLite virtual-machine work even when a query
produces few rows. Runtime-wide WASM fuel or time metering remains an Octra
protocol responsibility; the contract cannot meter work outside SQLite.

Local config, backups, upgrade bundles, wallet files written by the CLI, and
RPC traces are created with owner-only permissions on Unix. Trace creation
refuses an existing path rather than truncating it.

## Reporting

For now, report issues privately to the repository owner before publishing a
public proof. Include:

- affected command or contract method
- expected behavior
- observed behavior
- whether funds, wallet material, or Circle state can be affected
- minimal reproduction steps

## Policy Boundary

The current WASM import surface does not expose authenticated caller identity.
Do not treat wallet strings passed through SQL or client parameters as
authorization. Owner writes are enforced by OSW1 signatures; future multi-role
wallet policy must use Octra-native method policy or a host-authenticated caller
import before SQL execution.
