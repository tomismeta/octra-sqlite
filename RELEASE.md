# Release Notes

## 0.1.0

This release is a reference architecture for running real SQLite inside an
Octra `wasm_v1` Circle with page-backed Circle key-value storage. The software
version is network-neutral; the published live proof for this release is on
devnet.

## Proven

- SQLite 3.53.2 runs inside the deployed Circle program.
- SQLite pages persist in Circle key-value storage across calls.
- Receipt-confirmed `exec` calls can create tables, insert rows, update rows,
  delete rows, and read the final state back with SQL.
- New databases are owner-bound by default with method-bound owner write
  intents.
- A non-owner wallet can read but receives an explicit `octra.sqlite.auth`
  event when it attempts to write.
- A stock local SQLite client can load the Octra live extension and display data
  fetched from the live Circle.
- The WASM import/export surface is audited by script.
- Local tests cover the typed result codec, owner write intent vectors, and core
  contract behavior.
- `octra-sqlite status` checks local config, wallet discovery, release manifest,
  bundled WASM bytes/hash, and live database health when credentials are present.
- The public proof Circle contains one intentional sample table, `person`.

## Not Claimed Yet

- A published mainnet deployment proof.
- Multi-wallet writer grants and revocation.
- Native Octra method access control for SQL roles.
- Bit-for-bit reproducible builds across arbitrary host toolchains.
- A stable public package API beyond the starter CLI and documented scripts.

## Live Devnet Proof

```text
circle: oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR
version: 1
personalized_code_hash: 1725953bb3dfccf9363a1eb27c4a1eeab1f5da53913465dbf4d061ca23e41c8d
bundled_wasm_hash: 0e28ecc233306fd59539a22209be633fa7e6ca7410c84ce7c940abfcfb372e7a
code_bytes: 607496
circle_create_tx: c6f8bfad04e380bbdc460c8841c4d680edcbcc0a37bac20864fa9e6ccc35381d
initializer_tx: 41616f60bf2047c8374d3d3b0af7ae70960cfc9400aa12d0d4518ec6e9a222dd
non_owner_denied_tx: 9f8f32c600ff1952b256ba40a4326b56b897d198c324de0708d48c4631851e00
manifest: release/octra-sqlite-0.1.0.json
```
