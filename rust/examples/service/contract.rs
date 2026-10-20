//! The generator for `examples/service/`'s contract, in the one-call form.
//!
//! This is the whole of what a service writes to publish a contract: its own configuration module,
//! the loader it boots with, and the identity of its image. Everything else — the `--format`
//! vocabulary, the labels, the Dockerfile block, the stamping — is
//! [`terrace_config::schema::cli`]. `examples/service/Dockerfile` runs it in a build stage:
//!
//! ```text
//! cargo run --example service-contract --features schema-cli -- --format contract
//! ```
//!
//! Its output is `examples/service/contract.json` byte for byte, which `tests/example_service.rs`
//! holds against the same types.

#[expect(
    dead_code,
    reason = "the module is the service's own and is shared by path: this generator calls only the \
              loader and the types, and leaves booting to `main.rs`"
)]
#[path = "config.rs"]
mod config;

use std::process::ExitCode;

use terrace_config::schema::App;

fn main() -> ExitCode {
    let app = App::new("orders-service").version(config::SERVICE_VERSION);
    terrace_config::schema::cli::main::<config::Config>(&config::loader(), app)
}
