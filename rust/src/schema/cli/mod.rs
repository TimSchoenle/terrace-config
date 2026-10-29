//! The generator every service writes beside its configuration, written once.
//!
//! A service using [`schema`](crate::schema) ends up with the same program in `examples/`: parse a
//! `--format`, build the schema, dispatch to one of six renderings, stamp a build identity onto the
//! contract, print, exit. It is two hundred lines of which about eight are that service's own, and
//! copies of it in different repositories drift apart, for example about how to cut a `LABEL` block
//! out of a Dockerfile.
//!
//! What is actually service-specific is small and it is exactly this:
//!
//! - the root type and the loader's prefix, which produce the [`Schema`];
//! - the [`App`] identity: name, source URL, and a version from `CARGO_PKG_VERSION`;
//! - the JSON Schema's `title` and `$id`;
//! - the [`External`](crate::schema::External) surface, which no derive can see.
//!
//! Everything else is this module.
//!
//! # The one-call form
//!
//! A service whose defaults are its `Default` impl and that publishes nothing beyond what its types
//! describe needs none of the builder. [`main`] is the whole generator. It takes the loader the
//! service already boots with, so the contract is derived from the same prefix, reserved names and
//! file variables that [`Terrace::load`](crate::Terrace::load) will read. It also takes the app
//! identity, which [`cargo_app!`](crate::cargo_app) fills from the calling crate's own
//! `Cargo.toml`:
//!
//! ```no_run
//! # use serde::{Deserialize, Serialize};
//! # use terrace_config::schema::Describe;
//! # #[derive(Deserialize, Serialize, Default, Describe)]
//! # struct Config {
//! #     /// Port the listener binds to.
//! #     #[serde(default)]
//! #     port: u16,
//! # }
//! fn main() -> std::process::ExitCode {
//!     let loader = terrace_config::Terrace::new("ORDERS_");
//!     terrace_config::schema::cli::main::<Config>(&loader, terrace_config::cargo_app!("orders"))
//! }
//! ```
//!
//! Anything more, such as an external surface, a JSON Schema `$id` or a TOML preamble, is [`Cli`]
//! with [`Cli::run`] in place of [`main`], which keeps the same one-call shape.
//!
//! # Four layers, and each is usable without the one above it
//!
//! | Layer | Use it alone when |
//! |---|---|
//! | [`Format`] | you parse arguments yourself and want the vocabulary |
//! | [`Request`] | you parse with `clap` and want the dispatch |
//! | [`Cli`] | you want the whole program |
//! | [`verify`] | you have an image to check, and no generator to run |
//!
//! [`Cli::main`] is the convenient layer and it is also the one that makes decisions for you: it
//! reads `std::env::args`, prints to stdout and stderr, and returns an [`ExitCode`]. A consumer
//! with a real argument parser, or one rendering into a build script rather than a pipe, drops to
//! [`Cli::render`] and keeps everything else.
//!
//! # The whole of a service's generator
//!
//! ```no_run
//! use std::process::ExitCode;
//!
//! use serde::{Deserialize, Serialize};
//! use terrace_config::Terrace;
//! use terrace_config::schema::cli::Cli;
//! use terrace_config::schema::{App, Describe, External, ExternalVar, JsonSchema};
//!
//! #[derive(Deserialize, Serialize, Default, Describe)]
//! struct Config {
//!     /// Bundle directory the readiness probe checks.
//!     #[serde(default)]
//!     dist_dir: String,
//! }
//!
//! fn main() -> ExitCode {
//!     let schema = Terrace::new("PORTFOLIO_")
//!         .schema::<Config>()
//!         .with_defaults_from(&Config::default());
//!     let schema = match schema {
//!         Ok(schema) => schema,
//!         Err(error) => {
//!             eprintln!("{error}");
//!             return ExitCode::FAILURE;
//!         }
//!     };
//!
//!     Cli::new(
//!         App::new("portfolio")
//!             .version(concat!("v", env!("CARGO_PKG_VERSION")))
//!             .source("https://github.com/TimSchoenle/Portfolio"),
//!     )
//!     .json_schema(
//!         JsonSchema::new()
//!             .title("portfolio configuration")
//!             .id("https://github.com/TimSchoenle/Portfolio/config.schema.json"),
//!     )
//!     .contract_with(&|builder| {
//!         builder.external(External::new().var(
//!             ExternalVar::new("PORT").owner("dioxus").ty("u16").default("8080"),
//!         ))
//!     })
//!     .main(schema)
//! }
//! ```

