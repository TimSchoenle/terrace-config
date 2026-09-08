//! The generator every service writes beside its configuration, written once.
//!
//! A service using [`schema`](crate::schema) ends up with the same program in `examples/`: parse a
//! `--format`, build the schema, dispatch to one of six renderings, stamp a build identity onto
//! the contract, print, exit. It is two hundred lines of which about eight are that service's own,
//! and it gets copied between repositories — which is how three of them end up disagreeing about
//! how to cut a `LABEL` block out of a Dockerfile.
//!
//! What is actually service-specific is small and it is exactly this:
//!
//! - the root type and the loader's prefix, which produce the [`Schema`];
//! - the [`App`] identity — name, source URL, and a version from `CARGO_PKG_VERSION`;
//! - the JSON Schema's `title` and `$id`;
//! - the [`External`](crate::schema::External) surface, which no derive can see.
//!
//! Everything else is this module.
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
//! The layering is not decoration. [`Cli::main`] is the convenient one and it is also the one that
//! makes decisions for you — it reads `std::env::args`, prints to stdout and stderr, and returns
//! an [`ExitCode`]. A consumer with a real argument parser, or one rendering into a build script
//! rather than a pipe, drops to [`Cli::render`] and keeps everything else.
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

use std::process::ExitCode;

pub use format::{Format, UnknownFormat};
pub use request::{Request, USAGE, UsageError};

use crate::schema::{
    App, Column, Contract, ContractBuilder, Error, JsonSchema, Schema, TomlExample,
};

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
    /// The [`App`] should carry the version the *image tag* uses: `concat!("v",
    /// env!("CARGO_PKG_VERSION"))` rather than `env!` alone, because the field exists to be
    /// compared against a tag and the tags say `v2.5.0`. A `--version` argument overrides it — see
    /// [`Request::stamp`].
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
    /// The callback receives the [`ContractBuilder`] with the app and the schema already on it,
    /// and returns it with whatever the service has to add — almost always an
    /// [`External`](crate::schema::External) surface, which is the half no derive can find and the
    /// half a chart most needs.
    ///
    /// A [`ContractBuilder`] rather than an `External` so that
    /// [`closed`](ContractBuilder::closed), [`title`](ContractBuilder::title) and
    /// [`require_present`](ContractBuilder::require_present) stay reachable without this type
    /// growing a setter per option and going stale the moment the builder gains another.
    #[must_use]
    pub fn contract_with(
        mut self,
        contract: &'a dyn Fn(ContractBuilder) -> ContractBuilder,
    ) -> Self {
        self.contract = Some(contract);
        self
    }

    /// Render one request against a schema.
    ///
    /// The schema is taken by value because [`Request::only`] subsets it. Pass the whole
    /// configuration: slicing it here is what keeps the real key paths — a page documenting
    /// `cloudflare.*` when the file says `csp.cloudflare.*` is worse than no page — and what lets
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
            // A subsystem page gets the key table alone. The loader's own variables belong once,
            // on the page documenting the whole configuration, rather than repeated above every
            // slice — and `to_json` still carries them whichever page this is.
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
    /// Prints the rendering to stdout with exactly one trailing newline — the renderings
    /// themselves disagree about whether they end in one — and returns
    /// [`ExitCode::SUCCESS`]; on any failure prints the message to stderr and returns
    /// [`ExitCode::FAILURE`]. Nothing is written to stdout on failure, so a step redirecting into
    /// a committed file leaves it untouched rather than half-written.
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
                // and the two label formats do not. Printing them all with `println!` gave the
                // first group a trailing blank line, which is invisible on a terminal and a diff
                // in a committed `config.example.toml`.
                println!("{}", rendered.trim_end_matches('\n'));
                ExitCode::SUCCESS
            }
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

/// Report and fail, in the one place both failure paths meet.
fn fail(error: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("{error}");
    ExitCode::FAILURE
}
