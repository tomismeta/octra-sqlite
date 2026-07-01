# Remilia Read API

Tiny read-only HTTP example for the public `remilia` database.

This is an application integration example, not a supported server framework.
It reuses the same Rust client boundary as the CLI and exposes one read route.
Writes are intentionally not implemented.

Run it after `octra-sqlite setup`:

```sh
cargo run --example remilia-read-api
curl http://127.0.0.1:8787/collections/milady
```

Use a different saved database or address:

```sh
OCTRA_SQLITE_DATABASE=remilia \
OCTRA_SQLITE_API_ADDR=127.0.0.1:9000 \
cargo run --example remilia-read-api
```

Route:

```text
GET /collections/<opensea_slug>
```
