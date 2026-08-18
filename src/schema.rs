//! Every key a configuration *could* carry, in a form another program can read.
//!
//! The other modules answer "what did this deployment supply?". This one answers "what is there
//! to supply?" — the question a reference table in a README is trying to answer, and the one the
//! loader itself never asks, because [`Terrace`](crate::Terrace) hands the merged figment to
//! `serde` and never learns the shape of what came back.
//!
//! The shape therefore has to come from the type, at compile time, via
//! [`#[derive(Describe)]`](macro@Describe). Most of such a table is recoverable without a macro —
//! the key path, its environment spelling, whether a value is required. Three things are not, and
//! all three are gone before any runtime sees the type: the sentence saying what the key is
//! *for*, the type it takes, and the variants an enum-valued key accepts.
//!
//! # Two outputs
//!
//! [`Schema::to_json`] is the contract: a versioned document with every field of every key,
//! including the ones [`Schema::to_markdown`] leaves out to stay readable. Point a documentation
//! pipeline at it and render whatever that pipeline wants.
//!
//! [`Schema::to_markdown`] is for the case where the pipeline is `>> README.md`. It emits GitHub
//! -flavoured tables that can be pasted in unmodified. Its documentation column carries the
//! summary — the first paragraph — of each `///` comment, on rustdoc's own convention: whatever a
//! field says below its summary line belongs on the field's own documentation page, not inside a
//! table cell, and [`Schema::to_json`] still carries all of it.
//!
//! ```no_run
//! use serde::Deserialize;
//! use terrace_config::{Terrace, schema::Describe};
//!
//! #[derive(Deserialize, Default, serde::Serialize, Describe)]
//! struct Config {
//!     /// Bundle directory the readiness probe checks.
//!     #[serde(default = "default_dist")]
//!     dist_dir: String,
//!     /// Bearer token lifting the GitHub API rate limit.
//!     #[config(secret)]
//!     token: Option<String>,
//! }
//!
//! fn default_dist() -> String { "public".to_owned() }
//!
//! let schema = Terrace::new("PORTFOLIO_")
//!     .schema::<Config>()
//!     .with_defaults_from(&Config { dist_dir: default_dist(), token: None })?;
//!
//! println!("{}", schema.to_markdown());
//! # Ok::<(), terrace_config::Error>(())
//! ```

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::dialect::Dialect;
use crate::error::Error;

pub use terrace_config_macros::Describe;

/// The version of the JSON document [`Schema::to_json`] produces.
///
/// Bumped when an existing field changes meaning or disappears — **not** when one is added. A
/// consumer that ignores unknown fields never needs to look at this; one that does not should
/// refuse a version it was not written against.
///
/// Adding [`Key::ty`], [`Key::values`] and [`Key::aliases`] therefore did not bump it. Splitting
/// [`Key::default`] from [`Key::note`] would have, since `default` used to hold prose — but no
/// document with the old meaning was ever published, so a bump would announce a migration nobody
/// can have performed. This is version 1, and version 1 is the shape described here.
pub const SCHEMA_VERSION: u32 = 1;

/// How deep [`Sink::nested`] will recurse before it decides the type is cyclic.
///
/// A configuration struct that contains itself has no finite key set, so there is no correct
/// output — only a stack overflow, or this.
const MAX_DEPTH: usize = 32;

/// A type whose configuration keys can be enumerated.
///
/// Derived with [`#[derive(Describe)]`](macro@Describe). Implementing it by hand is supported
/// and occasionally necessary — a newtype over a `HashMap` has keys the derive cannot see — but
/// the derive is the reason this exists.
pub trait Describe {
    /// Push this type's keys into `sink`, relative to whatever prefix `sink` already holds.
    fn describe(sink: &mut Sink);
}

/// A type whose accepted values are a fixed set — an enum of unit variants.
///
/// Derived by [`#[derive(Describe)]`](macro@Describe) on an enum, and pulled into a key by
/// `#[config(values)]` on the field. The variants are spelled the way `serde` will accept them,
/// `#[serde(rename_all)]` and all, because a table that printed `Info` where the file must say
/// `info` documents a value nobody can set.
pub trait Values {
    /// Every value the type accepts, in declaration order.
    const VARIANTS: &'static [&'static str];
}

