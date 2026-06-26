# Release Manifests

This directory contains machine-readable release manifests for audited,
network-specific artifacts.

`octra-sqlite-0.1.0.json` is the blessed Circle WASM artifact used by the
default `octra-sqlite new` path. `octra-sqlite status` checks the manifest
against the bundled `circle/wasm/octra_sqlite_circle.wasm` bytes before it checks
any live Circle.
