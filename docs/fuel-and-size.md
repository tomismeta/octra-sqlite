# Fuel And Size

`octra-sqlite` tracks fuel and footprint as release hygiene. The goal is to
keep the Circle program small, make runtime costs visible, and avoid security
or SQLite-fidelity shortcuts.

Use the measurement script before releases or any change that touches
`circle/source`, `circle/wasm`, OSR1/OSW1, query execution, restore batching, or
the package include list:

```sh
scripts/measure-wasm.sh
scripts/measure-wasm.sh --json
```

CI runs the same script with `--skip-fuel` and size budgets so routine pull
requests catch footprint regressions without duplicating the full
`wasm-behavior` harness run.

## Baseline

Baseline captured on 2026-08-03 from `0.6.4`:

| Metric | Value |
| --- | ---: |
| Packaged crate | about 501000 bytes |
| Bundled Circle WASM | 611677 bytes |
| Bundled Circle WASM SHA-256 | `8fe0dad1a4bb4fcfc7afab626a58eda45edeac3b25607f130b201997698d8bcf` |
| Public `query_typed` read, `select 1` | 411352 fuel |
| Sealed-read auth WASM delta | 0 fuel |
| Unsigned bootstrap `exec`, `select 1` | 916881 fuel |
| Auth denied before signature verification | 17654 fuel |
| Bad-signature OSW1 verification | 107748311 fuel |
| Signed `exec`, `select 1` | 108655569 fuel |
| Signed tiny write | 109255678 fuel |
| Signed restore-like batch | 111929982 fuel |
| Representative Vitals-style bounded query | 1343069 fuel |

The fuel values are Wasmtime harness values. They are useful for relative
regression detection and hotspot identification, but they are not a contractual
Octra network fuel schedule. The packaged crate size is approximate because the
compressed `.crate` byte count can vary slightly across local packaging runs.

The representative Vitals query is a stable synthetic fixture: an indexed
time-series table queried for the latest 30 rows. It exists to catch broad read
path regressions until a real Vitals fixture belongs in this repository.

## 0.6.4 Footprint Improvement

`0.6.4` excludes `RELEASE.md` from the published crate package while keeping it
in the repository. That file is maintainer release-process documentation, not a
user install, API, operation, or verification artifact. Downstream users still
receive the CLI, library source, tests, README, selected docs, examples, release
manifests, bundled Circle WASM, and release scripts needed for install and
verification.

Measured package result:

| Package | Bytes | Files |
| --- | ---: | ---: |
| Before excluding maintainer release runbook | about 509000 | 92 |
| After excluding maintainer release runbook | about 501000 | 91 |
| Reduction | about 8500 | 1 |

That is a 1.7% compressed package reduction without changing runtime behavior,
SQLite semantics, OSW1, storage format, command names, or the bundled Circle
WASM.

## Interpretation

OSW1 signature verification dominates owner-signed writes. In the baseline,
authentication denial before verification is cheap, while a bad-signature verify
costs about the same as a successful signed write. That means SQLite execution
is not the first bottleneck for tiny writes.

Public reads and bounded indexed reads are much cheaper than owner-signed
writes. That matches the product model: public-read Circles are the right path
for query mirrors, while writes should be batched deliberately.

Sealed-read auth does not add Circle WASM fuel in this harness because OSR1 view
authentication is handled by the Octra RPC/read path before `octra_query`
executes. The query itself runs the same Circle method.

## Release Gate

Use this local gate before release candidates:

```sh
scripts/measure-wasm.sh --json
```

Treat these as review triggers, not automatic blockers:

- Bundled WASM grows without a SQLite upgrade or explicit contract change.
- Packaged crate grows because historical artifacts or generated files were
  included.
- `auth_bad_signature_verify` or `signed_exec_select` moves materially without
  an auth-path change.
- Representative query fuel grows without a query/result codec change.

CI currently enforces:

```sh
scripts/measure-wasm.sh --skip-fuel --json \
  --max-wasm-bytes 700000 \
  --max-package-bytes 2500000
```

Those budgets leave room for normal SQLite patch movement while catching
accidental package bloat.

## Optimization Policy

Allowed:

- Measure first, then optimize the proven hotspot.
- Batch writes so one OSW1 signature protects more useful SQL.
- Keep read queries bounded and index-backed.
- Remove unused package artifacts.
- Tune SQLite compile flags only when tests prove the removed feature is outside
  the intended SQLite surface.

Not allowed:

- Weakening OSW1 owner-write intent.
- Replacing SQLite semantics with project-local SQL shortcuts.
- Embedding historical WASM binaries in the crate.
- Optimizing primarily around a temporary devnet limit.
- Adding large dependencies for marginal size or fuel movement.

## Protocol Ask

The largest fuel lever is native Octra signature verification. A host import
such as deterministic Ed25519 verify would let the Circle program keep OSW1's
security model while moving the expensive primitive into the host runtime.

Required properties:

- deterministic result across validators
- verifies the same bytes OSW1 signs today
- does not expose private key material to the Circle
- fails closed with no state commit
- available before SQLite execution

Until that exists, `octra-sqlite` should treat in-WASM Ed25519 verification as
the expected write-path cost and focus on batching and bounded query design.