/// One documented key, as the derive reports it.
///
/// This is what a `///` comment and a `#[config(...)]` attribute survive as. The derive always
/// supplies string literals, which are `&'static str`; the lifetime is borrowed rather than
/// `'static` because [`Sink::leaf`] copies every field into a [`String`] on the way in, so
/// demanding `'static` would buy nothing and would stop a hand-written [`Describe`] — a test
/// harness, a fuzz oracle — from reporting keys it computed at runtime.
#[derive(Debug, Clone, Copy)]
pub struct Leaf<'a> {
    /// The key's own segment, after `rename`/`rename_all` — not the full path.
    pub name: &'a str,
    /// The `///` comment, dedented, newlines preserved. Empty when there was none.
    pub docs: &'a str,
    /// The field's type as written, with any `Option<…>` stripped.
    pub ty: Option<&'a str>,
    /// The fixed set of values the key accepts, from `#[config(values)]`.
    pub values: Option<&'a [&'a str]>,
    /// Extra key names `serde` also accepts, from `#[serde(alias = "…")]`.
    pub aliases: &'a [&'a str],
    /// A `#[config(note = "…")]` annotation, reported *alongside* the observed default rather
    /// than in place of it. The prose says what the value means; the value itself is observed.
    pub note: Option<&'a str>,
    /// Whether loading fails when nothing supplies this key.
    pub required: bool,
    /// Whether the value is secret, and so must never be rendered.
    pub secret: bool,
}

/// The accumulator a [`Describe`] implementation writes into.
///
/// Only [`Schema::describe`] constructs one; a `describe` implementation receives it and reports
/// into it.
#[derive(Debug)]
pub struct Sink {
    /// The path segments currently open, innermost last.
    prefix: Vec<String>,
    /// The keys collected so far, in declaration order — which is the order a hand-written
    /// table would have used, and carries grouping that sorting destroys.
    keys: Vec<Key>,
    /// The paths already seen, so a collision is caught rather than duplicated into the output.
    seen: BTreeSet<String>,
}

impl Sink {
    fn new() -> Self {
        Self {
            prefix: Vec::new(),
            keys: Vec::new(),
            seen: BTreeSet::new(),
        }
    }

    /// Record one key at the current prefix.
    ///
    /// # Panics
    /// If two fields resolve to the same key path. That is a bug in the annotations rather than
    /// in the input — a `rename` colliding with a sibling, or two `flatten`ed structs sharing a
    /// field — and a documentation table that silently lists a key twice is worse than one that
    /// refuses to be generated.
    pub fn leaf(&mut self, leaf: Leaf<'_>) {
        let mut path = self.prefix.join(".");
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(leaf.name);

        assert!(
            self.seen.insert(path.clone()),
            "`{path}` is described twice. Two fields resolve to one key path, so the schema \
             cannot say which one the documentation is about."
        );

        self.keys.push(Key {
            path,
            env: None,
            env_file: None,
            secrets_file: None,
            docs: leaf.docs.to_owned(),
            ty: leaf.ty.map(str::to_owned),
            values: leaf
                .values
                .unwrap_or_default()
                .iter()
                .copied()
                .map(str::to_owned)
                .collect(),
            aliases: alias_paths(&self.prefix, leaf.aliases),
            default: None,
            note: leaf.note.map(str::to_owned),
            required: leaf.required,
            secret: leaf.secret,
            reserved: false,
        });
    }

    /// Record a subtree under `segment`.
    ///
    /// # Panics
    /// If nesting exceeds 32 levels, which means the type contains itself and has no finite set
    /// of keys to report.
    pub fn nested(&mut self, segment: &str, describe: impl FnOnce(&mut Self)) {
        assert!(
            self.prefix.len() < MAX_DEPTH,
            "`{}` nests more than {MAX_DEPTH} levels deep. A configuration type that contains \
             itself has no finite set of keys.",
            self.prefix.join(".")
        );
        self.prefix.push(segment.to_owned());
        describe(self);
        self.prefix.pop();
    }
}

