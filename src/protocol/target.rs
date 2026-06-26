use super::error::{ProtocolError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseTarget {
    pub raw: String,
    pub network: String,
    pub circle: String,
    pub rpc: String,
}

pub fn parse_database_target(
    value: &str,
    default_network: Option<&str>,
    default_rpc: Option<&str>,
) -> Result<DatabaseTarget> {
    let default_rpc = default_rpc.unwrap_or_default().to_string();
    if let Some(rest) = value.strip_prefix("oct://") {
        let without_query = rest.split('?').next().unwrap_or(rest);
        let pieces: Vec<&str> = without_query
            .trim_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        let (network, circle) = match pieces.as_slice() {
            [circle] => (
                default_network
                    .ok_or_else(|| {
                        ProtocolError::new("network is required for oct://<circle-id> URIs")
                    })?
                    .to_string(),
                (*circle).to_string(),
            ),
            [network, circle] => ((*network).to_string(), (*circle).to_string()),
            _ => {
                return Err(ProtocolError::new(
                    "oct database URI must look like oct://NETWORK/<circle-id>",
                ));
            }
        };
        if !circle.starts_with("oct") {
            return Err(ProtocolError::new("Circle ID must start with oct"));
        }
        return Ok(DatabaseTarget {
            raw: value.to_string(),
            network,
            circle,
            rpc: default_rpc,
        });
    }
    if value.starts_with("oct") {
        return Ok(DatabaseTarget {
            raw: value.to_string(),
            network: default_network
                .ok_or_else(|| ProtocolError::new("network is required for bare Circle IDs"))?
                .to_string(),
            circle: value.to_string(),
            rpc: default_rpc,
        });
    }
    Err(ProtocolError::new(format!(
        "unknown database {value}; use a database name, Circle ID, or oct://NETWORK/<circle-id>"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_oct_database_target() {
        let target =
            parse_database_target("oct://devnet/octABC", None, Some("http://rpc")).unwrap();
        assert_eq!(target.network, "devnet");
        assert_eq!(target.circle, "octABC");
        assert_eq!(target.rpc, "http://rpc");
    }

    #[test]
    fn bare_circle_uses_default_network() {
        let target = parse_database_target("octABC", Some("devnet"), None).unwrap();
        assert_eq!(target.network, "devnet");
        assert_eq!(target.circle, "octABC");
    }
}
