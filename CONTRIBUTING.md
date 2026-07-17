# Contributing

`octra-sqlite` is a small reference architecture for running the real SQLite C
engine inside an Octra Circle. Contributions should make that architecture
clearer, smaller, safer, or easier to use.

## Principles

- Keep it SQLite-first. Prefer SQLite semantics, SQLite terminology, and SQLite
  C APIs over project-specific inventions.
- Keep it Octra-native. Use Circles, Octra RPC, read modes, OSW1 owner-write
  intent, and deployment manifests directly.
- Keep the trusted surface small. Contract and WASM code should stay boring,
  deterministic, and easy to audit.
- Treat the CLI as the primary product surface. Automation should use the same
  commands with `--json`, not a separate agent path.
- Do not add pre-1.0 compatibility aliases or hidden legacy paths without a
  strong reason. There are no production users to carry yet; get the names
  right.
- Keep secrets out of logs, JSON output, examples, manifests, and tests.
- Put proof beside claims: tests, manifests, live proof metadata, or docs should
  back user-visible behavior.

## Before Changing Code

- Read the surrounding module first and follow its local style.
- Keep changes scoped. Avoid unrelated refactors in the same patch.
- Avoid new dependencies unless they remove real complexity or match an
  established project boundary.
- Treat `circle/source/octra_sqlite_circle.c` as consensus-critical code.
- Keep role and policy enforcement outside SQL unless Octra exposes native
  authenticated caller identity or policy hooks to WASM.

## Checks

Run the focused checks for your change. Before a release or public API change,
run the full local gate:

```sh
cargo fmt --check
cargo test --locked
cargo test --locked --doc
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --no-default-features --lib
cargo check --locked --no-default-features --features http
find scripts -name '*.sh' -exec bash -n {} \;
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
```

For local CLI smoke checks:

```sh
cargo run --locked -- status --skip-network
cargo run --locked -- commands --json
cargo run --locked -- limits --json
```

If you changed the bundled WASM, also run:

```sh
bash scripts/build-wasm.sh
bash scripts/audit-wasm.sh circle/wasm/octra_sqlite_circle.wasm
OCTRA_SQLITE_WASM=circle/wasm/octra_sqlite_circle.wasm \
  cargo test --locked --features wasm-behavior --test wasm_host_harness
```

## Release Notes And Manifests

Update `CHANGELOG.md`, `RELEASE.md`, `docs/toolchain.md`, and the matching
`release/octra-sqlite-VERSION.json` when a change affects public behavior,
package contents, artifact bytes, hashes, deployment proofs, or release claims.

For crates.io preparation, verify the package before tagging:

```sh
cargo package --list
cargo publish --dry-run --locked
```

## Release Flow

Prepare releases on one focused branch and merge through a green pull request.
Keep history linear with squash or rebase; do not publish from a dirty tree or
an unreviewed branch. After the version, changelog, release notes, manifest, and
package dry run agree, tag the exact clean `main` commit and publish its GitHub
release.

The `publish.yml` workflow exchanges GitHub OIDC for a short-lived crates.io
token and publishes that exact release tag. crates.io must trust
`tomismeta/octra-sqlite`, `publish.yml`, and the `release` environment. Keep
that environment approval-gated, non-bypassable, and restricted to `v*` refs;
no long-lived registry token belongs in GitHub secrets.

## Security

Do not include private keys, wallet files, passphrases, RPC credentials, or
private Circle data in issues, pull requests, examples, traces, release
manifests, or tests. Report security concerns through [SECURITY.md](./SECURITY.md).
