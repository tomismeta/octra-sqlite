# Devnet Proof

Current public devnet portability proof Circle:

```text
octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
```

Published `v0.3.1` proof snapshot:

```text
database: oct://devnet/octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
circle: octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
circle url: https://devnet.octrascan.io/address.html?addr=octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
program: version 2, bytes 609404, personalized hash 2e8fae91e2372293f4554fed164ff31c07df3e423bd36eba31e1b8e40a760e9f
bundled wasm hash: 39635962bffb470daced92396ee27e206e6b3ea000b4ec7a954d3bcd05ba662b
storage: 3 pages, 12288 bytes
backup: 12288 bytes, generation 1, sha256 5134da2b7c0e03c99a139e165469f35d824f0ede7a5a4f3433625b0d1021cb42
backup integrity: sqlite3 pragma integrity_check = ok
auth owner pubkey: 2e2bd06cb8f5584aa0524074bc8b5c99122dc9b43f4e6467f84f406507e49feb
auth database id: 2b2ce4c282bba87be3a113a571a334129bd49d329a247ca170dbd8bf502c8682
circle create tx: 318ca1a98df95bedb87d1042d0555eecc94660bbf828813a148bf11393ed73ed
circle create: https://devnet.octrascan.io/tx.html?hash=318ca1a98df95bedb87d1042d0555eecc94660bbf828813a148bf11393ed73ed
initializer tx: 971b50d434226e7892bb3e5f926a1dced9dd35df1df4bfe4266351116c3bc5f0
initializer: https://devnet.octrascan.io/tx.html?hash=971b50d434226e7892bb3e5f926a1dced9dd35df1df4bfe4266351116c3bc5f0
program update tx: 3d1a3e308f2a29b4c7748745b269841b4025ebb777fe51629e066139c6446fd7
program update: https://devnet.octrascan.io/tx.html?hash=3d1a3e308f2a29b4c7748745b269841b4025ebb777fe51629e066139c6446fd7
non-owner denied tx: 567559d31f4c8fa3a0f5eff42f8ea8b417ee2269ab1a0b5c404241de5ff6b6a1
non-owner denied: https://devnet.octrascan.io/tx.html?hash=567559d31f4c8fa3a0f5eff42f8ea8b417ee2269ab1a0b5c404241de5ff6b6a1
full restore circle: octqdTL8vFxiLmJw7JbUYoqiJNaTDTmNT4pjWHWdLUjoRWq
full restore create tx: a7483e37ab12bb1f74d4c43e9f7659accca996638982c54c58a0d6d43eeb1d73
full restore create: https://devnet.octrascan.io/tx.html?hash=a7483e37ab12bb1f74d4c43e9f7659accca996638982c54c58a0d6d43eeb1d73
full restore initializer tx: 708b0efcdad02e7efbd88a4ac2d0904cd42a0025b35077685241261335ac4c50
full restore initializer: https://devnet.octrascan.io/tx.html?hash=708b0efcdad02e7efbd88a4ac2d0904cd42a0025b35077685241261335ac4c50
full restore trigger write tx: 7e8d9e04241cea6d0160dc7adf32b543b14563c289cd1717d8a83edf0d5a60b3
full restore trigger write: https://devnet.octrascan.io/tx.html?hash=7e8d9e04241cea6d0160dc7adf32b543b14563c289cd1717d8a83edf0d5a60b3
table restore circle: octZG9aUMQ3Ho2pnw4YFmaMyyyKxUs1xkW4ArVs3jBb9txZ
table restore create tx: 4c7c106e6de05051baf030acc409b4ab8b02b32622475f914c8a235f5a72ad0a
table restore create: https://devnet.octrascan.io/tx.html?hash=4c7c106e6de05051baf030acc409b4ab8b02b32622475f914c8a235f5a72ad0a
csv import circle: octHrZkieUhhJbr8Acxkk7PV3C4U2W89ceVCV5jni6LZRJ3
csv import tx: 3e315b5b7f0847fa360d14d4103a6d28da51fe57788324a55f8d983965f8bac6
csv import: https://devnet.octrascan.io/tx.html?hash=3e315b5b7f0847fa360d14d4103a6d28da51fe57788324a55f8d983965f8bac6
```

Verification commands:

```sh
octra-sqlite status oct://devnet/octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY
octra-sqlite verify oct://devnet/octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY --integrity
octra-sqlite oct://devnet/octE4pHEmLd47zRdC7LRDGjQWPJPJ5zbmNcL1ixfn7aCzSY ".backup main proof.sqlite"
sqlite3 proof.sqlite "pragma integrity_check;"
```

`v0.2.1` public Remilia example Circle:

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
