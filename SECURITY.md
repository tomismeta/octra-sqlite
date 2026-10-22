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

`sealed` requires Circle-owner-authenticated reads under the current upstream RPC
contract; it is not encryption. Neither sealed nor public mode encrypts SQLite
pages, query results, SQL, or values. The owner restriction is established by
upstream source. Isolated devnet non-owner rejection was observed separately
on the preceding `6108d174…` and current `69b9a16e…` artifacts, not across all
networks (see [the policy evidence](docs/policy.md#current-model)).
The current artifact also has confirmed devnet owner-signed sealed reads.
State-changing SQL and its values are part of the signed Octra transaction
message and remain visible in transaction history. `exec` keeps full SQL out of
the optional SQL event, but it cannot make
the containing transaction private.

Public-read mode also exposes `backup_chunk`. Anyone, without a wallet, can query
a public database and reconstruct its complete SQLite file; public means full
database export, not only access to selected application queries.

Do not put secrets or confidential records in a Circle database unless Octra
adds a documented encryption and access-control layer that covers this exact
execution path.

## Resource Limits

The Circle program applies deterministic SQLite progress-handler budgets to
queries and writes. These bound SQLite virtual-machine work even when a query
produces few rows. Runtime-wide WASM fuel or time metering remains an Octra
protocol responsibility; the contract cannot meter work outside SQLite.

One write can stage at most 1,024 distinct dirty SQLite pages. Broad updates,
index builds, or other write-heavy statements can reach that transaction limit
before the database reaches its total file-size limit.

Local config, backups, upgrade bundles, wallet files written by the CLI, and
RPC traces are created with owner-only permissions on Unix. Trace creation
refuses an existing path rather than truncating it.

Private-file replacement writes and syncs a temporary sibling, then uses native
`std::fs::rename`; it never deletes the destination to retry replacement.
Rust 1.96 supports replacement on Windows, but native Windows runtime checks
remain unexercised. Unix mode `0600` is not a Windows ACL guarantee, and parent
directory syncing is best-effort rather than a portable crash-durability claim.

Rollback rechecks program hash, storage generation, and owner sequence against
the reviewed state immediately before submitting. Changed state aborts for
replan, including forced rollback. This is not chain-side compare-and-swap:
pause external writers through confirmation to limit the residual TOCTOU race.

The 0.6.4 release corrects the OSW1 verifier output capacity to include
TweetNaCl's signature-plus-message copy. It does not change OSW1 authorization
or SQL limits; local boundary tests are not a network deployment proof.

## Reporting

For now, report issues privately to the repository owner before publishing a
public proof. Include:

- affected command or contract method
- expected behavior
- observed behavior
- whether funds, wallet material, or Circle state can be affected
- minimal reproduction steps

## Policy Boundary

Current upstream LiteNode exposes authenticated caller and Circle identity through
`host_caller_*` and `host_self_*` imports. The bundled octra-sqlite program does not
use those imports; their exposure upstream is not a deployed authorization
migration or proof of availability across supported networks. The existing
[native-caller research](docs/proofs/native-caller.md) remains separate from the
production program.

Do not treat wallet strings passed through SQL or client parameters as
authorization. OSW1 signatures still govern writes. The upstream RPC sealed-read
gate is separate from that program write boundary; correcting advertised read
capabilities does not implement a new ACL. Future multi-role wallet policy must
use Octra-native method policy or a host-authenticated caller import before SQL
execution, with migration and network verification.
