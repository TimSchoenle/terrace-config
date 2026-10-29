//! One module per subcommand group. Each handler parses nothing and returns the exit code.

pub(crate) mod adopt;
pub(crate) mod config;
pub(crate) mod diff;
pub(crate) mod document;
pub(crate) mod explain;
pub(crate) mod gates;
pub(crate) mod image;
pub(crate) mod init;
pub(crate) mod secrets;
pub(crate) mod sync;
