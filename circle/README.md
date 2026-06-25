# Circle Program

This directory contains the Octra Circle program.

- `source/`: C source for the `wasm_v1` Circle program that embeds SQLite.
- `wasm/`: prebuilt deployable WASM used by `octra-sqlite new`.

Normal users use the bundled WASM. Builders who change `source/` can rebuild
with:

```sh
bash scripts/build-wasm.sh
```
