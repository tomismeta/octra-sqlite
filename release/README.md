# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.2.1.json` is the current release manifest checked by
`octra-sqlite status`.

`octra-sqlite-0.1.0.json` and `octra-sqlite-0.2.0.json` record earlier
blessed Circle WASM artifacts. The `0.2.1` release rebuilds the Circle WASM to
accept SQLite trailing comments on single-statement reads.
