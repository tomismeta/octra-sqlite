# Examples

Runnable examples for `octra-sqlite` live here so the top-level README can stay
minimal.

## Artists

The smallest example mirrors the top-level README.

```sh
octra-sqlite new art < examples/artists.sql
octra-sqlite status art --ready
octra-sqlite art "select * from artist order by name;"
octra-sqlite art ".schema artist"
```

Machine-readable creation:

```sh
octra-sqlite new art --schema examples/artists.sql --manifest art.octra-sqlite.json --json
octra-sqlite status art --ready --json
```

## Remilia Collections

The default config does not preload this database. This walkthrough creates your
own `my_collections` database so the example is explicit and safe to edit.

Create a new Circle-backed database and seed it with the example SQL:

```sh
octra-sqlite new my_collections < examples/remilia-collections.sql
```

Read it back:

```sh
octra-sqlite my_collections "select name, launched_month, relationship, chain from collection order by launched_month, name;"
```

Add a collection:

```sh
octra-sqlite my_collections "insert into collection(name,opensea_slug,chain,relationship,launched_month,date_precision) values ('Example Collection','example-collection','Ethereum','Remilia adjacent','2026-06-01','month');"
```

Update a collection:

```sh
octra-sqlite my_collections "update collection set relationship = 'Remilia' where name = 'Example Collection';"
```

Delete a collection:

```sh
octra-sqlite my_collections "delete from collection where name = 'Example Collection';"
```

Inspect the live Circle-backed database:

```sh
octra-sqlite my_collections ".tables"
octra-sqlite my_collections ".schema collection"
octra-sqlite status my_collections
octra-sqlite verify my_collections
```

## Remilia Read API

[`remilia-read-api/`](./remilia-read-api/) is a tiny read-only Rust HTTP
example. It shows how an application can build on the same client boundary as
the CLI without making this repo a web framework.

```sh
cargo run --example remilia-read-api
curl http://127.0.0.1:8787/collections/milady
```
