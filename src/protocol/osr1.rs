use super::error::{Error, Result};
use base64::{Engine as _, engine::general_purpose};
use serde_json::{Value, json};

pub const TYPED_PREFIX: &str = "OSR1:";
const MAX_ENVELOPE_BYTES: usize = 65_526;
const MAX_ENCODED_PAYLOAD_BYTES: usize = MAX_ENVELOPE_BYTES - TYPED_PREFIX.len();
const MAX_DECODED_PAYLOAD_BYTES: usize = (MAX_ENCODED_PAYLOAD_BYTES / 4) * 3;
const MAX_COLUMNS: usize = 128;
const MAX_ROWS: usize = 512;

pub fn decode_typed_result(encoded: &str) -> Result<Value> {
    if encoded.len() > MAX_ENCODED_PAYLOAD_BYTES {
        return Err(Error::new("typed result exceeds maximum payload size"));
    }
    let raw = general_purpose::STANDARD.decode(encoded)?;
    if raw.len() > MAX_DECODED_PAYLOAD_BYTES {
        return Err(Error::new("typed result exceeds maximum payload size"));
    }
    if raw.len() < 12 || &raw[..4] != b"OSR1" {
        return Err(Error::new("bad typed result magic"));
    }
    let mut offset = 4usize;
    let col_count = read_u32(&raw, &mut offset)? as usize;
    let row_count = read_u32(&raw, &mut offset)? as usize;
    if col_count > MAX_COLUMNS {
        return Err(Error::new("typed result exceeds maximum column count"));
    }
    if row_count > MAX_ROWS {
        return Err(Error::new("typed result exceeds maximum row count"));
    }
    let cell_count = row_count
        .checked_mul(col_count)
        .ok_or_else(|| Error::new("typed result cell count overflow"))?;
    if cell_count > raw.len().saturating_sub(offset) {
        return Err(Error::new("typed result cell count exceeds payload"));
    }
    let mut columns = Vec::new();
    columns
        .try_reserve_exact(col_count)
        .map_err(|_| Error::new("typed result column allocation failed"))?;
    for _ in 0..col_count {
        let bytes = read_bytes(&raw, &mut offset)?;
        columns.push(
            String::from_utf8(bytes.to_vec())
                .map_err(|_| Error::new("typed result column name is not valid UTF-8"))?,
        );
    }
    let mut rows = Vec::new();
    rows.try_reserve_exact(row_count)
        .map_err(|_| Error::new("typed result row allocation failed"))?;
    for _ in 0..row_count {
        let mut row = Vec::new();
        row.try_reserve_exact(col_count)
            .map_err(|_| Error::new("typed result cell allocation failed"))?;
        for _ in 0..col_count {
            row.push(read_cell(&raw, &mut offset)?);
        }
        rows.push(Value::Array(row));
    }
    if offset != raw.len() {
        return Err(Error::new("typed result has trailing bytes"));
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
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::new("typed result offset overflow"))?;
    if end > raw.len() {
        return Err(Error::new("truncated u32"));
    }
    let value = u32::from_be_bytes(raw[*offset..end].try_into().unwrap());
    *offset = end;
    Ok(value)
}

fn read_u64(raw: &[u8], offset: &mut usize) -> Result<u64> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| Error::new("typed result offset overflow"))?;
    if end > raw.len() {
        return Err(Error::new("truncated u64"));
    }
    let value = u64::from_be_bytes(raw[*offset..end].try_into().unwrap());
    *offset = end;
    Ok(value)
}

fn read_bytes<'a>(raw: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    let len = read_u32(raw, offset)? as usize;
    let end = offset
        .checked_add(len)
        .ok_or_else(|| Error::new("typed result offset overflow"))?;
    if end > raw.len() {
        return Err(Error::new("truncated bytes"));
    }
    let bytes = &raw[*offset..end];
    *offset = end;
    Ok(bytes)
}

fn read_cell(raw: &[u8], offset: &mut usize) -> Result<Value> {
    if *offset >= raw.len() {
        return Err(Error::new("truncated cell"));
    }
    let tag = raw[*offset];
    *offset += 1;
    match tag {
        0 => Ok(Value::Null),
        1 => Ok(Value::Number((read_u64(raw, offset)? as i64).into())),
        2 => {
            let bits = read_u64(raw, offset)?;
            let value = f64::from_bits(bits);
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| Error::new("typed result REAL must be finite"))
        }
        3 => Ok(Value::String(
            String::from_utf8(read_bytes(raw, offset)?.to_vec())
                .map_err(|_| Error::new("typed result TEXT is not valid UTF-8"))?,
        )),
        4 => Ok(json!({
            "type": "blob",
            "base64": general_purpose::STANDARD.encode(read_bytes(raw, offset)?),
        })),
        _ => Err(Error::new(format!("unknown typed result cell tag {tag}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_typed_result_cells() {
        let vector: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/osr1/basic.json")).unwrap();
        let encoded = vector["payload_b64"].as_str().unwrap();
        let decoded = decode_typed_result(encoded).unwrap();
        assert_eq!(decoded, vector["expected"]);
        assert_eq!(decoded["columns"][1], "integer");
        assert_eq!(decoded["rows"][0][0], Value::Null);
        assert_eq!(decoded["rows"][0][1], -7);
        assert_eq!(decoded["rows"][0][2], 1000.0);
        assert_eq!(decoded["rows"][0][3], "Ada");
        assert_eq!(decoded["rows"][0][4]["base64"], "QUI=");
    }

    fn encode(raw: &[u8]) -> String {
        general_purpose::STANDARD.encode(raw)
    }

    #[test]
    fn rejects_counts_before_allocating() {
        let mut raw = b"OSR1".to_vec();
        raw.extend_from_slice(&u32::MAX.to_be_bytes());
        raw.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_typed_result(&encode(&raw)).is_err());
    }

    #[test]
    fn rejects_invalid_utf8() {
        let mut raw = b"OSR1".to_vec();
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.extend_from_slice(&0u32.to_be_bytes());
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.push(0xff);
        assert!(decode_typed_result(&encode(&raw)).is_err());
    }

    #[test]
    fn rejects_non_finite_reals() {
        let mut raw = b"OSR1".to_vec();
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.extend_from_slice(&1u32.to_be_bytes());
        raw.push(b'x');
        raw.push(2);
        raw.extend_from_slice(&f64::NAN.to_bits().to_be_bytes());
        assert!(decode_typed_result(&encode(&raw)).is_err());
    }

    #[test]
    fn rejects_oversized_payloads_before_decoding() {
        let encoded = "A".repeat(MAX_ENCODED_PAYLOAD_BYTES + 1);
        assert!(decode_typed_result(&encoded).is_err());
    }
}
