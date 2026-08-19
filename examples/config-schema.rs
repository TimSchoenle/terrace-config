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
//! cargo run --example config-schema -- --format dockerfile        # paste into the Dockerfile
//! cargo run --example config-schema -- --format kube --image "$IMAGE" > contract.metadata.yaml
//! cargo run --example config-schema -- --format kube --target workload --image "$IMAGE"
//! cargo run --example config-schema -- --format kube-labels --image "$IMAGE"
//! cargo run --example config-schema -- --format contract --revision "$(git rev-parse HEAD)" \
//!                                       --created "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
//! cargo run --example config-schema -- --format markdown --only csp > docs/csp.md
//! ```
//!
//! where `IMAGE` is the digest-pinned reference the push produced —
//! `ghcr.io/you/portfolio@sha256:48e2…`. It is a flag rather than something this generator works
//! out because a contract deliberately names no image; see the build outputs below.
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
//! `kube` and `kube-labels` are consumed by neither: they belong to whatever renders the
//! deployment, which is the only side that knows the digest an image was pushed under. That is
//! why `--image` is an argument here and not something this generator can work out — a contract
//! deliberately names no image. See [`schema::kube`](terrace_config::schema::kube).
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
    App, Column, Contract, DEFAULT_PATH, Describe, External, ExternalVar, JsonSchema, Schema, kube,
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
        Format::Contract => contract(schema, options)?.to_json(),
        Format::Labels => Ok(contract(schema, options)?
            .labels(DEFAULT_PATH)
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n")),
        Format::Dockerfile => Ok(contract(schema, options)?
            .to_dockerfile_labels(DEFAULT_PATH)
            .trim_end()
            .to_owned()),
        // Indented by two, because that is where a `metadata:` at the left margin puts its
        // children and a `ConfigMap`'s is at the left margin. A pod template's is deeper, and
        // `Metadata::to_yaml` takes the indent rather than guessing — this flagless example
        // picks the one that is right for the object the stamp is usually going on.
        Format::Kube => Ok(contract(schema, options)?
            .kube_metadata(&options.target(), &options.images())?
            .to_yaml(2)
            .trim_end()
            .to_owned()),
        // Labels first, then annotations, each map in its own order — the same deterministic
        // output `--format labels` gives, for the same consumer: a shell loop feeding something
        // that takes one `NAME=value` at a time.
        Format::KubeLabels => {
            let metadata =
                contract(schema, options)?.kube_metadata(&options.target(), &options.images())?;
            Ok(metadata
                .labels()
                .iter()
                .chain(metadata.annotations())
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("\n"))
        }
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
fn contract(schema: Schema, options: &Options) -> Result<Contract, terrace_config::Error> {
    // Spelled as the image tag spells it. `CARGO_PKG_VERSION` alone yields `2.5.0` where the
    // images are tagged `v2.5.0`, and the field exists to be compared against a tag.
    let mut app = App::new("portfolio")
        .version(concat!("v", env!("CARGO_PKG_VERSION")))
        .source("https://github.com/TimSchoenle/Portfolio");

    // The two fields that legitimately differ between builds of one source tree, and the reason
    // they are flags rather than something read here: this generator reads nothing from its
    // environment, so that a documentation job and a container build produce the same bytes.
    // Passing them makes that difference explicit and keeps `--format contract` reproducible when
    // they are omitted.
    if let Some(revision) = &options.revision {
        app = app.revision(revision);
    }
    if let Some(created) = &options.created {
        app = app.created(created);
    }

    schema
        .into_contract(app)
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
    /// The commit this build is of, for `--format contract`.
    revision: Option<String>,
    /// When this build happened, RFC 3339, for `--format contract`.
    created: Option<String>,
    /// Every image that reads the document, digest-pinned, for the `kube` formats.
    ///
    /// Repeatable and order-preserving: the annotation lists them in declaration order, and a
    /// generator that sorted them would make a diff out of a chart adding an image at the end.
    images: Vec<String>,
    /// The key inside the object's `data` that is the document.
    document_key: String,
    /// How that document is spelled.
    document_format: String,
    /// Which object the stamp is for.
    workload: bool,
}

impl Options {
    /// The object being stamped.
    ///
    /// A workload takes neither the key nor the format — a pod is not a document — and
    /// `Options::from_args` refuses the flags rather than dropping them here, so that a build
    /// passing `--document-key` to a pod template is told rather than obeyed.
    fn target(&self) -> kube::Target {
        if self.workload {
            kube::Target::workload()
        } else {
            kube::Target::document(
                self.document_key.clone(),
                kube::Format::from(self.document_format.as_str()),
            )
        }
    }

    /// The image references, in the order they were given.
    fn images(&self) -> Vec<&str> {
        self.images.iter().map(String::as_str).collect()
    }
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
    /// The same labels as the `LABEL` instruction to paste into a Dockerfile.
    Dockerfile,
    /// The Kubernetes `metadata` block a chart stamps onto the object holding the document, or
    /// onto the pod template that mounts it — ready to paste into a Helm template.
    Kube,
    /// The same block as one `NAME=value` per line, labels first, for a shell loop.
    KubeLabels,
}

impl Format {
    /// Whether this rendering describes a whole image rather than a slice of a configuration.
    ///
    /// A contract that quietly omitted the keys `--only` cut would be a contract asserting the
    /// image does not read them, which is the one claim in the document that must never be wrong.
    fn whole_image(self) -> bool {
        matches!(
            self,
            Self::Contract | Self::Labels | Self::Dockerfile | Self::Kube | Self::KubeLabels
        )
    }

    /// Whether this rendering stamps a Kubernetes object, and so needs to know which images read
    /// the document.
    fn kubernetes(self) -> bool {
        matches!(self, Self::Kube | Self::KubeLabels)
    }
}

impl Options {
    /// JSON and everything, unless asked otherwise: those are the outputs that lose nothing.
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            format: Format::Json,
            only: String::new(),
            revision: None,
            created: None,
            images: Vec::new(),
            // The two the loader itself would look for, so the common case passes no flags. A
            // service that mounts its document somewhere else says so; a service that does not
            // gets the answer it would have typed.
            document_key: "config.toml".to_owned(),
            document_format: "toml".to_owned(),
            workload: false,
        };
        // Tracked rather than inferred from the values: the defaults are legitimate values, so
        // "the key is `config.toml`" cannot distinguish a build that said so from one that said
        // nothing — and only the first is a mistake worth refusing beside `--target workload`.
        let mut document_flags = false;
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
                        Some("dockerfile") => Format::Dockerfile,
                        Some("kube" | "kubernetes") => Format::Kube,
                        Some("kube-labels") => Format::KubeLabels,
                        Some(other) => return Err(format!("unknown format `{other}`; {USAGE}")),
                        None => return Err(format!("--format takes a value; {USAGE}")),
                    };
                }
                "--only" => {
                    options.only = args
                        .next()
                        .ok_or_else(|| format!("--only takes a key prefix; {USAGE}"))?;
                }
                // Repeatable, and the repetition is the point: one document read by a web
                // binary and a worker is the union case the annotation exists for.
                "--image" => {
                    options.images.push(
                        args.next()
                            .ok_or_else(|| format!("--image takes an image reference; {USAGE}"))?,
                    );
                }
                "--document-key" => {
                    options.document_key = args
                        .next()
                        .ok_or_else(|| format!("--document-key takes a key; {USAGE}"))?;
                    document_flags = true;
                }
                "--document-format" => {
                    options.document_format = args
                        .next()
                        .ok_or_else(|| format!("--document-format takes a format; {USAGE}"))?;
                    document_flags = true;
                }
                "--target" => {
                    options.workload = match args.next().as_deref() {
                        Some("document") => false,
                        Some("workload") => true,
                        Some(other) => return Err(format!("unknown target `{other}`; {USAGE}")),
                        None => return Err(format!("--target takes a value; {USAGE}")),
                    };
                }
                "--revision" => {
                    options.revision = Some(
                        args.next()
                            .ok_or_else(|| format!("--revision takes a commit; {USAGE}"))?,
                    );
                }
                "--created" => {
                    options.created = Some(
                        args.next()
                            .ok_or_else(|| format!("--created takes a timestamp; {USAGE}"))?,
                    );
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

        // Refused rather than defaulted. A stamp naming no image says that nothing reads the
        // document, and that is not a claim any validator can act on: the annotation exists so
        // that something holding a running pod can decide whether this document is that pod's
        // configuration. There is no value this generator could invent either — a contract
        // deliberately carries no digest, because a digest is what building the image produces.
        if options.format.kubernetes() && options.images.is_empty() {
            return Err(format!(
                "--format kube needs at least one --image, digest-pinned. A stamp naming no \
                 image says that nothing reads this document, and the digest is the one fact \
                 about the image this generator cannot know. {USAGE}"
            ));
        }

        // A pod is not a document, so a pod template carries neither the key nor the format.
        // Refused rather than dropped: a build passing these to a workload has the two template
        // blocks the wrong way round, and quietly emitting the pod's stamp would hide that until
        // a validator went looking for a document key nobody ever wrote.
        if options.workload && document_flags {
            return Err(format!(
                "--document-key and --document-format describe a document, and --target \
                 workload stamps a pod template, which is not one. Stamp the object holding the \
                 document with --target document, and the pod template with neither. {USAGE}"
            ));
        }

        Ok(options)
    }
}

const USAGE: &str = "usage: config-schema \
                     [--format json|markdown|toml|json-schema|contract|labels|dockerfile|kube\
                     |kube-labels] \
                     [--only <key-prefix>] [--revision <commit>] [--created <rfc3339>] \
                     [--image <name@sha256:...>]... [--target document|workload] \
                     [--document-key <key>] [--document-format <fmt>]";
