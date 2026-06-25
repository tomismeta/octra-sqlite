# Devnet Proof

Current public devnet Circle:

```text
oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR
```

Published `v0.1.0` proof:

```text
database: oct://devnet/oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR
circle: oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR
program: version 1, bytes 607496, personalized hash 1725953bb3dfccf9363a1eb27c4a1eeab1f5da53913465dbf4d061ca23e41c8d
bundled wasm hash: 0e28ecc233306fd59539a22209be633fa7e6ca7410c84ce7c940abfcfb372e7a
storage: 2 pages, 8192 bytes
storage adapter: circle_key_value_page_vfs
commit protocol: generation_manifest_v4
auth owner pubkey: 2e2bd06cb8f5584aa0524074bc8b5c99122dc9b43f4e6467f84f406507e49feb
auth database id: 9a97e9c1e926f1ae9122bd9607434e60fd3f00c27751e76a693510eeb269f557
auth sequence: 84
```

Transactions:

```text
circle_create_tx: c6f8bfad04e380bbdc460c8841c4d680edcbcc0a37bac20864fa9e6ccc35381d
initializer_tx: 41616f60bf2047c8374d3d3b0af7ae70960cfc9400aa12d0d4518ec6e9a222dd
non_owner_denied_tx: 9f8f32c600ff1952b256ba40a4326b56b897d198c324de0708d48c4631851e00
```

Verification commands:

```sh
octra-sqlite status oct://devnet/oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR
octra-sqlite oct://devnet/oct236kMS6wDL9r3S5UnwgFJoS79V4kPUpgPVcQNcdnfsPR "select * from person;"
```

Schema:

```text
+-------+--------+------------------------------------------------------------------------+
| type  | name   | sql                                                                    |
+-------+--------+------------------------------------------------------------------------+
| table | person | CREATE TABLE person(first_name text not null, last_name text not null) |
+-------+--------+------------------------------------------------------------------------+
```

Rows:

```text
+------------+-----------+
| first_name | last_name |
+------------+-----------+
| Ada        | Lovelace  |
| Grace      | Hopper    |
+------------+-----------+
```

Owner-only write proof:

```text
owner wallet: octCpJ1SJNi7NBNEjo9DnMfhy4fH3HGDrXN7JL1UhoGYgCB
reader wallet: oct6NBfpTfR9zHdDy5UftWk3FstMYpjp47gDgcTQF5EAxvY
reader query: succeeded
reader write: rejected before SQLite execution
transaction status: rejected
transaction error: circle_call_failed : wasm export returned 403
auth event: auth_not_authorized:auth_denied:signed exec signer is not the database owner
final readback: no Mallory row
```

When `OCTRA_SQLITE_TRACE_SQL_EVENT=1` is set, successful writes also emit:

```text
event: octra.sqlite.sql
value: sql_text:<SQL>
```
