# Remilia Read API

Tiny read-only HTTP example for a public-read Remilia collections database.

This is an application integration example, not a supported server framework.
It reuses the same Rust client boundary as the CLI and exposes one read route.
Writes are intentionally not implemented.

Create a public-read Remilia database:

```sh
octra-sqlite new remilia_public --read-mode public --schema examples/remilia-collections.sql
octra-sqlite status remilia_public --ready
```

Run the API against the saved database:

```sh
OCTRA_SQLITE_DATABASE=remilia_public \
cargo run --example remilia-read-api
curl http://127.0.0.1:8787/collections/milady
```

Use a different saved database, public `oct://` URI, or address:

```sh
OCTRA_SQLITE_DATABASE='oct://devnet/oct...?read_mode=public' \
OCTRA_SQLITE_API_ADDR=127.0.0.1:9000 \
cargo run --example remilia-read-api
```

Route:

```text
GET /collections/<opensea_slug>
```
