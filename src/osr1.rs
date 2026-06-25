use anyhow::{bail, Result};
use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};

pub(crate) const TYPED_PREFIX: &str = "OSR1:";

pub(crate) fn decode_typed_result(encoded: &str) -> Result<Value> {
    let raw = general_purpose::STANDARD.decode(encoded)?;
    if raw.len() < 12 || &raw[..4] != b"OSR1" {
        bail!("bad typed result magic");
    }
    let mut offset = 4usize;
    let col_count = read_u32(&raw, &mut offset)? as usize;
    let row_count = read_u32(&raw, &mut offset)? as usize;
    let mut columns = Vec::with_capacity(col_count);
    for _ in 0..col_count {
        let bytes = read_bytes(&raw, &mut offset)?;
        columns.push(String::from_utf8_lossy(bytes).to_string());
    }
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let mut row = Vec::with_capacity(col_count);
        for _ in 0..col_count {
            row.push(read_cell(&raw, &mut offset)?);
        }
        rows.push(Value::Array(row));
    }
    if offset != raw.len() {
        bail!("typed result has trailing bytes");
    }
    Ok(json!({
        "ok": true,
        "codec": "octra_sqlite_result_v1",
        "columns": columns,
        "rows": rows,
        "row_count": row_count,
    }))
}

fn read_u32(raw: &[u8], offset: &mut usize) -> Result<u32> {
    if *offset + 4 > raw.len() {
        bail!("truncated u32");
    }
    let value = u32::from_be_bytes(raw[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    Ok(value)
}

fn read_u64(raw: &[u8], offset: &mut usize) -> Result<u64> {
    if *offset + 8 > raw.len() {
        bail!("truncated u64");
    }
    let value = u64::from_be_bytes(raw[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(value)
}

fn read_bytes<'a>(raw: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    let len = read_u32(raw, offset)? as usize;
    if *offset + len > raw.len() {
        bail!("truncated bytes");
    }
    let bytes = &raw[*offset..*offset + len];
    *offset += len;
    Ok(bytes)
}

fn read_cell(raw: &[u8], offset: &mut usize) -> Result<Value> {
    if *offset >= raw.len() {
        bail!("truncated cell");
    }
    let tag = raw[*offset];
    *offset += 1;
    match tag {
        0 => Ok(Value::Null),
        1 => Ok(Value::Number((read_u64(raw, offset)? as i64).into())),
        2 => {
            let bits = read_u64(raw, offset)?;
            let value = f64::from_bits(bits);
            Ok(serde_json::Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null))
        }
        3 => Ok(Value::String(
            String::from_utf8_lossy(read_bytes(raw, offset)?).to_string(),
        )),
        4 => Ok(json!({
            "type": "blob",
            "base64": general_purpose::STANDARD.encode(read_bytes(raw, offset)?),
        })),
        _ => bail!("unknown typed result cell tag {tag}"),
    }
}
