use std::fmt;

/// Result alias for octra-sqlite client operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error returned by octra-sqlite client operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    code: Option<String>,
    message: String,
}

/// Stable category for a client error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Authentication or owner-authorization failure.
    Authorization,
    /// Local configuration or option failure.
    Config,
    /// Invalid or inconsistent response data.
    Decode,
    /// Local filesystem or stream failure.
    Io,
    /// OSR1, OSW1, target, or transaction protocol failure.
    Protocol,
    /// Submitted transaction receipt reported failure.
    Receipt,
    /// Octra RPC rejected or could not satisfy a request.
    Rpc,
    /// Receipt or readiness wait exceeded its deadline.
    Timeout,
    /// HTTP or custom transport failure.
    Transport,
    /// Wallet loading, key validation, or signing failure.
    Wallet,
    /// Error without a narrower stable category.
    Other,
}

impl Error {
    /// Construct an uncategorized client error.
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(ErrorKind::Other, message)
    }

    /// Construct an error with a stable broad category.
    pub fn with_kind(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: None,
            message: message.into(),
        }
    }

    pub(crate) fn with_code(
        kind: ErrorKind,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code: Some(code.into()),
            message: message.into(),
        }
    }

    pub(crate) fn with_context(mut self, context: impl AsRef<str>) -> Self {
        self.message = format!("{}; {}", self.message, context.as_ref());
        self
    }

    /// Return the stable broad category for this error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Precise source error code when the RPC, Circle, or receipt supplied one.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
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
impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        Self::with_kind(ErrorKind::Transport, error.to_string())
    }
}
