//! Dump a configuration surface for a documentation job.
//!
//! This is the shape the `schema` feature is built for, and it is meant to be copied into the
//! service whose configuration is being documented — a handful of lines that a CI step can run
//! and redirect somewhere:
//!
//! ```text
//! cargo run --example config-schema -- --format json              > docs/config.json
//! cargo run --example config-schema -- --format markdown          > docs/config.md
//! cargo run --example config-schema -- --format markdown-loader   >> docs/config.md
//! cargo run --example config-schema -- --format toml              > config.example.toml
//! cargo run --example config-schema -- --format json-schema       > config.schema.json
//! cargo run --example config-schema -- --format contract          > contract.json
//! cargo run --example config-schema -- --format labels            > contract.labels
//! cargo run --example config-schema -- --format dockerfile        # paste into the Dockerfile
//! cargo run --example config-schema -- --format contract --revision "$(git rev-parse HEAD)" \
//!                                       --created "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
//! cargo run --example config-schema -- --format markdown --only csp > docs/csp.md
//! ```
//!
//! `json-schema` and `toml` are the two worth wiring into CI with a `--check`-style diff. A
//! reference table that has drifted reads wrong; an example file that has drifted gets *copied*
//! into a deployment, and a JSON Schema that has drifted tells an editor to underline a key that
//! is perfectly valid.
//!
//! # The build outputs
//!
//! `contract` and `labels` are the two a container build consumes rather than a documentation
//! job. The contract is copied into the image and attached to its digest in the registry; the
//! labels are what let anything find it without pulling a layer. See
//! [`schema::Contract`](terrace_config::schema::Contract) for the shape and
//! [`External`](terrace_config::schema::External) for the half no derive can see — the variables
//! this image reads that are nobody's configuration key.
//!
//! It reads nothing from the environment, so it produces the same answer on a developer's
//! machine and on a runner where none of the variables it describes exist.
//!
//! # A configuration is not one file
//!
//! The types below are deliberately in separate modules, as they would be in a real service —
//! each `Describe` derived beside the code that consumes the values, often in a different crate
//! entirely. `#[config(nested)]` is a trait bound, so it follows the *type*: describing the root
//! walks the whole tree wherever it lives, and nothing has to be registered anywhere central.
//!
//! `--only` goes the other way, slicing one subsystem out for a page of its own.

use std::process::ExitCode;

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::cli::Cli;
use terrace_config::schema::{
    App, ContractBuilder, Describe, Docs, External, ExternalVar, JsonSchema, TomlExample,
};

/// The root. Everything under it lives somewhere else.
#[derive(Deserialize, Serialize, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist_dir")]
    dist_dir: String,
    #[config(nested)]
    csp: csp::Csp,
    #[config(nested)]
    github: github::Github,
    /// How much the service says.
    #[config(values)]
    #[serde(default)]
    log_level: LogLevel,
}

/// An enum of unit variants is the set of values one key accepts, so `Describe` on it reports
/// those spellings rather than leaving the table to name a type nobody can see inside.
#[derive(Deserialize, Serialize, Default, Describe)]
#[serde(rename_all = "lowercase")]
enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
}

fn default_dist_dir() -> String {
    "public".to_owned()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dist_dir: default_dist_dir(),
            csp: csp::Csp::default(),
            github: github::Github::default(),
            log_level: LogLevel::default(),
        }
    }
}

/// Stands in for a `csp` module — or a `myservice-csp` crate.
mod csp {
    use serde::{Deserialize, Serialize};
    use terrace_config::schema::Describe;

    #[derive(Deserialize, Serialize, Default, Describe)]
    pub(crate) struct Csp {
        /// Hash the document's inline scripts instead of allowing `'unsafe-inline'`.
        ///
        /// The two are mutually exclusive by specification: a `script-src` carrying any hash
        /// makes a browser ignore `'unsafe-inline'` entirely, so turning this on and leaving an
        /// inline script unhashed blocks the script rather than falling back.
        ///
        /// Only the first paragraph reaches the Markdown table. The rest is here for whoever
        /// reads the type, which is what the rest of a `///` comment is always for — and
        /// `to_json` carries all of it for a pipeline that wants to render more.
        #[serde(default)]
        pub(crate) hash_inline_scripts: bool,
        #[config(nested)]
        pub(crate) cloudflare: Cloudflare,
    }

    #[derive(Deserialize, Serialize, Default, Describe)]
    pub(crate) struct Cloudflare {
        /// Per-response nonce for the script Cloudflare injects at the edge.
        #[serde(default)]
        pub(crate) script_nonce: bool,
        /// Admit the Turnstile widget — `script-src` and `frame-src`.
        #[serde(default)]
        pub(crate) turnstile: bool,
    }
}

/// Stands in for a `github` module, which knows nothing about `csp`.
mod github {
    use secrecy::SecretString;
    use serde::{Deserialize, Serialize};
    use terrace_config::schema::Describe;

