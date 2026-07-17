//! Errors raised by transport-independent protocol codecs.

use std::fmt;

/// Result alias for protocol encoding and decoding.
pub type Result<T> = std::result::Result<T, Error>;

/// Error returned by transport-independent protocol code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    /// Construct a protocol error with human-readable context.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Self::new(error.to_string())
    }
}
