//! The `service` example's own configuration, exercised through
//! [`terrace_config::testing::Harness`].
//!
//! This shares `examples/service/config.rs` with `#[path]` rather than redeclaring its `Config`
//! type here. A test file that keeps its own copy of a config type can drift from what the
//! example actually loads and stay green while the example itself has stopped compiling, or
//! worse, still compiles but no longer matches what this file thinks it asserts. Sharing the
//! module makes that impossible: a change to the example's shape that this file does not know
//! about is a compile error here, and a behavioural change the example makes is a test failure
//! here — either way, the example cannot rot silently the way `config-schema` (a `schema`-only
//! demonstration, checked by `cargo clippy --all-targets` alone) can.

#![cfg(all(feature = "loader", feature = "testing"))]

use secrecy::ExposeSecret as _;
use terrace_config::testing::Harness;

#[path = "../examples/service/config.rs"]
mod config;

/// A sandbox over the example's own loader, with the process environment cleared: several
/// assertions below are about what the environment does *not* contain, and a developer machine
/// with `ORDERS_PORT` already exported would otherwise decide the outcome.
fn harness() -> Harness {
    Harness::over(config::loader())
}

#[test]
fn compiled_defaults_are_enough_to_boot() {
    harness().run(|jail| {
        let config: config::Config = jail.load()?;

        assert_eq!(config.bind_addr, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.log_level, config::LogLevel::Info);
        assert_eq!(config.database.max_connections, 10);
        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://localhost/orders_dev"
        );
        Ok(())
    });
}

#[test]
fn a_toml_file_overrides_the_listener_and_the_database() {
    harness().run(|jail| {
        jail.config(
            r#"
            bind_addr = "0.0.0.0"
            port = 9090

            [database]
            url = "postgres://file/orders"
            max_connections = 25
            "#,
        )?;

        let config: config::Config = jail.load()?;

        assert_eq!(config.bind_addr, "0.0.0.0");
        assert_eq!(config.port, 9090);
        assert_eq!(config.database.max_connections, 25);
        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://file/orders"
        );
        Ok(())
    });
}

#[test]
fn an_environment_variable_outranks_the_toml_file() {
    harness().run(|jail| {
        jail.config("port = 9090\n")?;
        jail.env_key("port", "7070");

        let config: config::Config = jail.load()?;

        assert_eq!(config.port, 7070);
        Ok(())
    });
}

#[test]
fn the_database_url_can_come_from_a_mounted_secret() {
    harness().run(|jail| {
        // A trailing newline, which is what a Kubernetes `Secret` key and every editor produce.
        jail.secret("database__url", "postgres://secret/orders\n")?;

        let config: config::Config = jail.load()?;

        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://secret/orders"
        );
        Ok(())
    });
}

#[test]
fn the_database_url_can_come_from_file_indirection() {
    harness().run(|jail| {
        jail.indirection("database.url", "postgres://indirect/orders")?;

        let config: config::Config = jail.load()?;

        assert_eq!(
            config.database.url.expose_secret(),
            "postgres://indirect/orders"
        );
        Ok(())
    });
}

#[test]
fn an_unrecognised_log_level_is_refused_rather_than_silently_defaulted() {
    harness().run(|jail| {
        jail.env_key("log_level", "verbose");

        jail.load::<config::Config>()
            .expect_err("`verbose` is not one of the four compiled spellings");
        Ok(())
    });
}

#[test]
fn the_database_url_supplied_by_both_a_secret_and_the_environment_is_refused() {
    harness().run(|jail| {
        jail.secret("database__url", "postgres://secret/orders")?;
        jail.env_key("database.url", "postgres://environment/orders");

        let error = jail
            .load::<config::Config>()
            .expect_err("the default shadow policy rejects a key two mechanisms supplied");
        assert!(error.to_string().contains("database"), "{error}");
        Ok(())
    });
}

/// `examples/service/contract.json` is a checked-in file, not a build artefact: nothing
/// regenerates it automatically, so nothing but this test stops it from drifting out from under
/// `config::Config`'s actual shape the moment a field changes here without `--contract` being
/// re-run by hand. `cargo test --all-features` already runs in CI on every push, which is what
/// makes this the same guarantee `cargo fmt --check` gives formatting — checked fresh, every
/// time, never trusted merely because it once matched.
#[cfg(feature = "schema")]
#[test]
fn the_checked_in_contract_matches_what_the_types_render_today() {
    let rendered = config::contract().expect("the example's own types render a contract");
    let checked_in = include_str!("../examples/service/contract.json");

    assert_eq!(
        rendered.trim_end(),
        checked_in.trim_end(),
        "examples/service/contract.json is stale — regenerate it with:\n\
         cargo run --example service --features schema -- --contract > examples/service/contract.json"
    );
}
