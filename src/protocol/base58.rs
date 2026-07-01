pub(crate) fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if bytes.iter().all(|byte| *byte == 0) {
        return "1".repeat(bytes.len());
    }

    let mut digits = vec![0u8];
    for byte in bytes {
        let mut carry = u32::from(*byte);
        for digit in &mut digits {
            let value = u32::from(*digit) * 256 + carry;
            *digit = (value % 58) as u8;
            carry = value / 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }

    let mut encoded = String::new();
    for byte in bytes {
        if *byte == 0 {
            encoded.push('1');
        } else {
            break;
        }
    }
    for digit in digits.iter().rev() {
        encoded.push(ALPHABET[usize::from(*digit)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_vectors() {
        assert_eq!(encode(&[0]), "1");
        assert_eq!(encode(&[0, 0]), "11");
        assert_eq!(encode(&[1]), "2");
        assert_eq!(encode(b"hello world"), "StV1DL6CwTryKyV");
    }
}
