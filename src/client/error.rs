use std::fmt;

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    kind: ClientErrorKind,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClientErrorKind {
    Authorization,
    Config,
    Decode,
    Io,
    Protocol,
    Receipt,
    Rpc,
    Timeout,
    Transport,
    Wallet,
    Other,
}

impl ClientError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(ClientErrorKind::Other, message)
    }

    pub fn with_kind(kind: ClientErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ClientErrorKind {
        self.kind
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

impl From<crate::protocol::error::ProtocolError> for ClientError {
    fn from(error: crate::protocol::error::ProtocolError) -> Self {
        Self::with_kind(ClientErrorKind::Protocol, error.to_string())
    }
}

impl From<base64::DecodeError> for ClientError {
    fn from(error: base64::DecodeError) -> Self {
        Self::with_kind(ClientErrorKind::Decode, error.to_string())
    }
}

impl From<hex::FromHexError> for ClientError {
    fn from(error: hex::FromHexError) -> Self {
        Self::with_kind(ClientErrorKind::Decode, error.to_string())
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        Self::with_kind(ClientErrorKind::Decode, error.to_string())
    }
}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::with_kind(ClientErrorKind::Io, error.to_string())
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(error: reqwest::Error) -> Self {
        Self::with_kind(ClientErrorKind::Transport, error.to_string())
    }
}