mod format;
mod request;
pub mod verify;

use std::io::Write as _;
use std::process::ExitCode;

pub use format::{Format, UnknownFormat};
pub use request::{Request, USAGE, UsageError};

use serde::Serialize;

use crate::Terrace;
use crate::schema::{
    App, Column, Contract, ContractBuilder, Describe, Error, JsonSchema, Schema, TomlExample,
};

/// The calling crate's [`App`] identity, from its own `Cargo.toml`.
///
/// A macro rather than a function because the values are the *caller's*: `env!` expands where it
/// is written, so only a macro reads the service's package rather than this crate's.
///
/// - `cargo_app!()` names the app after the package, `CARGO_PKG_NAME`;
/// - `cargo_app!("orders")` names it explicitly, for a package not named as its image is.
///
/// The version is `CARGO_PKG_VERSION` prefixed with `v`, the spelling image tags conventionally use
/// and the one [`Cli::new`] recommends. A build tagging without the prefix, or stamping a release
/// version, passes `--version` and that wins (see [`Request::stamp`]). The source is
/// `CARGO_PKG_REPOSITORY`, left unset when the package declares none.
#[macro_export]
macro_rules! cargo_app {
    () => {
        $crate::cargo_app!(::core::env!("CARGO_PKG_NAME"))
    };
    ($name:expr $(,)?) => {{
        let app = $crate::schema::App::new($name)
            .version(::core::concat!("v", ::core::env!("CARGO_PKG_VERSION")));
        match ::core::env!("CARGO_PKG_REPOSITORY") {
            "" => app,
            repository => app.source(repository),
        }
    }};
}

/// The whole generator for a configuration whose defaults are its [`Default`]: parse this
/// process's arguments, render, print, exit.
///
/// `loader` is the [`Terrace`] the service boots with. Pass the same value, not a second one built
/// to match, because every spelling in the contract is derived from it and a contract derived from
/// a different loader describes variables the service does not read. `app` is the identity to
/// publish, which [`cargo_app!`](crate::cargo_app) builds from the calling crate.
///
/// This is [`Cli::new`]`(app)`[`.run::<C>(loader)`](Cli::run), for the service that needs nothing
/// else from [`Cli`].
#[must_use]
pub fn main<C: Describe + Default + Serialize>(loader: &Terrace, app: App) -> ExitCode {
    Cli::new(app).run::<C>(loader)
}

/// A configuration generator, configured with the four things that are a service's own.
///
/// Borrows rather than owns the two callbacks and the schema options, because a `Cli` is built
/// inline in `main`, used once and dropped. Nothing here outlives the statement that builds it.
pub struct Cli<'a> {
    app: App,
    json_schema: Option<JsonSchema>,
    toml_example: Option<TomlExample>,
    columns: &'a [Column],
    contract: Option<&'a dyn Fn(ContractBuilder) -> ContractBuilder>,
}

impl<'a> Cli<'a> {
    /// A generator for one application.
    ///
    /// The [`App`] should carry the version the *image tag* uses:
    /// `concat!("v", env!("CARGO_PKG_VERSION"))` rather than `env!` alone, because the field exists
    /// to be compared against a tag and the tags say `v2.5.0`. A `--version` argument overrides it
    /// through [`Request::stamp`].
    #[must_use]
    pub fn new(app: App) -> Self {
        Self {
            app,
            json_schema: None,
            toml_example: None,
            columns: Column::DEFAULT,
            contract: None,
        }
    }

