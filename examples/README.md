# Examples

Runnable examples for `octra-sqlite` live here so the top-level README can stay
minimal.

## Organization / Person

Create a new Circle-backed database:

```sh
octra-sqlite new organization
```

Create and seed the table:

```sh
octra-sqlite organization < examples/organization-person.sql
```

Read it back:

```sh
octra-sqlite organization "select rowid, first_name, last_name from person order by rowid;"
```

Add a record:

```sh
octra-sqlite organization "insert into person(first_name,last_name) values ('Alan','Turing');"
```

Update a record:

```sh
octra-sqlite organization "update person set last_name = 'Hamilton' where first_name = 'Katherine';"
```

Delete a record:

```sh
octra-sqlite organization "delete from person where first_name = 'Grace';"
```

Inspect the live Circle-backed database:

```sh
octra-sqlite organization ".tables"
octra-sqlite organization ".schema"
octra-sqlite organization "select rowid, first_name, last_name from person order by rowid;"
octra-sqlite doctor organization
octra-sqlite verify organization
```