/// One key a configuration can carry, in every spelling that can supply it.
///
/// The three spellings are [`Option`] because not every key has them. A key path that does not
/// survive the round trip back through [`Dialect::key_path`] cannot be named in the environment
/// at all — `#[serde(rename_all = "camelCase")]` produces exactly this, because an environment
/// key is folded to lower case on the way in and `distDir` never comes back. A table that
/// printed a spelling nobody can use would be worse than one that says there is none.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Key {
    /// The figment/TOML path, e.g. `csp.cloudflare.turnstile`.
    pub path: String,
    /// The environment variable supplying it directly, e.g. `PORTFOLIO_CSP__CLOUDFLARE__TURNSTILE`.
    pub env: Option<String>,
    /// The variable naming a *file* holding it, e.g. `PORTFOLIO_GITHUB__TOKEN_FILE`.
    pub env_file: Option<String>,
    /// The file name inside the secrets directory that supplies it, e.g. `github__token`.
    pub secrets_file: Option<String>,
    /// The `///` comment on the field. Empty when there was none.
    pub docs: String,
    /// The field's type as written, with any `Option<…>` stripped — `required` already says
    /// whether the key may be left out, so `Option<String>` would say it twice.
    ///
    /// Token text, not a resolved type: a type alias appears as its alias and `SecretString`
    /// appears as itself, because a derive has only tokens. That is what the field says, and
    /// inventing a type language that claimed otherwise would be worse than printing it.
    pub ty: Option<String>,
    /// The fixed set of values the key accepts, spelled as `serde` accepts them. Empty when the
    /// key is not a choice.
    pub values: Vec<String>,
    /// Other key paths that supply this same key, from `#[serde(alias = "…")]`.
    ///
    /// Full paths, so each one's environment and file spellings derive exactly as
    /// [`Self::path`]'s do. An alias left out of the schema is a spelling that works and is
    /// documented nowhere.
    pub aliases: Vec<String>,
    /// What the value is when nothing supplies it, rendered for display. [`None`] means unset —
    /// or, for a [`required`](Self::required) key, that there is no default to have.
    ///
    /// Filled in by [`Schema::with_defaults_from`]; the exact value, never prose.
    pub default: Option<String>,
    /// The `#[config(note = "…")]` prose explaining what that default *means*.
    ///
    /// Separate from [`Self::default`] because they answer different questions — `0` is what an
    /// operator compares against what they set, and "permanent" is why they would leave it
    /// alone. Collapsing them into one string, which is what this used to do, meant the observed
    /// value was discarded and had to be hand-written into the prose to appear at all.
    pub note: Option<String>,
    /// Whether loading fails when nothing supplies this key.
    pub required: bool,
    /// Whether the value is secret. Its default is never rendered, even if one was observed.
    pub secret: bool,
    /// Whether the loader reserves this key, so only the environment may supply it.
    pub reserved: bool,
}

/// What a variable the loader itself reads is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderRole {
    /// Names the TOML layer.
    Config,
    /// Names the secrets directory.
    SecretsDir,
    /// Read directly from the environment, so no file may supply it.
    Reserved,
}

/// A variable the loader reads to decide what the layers *are*, rather than a configuration key.
///
/// These never appear in the config struct, so no derive can find them — but an operator setting
/// the service up needs them more than they need any single key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVar {
    /// The full environment spelling, e.g. `PORTFOLIO_CONFIG`.
    pub env: String,
    /// What it is for.
    pub role: LoaderRole,
    /// What it does.
    pub docs: String,
    /// What the loader falls back to when it is unset, if anything.
    pub default: Option<String>,
}

/// The environment spelling this loader reads, minus the keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialectInfo {
    /// The prefix every configuration variable carries, e.g. `PORTFOLIO_`.
    pub prefix: String,
    /// What separates nesting levels in an environment key, e.g. `__`.
    pub nesting_separator: String,
    /// What marks a variable holding a path rather than a value, e.g. `_FILE`.
    pub indirection_suffix: String,
}

/// The whole configuration surface of one application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    /// The version of this document's shape. See [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// How keys are spelled in the environment.
    pub dialect: DialectInfo,
    /// The variables the loader reads before the layers exist.
    pub loader: Vec<LoaderVar>,
    /// Every key the configuration type can carry, in declaration order.
    pub keys: Vec<Key>,
}

impl Schema {
    /// The keys of `T`, spelled according to `dialect`.
    ///
    /// [`Terrace::schema`](crate::Terrace::schema) is the usual entry point: it fills in the
    /// loader variables as well, which a bare [`Dialect`] does not know about.
    ///
    /// # Panics
    /// As [`Sink::leaf`] and [`Sink::nested`]: two fields resolving to one key path, or a type
    /// that contains itself.
    #[must_use]
    pub fn describe<T: Describe + ?Sized>(dialect: &Dialect) -> Self {
        Self::describe_at::<T>(dialect, "")
    }

    /// The keys of `T`, as they are spelled when `T` sits at `root` in a larger configuration.
    ///
    /// A configuration type is rarely one struct in one file — it is a root that nests structs
    /// from other modules and other crates, each deriving [`Describe`] beside the code that
    /// consumes it. [`Self::describe`] on the root walks all of it, wherever it lives, because
    /// `#[config(nested)]` is a trait bound and follows the type rather than the file.
    ///
    /// This is for the other direction: documenting one subsystem on its own page, spelled the
    /// way an operator will have to spell it. `describe_at::<Csp>(dialect, "csp")` produces
    /// `csp.cloudflare.turnstile`, where `describe::<Csp>` alone would produce
    /// `cloudflare.turnstile` — a path that appears in no configuration file anywhere.
    ///
    /// An empty `root` is [`Self::describe`]. Use [`Self::subset`] instead when a whole schema
    /// has already been built and only part of it is wanted.
    ///
    /// # Panics
    /// As [`Sink::leaf`] and [`Sink::nested`]: two fields resolving to one key path, or a type
    /// that contains itself.
    #[must_use]
    pub fn describe_at<T: Describe + ?Sized>(dialect: &Dialect, root: &str) -> Self {
        let mut sink = Sink::new();
        sink.prefix = root
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        T::describe(&mut sink);

        let mut keys = sink.keys;
        for key in &mut keys {
            key.env = env_spelling(dialect, &key.path);
            key.env_file = key
                .env
                .as_ref()
                .map(|env| format!("{env}{}", dialect.indirection_suffix()))
                // The suffix is a parameter too, so a usable `env` does not make the indirection
                // variable usable — `_FILE` could just as easily be `=`.
                .filter(|name| is_settable_env_name(name));
            key.secrets_file = secrets_file_name(dialect, &key.path);
            key.reserved = key
                .env
                .as_deref()
                .is_some_and(|env| dialect.is_reserved(env));
            // A reserved key is read straight from the environment, so neither file mechanism
            // can supply it. Saying otherwise would document a path that errors on use.
            if key.reserved {
                key.env_file = None;
                key.secrets_file = None;
            }
        }

        Self {
            schema_version: SCHEMA_VERSION,
            dialect: DialectInfo {
                prefix: dialect.prefix().to_owned(),
                nesting_separator: dialect.separator().to_owned(),
                indirection_suffix: dialect.indirection_suffix().to_owned(),
            },
            loader: Vec::new(),
            keys,
        }
    }

