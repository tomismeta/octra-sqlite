use std::fmt;

/// Result alias for octra-sqlite client operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error returned by octra-sqlite client operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

/// Stable category for a client error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
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

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(ErrorKind::Other, message)
    }

    pub fn with_kind(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<crate::protocol::error::Error> for Error {
    fn from(error: crate::protocol::error::Error) -> Self {
        Self::with_kind(ErrorKind::Protocol, error.to_string())
    }
}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Self::with_kind(ErrorKind::Decode, error.to_string())
    }
}

impl From<hex::FromHexError> for Error {
    fn from(error: hex::FromHexError) -> Self {
        Self::with_kind(ErrorKind::Decode, error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::with_kind(ErrorKind::Decode, error.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::with_kind(ErrorKind::Io, error.to_string())
    }
}

#[cfg(feature = "http")]
impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::with_kind(ErrorKind::Transport, error.to_string())
    }
}
