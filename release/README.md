# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.3.0.json` is the current draft release manifest checked by
`octra-sqlite status`.

`octra-sqlite-0.1.0.json`, `octra-sqlite-0.2.0.json`, and
`octra-sqlite-0.2.1.json` record earlier blessed Circle WASM artifacts. The
`0.3.0` release rebuilds the Circle WASM to add pinned SQLite page streaming
for local `.sqlite` backups.
