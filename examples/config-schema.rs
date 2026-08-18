//! Dump a configuration surface for a documentation job.
//!
//! This is the shape the `schema` feature is built for, and it is meant to be copied into the
//! service whose configuration is being documented — a handful of lines that a CI step can run
//! and redirect somewhere:
//!
//! ```text
//! cargo run --example config-schema -- --format json              > docs/config.json
//! cargo run --example config-schema -- --format markdown          > docs/config.md
//! cargo run --example config-schema -- --format toml              > config.example.toml
//! cargo run --example config-schema -- --format json-schema       > config.schema.json
//! cargo run --example config-schema -- --format contract          > contract.json
//! cargo run --example config-schema -- --format labels            > contract.labels
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
use terrace_config::schema::{
    App, Column, Contract, DEFAULT_PATH, Describe, External, ExternalVar, JsonSchema, Schema,
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

fn main() -> ExitCode {
    let options = match Options::from_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    match render(&options) {
        Ok(rendered) => {
            println!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn render(options: &Options) -> Result<String, terrace_config::Error> {
    // Built whole and then sliced, rather than described from the subsystem's own type: the whole
    // schema is the only place the real key paths exist, and a page documenting `cloudflare.*`
    // when the file says `csp.cloudflare.*` is worse than no page.
    //
    // The defaults come from a value the caller builds, not from the environment: a documentation
    // job runs where none of these variables are set, and that is the point.
    let schema = Terrace::new("PORTFOLIO_")
        .reserve("PORTFOLIO_PROFILE")
        .schema::<Config>()
        .with_defaults_from(&Config::default())?
        .subset(&options.only);

    match options.format {
        Format::Json => schema.to_json(),
        // A subsystem page gets the key table alone. The loader variables belong once, on the
        // page that documents the whole configuration, rather than repeated above every slice —
        // and `to_json` still carries them whichever page this is.
        Format::Markdown if !options.only.is_empty() => {
            Ok(schema.to_markdown_keys(Column::DEFAULT))
        }
        Format::Markdown => Ok(schema.to_markdown()),
        // A slice of the configuration is a slice of the file too, so nothing here needs the
        // `--only` special case the Markdown arm does: `subset` has already cut the keys, and
        // both renderings are built from whatever keys are left.
        Format::Toml => Ok(schema.to_toml_example()),
        Format::JsonSchema => schema.to_json_schema_with(
            &JsonSchema::new()
                .title("portfolio configuration")
                .id("https://github.com/TimSchoenle/terrace-config/config.schema.json"),
        ),
        // Whole-image outputs, so a slice of the configuration is not a thing either can be —
        // `Options::from_args` refuses the combination rather than publishing a contract that
        // silently omits the keys `--only` cut.
        Format::Contract => contract(schema)?.to_json(),
        Format::Labels => Ok(contract(schema)?
            .labels(DEFAULT_PATH)
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n")),
    }
}

/// The whole contract this image publishes: every configuration key, and everything else it reads.
///
/// The `external` half is the part a derive cannot reach and the part a chart most needs. These
/// three belong to the Dioxus toolchain, which reads them from the environment before any of this
/// crate's layers exist — so they are not configuration keys, they are still set by the chart, and
/// a validator told only about the `PORTFOLIO_` namespace would have to either flag them or ignore
/// every variable outside it. Declared here, they are checked like any key: a chart passing
/// `PORT: "http"` fails the same gate a chart passing `PORTFOLIO_GITHUB__TTL_SECS: "soon"` fails.
fn contract(schema: Schema) -> Result<Contract, terrace_config::Error> {
    schema
        .into_contract(
            // Spelled as the image tag spells it. `CARGO_PKG_VERSION` alone yields `2.5.0`
            // where the images are tagged `v2.5.0`, and the field exists to be compared against
            // a tag.
            App::new("portfolio")
                .version(concat!("v", env!("CARGO_PKG_VERSION")))
                .source("https://github.com/TimSchoenle/Portfolio"),
        )
        .external(
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
                // What `Unknown::Reject` costs, and it is not zero even for a `scratch` image
                // running one static binary: a pod carries names no image asked for. These have
                // no owner here, which is the one case `ignore` is for.
                .ignore("KUBERNETES_*")
                .ignore("HOSTNAME"),
        )
        .build()
}

/// What to emit, and how much of it.
struct Options {
    format: Format,
    /// The subtree to keep. Empty means the whole configuration.
    only: String,
}

/// Which rendering to emit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    /// The versioned schema, for a pipeline that renders its own tables.
    Json,
    /// GitHub-flavoured tables, for a pipeline whose next step is `>> README.md`.
    Markdown,
    /// The commented file an operator copies to `config.toml`.
    Toml,
    /// A JSON Schema, for an editor to validate that file against.
    JsonSchema,
    /// The document a build embeds in its image and attaches to its digest.
    Contract,
    /// The image labels that make that document discoverable, one `NAME=value` per line.
    Labels,
}

impl Format {
    /// Whether this rendering describes a whole image rather than a slice of a configuration.
    ///
    /// A contract that quietly omitted the keys `--only` cut would be a contract asserting the
    /// image does not read them, which is the one claim in the document that must never be wrong.
    fn whole_image(self) -> bool {
        matches!(self, Self::Contract | Self::Labels)
    }
}

impl Options {
    /// JSON and everything, unless asked otherwise: those are the outputs that lose nothing.
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            format: Format::Json,
            only: String::new(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--format" => {
                    options.format = match args.next().as_deref() {
                        Some("json") => Format::Json,
                        Some("markdown" | "md") => Format::Markdown,
                        Some("toml") => Format::Toml,
                        Some("json-schema" | "jsonschema") => Format::JsonSchema,
                        Some("contract") => Format::Contract,
                        Some("labels") => Format::Labels,
                        Some(other) => return Err(format!("unknown format `{other}`; {USAGE}")),
                        None => return Err(format!("--format takes a value; {USAGE}")),
                    };
                }
                "--only" => {
                    options.only = args
                        .next()
                        .ok_or_else(|| format!("--only takes a key prefix; {USAGE}"))?;
                }
                other => return Err(format!("unknown argument `{other}`; {USAGE}")),
            }
        }

        // Refused rather than silently ignored. A contract is a claim about what a whole image
        // reads, so one built from a slice would assert that the keys `--only` cut do not exist —
        // and a validator believing that rejects a chart which is setting them correctly.
        if options.format.whole_image() && !options.only.is_empty() {
            return Err(format!(
                "--only slices a configuration, and this format describes a whole image; a \
                 contract built from a slice would claim the image does not read the keys it \
                 cut. {USAGE}"
            ));
        }

        Ok(options)
    }
}

const USAGE: &str = "usage: config-schema \
                     [--format json|markdown|toml|json-schema|contract|labels] \
                     [--only <key-prefix>]";
