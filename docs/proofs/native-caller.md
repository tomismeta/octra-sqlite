# Native Caller Probe

This isolated probe evaluates Octra caller/self host imports without changing
the octra-sqlite Circle program. It is design evidence, not release code and is
excluded from the published crate.

Reviewed runtime source:

- LiteNode commit `786ff3d1afeaa48752d56edc2b8338d30ba1d225`
- Probe source SHA-256 `eab6076880847f17b820fd95119d042ea920c9561b5a7e99b3c38ed45d57fa09`
- Compiled WASM SHA-256 `3d9fb1b8ff563fbac97087bde65fc5a36d7c35bde3e735ab17f8b4e34dea62a3`
- Compiled WASM size: 1,554 bytes

Reproduce the binary with LLVM clang 22:

```sh
clang --target=wasm32 -Oz -flto -nostdlib \
  -Wl,--no-entry -Wl,--export-memory -Wl,--allow-undefined -Wl,--strip-all \
  -o native-caller-probe.wasm docs/proofs/native-caller-probe.c
```

Source inspection establishes that LiteNode supplies the authenticated
transaction sender to updates and the zero address to unsigned public views.
A local host-harness run establishes the ABI behavior: owner caller and Circle
self were returned, the owner write persisted one key, and a non-owner write
returned `403` without a storage effect.

The temporary host-harness adaptation was not retained, so that local result
is not independently reproducible from this repository. Treat it as supporting
ABI evidence only. The source inspection above establishes authentication; the
live owner/non-owner devnet transactions below remain the required network
proof.

Two isolated deployments were submitted while devnet remained at epoch
`1331406`:

```text
first tx: 4a48a8c798361cfb7cf15b13a864edf920d196f414ccd5ea16d60ecf31002c51
first circle: oct4VmsqxromHqET7WQQxMFd6bzdRq4XG1i1axABg3nioM6
result: expired from staging; transaction and Circle are not found

current tx: 58076f016b4ee2db4d07317924db0070344015b0948570b57e7636175a89436d
current circle: octHukWDgn7zYaHjt8HxETE7wD1UFNDNo1D8N3XXcNJGJ37
result at 2026-08-11 review close: pending; no Circle committed
```

Neither attempt is network proof or a partial deployment. Do not resubmit while
the current transaction is pending. Do not promote native-caller authorization
until owner and non-owner devnet transactions confirm and the gates in
`docs/policy.md` pass.
