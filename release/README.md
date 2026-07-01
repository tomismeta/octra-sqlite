# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.3.4.json` is the current release manifest checked by
`octra-sqlite status`.

`octra-sqlite-0.1.0.json`, `octra-sqlite-0.2.0.json`,
`octra-sqlite-0.2.1.json`, `octra-sqlite-0.3.0.json`, and
`octra-sqlite-0.3.1.json` record earlier blessed Circle WASM artifacts. The
`0.3.2` release keeps the `0.3.1` Circle WASM and hardens automation output
around it. The `0.3.3` manifest records the rebuilt empty-bootstrap Circle
WASM and its deployed devnet proof. The `0.3.4` manifest is a client-only CLI
productization release over the same deployed Circle WASM proof.
