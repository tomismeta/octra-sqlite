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

## Now: Runtime Alignment

Themes: **Octra**, **Architecture**, **Security**, **Developer Experience**,
**Scalability**

- Detect walletless program reads from Octra's `privacy_class: public`
  semantics instead of coupling them to browser or resource policy.
- Expose typed `Database::storage_info()` and confirmed-write receipt effort
  without changing the CLI command language or hiding the raw response.
- Validate current LiteNode behavior for authenticated caller identity,
  self identity, fuel, and public views with source review and live probes.
- Keep OSW1 unchanged while a native-caller prototype receives migration,
  rollback, older-host, and security review.
- Document Circle assets as an optional application payload plane beside
  SQLite, never as remote database pages or a hidden storage abstraction.

Exit: compatibility tests, live protocol evidence, full gates, footprint
measurement, and panel approval. Release scope is chosen only after those
facts are complete.

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
- Adopt native caller authorization only after network support, migration, and
  rollback are proven; keep OSW1 until the replacement is strictly stronger and
  smaller.
- Consider a separate read-only client crate only after the core library
  boundary proves stable and the split removes meaningful dependency weight.

### Selective SQLite API Backlog

Cloudflare Durable Objects is a useful product reference, not a parity target.
Consider bound query parameters, portable backups, protocol-native
checkpoints/PITR, and query-plan ergonomics when Octra and real workloads make
them honest. `Database::storage_info()` and receipt effort establish the small
observability core now. Existing SQL can already run `EXPLAIN QUERY PLAN`; a
dedicated `.eqp` surface must earn its command weight.

Do not add a hidden key-value API, alarms or schedulers, fake cursor semantics,
remote transaction callbacks, cross-Circle SQLite pages, or extensions chosen
for feature-count parity. Circle assets and WebCLI hosting remain separate
Octra capabilities that applications may compose with the database. WebCLI is
a GPL-licensed behavioral reference; octra-sqlite implementations remain
independent and do not copy its code.

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
