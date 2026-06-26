# Examples

Runnable examples for `octra-sqlite` live here so the top-level README can stay
minimal.

## Remilia Collections

Create a new Circle-backed database and seed it with the example SQL:

```sh
octra-sqlite new remilia < examples/remilia-collections.sql
```

Read it back:

```sh
octra-sqlite remilia "select name, launched_month, relationship, chain from collection order by launched_month, name;"
```

Add a collection:

```sh
octra-sqlite remilia "insert into collection(name,opensea_slug,chain,relationship,launched_month,date_precision) values ('Example Collection','example-collection','Ethereum','Remilia adjacent','2026-06-01','month');"
```

Update a collection:

```sh
octra-sqlite remilia "update collection set relationship = 'Remilia' where name = 'Example Collection';"
```

Delete a collection:

```sh
octra-sqlite remilia "delete from collection where name = 'Example Collection';"
```

Inspect the live Circle-backed database:

```sh
octra-sqlite remilia ".tables"
octra-sqlite remilia ".schema collection"
octra-sqlite status remilia
octra-sqlite verify remilia
```
