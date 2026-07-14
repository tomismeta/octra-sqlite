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
- **Architecture**: one product path, clear module ownership, and a small public
  surface.
- **Developer Experience**: fast setup, SQLite-shaped workflows, useful errors,
  and concise documentation.
- **Operations**: backup, restore, upgrade, rollback, readiness, and stable
  automation output.
- **Octra**: adopting native host capabilities without recreating them in the
  Circle program.

## Now: Stabilize And Carve

Themes: **Security**, **Architecture**, **Developer Experience**, **Operations**

- Soak deterministic SQL budgets and the SQLite 3.53.3 upgrade path on devnet.
- Keep the top-level command set frozen.
- Split `src/cli/mod.rs` by responsibility in a behavior-only change: database
  lifecycle, onboarding, inspection, SQL dispatch, catalogs, and tests.
- Keep public-export, foreign-key, dirty-page, restore, and event-disclosure
  limits explicit in docs and `limits --json`.
- Keep `0.6.1` soaking until field feedback or material utility earns the next
  tag.

Exit: current claims have local and devnet proof, and no CLI responsibility is
forced into a single catch-all module.

## Next: One Truth Path

Themes: **Architecture**, **Security**, **Developer Experience**, **Operations**

- Route ordinary CLI query and execute flows through `Client` and `Database`.
- Promote only the lifecycle capabilities applications genuinely need; keep
  `client::raw` as adapter and control-plane plumbing.
- Replace English-substring automation classification with structured error
  codes at construction sites.
- Strengthen secret ownership so inline key material is not freely cloned and
  is zeroized on drop.
- Fold curated docs.rs examples and API documentation into the next
  utility-bearing release instead of shipping a docs-only patch.
- Establish trusted publishing and keep release hygiene on branches with
  squash or rebase merges.

Exit: the CLI exercises the same happy-path API that downstream Rust programs
use, with one source of truth for read modes, receipts, and errors.

## Later: Operator And Host Maturity

Themes: **Scalability**, **Operations**, **Octra**

- Add restore checkpoints only if real multi-batch workloads justify them.
- Tune page, dirty-page, and execution budgets only from measured workloads and
  with WASM harness plus devnet proof.
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