    #[derive(Deserialize, Serialize, Default, Describe)]
    pub(crate) struct Github {
        /// User whose repositories `update-repos` lists.
        #[serde(alias = "user")]
        pub(crate) username: String,
        /// Explicit repository set. Every active repository when unset.
        pub(crate) repos: Option<Vec<String>>,
        /// Bearer token lifting the GitHub API rate limit.
        ///
        /// A real secret type, not a `String`, because that is what a service holding one uses —
        /// and `SecretString` deliberately does not implement `Serialize`, which would otherwise
        /// stop the whole struct from deriving it and `with_defaults_from` from taking a
        /// `Config`. `skip_serializing` is the answer and costs nothing: a secret has no default
        /// worth printing, and `#[config(secret)]` renders `<redacted>` in place of one anyway.
        ///
        /// This field is here in the shape a consumer will have it so that
        /// `cargo clippy --all-targets` fails if that ever stops being true.
        #[config(secret)]
        #[serde(skip_serializing)]
        #[expect(
            dead_code,
            reason = "skipping serialisation is what leaves it unread here: this example only \
                      dumps a schema, so every other field is read by the `Serialize` derive and \
                      this one is not. A real service reads it."
        )]
        pub(crate) token: Option<SecretString>,
        /// Revalidation interval in seconds.
        #[config(note = "permanent")]
        #[serde(default)]
        pub(crate) ttl_secs: u64,
    }
}

/// Everything below is what a service actually writes. The rest of the program — the `--format`
/// vocabulary, the argument parsing, the dispatch, the printing and the exit code — is
/// [`Cli`](terrace_config::schema::cli::Cli), because it was the same program in every repository
/// that had one.
fn main() -> ExitCode {
    // Built whole and sliced by `--only` inside `Cli::render`, rather than described from a
    // subsystem's own type: the whole schema is the only place the real key paths exist, and a
    // page documenting `cloudflare.*` when the file says `csp.cloudflare.*` is worse than no page.
    //
    // The defaults come from a value built here, not from the environment: a documentation job
    // runs where none of these variables are set, and that is the point.
    let schema = Terrace::new("PORTFOLIO_")
        .reserve("PORTFOLIO_PROFILE")
        .schema::<Config>()
        .with_defaults_from(&Config::default());

    let schema = match schema {
        Ok(schema) => schema,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    Cli::new(
        // Spelled as the image tag spells it. `CARGO_PKG_VERSION` alone yields `2.5.0` where the
        // images are tagged `v2.5.0`, and the field exists to be compared against a tag. A build
        // passing `--version "$TAG"` overrides it.
        App::new("portfolio")
            .version(concat!("v", env!("CARGO_PKG_VERSION")))
            .source("https://github.com/TimSchoenle/terrace-config"),
    )
    .json_schema(
        JsonSchema::new()
            .title("portfolio configuration")
            .id("https://github.com/TimSchoenle/terrace-config/config.schema.json"),
    )
    // The whole `///` comment rather than its summary. A reference table is read at a glance and
    // wants one sentence per key; `config.example.toml` is read once, while being filled in, and
    // the paragraph below the summary is where a key with a shape — a list, a map, a duration —
    // gets to show it.
    .toml_example(TomlExample::new().docs(Docs::Full))
    .contract_with(&external)
    .main(schema)
}

/// The half of the contract no derive can reach, and the half a chart most needs.
///
/// These three belong to the Dioxus toolchain, which reads them from the environment before any of
/// this crate's layers exist — so they are not configuration keys, they are still set by the
/// chart, and a validator told only about the `PORTFOLIO_` namespace would have to either flag
/// them or ignore every variable outside it. Declared here, they are checked like any key: a chart
/// passing `PORT: "http"` fails the same gate a chart passing `PORTFOLIO_GITHUB__TTL_SECS: "soon"`
/// fails.
fn external(builder: ContractBuilder) -> ContractBuilder {
    builder.external(
        External::new()
            .var(
                ExternalVar::new("PORT")
                    .owner("dioxus")
                    .ty("u16")
                    .default("8080")
                    .docs("Bind port. Read by the Dioxus toolchain, not by this loader."),
            )
            .var(
                ExternalVar::new("IP")
                    .owner("dioxus")
                    .ty("IpAddr")
                    .default("0.0.0.0")
                    .docs("Bind address. Read by the Dioxus toolchain, not by this loader."),
            )
            .var(
                ExternalVar::new("RUST_LOG")
                    .owner("tracing")
                    .ty("String")
                    .default("info")
                    .docs("Verbosity, as `tracing` directives — `info`, `web=debug,info`."),
            )
            // What `Unknown::Reject` costs, and it is not zero even for a `scratch` image running
            // one static binary: a pod carries names no image asked for. These have no owner here,
            // which is the one case `ignore` is for.
            .ignore("KUBERNETES_*")
            .ignore("HOSTNAME"),
    )
}
