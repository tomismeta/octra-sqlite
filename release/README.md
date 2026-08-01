# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.6.3.json` is the current release manifest checked by
`octra-sqlite status`.

`octra-sqlite-0.1.0.json`, `octra-sqlite-0.2.0.json`,
`octra-sqlite-0.2.1.json`, `octra-sqlite-0.3.0.json`, and
`octra-sqlite-0.3.1.json` record earlier blessed Circle WASM artifacts. The
`0.3.2` release keeps the `0.3.1` Circle WASM and hardens automation output
around it. The `0.3.3` manifest records the rebuilt empty-bootstrap Circle
WASM and its deployed devnet proof. The `0.4.0` manifest is a productization
release over the same Circle WASM proof and records a separate devnet
public-read proof Circle. The `0.5.0` manifest is a Rust API ontology release
over the same bundled Circle WASM proof. The `0.5.1` manifest is a README and
packaging polish release for the crates.io debut. The `0.5.2` manifest is CLI
readiness and raw-target polish over the same bundled Circle WASM proof. The
`0.6.0` manifest records the SQLite 3.53.3 WASM rebuild plus a devnet
upgrade, rollback, final re-upgrade proof, the current quick-start public-read
Circle status, and the metadata-only catalog of blessed historical WASM epochs.
The `0.6.1` manifest records the deterministic SQLite work budgets, corrected
generation-manifest capacity, hardening scope, and the previous `0.6.0` WASM in
that metadata-only catalog, with recorded local and devnet proof.
The `0.6.2` manifest is a client-only release over the identical `0.6.1` Circle
artifact and devnet proof. It records CLI/client convergence, source-owned error
codes, documentation curation, and trusted publishing without adding a new
engine epoch.
The `0.6.3` manifest records the SQLite 3.53.4 WASM rebuild, adds the
`0.6.1-0.6.2` 3.53.3 WASM as a metadata-only historical epoch, and records the
devnet rehearsal plus Octra Vitals mainnet upgrade proof completed before
crates.io publish.
