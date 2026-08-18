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
//! # Four renderings, one walk
//!
//! Walking the type is the expensive half and it happens once. What comes out of it is a
//! [`Schema`], and every artefact a service hand-maintains beside its configuration is a
//! rendering of that one value:
//!
//! | Rendering | For | Replaces |
//! |---|---|---|
//! | [`Schema::to_json`] | a documentation pipeline | — |
//! | [`Schema::to_markdown`] | `>> README.md` | the hand-written reference table |
//! | [`Schema::to_toml_example`] | the operator | `config.example.toml` |
//! | [`Schema::to_json_schema`] | an editor, or a Helm chart's `values.schema.json` | nothing that existed |
//! | [`Schema::into_contract`] | the image, and whatever deploys it | nothing that existed |
//!
//! [`Schema::to_json`] is the contract: a versioned document with every field of every key,
//! including the ones the other three leave out. Point a documentation pipeline at it and render
//! whatever that pipeline wants.
//!
//! [`Schema::to_markdown`] emits GitHub-flavoured tables that can be pasted in unmodified. Its
//! documentation column carries the summary — the first paragraph — of each `///` comment, on
//! rustdoc's own convention: whatever a field says below its summary line belongs on the field's
//! own documentation page, not inside a table cell, and [`Schema::to_json`] still carries all of
//! it.
//!
//! [`Schema::to_toml_example`] emits the file itself: every key as a comment carrying its
//! purpose, its type and its environment spelling, above an assignment showing the value it
//! already has. A key with a default is commented out, because setting it changes nothing; a
//! required key is not, because the file does not load without it. A secret is a placeholder,
//! never a value — this file is committed, and [`Key::secret`] exists to say which keys must not
//! be in it.
//!
//! [`Schema::to_json_schema`] emits a JSON Schema over the same keys, which is what makes an
//! editor complete and validate the TOML file, and what a Helm chart's `values.schema.json`
//! consumes. It is the only rendering that has to *interpret* [`Key::ty`] — see [`JsonSchema`]
//! for how far that interpretation goes and where it stops.
//!
//! [`Schema::into_contract`] is the fifth and the odd one out: not a rendering of the schema but a
//! document *containing* two of them, published with the image rather than with the source. It
//! exists because the reader is a deployment pipeline holding an image digest and nothing else —
//! see [`Contract`] for what that reader needs that no single rendering above supplies, and
//! [`External`] for the variables an image reads that no derive can find.
//!
//! # More than one root
//!
//! A workspace whose binaries read different parts of one configuration has no single root type
//! to describe. [`Schema::merge`] unions the schemas of the roots those binaries actually load,
//! so the document stays tied to them instead of to an aggregate type invented for the generator.
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

mod contract;
mod json_schema;
mod markdown;
mod rust_type;
mod toml_example;
mod tree;

