# Roadmap

The center stays fixed: real SQLite inside Octra Circles, a small consensus
surface, a SQLite-shaped CLI for humans and automation, and a Rust first story
of `Client -> Database -> query/execute`.

Roadmap items deepen that spine. Framework integrations, ORMs, alternate agent
commands, and application servers belong in examples or downstream projects.

Developer experience should be modeled after disciplined Rust crates such as
`rusqlite` in craft, not breadth. Keep the README product-first, make docs.rs
the curated Rust API front door when a utility-bearing release justifies the
work, and do not cut releases for documentation polish alone.

## Themes

- **Security**: authorization, secret handling, deterministic limits, and
  fail-closed trust boundaries.
- **Scalability**: measured storage and execution limits, bulk-operation
  guidance, and protocol constraints.
- **Architecture**: one product path, clear module responsibilities, and a small public
  surface.
- **Developer Experience**: fast setup, SQLite-shaped workflows, useful errors,
  and concise documentation.
- **Operations**: backup, restore, upgrade, rollback, readiness, and stable
  automation output.
- **Octra**: adopting native host capabilities without recreating them in the
  Circle program.

## Now: 0.6.4 Measured Operations

Themes: **Security**, **Operations**, **Developer Experience**

- Keep the top-level command set, JSON envelopes, root Rust types, OSR1/OSW1,
  storage accounting, execution budgets, bundled SQLite 3.53.4 engine, and
  runtime dependency graph unchanged.
- Establish fuel and footprint measurement with a documented baseline, local
  measurement script, and CI size budgets.
- Improve receipt follow-up diagnostics for missing receipts and terminal
  rejected/dropped transactions.
- Classify runtime WASM fuel exhaustion as `exec_budget_exceeded` and give
  operator guidance around bounded queries, indexes, batching, and receipt
  follow-up.
- Record host-native deterministic Ed25519 verification as the main Octra
  protocol ask for reducing owner-signed write fuel without weakening OSW1.

Exit: full local gates, WASM harness/audit, package dry run, clean branch/PR
hygiene, and the review panel confirm a patch-compatible client/tooling release;
then `0.6.4` can publish without pulling in unrelated roadmap work.

## Next: 0.7.0 Secret Ownership

Themes: **Security**, **Architecture**, **Developer Experience**, **Octra**

- Redesign inline key material around explicit zeroizing ownership instead of
  freely cloned `String` fields.
- Remove `Clone` from secret-owning public types where the safer ownership model
  requires it.
- Define an external signer boundary only against a documented Octra-native
  signing protocol; do not invent a blind localhost signer.
- Promote lifecycle capabilities from `client::raw` only when real application
  integrations prove they belong in the stable data plane.
- Publish a concise migration note for every intentional Rust API break and
  carry no aliases during `0.x`.

`0.7.0` is reserved for this work because changing `ClientOptions` secret
fields or clone semantics is a real Rust API break. Do not cut the minor merely
because it is next numerically.

Exit: secret material has one legible owner, signer behavior is explicit, and
the root API remains smaller than the raw/control-plane layer.

## Later: Operator And Host Maturity

Themes: **Scalability**, **Operations**, **Octra**

- Add restore checkpoints only if real multi-batch workloads justify them.
- Tune page, dirty-page, and execution budgets only from measured workloads and
  with WASM harness plus devnet proof.
- Prefer host-native deterministic Ed25519 verification when Octra exposes it;
  OSW1 should stay intact, but the expensive primitive does not need to live in
  WASM forever.
- Adopt protocol-enforced WASM fuel, authenticated caller identity, and method
  policy when Octra exposes documented consensus-safe capabilities.
- Consider a separate read-only client crate only after the core library
  boundary proves stable and the split removes meaningful dependency weight.

Exit: long-lived databases are routine to operate, and host-native security can
be adopted without expanding the Circle into a policy engine.

## Stability Horizon

Before `1.0`, freeze OSR1 and OSW1, CLI JSON envelopes and error codes, the root
Rust API, Circle method names, and the storage-generation format. Human help,
interactive copy, and engine versions can continue to evolve without weakening
those contracts.

Every candidate change still answers four questions: Is it SQLite-like? Is it
Octra-native? Is it smaller than the alternative? Will a new user understand it
quickly?