    /// How `--format json-schema` renders.
    ///
    /// The `title` and `$id` are per-service and there is no useful default for the second: an
    /// `$id` is a URL under the service's own repository, and a wrong one is worse than none
    /// because an editor will try to resolve it.
    #[must_use]
    pub fn json_schema(mut self, json_schema: JsonSchema) -> Self {
        self.json_schema = Some(json_schema);
        self
    }

    /// How `--format toml` renders.
    ///
    /// The defaults suit a file kept beside a README. A service whose `config.example.toml` is the
    /// only documentation an operator gets wants [`Docs::Full`](crate::schema::Docs), because that
    /// file is read once while being filled in and the paragraph below each summary is the part
    /// that explains the shape; a service embedding the same rendering into a README wants
    /// `header(false)`, because the page already has one.
    #[must_use]
    pub fn toml_example(mut self, toml_example: TomlExample) -> Self {
        self.toml_example = Some(toml_example);
        self
    }

    /// Which columns `--format markdown` emits.
    ///
    /// [`Column::DEFAULT`] unless set.
    #[must_use]
    pub fn columns(mut self, columns: &'a [Column]) -> Self {
        self.columns = columns;
        self
    }

    /// The contract this image publishes, beyond what the schema already describes.
    ///
    /// The callback receives the [`ContractBuilder`] with the app and the schema already on it, and
    /// returns it with whatever the service has to add, almost always an
    /// [`External`](crate::schema::External) surface, which is the half no derive can find and the
    /// half a chart most needs.
    ///
    /// A [`ContractBuilder`] rather than an `External` so that [`closed`](ContractBuilder::closed),
    /// [`title`](ContractBuilder::title) and [`require_present`](ContractBuilder::require_present)
    /// stay reachable without this type needing a setter per option that the builder's next option
    /// would leave incomplete.
    #[must_use]
    pub fn contract_with(
        mut self,
        contract: &'a dyn Fn(ContractBuilder) -> ContractBuilder,
    ) -> Self {
        self.contract = Some(contract);
        self
    }

    /// Renders one request against a schema.
    ///
    /// The schema is taken by value because [`Request::only`] subsets it. Pass the whole
    /// configuration: slicing it here is what keeps the real key paths (a page documenting
    /// `cloudflare.*` when the file says `csp.cloudflare.*` is worse than no page), and what lets
    /// [`Request::validate`] refuse a slice for a whole-image format.
    ///
    /// The returned string is whatever the rendering produced, and the renderings disagree
    /// about a trailing newline: `to_markdown*` and `to_toml_example` end in one, the
    /// contract and the two label formats do not. Nothing is normalised here, because a
    /// caller writing into a build script or a buffer wants what was rendered.
    /// [`Self::main`] is where it becomes exactly one, for the file a shell redirects into.
    ///
    /// # Errors
    /// Returns whatever the underlying rendering does: [`Error::Invalid`] from
    /// [`ContractBuilder::build`] when the declared external surface is not one a validator could
    /// act on, or from the JSON writer.
    pub fn render(&self, request: &Request, schema: Schema) -> Result<String, Error> {
        let schema = schema.subset(request.only());

        match request.format() {
            Format::Json => schema.to_json(),
            // A subsystem page gets the key table alone. The loader's own variables belong once, on
            // the page documenting the whole configuration, rather than repeated above every slice,
            // and `to_json` still carries them whichever page this is.
            Format::Markdown if !request.only().is_empty() => {
                Ok(schema.to_markdown_keys(self.columns))
            }
            Format::Markdown => Ok(schema.to_markdown_with(self.columns)),
            // The loader's variables carry no keys, so the subset above did not touch them
            // and `Request::validate` has already refused an `--only` that implied it had.
            Format::MarkdownLoader => Ok(schema.to_markdown_loader()),
            Format::MarkdownKeys => Ok(schema.to_markdown_keys(self.columns)),
            Format::Toml => Ok(match &self.toml_example {
                Some(options) => schema.to_toml_example_with(options),
                None => schema.to_toml_example(),
            }),
            Format::JsonSchema => match &self.json_schema {
                Some(options) => schema.to_json_schema_with(options),
                None => schema.to_json_schema(),
            },
            Format::Contract => self.contract(schema, request)?.to_json(),
            Format::Labels => Ok(self
                .contract(schema, request)?
                .labels(request.path())
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("\n")),
            Format::Dockerfile => Ok(self
                .contract(schema, request)?
                .to_dockerfile_block(request.path())
                .trim_end()
                .to_owned()),
        }
    }