pub use contract::{
    ARTIFACT_TYPE, App, CONTRACT_VERSION, Contract, ContractBuilder, DEFAULT_PATH, External,
    ExternalVar, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION, Unknown,
};
pub use json_schema::{DRAFT_07, DRAFT_2020_12, JsonSchema};
pub use markdown::Column;
pub use terrace_config_macros::Describe;
pub use toml_example::TomlExample;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::dialect::Dialect;
use crate::error::Error;

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
            env_aliases: Vec::new(),
            env_file_aliases: Vec::new(),
            secrets_file_aliases: Vec::new(),
            // Filled in by `describe_at`, beside the spellings: all three are derived from what
            // the derive collected rather than collected themselves, and doing it in one place is
            // what keeps a hand-built `Sink` from producing a key whose constraint disagrees with
            // its type.
            constraint: None,
            text_constraint: None,
            text_form: TextForm::Unknown,
            default: None,
            default_value: None,
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
// No `Eq`: [`Self::default_value`] can hold a float, and a float is not reflexively equal to
// itself. Deriving the marker anyway would be a claim about `NaN` that this type cannot keep.
//
// `non_exhaustive` because this type grows: `constraint` and `text_constraint` were both added
// after it shipped, and the document's own versioning rule says an added field is not a breaking
// change. It cannot be one for a consumer of the *JSON* and a breaking change for a consumer of
// the *struct* — so construction goes through [`Sink::leaf`], which is where the invariants
// between these fields are maintained anyway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
    /// What a value of this key must be **once it is in the document**, as JSON Schema keywords
    /// — `type`, `enum`, the numeric bounds, `items`.
    ///
    /// Document space, not text space. A TOML file holds an integer and this describes it; an
    /// environment variable holds `"0"`, which fails `{"type": "integer"}` under every conforming
    /// validator. [`Self::text_constraint`] is the one that applies there, and the two are
    /// complementary rather than alternatives — a consumer checking a variable applies the text
    /// constraint to the raw characters and this one to whatever the parse produced.
    ///
    /// [`Schema::to_json_schema`] carries the same keywords *nested*, at the key's position in the
    /// document. This carries them flat, and the reason is the environment: a consumer checking
    /// [`Self::env`] has a variable name and a string, not a document, and digging the constraint
    /// out of a nested schema by dotted path is a step every consumer would reimplement. Without
    /// it they reimplement something worse — a vocabulary of Rust type names, in whatever language
    /// they are written in, with `PathBuf` as the trap: it is a string and nothing in the name
    /// says so.
    ///
    /// [`None`] means unconstrained, and says exactly as much as [`Self::ty`] does about a domain
    /// newtype: the key exists and nothing here can check its value.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub constraint: Option<serde_json::Value>,
    /// What the **unparsed text** supplying this key must be, for the layers that supply text.
    ///
    /// [`Self::env`] is the layer this is about. It is emitted only where it says something the
    /// text does not already say: a pattern for an integer, the two spellings of a boolean, the
    /// variants of a choice. [`None`] means the text is unconstrained — which is the answer for
    /// every string-like type, since an environment value is a string to begin with.
    ///
    /// [`None`] here means there is no pattern to match, which [`Self::text_form`] disambiguates:
    /// [`TextForm::Text`] means any text is fine, and [`TextForm::Unknown`] means nothing could be
    /// said. The two used to be indistinguishable, and a list-typed key was the case that made the
    /// difference cost a deployment.
    ///
    /// **It constrains form, not range.** A pattern matches characters, so `99999` is a
    /// well-formed integer and only [`Self::constraint`]'s `maximum` catches it not fitting a
    /// `u16` — which means a validator has to do both, in the order
    /// [`External`] sets out: match the text, parse it by the form that
    /// matched, then check the parsed value. Doing only the first leaves every bound in this
    /// document decorative.
    ///
    /// And a 64-bit range is not checkable from this document at all, by either constraint:
    /// `u64::MAX` is not representable as an IEEE double, so no `maximum` is published rather than
    /// one that is a different number than the type accepts. A `u64` key given
    /// `18446744073709551616` satisfies everything here and still fails to load. Loading the
    /// configuration with the real binary is what closes that gap, and no arrangement of these
    /// fields would.
    ///
    /// **The file layers are a different question, and the answer is usually "not at all".**
    /// [`Self::secrets_file`] and [`Self::env_file`] deliver their contents as strings with no
    /// parse, and `Figment::extract` does not coerce a string into a number or a boolean — so a
    /// key whose [`Self::constraint`] is anything but a string type cannot be supplied by either,
    /// whatever the file contains. That is deliberate, and [`provider`](crate::provider) explains
    /// it: those layers exist to carry secrets, and a secret is an opaque byte string. A chart
    /// mounting a key-named file for a numeric key has made a mistake no file contents can fix.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text_constraint: Option<serde_json::Value>,
    /// How to read the text this key is supplied as. See [`TextForm`].
    ///
    /// Always present, which is what makes it usable as the discriminator: a consumer reads this
    /// to decide how to parse before checking [`Self::constraint`], instead of inferring the parse
    /// from which keywords [`Self::text_constraint`] happens to carry.
    #[serde(default)]
    pub text_form: TextForm,
    /// Other key paths that supply this same key, from `#[serde(alias = "…")]`.
    ///
    /// Full paths. An alias left out of the schema is a spelling that works and is documented
    /// nowhere — which is why the spellings derived from these are published too, rather than
    /// left to a consumer to derive: see [`Self::env_aliases`].
    pub aliases: Vec<String>,
    /// Every *other* environment spelling this key answers to, one per [`Self::aliases`] entry
    /// that has one.
    ///
    /// [`Self::env`] is the spelling derived from [`Self::path`]; these are the spellings derived
    /// from the aliases, and **the loader reads all of them equally**. A validator deciding
    /// whether a variable is a key has to check these too.
    ///
    /// It is a membership set, not a parallel array: an alias whose path cannot be spelled in the
    /// environment at all contributes nothing here, so the indices do not line up with
    /// [`Self::aliases`] and nothing should assume they do.
    ///
    /// An alias is what a maintainer adds when renaming a key so that existing deployments keep
    /// working. Publishing only the canonical spelling turns that compatibility shim into a hard
    /// failure: a chart still using the old name is a *correct* deployment, and a gate that
    /// refuses it is refusing the one thing that made the rename safe.
    ///
    /// Always serialised, empty or not — unlike [`Self::constraint`] and [`Self::default`], which
    /// are omitted when unset. There, absence *means* something: no check is possible, no default
    /// exists. Here an absent list and an empty one say the same thing, so omitting it would save
    /// bytes at the cost of a distinction a reader has to discover is not one. And this is the hot
    /// path — steps 2 and 3 of the ordered list consult it for every variable on every container —
    /// which is the worst place to hand a consumer two shapes.
    #[serde(default)]
    pub env_aliases: Vec<String>,
    /// The `_FILE` spelling of each of [`Self::env_aliases`], where the dialect permits one.
    ///
    /// Always serialised, for [`Self::env_aliases`]' reason.
    #[serde(default)]
    pub env_file_aliases: Vec<String>,
    /// Every *other* secrets-directory file name this key answers to, one per alias that has one.
    ///
    /// Same reasoning as [`Self::env_aliases`], and measured the same way: a file named for an
    /// alias supplies the key exactly as one named for [`Self::path`] does.
    ///
    /// Worth knowing when checking for a key supplied twice: the loader's own shadow check
    /// compares *spellings*, so a canonical variable against an alias-named file is not the pair
    /// it reports. `serde` still refuses the load — with `duplicate field`, naming neither source
    /// — so the diagnostic that names both is the one a consumer builds from these fields.
    ///
    /// Always serialised, for [`Self::env_aliases`]' reason.
    #[serde(default)]
    pub secrets_file_aliases: Vec<String>,
    /// What the value is when nothing supplies it, rendered for display. [`None`] means unset —
    /// or, for a [`required`](Self::required) key, that there is no default to have.
    ///
    /// Filled in by [`Schema::with_defaults_from`]; the exact value, never prose.
    ///
    /// "Nothing supplies it" means nothing in the *program*. A container image that sets the key's
    /// environment variable in its own `ENV` block supplies it on every run, so what an operator
    /// omitting the key actually gets is this value only when the image is silent about it. The
    /// derive cannot see an `ENV` line, so a [`Contract`] is a claim about the code's defaults and
    /// not about the image's — see [`Contract`] for where that gap is recorded.
    pub default: Option<String>,
    /// That same default as the value it *is*, rather than as the text a table prints.
    ///
    /// The two are not redundant, because the rendering is lossy on purpose: a table cell reads
    /// better as `public` than as `"public"`, and `[a, b]` than as `["a", "b"]`. That is the
    /// right trade for a cell and the wrong one for a file — a generated `config.toml` in which
    /// a string default lost its quotes does not parse, and one in which a number gained them
    /// does not load. [`Schema::to_toml_example`] and [`Schema::to_json_schema`] therefore read
    /// this, and [`Schema::to_markdown`] reads [`Self::default`].
    ///
    /// [`None`] whenever [`Self::default`] is, and *also* when the key is
    /// [`secret`](Self::secret): the display string redacts the value, so keeping the value
    /// itself beside it would hand every consumer of the document the credential the redaction
    /// exists to withhold.
    pub default_value: Option<figment::value::Value>,
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

