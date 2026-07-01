use super::error::{ProtocolError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    #[default]
    Auto,
    Sealed,
    Public,
}

impl ReadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadMode::Auto => "auto",
            ReadMode::Sealed => "sealed",
            ReadMode::Public => "public",
        }
    }

    pub fn allows_unsigned_read(self) -> bool {
        matches!(self, ReadMode::Auto | ReadMode::Public)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseTarget {
    pub raw: String,
    pub network: String,
    pub circle: String,
    pub rpc: String,
    pub read_mode: ReadMode,
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
        let read_mode = read_mode_from_query(rest)?;
        return Ok(DatabaseTarget {
            raw: value.to_string(),
            network,
            circle,
            rpc: default_rpc,
            read_mode,
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
            read_mode: ReadMode::Auto,
        });
    }
    Err(ProtocolError::new(format!(
        "unknown database {value}; use a database name, Circle ID, or oct://NETWORK/<circle-id>"
    )))
}

fn read_mode_from_query(rest: &str) -> Result<ReadMode> {
    let Some((_, query)) = rest.split_once('?') else {
        return Ok(ReadMode::Auto);
    };
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "read_mode" {
            return match value {
                "auto" => Ok(ReadMode::Auto),
                "sealed" => Ok(ReadMode::Sealed),
                "public" => Ok(ReadMode::Public),
                _ => Err(ProtocolError::new(
                    "read_mode must be auto, sealed, or public",
                )),
            };
        }
    }
    Ok(ReadMode::Auto)
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
        assert_eq!(target.read_mode, ReadMode::Auto);
    }

    #[test]
    fn bare_circle_uses_default_network() {
        let target = parse_database_target("octABC", Some("devnet"), None).unwrap();
        assert_eq!(target.network, "devnet");
        assert_eq!(target.circle, "octABC");
        assert_eq!(target.read_mode, ReadMode::Auto);
    }

    #[test]
    fn parses_read_mode_query() {
        let target =
            parse_database_target("oct://devnet/octABC?read_mode=public", None, None).unwrap();
        assert_eq!(target.read_mode, ReadMode::Public);
    }
}
