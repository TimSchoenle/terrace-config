//! The configuration a small HTTP service reads at boot.
//!
//! This is the module [`super`]'s `main` loads and the exact module
//! `tests/example_service.rs` loads it through — included there with `#[path]` rather than
//! copied, so a change here that breaks a real load fails that test instead of drifting past it
//! unnoticed. See that file for the scenarios exercised.
//!
//! Every type below also derives [`Describe`] under the `schema` feature, cfg-gated rather than
//! unconditional: this example's whole point is that it keeps building under `loader` alone
//! (`required-features = ["loader"]` in `Cargo.toml`), and a bare `derive(Describe)` would pull
//! `schema` in regardless of what a caller asked for. `cargo run --example service --features
//! schema -- --contract` renders the same types this same file loads with — one definition, not a
//! second one that could drift from it — and `examples/service/contract.json` is that rendering,
//! checked by CI exactly the way `cargo fmt --check` checks formatting: regenerated fresh and
//! diffed, never trusted merely because it once matched.

use secrecy::SecretString;
use serde::Deserialize;
#[cfg(feature = "schema")]
use serde::Serialize;
#[cfg(feature = "schema")]
use terrace_config::schema::{App, Describe};
use terrace_config::{Error, Terrace};

/// The loader this service reads its configuration through.
///
/// A function rather than a constant so the example and its test build the identical [`Terrace`]
/// — cheap to construct, and the one place its prefix and reserved keys are spelled.
pub(crate) fn loader() -> Terrace {
    Terrace::new("ORDERS_").reserve("ORDERS_PROFILE")
}

/// Load the service's configuration through [`loader`].
///
/// # Errors
/// Returns [`Error`] if a required value is missing, a value fails to parse, or a file-backed
/// source — a secrets directory, a `_FILE` indirection — cannot be read.
// `main` is this function's only caller: `tests/example_service.rs` drives `loader()` through
// `testing::Harness` instead, which is the whole point of a harness — a sandboxed environment,
// not this process's real one. Compiled under `cfg(test)` — the integration test binary that
// includes this module by path — that leaves nothing left to call it.
#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "the integration test drives `loader()` through the harness"
    )
)]
pub(crate) fn load() -> Result<Config, Error> {
    loader().load()
}

/// Render this service's configuration as a contract document — the `schema` feature's own
/// rendering of exactly the types [`load`] deserialises into, not a hand-maintained description
/// of them that could say something [`load`] no longer does.
///
/// # Errors
/// Returns [`Error`] if two fields of [`Config`] resolve to one key path, or if building the
/// contract document itself is refused.
#[cfg(feature = "schema")]
pub(crate) fn contract() -> Result<String, Error> {
    let schema = loader()
        .schema::<Config>()
        .with_defaults_from(&Config::default())?;
    let contract = schema
        .into_contract(App::new("orders-service").version(concat!("v", env!("CARGO_PKG_VERSION"))))
        .build()?;
    contract.to_json()
}

/// The root configuration. Everything a booting service needs to know before it opens a socket.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "schema", derive(Serialize, Describe))]
pub(crate) struct Config {
    /// Address the HTTP listener binds to.
    #[serde(default = "default_bind_addr")]
    pub(crate) bind_addr: String,
    /// Port the HTTP listener binds to.
    #[serde(default = "default_port")]
    pub(crate) port: u16,
    /// Where orders are persisted.
    #[serde(default)]
    #[cfg_attr(feature = "schema", config(nested))]
    pub(crate) database: Database,
    /// How much the service says.
    #[serde(default)]
    #[cfg_attr(feature = "schema", config(values))]
    pub(crate) log_level: LogLevel,
}

impl Default for Config {
    /// Mirrors every field's own `#[serde(default = "…")]`, the same reason [`Database`] needs
    /// one: `with_defaults_from` serialises this to populate the contract's defaults column, and
    /// a boot with no config file, no environment variable and no mounted secret at all is what
    /// it renders defaults *from* — not a special case of the type.
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            port: default_port(),
            database: Database::default(),
            log_level: LogLevel::default(),
        }
    }
}

/// The database this service persists orders to.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "schema", derive(Serialize, Describe))]
pub(crate) struct Database {
    /// Connection string. A credential, so it is a `SecretString` rather than a `String` even
    /// though the compiled default below is not one — a real deployment supplies this through
    /// the secrets directory or `_FILE` indirection, never through the environment in plain
    /// sight.
    // Not rustdoc: the paragraph above is what reaches `examples/service/contract.json` — an
    // operator's document, not this source. `SecretString` deliberately does not implement
    // `Serialize`, so `#[serde(skip_serializing)]` is what lets the rest of the struct still
    // derive it; `#[config(secret)]` renders `<redacted>` in the contract in its place, rather
    // than the compiled default a reader might otherwise mistake for a real one.
    #[serde(default = "default_database_url", skip_serializing)]
    #[cfg_attr(feature = "schema", config(secret))]
    pub(crate) url: SecretString,
    /// Maximum number of pooled connections.
    #[serde(default = "default_max_connections")]
    pub(crate) max_connections: u32,
}

impl Default for Database {
    /// Mirrors the two fields' own `#[serde(default = "…")]` functions, so a config file that
    /// omits `[database]` entirely boots the same way as one that supplies an empty table — this
    /// is what a config without any operator input looks like, not a special case of it.
    fn default() -> Self {
        Self {
            url: default_database_url(),
            max_connections: default_max_connections(),
        }
    }
}

/// How much the service says, from quietest to loudest.
#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(Serialize, Describe))]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogLevel {
    /// Every request and response body.
    Trace,
    /// Every request, without bodies.
    Debug,
    /// Startup, shutdown, and anything an operator needs to see.
    #[default]
    Info,
    /// Only what is already wrong.
    Warn,
}

/// Loopback, so `cargo run --example service` binds somewhere without any configuration at all.
fn default_bind_addr() -> String {
    "127.0.0.1".to_owned()
}

/// A high, unprivileged port with no ecosystem meaning of its own.
fn default_port() -> u16 {
    8080
}

/// A local development database. **Never a real credential** — an operator overriding one field
/// of `database` still gets a value here, and this is the one that must be safe to print.
fn default_database_url() -> SecretString {
    SecretString::from("postgres://localhost/orders_dev".to_owned())
}

/// Generous enough for a development machine, conservative enough not to exhaust a small
/// database's own connection limit by default.
fn default_max_connections() -> u32 {
    10
}