/// What form the text supplying a key takes, and so how to read it.
///
/// The reason this is a field rather than something a consumer infers from
/// [`Key::text_constraint`]: inference works while the crate emits two shapes and becomes a guess
/// the moment it emits a third — which it now does. And a bare [`None`] constraint used to mean
/// two incompatible things, "any text is fine" and "this needs a structured literal nobody
/// described"; a chart setting `PORTFOLIO_GITHUB__REPOS=a,b` passed every gate and failed at boot
/// because a validator could not tell them apart. This is what tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TextForm {
    /// Any text. A `String`, a `PathBuf`, a `SecretString` — nothing to check and nothing to
    /// parse.
    ///
    /// Only for types that genuinely accept anything. A type whose `Deserialize` parses and can
    /// refuse — `IpAddr`, `Url`, `char` — is [`Self::Unknown`], because `Text` promises a check
    /// was not needed and the loader would be contradicting it.
    Text,
    /// Digits with an optional sign. Read it as an integer, then check [`Key::constraint`], which
    /// is where a `minimum` and `maximum` live.
    Integer,
    /// `true` or `false`, and nothing else — not `TRUE`, not `1`, not `yes`.
    ///
    /// Surrounding whitespace is permitted for [`Self::Choice`]'s reason and measured the same
    /// way: `"true "` and `" false"` both load through the environment layer.
    Boolean,
    /// One of [`Key::values`], spelled as `serde` accepts it.
    ///
    /// [`Key::constraint`] carries the bare set, because a TOML document must spell a variant
    /// exactly. [`Key::text_constraint`] carries the same set with surrounding whitespace
    /// permitted, because figment's `Env` provider trims before it compares — measured, `"info "`
    /// and `"\tinfo\n"` both load. The two differ, which is why they are two fields.
    Choice,
    /// A TOML literal: an array, an inline table. Only the environment layer can carry one at all
    /// — the file layers deliver text with no parse, so a key of this form cannot be supplied by a
    /// secrets file or a `_FILE` path whatever it contains. That is a consequence of the general
    /// rule rather than a rule of its own: an array is not a string in document space, and
    /// [`Key::constraint`] is where "can a file supply this key" is answered. **Do not read this
    /// form as that rule** — a key can be [`Self::Unknown`] and still mount from a file, which is
    /// every type whose `Deserialize` parses a string.
    ///
    /// The one form whose second step needs a parser. [`Self::Integer`], [`Self::Boolean`] and
    /// [`Self::Choice`] are read with any language's own primitives; reading one of these means
    /// parsing TOML. A consumer without a TOML parser can still apply
    /// [`Key::text_constraint`] — the bracket form is the half that catches `a,b` — and should
    /// skip [`Key::constraint`] rather than guess at the value.
    Structured,
    /// Nothing certain is known: a domain newtype, a float, a type this crate does not interpret.
    /// No check is possible, and unlike [`Self::Text`] that is a gap rather than an answer.
    ///
    /// Also where a form a *later* version of this crate emits lands, which is the same answer for
    /// the same reason: a check exists and this document cannot describe it to you. Without the
    /// fallback, one unfamiliar form on one key would make the whole document unreadable — see
    /// [`CONTRACT_VERSION`].
    #[default]
    #[serde(other)]
    Unknown,
}

