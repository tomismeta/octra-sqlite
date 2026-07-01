pub mod client;
pub mod protocol;

#[cfg(feature = "cli")]
#[path = "cli/mod.rs"]
mod cli_impl;

#[cfg(feature = "cli")]
pub use cli_impl::{run as run_cli, run_with_exit_code as run_cli_with_exit_code};

#[cfg(feature = "cli")]
pub mod cli {
    pub use crate::run_cli as run;
}