    /// The whole program: parse this process's arguments, render, print, exit.
    ///
    /// Prints the rendering to stdout with exactly one trailing newline, since the renderings
    /// themselves disagree about whether they end in one, and returns [`ExitCode::SUCCESS`]. When
    /// the arguments do not parse or the rendering fails, it prints the message to stderr, writes
    /// nothing to stdout, and returns [`ExitCode::FAILURE`]. An argument that is not valid Unicode
    /// is one that does not parse. When writing or flushing stdout reports an error, such as a
    /// closed pipe, it returns [`ExitCode::FAILURE`] too, and part of the rendering may already be
    /// out. A stdout that is not open for writing at all is not reported: the standard library
    /// discards output to a closed descriptor as written, so the call succeeds with nothing
    /// printed. A shell redirect truncates its target before this runs, so a step that
    /// regenerates a committed file should write to a temporary file and replace the target only
    /// on success.
    #[must_use]
    pub fn main(&self, schema: Schema) -> ExitCode {
        let request = match Request::from_env() {
            Ok(request) => request,
            Err(error) => return fail(&error),
        };

        match self.render(&request, schema) {
            Ok(rendered) => {
                // Exactly one trailing newline, whatever the rendering ended with. The renderings
                // disagree: `to_markdown*` and `to_toml_example` end in one already, the contract
                // and the two label formats do not. Appending a newline to every one gave the
                // first group a trailing blank line, which is invisible on a terminal and a diff
                // in a committed `config.example.toml`.
                //
                // Written rather than `println!`ed: that macro panics when stdout is gone, and a
                // closed pipe is an ordinary way for a caller to stop reading.
                let mut stdout = std::io::stdout().lock();
                match writeln!(stdout, "{}", rendered.trim_end_matches('\n'))
                    .and_then(|()| stdout.flush())
                {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => fail(&format_args!("cannot write to stdout: {error}")),
                }
            }
            Err(error) => fail(&error),
        }
    }

    /// The whole program for a configuration whose defaults are its [`Default`].
    ///
    /// Describes `C` through `loader`, takes every key's default from `C::default()`, and hands the
    /// schema to [`Self::main`]. A documentation job runs where none of the configuration's
    /// variables are set, so the defaults come from the type rather than from loading anything.
    ///
    /// A configuration whose `Default` is not its compiled default, or that has none, builds the
    /// [`Schema`] itself and calls [`Self::main`].
    #[must_use]
    pub fn run<C: Describe + Default + Serialize>(&self, loader: &Terrace) -> ExitCode {
        match loader.schema::<C>().with_defaults_from(&C::default()) {
            Ok(schema) => self.main(schema),
            Err(error) => fail(&error),
        }
    }

    /// The contract for this build: the app identity, stamped with what the request was told.
    fn contract(&self, schema: Schema, request: &Request) -> Result<Contract, Error> {
        let builder = schema.into_contract(request.stamp(self.app.clone()));
        let builder = match self.contract {
            Some(with) => with(builder),
            None => builder,
        };
        builder.build()
    }
}

/// Reports and fails, in the one place both failure paths meet.
///
/// Written rather than `eprintln!`ed, which panics when stderr is gone. A failed write there is
/// dropped: no channel is left to report it on, and the exit code still says the run failed.
fn fail(error: &dyn std::fmt::Display) -> ExitCode {
    let _unreported = writeln!(std::io::stderr(), "{error}");
    ExitCode::FAILURE
}
