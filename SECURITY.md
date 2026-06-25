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
authorization. Wallet roles must be enforced through Octra-native method policy
or a host-authenticated caller import before SQL execution.
