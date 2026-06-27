# Devnet Proof

Current public devnet example Circle:

```text
oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
```

Published `v0.2.1` proof snapshot:

```text
database: oct://devnet/oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
circle: oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
program: version 4, bytes 607800, personalized hash 37e377095b33437ad3ebbda0cd67766e005cfe0b82967d6abdcfabb5427f2f46
bundled wasm hash: 29861d38ddad25f5cd2b153bb70cfa6b1b54ebd2532fe931fa1f012b7f39ca9c
storage: 4 pages, 16384 bytes
storage adapter: circle_key_value_page_vfs
commit protocol: generation_manifest_v4
auth owner pubkey: 2e2bd06cb8f5584aa0524074bc8b5c99122dc9b43f4e6467f84f406507e49feb
auth database id: d1b9fcaa9616b15bb59c1b20d4d84889f73938051fa517f97365df391db3427d
auth sequence: 107
circle create tx: 1a1817d310278a3814d5446b1869a098ce4055be2421aa31694d3bb4a51312cb
circle create: https://devnet.octrascan.io/tx.html?hash=1a1817d310278a3814d5446b1869a098ce4055be2421aa31694d3bb4a51312cb
initializer tx: da10d2af72c3b4be2053fe47cc65b9e4073bd31f52fcdc85451a0cefbbbdbf43
initializer: https://devnet.octrascan.io/tx.html?hash=da10d2af72c3b4be2053fe47cc65b9e4073bd31f52fcdc85451a0cefbbbdbf43
program update tx: 98ce68ef74d9c4ef50bdf0654201d67cd74822da0231f6ce4cd5c30e1f0311f1
program update: https://devnet.octrascan.io/tx.html?hash=98ce68ef74d9c4ef50bdf0654201d67cd74822da0231f6ce4cd5c30e1f0311f1
non-owner denied tx: 08ea0b734025ed87d5694af6f2800bcec55411815e6b7724a325159ac6b6d3b3
non-owner denied: https://devnet.octrascan.io/tx.html?hash=08ea0b734025ed87d5694af6f2800bcec55411815e6b7724a325159ac6b6d3b3
```

Verification commands:

```sh
octra-sqlite status oct://devnet/oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
octra-sqlite oct://devnet/oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA "select name, launched_month, relationship, chain from collection order by launched_month, name;"
```

Schema:

```sql
create table collection(
  name text primary key,
  opensea_slug text not null,
  chain text not null,
  relationship text not null,
  launched_month text not null,
  date_precision text not null
);
```

Rows:

```text
+-------------------------+----------------+------------------+----------+
| name                    | launched_month | relationship     | chain    |
+-------------------------+----------------+------------------+----------+
| Milady Maker            | 2021-08-01     | Remilia          | Ethereum |
| Banners NFT             | 2022-07-01     | Remilia          | Ethereum |
| Redacted Remilio Babies | 2022-08-01     | Remilia          | Ethereum |
| SchizoPosters           | 2023-03-01     | Remilia adjacent | Ethereum |
| Bonkler                 | 2023-04-01     | Remilia          | Ethereum |
| YAYO NFT                | 2023-05-01     | Remilia adjacent | Ethereum |
| Milady Fumo Babies      | 2023-12-01     | Remilia adjacent | Ethereum |
| Yumemono                | 2025-03-01     | Remilia adjacent | Ethereum |
| World Computer Netizens | 2026-02-01     | Remilia adjacent | MegaETH  |
| moemoe LLC              | 2026-02-01     | Remilia adjacent | Ethereum |
+-------------------------+----------------+------------------+----------+
```

Owner-only write policy:

```text
owner wallet: octCpJ1SJNi7NBNEjo9DnMfhy4fH3HGDrXN7JL1UhoGYgCB
auth: OSW1 owner write intent
owner wallet can write: yes
other authenticated wallets can read through the signed view path
other authenticated wallets cannot write unless they hold the owner key
non-owner rejection tx: 08ea0b734025ed87d5694af6f2800bcec55411815e6b7724a325159ac6b6d3b3
non-owner wallet: oct6NBfpTfR9zHdDy5UftWk3FstMYpjp47gDgcTQF5EAxvY
rejection reason: wasm export returned 403
```

When `OCTRA_SQLITE_TRACE_SQL_EVENT=1` is set, successful writes also emit:

```text
event: octra.sqlite.sql
value: sql_text:<SQL>
```