    /// The part of this schema under `prefix`, spellings and all.
    ///
    /// For a documentation site that gives each subsystem its own page: build the whole schema
    /// once — which is the only way the paths are guaranteed to be the real ones — then slice it
    /// per page. `subset("csp")` keeps `csp.cloudflare.turnstile` and drops `github.token`; an
    /// empty `prefix` keeps everything.
    ///
    /// The loader variables are kept, because they are not part of any subtree and an operator
    /// reading one page still has to know where the configuration file comes from. A page that
    /// says so elsewhere renders [`Self::to_markdown_keys`] instead of [`Self::to_markdown`],
    /// which leaves them out without removing them from [`Self::to_json`]'s contract.
    #[must_use]
    pub fn subset(mut self, prefix: &str) -> Self {
        if !prefix.is_empty() {
            // The `.` matters: `csp` must not take `cspx.enabled` with it.
            let nested = format!("{prefix}.");
            self.keys
                .retain(|key| key.path == prefix || key.path.starts_with(&nested));
        }
        self
    }

    /// Fill in each key's default from a value representing "nothing was supplied".
    ///
    /// What that value is, is the caller's to decide, because only the caller knows: usually
    /// `T::default()`, but a type whose `Default` and whose `#[serde(default = "…")]` disagree
    /// should pass whatever `serde` would actually produce. Keys absent from it stay unset, and
    /// a `#[config(default = "…")]` override is left alone.
    ///
    /// A [`secret`](Key::secret) key renders `<redacted>` rather than its value. The rendering
    /// here is not a debugging aid — it is written into documentation, which is the last place a
    /// credential should end up, and a default that *is* a credential is exactly the case where
    /// this matters.
    ///
    /// # Errors
    /// Returns [`Error::Figment`] if `value` cannot be serialised.
    pub fn with_defaults_from<T: Serialize + ?Sized>(mut self, value: &T) -> Result<Self, Error> {
        let root = figment::value::Value::serialize(value).map_err(Box::new)?;
        for key in &mut self.keys {
            // A required key has no default *by definition* — loading fails when nothing
            // supplies it. Whatever `Default` happened to put in the field is an artefact of
            // constructing the value at all, and printing it as a default would tell an operator
            // they can leave the key out.
            //
            // A `note` is no longer a reason to skip: the prose explains what the value means and
            // the value is still the value, so both are reported. Skipping here — which is what
            // this did — threw the observed value away and left the prose as the only place it
            // could appear, hand-copied.
            if key.required {
                continue;
            }
            let Some(rendered) = root.find_ref(&key.path).and_then(|v| render_value(v, 0)) else {
                continue;
            };
            // Redaction after rendering, not before: a secret that is *unset* by default is not
            // a secret worth hiding, and `<redacted>` in place of "unset" would read as though
            // the service ships with a credential baked in.
            key.default = Some(if key.secret {
                "<redacted>".to_owned()
            } else {
                rendered
            });
        }
        Ok(self)
    }

