use serde::Serialize;

#[derive(Clone, PartialEq, Serialize)]
pub struct Tx {
    pub from: String,
    pub to_: String,
    pub amount: String,
    pub nonce: i64,
    pub ou: String,
    pub timestamp: f64,
    pub op_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub encrypted_data: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    pub signature: String,
    pub public_key: String,
}

pub fn canonical_tx(tx: &Tx) -> String {
    let mut s = String::new();
    s.push_str("{\"from\":\"");
    s.push_str(&escape_json_string(&tx.from));
    s.push_str("\",\"to_\":\"");
    s.push_str(&escape_json_string(&tx.to_));
    s.push_str("\",\"amount\":\"");
    s.push_str(&escape_json_string(&tx.amount));
    s.push_str("\",\"nonce\":");
    s.push_str(&tx.nonce.to_string());
    s.push_str(",\"ou\":\"");
    s.push_str(&escape_json_string(&tx.ou));
    s.push_str("\",\"timestamp\":");
    s.push_str(&canonical_timestamp(tx.timestamp));
    s.push_str(",\"op_type\":\"");
    s.push_str(&escape_json_string(&tx.op_type));
    s.push('"');
    if !tx.encrypted_data.is_empty() {
        s.push_str(",\"encrypted_data\":\"");
        s.push_str(&escape_json_string(&tx.encrypted_data));
        s.push('"');
    }
    if !tx.message.is_empty() {
        s.push_str(",\"message\":\"");
        s.push_str(&escape_json_string(&tx.message));
        s.push('"');
    }
    s.push('}');
    s
}

fn canonical_timestamp(value: f64) -> String {
    let mut text = serde_json::to_string(&value).unwrap_or_else(|_| format!("{value}"));
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

fn escape_json_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\u{0008}', "\\b")
        .replace('\u{000c}', "\\f")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_tx_omits_empty_optional_fields() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "200000".into(),
            timestamp: 1.0,
            op_type: "deploy_circle".into(),
            encrypted_data: String::new(),
            message: "{\"runtime\":\"wasm_v1\",\"code_b64\":\"QUJD\"}".into(),
            signature: String::new(),
            public_key: String::new(),
        };
        let canonical = canonical_tx(&tx);
        assert!(!canonical.contains("encrypted_data"));
        assert!(canonical.contains("\"op_type\":\"deploy_circle\""));
        assert!(canonical.contains("\\\"runtime\\\":\\\"wasm_v1\\\""));
    }

    #[test]
    fn wire_tx_omits_empty_optional_fields() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "200000".into(),
            timestamp: 1.0,
            op_type: "deploy_circle".into(),
            encrypted_data: String::new(),
            message: "{\"runtime\":\"wasm_v1\",\"code_b64\":\"QUJD\"}".into(),
            signature: "sig".into(),
            public_key: "pub".into(),
        };
        let wire = serde_json::to_value(tx).unwrap();
        assert!(wire.get("encrypted_data").is_none());
        assert!(wire.get("message").is_some());
    }

    #[test]
    fn canonical_tx_matches_field_order() {
        let tx = Tx {
            from: "octA".into(),
            to_: "octB".into(),
            amount: "0".into(),
            nonce: 7,
            ou: "1000".into(),
            timestamp: 1.0,
            op_type: "circle_call".into(),
            encrypted_data: "exec".into(),
            message: "[\"select 1;\"]".into(),
            signature: String::new(),
            public_key: String::new(),
        };
        assert_eq!(
            canonical_tx(&tx),
            "{\"from\":\"octA\",\"to_\":\"octB\",\"amount\":\"0\",\"nonce\":7,\"ou\":\"1000\",\"timestamp\":1.0,\"op_type\":\"circle_call\",\"encrypted_data\":\"exec\",\"message\":\"[\\\"select 1;\\\"]\"}"
        );
    }
}
