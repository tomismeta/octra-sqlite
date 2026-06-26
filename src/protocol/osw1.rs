use super::error::{ProtocolError, Result};

pub const DOMAIN: &[u8] = b"octra-sqlite.osw1.v1\0";

pub fn frame(db_id: &[u8; 32], sequence: u64, method: &str, sql: &str) -> Result<Vec<u8>> {
    if method.is_empty() || method.len() > 16 {
        return Err(ProtocolError::new("OSW1 method must be 1..16 bytes"));
    }
    if sql.len() > u32::MAX as usize {
        return Err(ProtocolError::new("OSW1 SQL is too large"));
    }
    let mut message = Vec::with_capacity(DOMAIN.len() + 32 + 8 + 2 + method.len() + 4 + sql.len());
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(db_id);
    message.extend_from_slice(&sequence.to_be_bytes());
    message.extend_from_slice(&(method.len() as u16).to_be_bytes());
    message.extend_from_slice(method.as_bytes());
    message.extend_from_slice(&(sql.len() as u32).to_be_bytes());
    message.extend_from_slice(sql.as_bytes());
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn osw1_frame_matches_golden_vector() {
        let digest = Sha256::digest(b"test-db-id");
        let mut db_id = [0u8; 32];
        db_id.copy_from_slice(&digest);
        let message = frame(&db_id, 42, "exec", "select 1;").unwrap();
        assert_eq!(
            hex::encode(message),
            "6f637472612d73716c6974652e6f7377312e7631001fce55ad53f355909514a6a349e2afb2a22cf3bca124d239a9ace46a4108c482000000000000002a0004657865630000000973656c65637420313b"
        );
    }
}