    /// The schema as a JSON document — the machine-readable contract.
    ///
    /// Every field of every key, including the ones [`Self::to_markdown`] omits. Pretty-printed,
    /// because the usual consumer of the file is a diff.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if serialisation fails, which for this type means the JSON
    /// writer failed rather than the data being unrepresentable.
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self)
            .map_err(|e| Error::Invalid(format!("the schema could not be written as JSON: {e}")))
    }

    /// The schema as GitHub-flavoured Markdown, ready to paste into a README.
    ///
    /// Two tables: the variables the loader reads, then the configuration keys under
    /// [`Column::DEFAULT`]. Use [`Self::to_markdown_with`] to choose the columns.
    ///
    /// A [`Column::Docs`] cell carries the *summary* of the `///` comment — its first paragraph,
    /// as rustdoc means the word — rather than the whole of it. [`Key::docs`] keeps the whole
    /// text for [`Self::to_json`], so nothing is lost; a table cell is simply not where the four
    /// paragraphs below the summary belong.
    ///
    /// Ends with a newline, so appending another section needs no separator of its own.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        self.to_markdown_with(Column::DEFAULT)
    }

    /// Both tables, with a chosen set of key columns.
    ///
    /// The loader-variable table leads when there is one: its three columns are not the key
    /// columns, and an operator who cannot find `<PREFIX>CONFIG` cannot use any of the rest.
    ///
    /// [`Self::to_markdown_loader`] and [`Self::to_markdown_keys`] are the two halves on their
    /// own, for a page that wants them apart.
    ///
    /// Ends with a newline, as [`Self::to_markdown`] does.
    #[must_use]
    pub fn to_markdown_with(&self, columns: &[Column]) -> String {
        let loader = self.to_markdown_loader();
        let keys = self.to_markdown_keys(columns);
        if loader.is_empty() {
            keys
        } else {
            // The blank line between them: two tables run together are one malformed table.
            format!("{loader}\n{keys}")
        }
    }

    /// The loader-variable table alone.
    ///
    /// A documentation page with one key table per subsystem wants these variables once, not
    /// repeated above every table — and the subsystem pages want [`Self::to_markdown_keys`] with
    /// no loader table at all. Emitting the pair together is the common case, not the only one,
    /// so each half is reachable on its own rather than through clearing a field.
    ///
    /// Empty when the schema has no loader variables, which is what
    /// [`Schema::describe`](Self::describe) produces on its own — a header with no rows under it
    /// would be a table promising variables that do not exist.
    ///
    /// Ends with a newline when it is not empty.
    #[must_use]
    pub fn to_markdown_loader(&self) -> String {
        let mut out = String::new();
        if self.loader.is_empty() {
            return out;
        }

        out.push_str("| Variable | Role | Default | Purpose |\n");
        out.push_str("|---|---|---|---|\n");
        for var in &self.loader {
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} |",
                escape(&var.env),
                var.role.label(),
                optional_code(var.default.as_deref()),
                cell(&var.docs),
            );
        }

        out
    }

    /// The configuration-key table alone, with a chosen set of columns.
    ///
    /// The counterpart to [`Self::to_markdown_loader`]: the table for a page that documents one
    /// subsystem and has said where the configuration file comes from somewhere else.
    ///
    /// A schema with no keys still renders its header, unlike the loader table. An empty
    /// configuration section is a real shape — a subsystem that reads nothing yet — and the
    /// header is what says the section was generated rather than forgotten.
    ///
    /// Ends with a newline.
    #[must_use]
    pub fn to_markdown_keys(&self, columns: &[Column]) -> String {
        let mut out = String::new();
        let header: Vec<&str> = columns.iter().map(|c| c.heading()).collect();
        let _ = writeln!(out, "| {} |", header.join(" | "));
        let _ = writeln!(out, "|{}|", vec!["---"; columns.len()].join("|"));
        for key in &self.keys {
            let cells: Vec<String> = columns.iter().map(|c| c.render(key)).collect();
            let _ = writeln!(out, "| {} |", cells.join(" | "));
        }

        out
    }
}

impl LoaderRole {
    /// The word this role goes by in a rendered table.
    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::SecretsDir => "secrets dir",
            Self::Reserved => "reserved",
        }
    }
}

/// One column of the Markdown key table.
///
/// The full set is deliberately wider than [`Self::DEFAULT`]: everything is available to a
/// caller who wants it, and the default stays narrow enough to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Column {
    /// The TOML/figment key path.
    Path,
    /// What kind of value the key takes — its type, or the choices it accepts.
    Type,
    /// Other key paths that supply the same key.
    Aliases,
    /// The environment variable supplying the value directly.
    Env,
    /// The variable naming a file holding the value.
    EnvFile,
    /// The file name inside the secrets directory.
    SecretsFile,
    /// The value when nothing supplies the key, with its note in parentheses.
    Default,
    /// That value alone, with no note folded in. Pair it with [`Self::Note`].
    DefaultValue,
    /// The `#[config(note = "…")]` prose on its own, for a table that keeps the two apart.
    ///
    /// Pair it with [`Self::DefaultValue`], not [`Self::Default`], which already carries the
    /// note. A column that rendered differently depending on which *other* columns were asked
    /// for would be the kind of surprise a generated table cannot afford.
    Note,
    /// `required`, `secret` and `reserved`, collapsed into one cell.
    Flags,
    /// Whether the key must be supplied.
    Required,
    /// Whether the value is secret.
    Secret,
    /// The `///` comment.
    Docs,
}

