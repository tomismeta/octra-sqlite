# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.6.0.json` is the current release manifest checked by
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
upgrade, rollback, final re-upgrade proof, and the current quick-start
public-read Circle status. `release/wasm/*.wasm` contains the blessed
historical WASM catalog used by `upgrade` to reconstruct rollback bytes for
known pre-0.6.0 deployments.