/// What a variable the loader itself reads is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LoaderRole {
    /// Names the TOML layer.
    Config,
    /// Names the secrets directory.
    SecretsDir,
    /// Read directly from the environment, so no file may supply it.
    Reserved,
    /// A role a later version of this crate emits and this one has no name for.
    ///
    /// A *new* variant rather than folding into one of the three above, and the distinction is
    /// load-bearing. Every role means "the loader reads this variable", so the ordered list's step
    /// 1 is satisfied by [`LoaderVar::env`] alone whatever this says — but a consumer looking for
    /// the secrets directory matches on the role, and an unknown one read as [`Self::Config`]
    /// would hand it the wrong variable. This keeps step 1 working and that lookup honest.
    #[serde(other)]
    Other,
}

/// A variable the loader reads to decide what the layers *are*, rather than a configuration key.
///
/// These never appear in the config struct, so no derive can find them — but an operator setting
/// the service up needs them more than they need any single key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DialectInfo {
    /// The prefix every configuration variable carries, e.g. `PORTFOLIO_`.
    pub prefix: String,
    /// What separates nesting levels in an environment key, e.g. `__`.
    pub nesting_separator: String,
    /// What marks a variable holding a path rather than a value, e.g. `_FILE`.
    pub indirection_suffix: String,
}