impl Column {
    /// The columns [`Schema::to_markdown`] emits: everything an operator needs, and nothing that
    /// pushes the table past the width of a page.
    ///
    /// The two file spellings are left out because both are mechanical — [`Self::SecretsFile`]
    /// is `path` with the separator substituted, and [`Self::EnvFile`] is [`Self::Env`] with the
    /// dialect's documented suffix appended. Neither adds anything the reader cannot derive from
    /// a column already in front of them plus one sentence of prose, and dropping the pair keeps
    /// the table inside a page. Ask for either by name through [`Schema::to_markdown_with`].
    ///
    /// [`Self::Flags`] carries what [`Self::Required`] and [`Self::Secret`] would have taken two
    /// columns to say, and [`Self::Aliases`] is empty for almost every key.
    ///
    /// [`Self::Type`] *is* here, because without it a required key shows an em dash for its
    /// default and the reader has no way to tell whether to supply a string, a number or a list.
    pub const DEFAULT: &'static [Self] = &[
        Self::Path,
        Self::Type,
        Self::Env,
        Self::Default,
        Self::Flags,
        Self::Docs,
    ];

    fn heading(self) -> &'static str {
        match self {
            Self::Path => "TOML",
            Self::Type => "Type",
            Self::Aliases => "Also accepts",
            Self::Env => "Environment",
            Self::EnvFile => "File indirection",
            Self::SecretsFile => "Secrets file",
            Self::Default | Self::DefaultValue => "Default",
            Self::Note => "Note",
            Self::Flags => "Flags",
            Self::Required => "Required",
            Self::Secret => "Secret",
            Self::Docs => "Purpose",
        }
    }

    fn render(self, key: &Key) -> String {
        match self {
            // Escaped like every other cell. A key path is not prose, but it is not the
            // table author's to choose either — `#[serde(rename = "a|b")]` puts a cell
            // separator in it, and an unescaped one adds a column to the row.
            Self::Path => format!("`{}`", escape(&key.path)),
            // The choices when there are any, because `LogLevel` tells an operator nothing they
            // can act on and `trace | debug | info` tells them exactly what to type. The type
            // name stays in front of them, since it is what they will see in the source.
            Self::Type => match (&key.ty, key.values.as_slice()) {
                (_, []) => optional_code(key.ty.as_deref()),
                (ty, values) => {
                    let choices = values
                        .iter()
                        .map(|value| format!("`{}`", escape(value)))
                        .collect::<Vec<_>>()
                        .join(r" \| ");
                    match ty {
                        Some(ty) => format!("`{}`: {choices}", escape(ty)),
                        None => choices,
                    }
                }
            },
            Self::Aliases => {
                if key.aliases.is_empty() {
                    "—".to_owned()
                } else {
                    key.aliases
                        .iter()
                        .map(|alias| format!("`{}`", escape(alias)))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            Self::Env => optional_code(key.env.as_deref()),
            Self::EnvFile => optional_code(key.env_file.as_deref()),
            Self::SecretsFile => optional_code(key.secrets_file.as_deref()),
            // The exact value leads, because that is what an operator compares against what they
            // set; the note explains what it means. An unset default is written as prose rather
            // than as an empty code span, so `unset (ISR off)` reads as one phrase.
            Self::Default | Self::DefaultValue => {
                let value = match &key.default {
                    Some(default) => format!("`{}`", escape(default)),
                    None if key.required => "—".to_owned(),
                    None => "unset".to_owned(),
                };
                match (self, &key.note) {
                    (Self::Default, Some(note)) => format!("{value} ({})", cell(note)),
                    _ => value,
                }
            }
            Self::Flags => {
                let mut notes = Vec::new();
                if key.required {
                    notes.push("required");
                }
                if key.secret {
                    notes.push("secret");
                }
                if key.reserved {
                    notes.push("reserved");
                }
                if notes.is_empty() {
                    "—".to_owned()
                } else {
                    notes.join(", ")
                }
            }
            Self::Required => yes_or_dash(key.required),
            Self::Secret => yes_or_dash(key.secret),
            Self::Note => key.note.as_deref().map_or_else(|| "—".to_owned(), cell),
            Self::Docs => summary_cell(&key.docs),
        }
    }
}

fn yes_or_dash(flag: bool) -> String {
    if flag { "yes" } else { "—" }.to_owned()
}

/// A spelling as inline code, or an em dash when there is none.
fn optional_code(value: Option<&str>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("`{}`", escape(value)))
}

/// Prose in a table cell: newlines become breaks, and `|` stops ending the cell early.
fn cell(text: &str) -> String {
    if text.is_empty() {
        return "—".to_owned();
    }
    escape(text).replace('\n', "<br>")
}

