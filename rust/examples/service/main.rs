//! A small HTTP-shaped service, wired to load its configuration through [`terrace_config`].
//!
//! Unlike `config-schema`, booting is the primary job here — it is the other half of the crate:
//! the layered loader a service actually boots with, exercised the way a real deployment
//! exercises it. `--contract` is a second, optional job the same binary can do once `schema` is
//! enabled, exactly the debug flag a real service might expose beside its normal boot path.
//!
//! ```text
//! cargo run --example service
//! ORDERS_PORT=9090 cargo run --example service
//! ORDERS_LOG_LEVEL=debug cargo run --example service
//! ORDERS_DATABASE__URL=postgres://prod/orders cargo run --example service
//! cargo run --example service --features schema -- --contract
//! ```
//!
//! A real deployment never sets `ORDERS_DATABASE__URL` in plain environment text. It mounts a
//! `Secret` volume and sets `ORDERS_SECRETS_DIR` at that path, or points
//! `ORDERS_DATABASE__URL_FILE` at one file — `tests/example_service.rs` boots this exact
//! configuration both ways.
//!
//! That test file includes `config` below with `#[path]` rather than redeclaring its types, so
//! this module is the one place `Config`'s shape is written and the one place a regression in it
//! can hide. `--contract` renders the same types from the same file, which is what keeps
//! `examples/service/contract.json` — regenerated and diffed by CI, never hand-edited — from
//! being a description that could quietly stop matching what `config::load` actually does.

mod config;

use std::process::ExitCode;

use secrecy::ExposeSecret as _;

fn main() -> ExitCode {
    #[cfg(feature = "schema")]
    if std::env::args().nth(1).as_deref() == Some("--contract") {
        return match config::contract() {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("failed to render contract: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let config = match config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to load configuration: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("listening on {}:{}", config.bind_addr, config.port);
    println!("log level: {:?}", config.log_level);
    println!(
        "database pool: {} connections",
        config.database.max_connections
    );
    // A real service passes this straight to its connection pool and never prints it. Exposed
    // here only to make the point in the terminal: the value loaded, and it is not the compiled
    // default once an operator overrides it.
    let url = config.database.url.expose_secret();
    println!(
        "database: {}",
        if url.contains("orders_dev") {
            "local development default"
        } else {
            "overridden"
        }
    );

    ExitCode::SUCCESS
}
