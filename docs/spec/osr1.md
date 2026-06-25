# OSR1 Typed Result Codec

`OSR1` is the stable typed result codec for Octra SQLite query results.

The contract returns typed results as:

```text
OSR1:<base64 payload>
```

The decoded payload is big-endian and has this layout:

```text
u8[4] magic = "OSR1"
u32 column_count
u32 row_count
repeat column_count:
  u32 utf8_column_name_len
  u8[utf8_column_name_len] utf8_column_name
repeat row_count * column_count:
  u8 cell_tag
  cell_payload
```

Cell tags:

```text
0 NULL     no payload
1 INTEGER  i64 two's-complement big-endian
2 REAL     IEEE-754 f64 bits big-endian
3 TEXT     u32 byte length, then UTF-8 bytes
4 BLOB     u32 byte length, then raw bytes
```

Clients must reject:

- bad magic
- truncated integers, lengths, names, or cells
- unknown cell tags
- trailing bytes after the final cell
- invalid base64 in the `OSR1:` envelope

Golden vectors live in `tests/fixtures/osr1/`. Every supported client
implementation should decode those vectors byte-for-byte before claiming OSR1
compatibility.