/// The whole configuration surface of one application.
// No `Eq`, for [`Key`]'s reason: a schema holds keys, and a key can hold a float default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            key.constraint = json_schema::constraint(key.ty.as_deref(), &key.values)
                .map(serde_json::Value::Object);
            let (form, text) = json_schema::text_constraint(key.ty.as_deref(), &key.values);
            key.text_form = form;
            key.text_constraint = text.map(serde_json::Value::Object);
            key.env = env_spelling(dialect, &key.path);
            key.env_file = key
                .env
                .as_ref()
                .map(|env| format!("{env}{}", dialect.indirection_suffix()))
                // The suffix is a parameter too, so a usable `env` does not make the indirection
                // variable usable — `_FILE` could just as easily be `=`.
                .filter(|name| is_settable_env_name(name));
            key.secrets_file = secrets_file_name(dialect, &key.path);

            // The same three derivations over the aliases, because the loader answers to all of
            // them. Deriving them here rather than describing the rule and letting each consumer
            // apply it is the reason `secrets_file` is a field at all: a derivation written in
            // prose is a derivation every implementation gets slightly differently wrong.
            key.env_aliases = key
                .aliases
                .iter()
                .filter_map(|alias| env_spelling(dialect, alias))
                .collect();
            key.env_file_aliases = key
                .env_aliases
                .iter()
                .map(|env| format!("{env}{}", dialect.indirection_suffix()))
                .filter(|name| is_settable_env_name(name))
                .collect();
            key.secrets_file_aliases = key
                .aliases
                .iter()
                .filter_map(|alias| secrets_file_name(dialect, alias))
                .collect();

            key.reserved = key
                .env
                .as_deref()
                .is_some_and(|env| dialect.is_reserved(env));
            // A reserved key is read straight from the environment, so neither file mechanism
            // can supply it. Saying otherwise would document a path that errors on use — and the
            // alias spellings of one are file mechanisms just as much as the canonical.
            if key.reserved {
                key.env_file = None;
                key.secrets_file = None;
                key.env_file_aliases.clear();
                key.secrets_file_aliases.clear();
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
    /// A [`secret`](Key::secret) key renders `<redacted>` rather than its value, and keeps no
    /// [`default_value`](Key::default_value) at all. The rendering here is not a debugging aid —
    /// it is written into documentation, which is the last place a credential should end up, and
    /// a default that *is* a credential is exactly the case where this matters.
    ///
    /// A type holding a secret is the case this bound bites on: `secrecy::SecretString` refuses
    /// to implement [`Serialize`] on purpose, so a config struct containing one cannot derive it
    /// either. Mark the field `#[serde(skip_serializing)]` — a secret has no default worth
    /// printing, and [`Key::secret`] would redact it anyway — or, when the type is not yours to
    /// annotate, build the value yourself and use [`Self::with_defaults_from_value`].
    ///
    /// # Errors
    /// Returns [`Error::Figment`] if `value` cannot be serialised.
    pub fn with_defaults_from<T: Serialize + ?Sized>(self, value: &T) -> Result<Self, Error> {
        let root = figment::value::Value::serialize(value).map_err(Box::new)?;
        Ok(self.with_defaults_from_value(&root))
    }

    /// Fill in each key's default from an already-serialised value.
    ///
    /// [`Self::with_defaults_from`] is this with the serialisation done for you, and is what a
    /// caller who owns the config type should reach for. This is the escape hatch for the case
    /// where the root type cannot implement [`Serialize`] at all — a field whose type is from
    /// another crate and refuses to, `secrecy::SecretString` being the one that turns up in a
    /// configuration — and the value has to be assembled by hand:
    ///
    /// ```
    /// # use terrace_config::Terrace;
    /// use figment::value::{Dict, Value};
    ///
    /// # struct Config;
    /// # impl terrace_config::schema::Describe for Config {
    /// #     fn describe(sink: &mut terrace_config::schema::Sink) {
    /// #         sink.leaf(terrace_config::schema::Leaf {
    /// #             name: "ttl_secs", docs: "", ty: Some("u64"), values: None,
    /// #             aliases: &[], note: None, required: false, secret: false,
    /// #         });
    /// #     }
    /// # }
    /// let mut defaults = Dict::new();
    /// defaults.insert("ttl_secs".to_owned(), Value::from(0u64));
    ///
    /// let schema = Terrace::new("MYSERVICE_")
    ///     .schema::<Config>()
    ///     .with_defaults_from_value(&Value::from(defaults));
    ///
    /// assert_eq!(schema.keys[0].default.as_deref(), Some("0"));
    /// ```
    ///
    /// `figment::util::nest` builds the same thing from a dotted path when the keys are nested.
    ///
    /// The rules are [`Self::with_defaults_from`]'s, because that is a thin wrapper over this: a
    /// required key keeps no default, a [`secret`](Key::secret) one renders `<redacted>`, and a
    /// path `root` does not carry stays unset.
    #[must_use]
    pub fn with_defaults_from_value(mut self, root: &figment::value::Value) -> Self {
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
            let Some(observed) = root.find_ref(&key.path) else {
                continue;
            };
            let Some(rendered) = render_value(observed, 0) else {
                continue;
            };
            // Redaction after rendering, not before: a secret that is *unset* by default is not
            // a secret worth hiding, and `<redacted>` in place of "unset" would read as though
            // the service ships with a credential baked in.
            if key.secret {
                key.default = Some("<redacted>".to_owned());
                continue;
            }
            key.default = Some(rendered);
            // Kept beside the rendering rather than in place of it: the renderings that write a
            // *file* need the value's type, and the one that writes a table cell needs it gone.
            key.default_value = Some(observed.clone());
        }
        self
    }

    /// The union of this schema and `other`.
    ///
    /// A workspace whose binaries each read part of one configuration surface has no single root
    /// type to describe — the point of the split is that neither binary sees the other's keys.
    /// Describing each root and merging is how such a workspace gets one document without
    /// inventing an aggregate type that exists only for the generator and can silently drift from
    /// every root it stands in for.
    ///
    /// `self`'s keys keep their order and `other`'s new ones are appended, so declaration order
    /// survives within each half. A key both halves describe is kept once — two binaries reading
    /// one shared key is the normal case, not a mistake.
    ///
    /// ```
    /// # use terrace_config::Terrace;
    /// # use terrace_config::schema::{Describe, Leaf, Sink};
    /// # struct Csp;
    /// # impl Describe for Csp {
    /// #     fn describe(sink: &mut Sink) {
    /// #         sink.leaf(Leaf { name: "csp", docs: "", ty: None, values: None,
    /// #             aliases: &[], note: None, required: false, secret: false });
    /// #     }
    /// # }
    /// # struct Github;
    /// # impl Describe for Github {
    /// #     fn describe(sink: &mut Sink) {
    /// #         sink.leaf(Leaf { name: "github", docs: "", ty: None, values: None,
    /// #             aliases: &[], note: None, required: false, secret: false });
    /// #     }
    /// # }
    /// let terrace = Terrace::new("PORTFOLIO_");
    /// let everything = terrace.schema::<Csp>().merge(terrace.schema::<Github>());
    ///
    /// assert_eq!(everything.keys.len(), 2);
    /// ```
    ///
    /// # Panics
    /// If the two schemas disagree — a different [`schema_version`](Self::schema_version) or
    /// [`dialect`](Self::dialect), or one key path or loader variable described differently on
    /// each side. Merging those would produce a document carrying two answers to one question,
    /// and the same reasoning applies as in [`Sink::leaf`]: a table that quietly picks one is
    /// worse than one that refuses to be generated. Describing both halves from the same
    /// [`Terrace`](crate::Terrace) rules out the version and dialect cases; a path described
    /// differently by two roots is a real disagreement between them, and is meant to be loud.
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        assert_eq!(
            self.schema_version, other.schema_version,
            "two schemas of different versions cannot be merged: one of them describes a \
             document shape the other does not."
        );
        assert_eq!(
            self.dialect, other.dialect,
            "two schemas of different dialects cannot be merged: every environment spelling in \
             the result would have to be read under two different sets of rules."
        );

        for var in other.loader {
            match self.loader.iter().find(|held| held.env == var.env) {
                Some(held) => assert!(
                    *held == var,
                    "`{}` is described twice and differently, so the merged schema cannot say \
                     which description the documentation is about.",
                    var.env
                ),
                None => self.loader.push(var),
            }
        }

        for key in other.keys {
            match self.keys.iter().find(|held| held.path == key.path) {
                // Identical is the shared-key case and is kept once. Anything else is two
                // descriptions of one key, which is the collision `Sink::leaf` refuses.
                Some(held) => assert!(
                    *held == key,
                    "`{}` is described twice and differently, so the merged schema cannot say \
                     which description the documentation is about.",
                    key.path
                ),
                None => self.keys.push(key),
            }
        }

        self
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
}