/// A doc comment in a table cell: its summary, on one line.
///
/// The whole comment used to go in, which put every paragraph of a field's rustdoc into one cell
/// and made a table out of an essay. The fix is rustdoc's own convention rather than a new
/// annotation to keep in step: the first paragraph is the summary, and a comment written the way
/// rustdoc asks for one already reads correctly here with nothing to change.
///
/// [`Key::docs`] keeps the whole text, so the JSON contract loses nothing and a pipeline that
/// wants the paragraphs below the summary can still render them.
///
/// Soft wraps inside that paragraph become spaces rather than `<br>`: they are the author's line
/// width, not a break they asked a reader to see. A comment opening with a list or a fenced block
/// has no leading paragraph to find, so its first block comes out instead, flattened the same way.
fn summary_cell(text: &str) -> String {
    let summary: Vec<&str> = text
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .collect();
    if summary.is_empty() {
        return "—".to_owned();
    }
    escape(&summary.join(" "))
}

/// The characters that would otherwise be read as table structure.
fn escape(text: &str) -> String {
    text.replace('\\', r"\\").replace('|', r"\|")
}

/// A default value as a table would show it.
///
/// Returns [`None`] for a value that means "absent", so an explicit `null` and a missing key
/// render the same way — which is what they mean to an operator.
fn render_value(value: &figment::value::Value, depth: usize) -> Option<String> {
    use figment::value::{Empty, Num, Value};

    // The same bound as `Sink::nested`, for the same reason: `Value` is a tree, and a default
    // deep enough to overflow the stack here would be a denial of service in a doc generator.
    if depth > MAX_DEPTH {
        return Some("…".to_owned());
    }

    Some(match value {
        // Quoted, so an empty default is distinguishable from an absent one in a rendered cell.
        Value::String(_, s) if s.is_empty() => "\"\"".to_owned(),
        Value::String(_, s) => s.clone(),
        Value::Char(_, c) => c.to_string(),
        Value::Bool(_, b) => b.to_string(),
        Value::Num(_, n) => match n {
            Num::U8(v) => v.to_string(),
            Num::U16(v) => v.to_string(),
            Num::U32(v) => v.to_string(),
            Num::U64(v) => v.to_string(),
            Num::U128(v) => v.to_string(),
            Num::USize(v) => v.to_string(),
            Num::I8(v) => v.to_string(),
            Num::I16(v) => v.to_string(),
            Num::I32(v) => v.to_string(),
            Num::I64(v) => v.to_string(),
            Num::I128(v) => v.to_string(),
            Num::ISize(v) => v.to_string(),
            Num::F32(v) => v.to_string(),
            Num::F64(v) => v.to_string(),
        },
        Value::Empty(_, Empty::None | Empty::Unit) => return None,
        Value::Array(_, items) => {
            let rendered: Vec<String> = items
                .iter()
                .map(|item| render_value(item, depth + 1).unwrap_or_else(|| "unset".to_owned()))
                .collect();
            format!("[{}]", rendered.join(", "))
        }
        // A dict at a leaf means the field wanted `#[config(nested)]`. Rendered as a TOML inline
        // table rather than dropped, so the output shows what is actually there.
        Value::Dict(_, dict) => {
            let rendered: Vec<String> = dict
                .iter()
                .map(|(k, v)| {
                    let v = render_value(v, depth + 1).unwrap_or_else(|| "unset".to_owned());
                    format!("{k} = {v}")
                })
                .collect();
            format!("{{ {} }}", rendered.join(", "))
        }
    })
}

/// The alias names of a leaf, as full key paths under the currently open prefix.
fn alias_paths(prefix: &[String], aliases: &[&str]) -> Vec<String> {
    let mut head = prefix.join(".");
    if !head.is_empty() {
        head.push('.');
    }
    aliases
        .iter()
        .map(|alias| format!("{head}{alias}"))
        .collect()
}

/// Whether an operating system could hold an environment variable of this name.
///
/// Not a style rule — a hard limit. POSIX and Windows both forbid `=` in a name, and a NUL ends
/// the string that carries it, so `std::env::set_var` panics on either. A schema that printed
/// such a name would be pointing an operator at a variable that cannot be created, which is the
/// same false claim as printing the wrong one. Found by the `schema` fuzz target, which set every
/// spelling the schema reported and died on the first one containing a NUL.
fn is_settable_env_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['\0', '='])
}

/// Whether a directory could hold an entry of this name.
///
/// A secrets-directory key is one entry *in* a directory, so anything that would make it a path
/// instead — a separator — or that cannot be in a file name at all names no file that
/// `SecretsDir` could ever read back.
fn is_nameable_file(name: &str) -> bool {
    !name.is_empty() && !name.contains(['\0', '/', '\\'])
}

