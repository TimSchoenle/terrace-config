//! One module per fuzz target, each exposing a single `check(&str)`.
//!
//! An oracle panics when the loader breaks a rule and returns otherwise. That is exactly the
//! contract `libfuzzer_sys::fuzz_target!` wants, and exactly the contract `#[test]` wants, which
//! is what lets the seed corpus double as a regression suite.

pub mod env_load;
pub mod schema;
pub mod secrets_dir;
pub mod toml_layers;
