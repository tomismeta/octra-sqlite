# Examples

Runnable examples for `octra-sqlite` live here so the top-level README can stay
minimal.

## Remilia Collections

The default config includes the public `remilia` example database. This
walkthrough creates your own `my_collections` database so the example is safe to
edit.

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
