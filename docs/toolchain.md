# Toolchain

The reference user path does not require a WASM toolchain. The repo ships the
audited Circle WASM at `circle/wasm/octra_sqlite_circle.wasm`; `octra-sqlite new`
and `octra-sqlite deploy` use that artifact by default.

## User Requirements

- Rust/Cargo 1.96 or newer for the CLI. `rustup stable` is recommended; distro
  packages can lag behind the lockfile. Use `cargo +stable ...` or
  `rustup default stable` when a server still defaults to an older toolchain.
  Cargo must support lockfile version 4.
- The stock `sqlite3` CLI only for local export/integrity workflows: `.dump`,
  `.fullschema`, and `verify --integrity`.
- A funded Octra wallet for writes and deploy/update calls on the configured
  network.
- Network access to the configured Octra RPC.

Users do not need Docker, Python, WABT, WASI, a C compiler, or local `sqlite3`
for the cold start path.

When copying source to a server, prefer a Git-native archive so platform
sidecar files are not included:

```sh
git archive --format=tar HEAD | tar -x -C /opt/octra-sqlite/source
```

If you must create a tarball on macOS, disable AppleDouble files:

```sh
COPYFILE_DISABLE=1 tar -cf octra-sqlite.tar .
```

## Builder Requirements

Only builders who modify `circle/source/octra_sqlite_circle.c` need:

- A WASI-capable `clang` that supports `--target=wasm32-wasip1`.
- `wasm-objdump` from WABT for import/export auditing.

Homebrew LLVM builders can set `WASI_SYSROOT` to the `wasi-libc` sysroot:

```sh
WASI_SYSROOT=/opt/homebrew/opt/wasi-libc/share/wasi-sysroot \
CC=/opt/homebrew/opt/llvm/bin/clang \
bash scripts/build-wasm.sh
```

## Current Bundled Build

The current bundled Circle WASM artifact is:

```text
compiler Homebrew clang version 22.1.8
sqlite_sha256 bdff0606a02956191b3b13452120a2e94c33c479a29681e16269cf8e78c9271f
sqlite3_c_upstream_sha3_256 e2b869d3003b33c942b7dd7063604fbc53553e1b66609570df39b9b7b558ee81
code_bytes 633864
code_hash 69b9a16e7d92084b7383cb8564d757784f249a24ca54ce49bb01a158a3a3fa2d
artifact circle/wasm/octra_sqlite_circle.wasm
```

The same values are recorded in `release/octra-sqlite-0.6.4.json` and checked by
`octra-sqlite status`. This SQLite 3.54.1 release has local compatibility and
isolated devnet upgrade/rollback proof. The official amalgamation archive
SHA3-256 is `92281c86e2957f9b9305890f9ac04204caa7b677cc868dc326d942689df28e30`.

TweetNaCl is compiled separately at `-O3` without LTO, with its fixed
field-multiplication loops fully unrolled. SQLite and the contract retain
`-Oz`/LTO. This preserves signature arithmetic and the audited import surface
while reducing OSW1 verification's native instruction cost.

The current rebuild also sizes the verifier's `opened` buffer to
`sizeof(signed_msg)`: TweetNaCl copies the full 64-byte signature plus message
before verification, not merely the recovered message. This capacity fix
preserves SQL limits and OSW1 framing. Maximum-length valid/invalid
`exec`/`exec_trace` and oversized-input cases are covered locally; those
behavior checks are not a substitute for a dedicated memory-safety canary.
The corrected-capacity canary has no recorded runtime result and must not be
reported as passed.

The JavaScript host and the original Wasmtime harness did not meter native
fuel. Passing those paths alone is not proof that a write fits Octra's native
runtime. The reproducible fixture below used 11,873,213 fuel against the
pinned public source snapshot; the original size-only artifact exhausted the
standard 20,000,000 limit. This is not a universal SQL budget guarantee.
The live node reports source `f7637a7eda10b78702ad56844707f7fd308db0e3`;
its exact source archive was unavailable (404), so equivalence to the replay
snapshot has not been established. Live deployment proof is separate.
A subsequent isolated devnet deployment confirmed the signed default/Unicode
fixture at 11,876,379 receipt effort with an explicit 15,000,000 OU budget;
persisted rows, backup integrity, and non-owner sealed-read rejection passed.
This fresh-deployment proof does not cover network upgrade or rollback.
That deployment used the preceding artifact (`6108d174…`), not the current
verifier-capacity correction (`69b9a16e…`). A separate fresh devnet deployment
of the current artifact has now confirmed its personalized program identity,
owner-signed sealed reads, default/Unicode persisted rows, and backup
integrity, plus signed non-owner sealed-read rejection. Normal devnet upgrade
from 3.53.4 to 3.54.1, clean rollback, and re-upgrade preserved the exact rows.
A new post-upgrade write, independent three-row read, and backup integrity
passed; rollback dry-run refused to cross the post-upgrade write.

Private-file replacement relies on Rust 1.96's native `std::fs::rename`
replacement support on Windows, without a delete-first fallback. Source/API
review is not native Windows execution proof; that runtime verification
remains outstanding.

The release manifest JSON is also the source of truth for the small historical
metadata catalog of blessed previous Circle WASM epochs: release range, byte
length, base WASM SHA-256, and GitHub source URL. The crate does not bundle
those old WASM bytes. `upgrade` uses the catalog only to guide operators toward
the correct previous artifact; actual rollback bytes still come from chain
history, local artifacts, or `--previous-wasm`, and are accepted only after the
reconstructed personalized hash matches the live program hash exactly.

## Reproduce The Native Fuel Check

`scripts/check-native-fuel.py` performs an offline, standard-profile native
replay with a public deterministic test seed, not a user wallet. It checks a
signed default/Unicode write, exact persisted rows, signature tampering, and
replay rejection. It requires Python 3, Node, a C compiler, and a separately
built native host. These are optional verification tools, not client/runtime
dependencies. The script fails closed if the unused HFHE service is invoked.

The measured native host was built from public LiteNode snapshot
[`7f9be4489870645cfcc95439638cde40f5da9f50`](https://github.com/octra-labs/lite_node/tree/7f9be4489870645cfcc95439638cde40f5da9f50),
using its locked Wasmi 0.40 dependency. On macOS, after extracting that snapshot:

```sh
export LITENODE=/path/to/lite_node-7f9be4489870645cfcc95439638cde40f5da9f50
native_target="$(mktemp -d)"
cargo rustc --locked --release \
  --manifest-path "$LITENODE/lib/core/circle_wasm_native/Cargo.toml" \
  --target-dir "$native_target" --lib -- \
  --crate-type=cdylib -C link-arg=-Wl,-undefined,dynamic_lookup
python3 scripts/check-native-fuel.py --native-library \
  "$native_target"/release/deps/liboctra_circle_wasm_host-*.dylib
```

Expected for the bundled artifact: `write_effort: 11873213`,
`bad_signature_effort: 8773699`, and both rejection checks pass. The script
records the artifact and native-library hashes. Re-run this check after every
compiler or crypto change before updating release integrity constants.
The build rejects non-Clang compilers and ignored/failed unroll directives;
that alone is not a substitute for measured native execution.

## Optional Rebuild

If you change the contract source, rebuild locally with:

```sh
bash scripts/build-wasm.sh
```

`scripts/build-wasm.sh` still prints the compiler version, SQLite source hash,
WASM byte length, and WASM SHA-256 hash on every build.

Docker and Python are not required for the normal build or client. Python is
used only by the optional native fuel audit above.
