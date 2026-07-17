//! Transport-independent formats used between the client and Circle program.
//!
//! Application code normally uses the typed crate-root API. These modules are
//! public for adapters that need to reproduce OSR1 results, OSW1 owner-write
//! intent, `oct://` targets, or canonical Octra transactions exactly.

#[cfg(feature = "cli")]
pub(crate) mod base58;
pub mod error;
pub mod osr1;
pub mod osw1;
pub mod target;
pub mod tx;