impl LoaderRole {
    /// The word this role goes by in a rendering.
    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::SecretsDir => "secrets dir",
            Self::Reserved => "reserved",
            Self::Other => "other",
        }
    }
}

/// How much of a key's `///` comment a rendering carries.
///
/// Rustdoc's own convention, made a parameter: the first paragraph of a comment is its summary
/// and the rest is detail. Which of the two a rendering wants is a property of the *rendering*
/// rather than of the comment — a Markdown table cell has a page width to stay inside, a
/// generated `config.toml` has a reader who is about to edit the key, and a JSON Schema
/// `description` is read in an editor's hover where length costs nothing.
///
/// [`Key::docs`] keeps the whole text whatever this is set to, so nothing chosen here is lost
/// from [`Schema::to_json`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Docs {
    /// Leave the comment out.
    None,
    /// The summary alone: the first paragraph, soft wraps flattened to spaces.
    #[default]
    Summary,
    /// The whole comment, its paragraphs and line breaks intact.
    Full,
}

impl Docs {
    /// The text this setting takes from `docs`, or [`None`] when there is nothing to take.
    fn of(self, docs: &str) -> Option<String> {
        let text = match self {
            Self::None => return None,
            Self::Summary => summary(docs),
            Self::Full => docs.trim_end().to_owned(),
        };
        (!text.is_empty()).then_some(text)
    }
}

/// The summary of a `///` comment: its first paragraph, on one line.
///
/// Soft wraps inside that paragraph become spaces rather than breaks — they are the author's line
/// width, not a break they asked a reader to see. A comment opening with a list or a fenced block
/// has no leading paragraph to find, so its first block comes out instead, flattened the same way.
fn summary(docs: &str) -> String {
    docs.lines()
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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
    use super::{Dialect, env_spelling, render_value, secrets_file_name};

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
}
