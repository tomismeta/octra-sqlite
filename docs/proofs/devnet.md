# Devnet Proof

Current public devnet example Circle:

```text
oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
```

Published `v0.1.0` proof snapshot:

```text
database: oct://devnet/oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
circle: oct9hZsGed3hihJMv3jBJhPVaKCmyEj2YEnArJVD3WhKTyA
program: version 1, bytes 607496, personalized hash f2adefb06cd7134fefe056ea74132a195e430550c17e3f4b0091bf40abf47213
bundled wasm hash: 0e28ecc233306fd59539a22209be633fa7e6ca7410c84ce7c940abfcfb372e7a
storage: 3 pages, 12288 bytes
storage adapter: circle_key_value_page_vfs
commit protocol: generation_manifest_v4
auth owner pubkey: 2e2bd06cb8f5584aa0524074bc8b5c99122dc9b43f4e6467f84f406507e49feb
auth database id: d1b9fcaa9616b15bb59c1b20d4d84889f73938051fa517f97365df391db3427d
auth sequence: 96
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
auth: owner write intent
owner wallet can write: yes
other authenticated wallets can read through the signed view path
other authenticated wallets cannot write unless they hold the owner key
```

When `OCTRA_SQLITE_TRACE_SQL_EVENT=1` is set, successful writes also emit:

```text
event: octra.sqlite.sql
value: sql_text:<SQL>
```