/// The environment spelling of `path`, when the environment can actually name it.
///
/// [`Dialect::env_spelling`] always produces *a* string; whether that string comes back as the
/// same key is the question. It does not when a path segment is not already lower case, when it
/// contains the separator, or when the whole name ends in the indirection suffix — the last
/// because such a variable is read as a path to a file rather than as a value.
fn env_spelling(dialect: &Dialect, path: &str) -> Option<String> {
    let name = dialect.env_spelling(path);
    if !is_settable_env_name(&name) {
        return None;
    }
    // Asked of the dialect rather than tested here, because "ends with the suffix" is not the
    // same question: `MYAPP_FILE` ends with `_FILE` and is still an ordinary key called `file`,
    // since the indirection layer needs something *between* the prefix and the suffix.
    if dialect.indirection_target(&name).is_some() {
        return None;
    }
    (env_layer_key(dialect, &name).as_deref() == Some(path)).then_some(name)
}

/// The key figment's environment layer makes of `name`, or [`None`] if it drops it.
///
/// Modelled on `Env::iter` rather than on [`Dialect::key_path`], because the environment layer is
/// figment's and does two things the dialect does not:
///
/// - **It trims.** The mapped key is trimmed at both ends, so a key path with surrounding
///   whitespace can never come back — `TEST_BY ` arrives as `by`, and a schema promising it would
///   supply `by ` sends an operator to set a variable that quietly fills in a different key. The
///   file layers do *not* trim, so a secrets-directory entry can still reach such a path; the two
///   mechanisms genuinely differ here and the schema has to say so separately.
/// - **It drops a key with an empty segment.** `a..b` and `.a` are refused outright rather than
///   nested, so no environment variable supplies them.
///
/// Found by the `schema` fuzz target, which set every spelling the schema reported and caught the
/// value landing at a neighbouring key.
fn env_layer_key(dialect: &Dialect, name: &str) -> Option<String> {
    let suffix = name.trim().strip_prefix(dialect.prefix())?;
    let mapped = suffix.replace(dialect.separator(), ".");
    let mapped = mapped.trim();
    if mapped.split('.').any(str::is_empty) {
        return None;
    }
    Some(mapped.to_ascii_lowercase())
}

/// The secrets-directory file name for `path`, when one can name it.
///
/// A secrets-directory entry is the key with the nesting separator substituted for `.`, no
/// prefix — and a name containing `.` is refused outright, which is what makes a separator
/// containing one produce no answer here.
fn secrets_file_name(dialect: &Dialect, path: &str) -> Option<String> {
    let name = path.replace('.', dialect.separator());
    if name.contains('.') || !is_nameable_file(&name) {
        return None;
    }
    (dialect.key_path(&name) == path).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{Dialect, env_spelling, escape, render_value, secrets_file_name};

    #[test]
    fn a_lower_case_path_gets_all_three_spellings() {
        let dialect = Dialect::new("TEST_");
        assert_eq!(
            env_spelling(&dialect, "auth.jwt_secret").as_deref(),
            Some("TEST_AUTH__JWT_SECRET")
        );
        assert_eq!(
            secrets_file_name(&dialect, "auth.jwt_secret").as_deref(),
            Some("auth__jwt_secret")
        );
    }

    /// `#[serde(rename_all = "camelCase")]` names a key the environment layer cannot reach:
    /// figment folds an environment key to lower case, so `distDir` never comes back.
    #[test]
    fn a_path_that_does_not_survive_the_fold_has_no_environment_spelling() {
        let dialect = Dialect::new("TEST_");
        assert_eq!(env_spelling(&dialect, "assets.distDir"), None);
        assert_eq!(secrets_file_name(&dialect, "assets.distDir"), None);
    }

    /// A key literally ending in `_file` collides with the indirection mechanism: setting the
    /// variable makes the loader try to *read* the value as a path.
    #[test]
    fn a_key_ending_in_the_indirection_suffix_has_no_environment_spelling() {
        let dialect = Dialect::new("TEST_");
        assert_eq!(env_spelling(&dialect, "unit.file"), None);
        assert_eq!(
            env_spelling(&dialect, "unit.filename").as_deref(),
            Some("TEST_UNIT__FILENAME")
        );
    }

    /// A separator containing `.` cannot be spelled as a secrets-directory file name at all —
    /// `SecretsDir::read` refuses any entry whose name contains one.
    #[test]
    fn a_dotted_separator_has_no_secrets_file_spelling() {
        let dialect = Dialect::new("TEST_").nesting_separator(".");
        assert_eq!(secrets_file_name(&dialect, "auth.jwt"), None);
    }

    #[test]
    fn absent_and_null_defaults_render_the_same() {
        let null = figment::value::Value::serialize(Option::<u8>::None).unwrap();
        assert_eq!(render_value(&null, 0), None);
    }

    #[test]
    fn a_pipe_in_a_doc_comment_does_not_end_the_cell() {
        assert_eq!(escape("a | b"), r"a \| b");
        assert_eq!(escape(r"a \ b"), r"a \\ b");
    }
}
