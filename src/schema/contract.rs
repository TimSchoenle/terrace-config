//! The document a build attaches to its image, and a deployment pipeline reads back.
//!
//! The other four renderings describe a configuration to somebody who already has the source.
//! This one is for the machine that does not: a Helm chart's CI job, holding an image digest and
//! a `config.toml` it rendered itself, with no way to know whether the two agree.
//!
//! # Why an envelope rather than a file per rendering
//!
//! Two of the renderings are machine-readable and neither subsumes the other:
//!
//! - [`Schema::to_json`] carries every key in every spelling — the environment variable, the
//!   `_FILE` variable, the secrets-directory file name — which is what lets a validator check the
//!   *environment* a chart sets and the *secret files* it mounts, not only the file it renders.
//! - [`Schema::to_json_schema`] carries none of those, and is the only half a stock JSON Schema
//!   validator can act on.
//!
//! Published as two artefacts they are two hashes, two fetches, and two chances to be half-stale.
//! [`Contract`] is both in one document, under one hash, so "the contract for this image digest"
//! names exactly one thing.
//!
//! # The part no derive can see
//!
//! A service reads variables that are not its configuration. The Dioxus toolchain reads `PORT`,
//! `IP` and `RUST_LOG` before any of this crate's layers exist; a base image contributes `PATH`
//! and `SSL_CERT_FILE`. None of them carry the loader's prefix, so no [`Describe`](super::Describe)
//! implementation can report them — and a validator that flags every variable it cannot account
//! for would flag all of them.
//!
//! [`External`] is where those are declared, and it is deliberately a *positive* declaration
//! rather than a suppression list. A variable named in [`External::env`] is checked like any
//! other key: its type is known, so a chart passing `PORT: "http"` fails the same gate that a
//! chart passing `PORTFOLIO_ISR__TTL_SECS: "soon"` fails. Only [`External::ignore`] suppresses,
//! and it exists for the variables that genuinely have no owner here — an operator's `TZ`, a base
//! image's `PATH`.
//!
//! ```
//! # use terrace_config::Terrace;
//! # use terrace_config::schema::{App, Describe, External, ExternalVar, Leaf, Sink};
//! # struct Config;
//! # impl Describe for Config {
//! #     fn describe(sink: &mut Sink) {
//! #         sink.leaf(Leaf { name: "dist_dir", docs: "", ty: Some("String"), values: None,
//! #             aliases: &[], note: None, required: false, secret: false });
//! #     }
//! # }
//! let contract = Terrace::new("PORTFOLIO_")
//!     .schema::<Config>()
//!     .into_contract(App::new("portfolio").version("v2.5.0"))
//!     .external(
//!         External::new()
//!             .var(
//!                 ExternalVar::new("PORT")
//!                     .ty("u16")
//!                     .docs("Bind port. Read by the Dioxus toolchain, not by this loader.")
//!                     .owner("dioxus"),
//!             )
//!             .var(ExternalVar::new("RUST_LOG").ty("String").owner("tracing"))
//!             .ignore("TZ"),
//!     )
//!     .build()?;
//!
//! println!("{}", contract.to_json()?);
//! # Ok::<(), terrace_config::Error>(())
//! ```
//!
//! # What it is not
//!
//! It is not a signature, and it is not evidence of anything on its own. A contract is only worth
//! reading when it is known to belong to the image being deployed, which is a property of how it
//! was published — attached to a digest, signed — rather than of what it says. [`ARTIFACT_TYPE`]
//! and the `dev.terrace.config.*` labels are the two halves of that publication, and they are
//! constants here so that the producer and the consumer cannot spell them differently.
//!
//! Nothing inside the document names the image, and that is deliberate rather than an omission:
//! the tie is the attachment. A consumer asks a digest for its [`ARTIFACT_TYPE`] referrers and
//! whatever comes back is that digest's contract, by construction — where a field claiming a
//! digest could only be written after the push, changing the bytes [`LABEL_SHA256`] was computed
//! over before it.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::json_schema::{self, DRAFT_07, JsonSchema};
use super::{Error, Schema, TextForm};

/// The version of the envelope [`Contract::to_json`] produces.
///
/// Independent of [`SCHEMA_VERSION`](super::SCHEMA_VERSION), which versions the
/// [`schema`](Contract::schema) *inside* it: a change to how the envelope is arranged and a change
/// to how a key is described are different events, and a consumer that only reads
/// [`Contract::external`] should not be told to re-check its parsing because a key gained a field.
///
/// Bumped when an existing field changes meaning or disappears — never when one is added, on the
/// same reasoning and with the same obligation on consumers: ignore what you do not recognise,
/// and refuse a version you were not written against rather than guessing at it.
///
/// Every field in this document is `snake_case`, including the ones the envelope adds. The
/// alternative was `camelCase` for the envelope and `snake_case` for the [`Schema`] it wraps,
/// which is what this started as — one document in two conventions, `text_constraint` on a key
/// and `textConstraint` on the variable beside it. A consumer writing a field name from memory
/// gets it right under one convention and guesses under two.
pub const CONTRACT_VERSION: u32 = 1;

/// The OCI artifact type a published contract is attached to its image under.
///
/// This is the whole discovery protocol on the registry side: `oras discover --artifact-type`
/// with this string against an image digest returns the contract for that exact build, or
/// nothing. Nothing means the image does not publish one — which is an answer, and a different
/// answer from "the contract could not be fetched".
pub const ARTIFACT_TYPE: &str = "application/vnd.terrace.config-schema.v1+json";

/// The image label carrying [`CONTRACT_VERSION`].
pub const LABEL_VERSION: &str = "dev.terrace.config.contract.version";

/// The image label naming where the contract is embedded in the image's own filesystem.
///
/// The registry copy is the one a pipeline fetches, because it costs no layer pull. This one is
/// what makes an image self-describing when there is no registry at all — an exported tarball, an
/// air-gapped mirror, a running container being inspected.
pub const LABEL_PATH: &str = "dev.terrace.config.contract.path";

/// The image label carrying the SHA-256 of the contract document, lower-case hex, unprefixed.
///
/// **Over the exact bytes [`Contract::to_json`] produced** — the whole document, no
/// canonicalisation, nothing excluded. A consumer checks it by hashing the file it has, which is
/// the cheapest check there is and only stays cheap while nothing is carved out of it. That is why
/// [`App`] carries no image digest: a field written after the push would change these bytes after
/// they were hashed, and the alternative — hashing a canonicalisation with that field removed —
/// makes every consumer in every language reproduce the carve-out before it can verify anything.
///
/// This crate takes no hashing dependency for a value the build already computes to name the file
/// it is copying. What the label buys is that three copies of one document are one guarantee: the
/// embedded file, the registry artifact and this label must agree, and a consumer that finds they
/// do not has found a build to refuse rather than a copy to prefer.
pub const LABEL_SHA256: &str = "dev.terrace.config.contract.sha256";

/// The image label carrying the loader's environment prefix, e.g. `PORTFOLIO_`.
///
/// Read before the document is fetched: it is what tells a validator which of a pod's environment
/// variables are this contract's business at all.
pub const LABEL_PREFIX: &str = "dev.terrace.config.prefix";

/// Where [`LABEL_PATH`] points unless a build says otherwise.
pub const DEFAULT_PATH: &str = "/config/contract.json";

/// The whole configuration surface of one image, in the shape a pipeline reads it.
///
/// Built with [`Schema::into_contract`]. The two schema halves are here because neither is enough
/// alone: [`Self::json_schema`] is the only one a stock validator acts on and carries no
/// environment spellings at all, while [`Self::schema`] carries every spelling and no validator
/// takes it. [`External`] is the third half — what this image reads that no derive can find.
///
/// It is not evidence of anything on its own. A contract is worth reading only when it is known to
/// belong to the image being deployed, which is a property of how it was published rather than of
/// what it says: see [`ARTIFACT_TYPE`] and [`Self::labels`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Contract {
    /// The version of this envelope's shape. See [`CONTRACT_VERSION`].
    pub terrace_contract: u32,
    /// Which build this describes.
    pub app: App,
    /// Every key the loader can carry, in every spelling that can supply it.
    pub schema: Schema,
    /// The same keys as a JSON Schema, for validating the document a chart renders.
    pub json_schema: Json,
    /// The surface outside the loader's namespace: what else this image reads, and what it does
    /// not care about.
    pub external: External,
}

/// Which build a [`Contract`] describes.
///
/// Every field here moves independently of the configuration surface, which is why they are
/// collected rather than scattered: a consumer diffing two contracts to see whether the
/// *configuration* changed diffs everything except this.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct App {
    /// The service's name, as its image is named.
    pub name: String,
    /// The release this was built from, **spelled as the image tag spells it**.
    ///
    /// `v2.5.0` where the images are tagged `v2.5.0`, and `2.5.0` where they are not — this field
    /// exists to be compared against a tag, and `env!("CARGO_PKG_VERSION")` yields the form
    /// without the `v`. A consumer comparing across that difference is the whole reason to say
    /// which form is meant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The commit it was built from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// When it was built, as an RFC 3339 timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Where the source lives.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    //
    // No `digest`. It looks like it belongs here and it cannot: the digest is what building the
    // image *produces*, so a document carrying it must be written after the push — and writing it
    // changes the bytes, which are the bytes [`LABEL_SHA256`] was computed over before the push.
    // There is no build order that satisfies both, and the field is unnecessary anyway: a
    // registry artifact's subject *is* a digest, so a consumer that fetched this document by
    // asking a digest for its [`ARTIFACT_TYPE`] referrers already knows which image it belongs to.
    // The attachment is the tie, not a field.
}

impl App {
    /// A named build with nothing else stated.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// The release this was built from.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// The commit this was built from.
    #[must_use]
    pub fn revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    /// When this was built, as an RFC 3339 timestamp.
    #[must_use]
    pub fn created(mut self, created: impl Into<String>) -> Self {
        self.created = Some(created.into());
        self
    }

    /// Where the source lives.
    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// The environment this image reads that the loader does not own.
///
/// Empty by default, and an empty `External` with [`Unknown::Reject`] is a strong claim: *every*
/// variable set on this container is either a configuration key or a mistake. Almost no real
/// deployment can make it — see [`Unknown::Reject`] for the three things a Kubernetes pod carries
/// that no image asked for.
///
/// # How a validator classifies a variable
///
/// Normative, and an ordered list because it has to be: the first match wins, and two consumers
/// that ran these in different orders would disagree about whether a deployment is valid. That is
/// the failure [`Self::ignore`]'s single wildcard form exists to prevent, reached through
/// evaluation order instead of through pattern syntax.
///
/// This list is repeated verbatim in the crate's README, which is what a pipeline implementer
/// reads. **Edit both or neither.** Two normative statements that disagree is the same defect as
/// no normative statement, and worse for being harder to notice.
///
/// For each environment variable set on a container:
///
/// 1. it is one of `schema.loader[].env` — a variable the loader reads to decide what the layers
///    are. Valid.
/// 2. it equals some `schema.keys[].env` — that key, supplied by the environment layer. Check it
///    in two steps; see *Two constraints* below.
/// 3. it equals some `schema.keys[].env_file` — that key, supplied by indirection. The value is a
///    path, so neither constraint applies to it; what applies is that the path is mounted.
/// 4. it begins with `schema.dialect.prefix` and matched none of the above — **reject**. It is a
///    key spelling nothing in this image reads: a rename nobody finished, or a typo. Neither
///    [`Self::env`] nor [`Self::ignore`] can reach this step, because [`ContractBuilder::build`]
///    refuses both when they carry the prefix.
/// 5. it equals some [`Self::env`] entry — check it the same two ways, against that entry's
///    [`text_constraint`](ExternalVar::text_constraint) and
///    [`constraint`](ExternalVar::constraint).
/// 6. it matches some [`Self::ignore`] pattern — skip it.
/// 7. otherwise — [`Self::unknown`].
///
/// Step 4 sitting above 5 and 6 is the load-bearing part. After `build`'s refusals the two cannot
/// disagree, so stating the order costs nothing and removes the question.
///
/// # Two constraints, and both are needed
///
/// A variable holds text and a configuration holds a value, so steps 2 and 5 are two checks:
///
/// 1. **Form.** The text must satisfy `text_constraint`, when there is one. `"http"` is not an
///    integer in any spelling, and this is the check that says so.
/// 2. **Range.** Read the text according to `text_form` — [`TextForm::Integer`] means parse it as
///    an integer — and check the result against `constraint`. This is where `minimum` and
///    `maximum` live, and it is the only step that can reach them: a pattern matches characters,
///    so `99999` is a perfectly well-formed integer and only a bound catches it not fitting a
///    `u16`.
///
/// `text_form` is what says which parse, rather than the shape of the constraint object: a
/// consumer inferring "pattern means integer" was right while there were two shapes and wrong as
/// soon as [`TextForm::Structured`] arrived. [`TextForm::Text`] and [`TextForm::Unknown`] have no
/// second step — the first because there is nothing to parse, the second because nothing is known
/// to parse it as.
///
/// Skipping the second leaves every bound in the document decorative, which is a deployment that
/// passes every gate and fails at boot. Skipping the first, or applying `constraint` to the raw
/// text, rejects `"0"` for an integer key — a correct deployment refused.
///
/// **A 64-bit range is not checkable from this document at all.** `u64::MAX` is not representable
/// as an IEEE double, so a `maximum` carrying it would be a different number than the type
/// accepts, and none is emitted rather than one that is wrong. A
/// `u64` key given `18446744073709551616` therefore satisfies everything published here and still
/// fails to load. Running the real binary against the rendered configuration is what closes that,
/// and there is no arrangement of these fields that would.
///
/// # The file layers are a separate question
///
/// The list above is about variables. A chart also mounts *files* — a key-named file in the
/// secrets directory, or a path a `_FILE` variable points at — and the rule there is blunter than
/// a constraint: those layers deliver their contents as strings with no parse, and
/// `Figment::extract` does not coerce a string into a number or a boolean. **A key whose
/// [`constraint`](super::Key::constraint) is anything but a string type cannot be supplied by
/// either, whatever the file contains.** Not "must match a pattern" — cannot be supplied at all.
///
/// That is deliberate rather than a limitation: those layers exist to carry secrets, and a secret
/// is an opaque byte string. So a chart mounting `isr__ttl_secs` as a secret file has made a
/// mistake no file contents can fix, and a validator can say so from the constraint alone.
///
/// # What this deliberately cannot say
///
/// Step 4 also catches what a *cluster* injects into the prefix, and the contract has no way to
/// declare it. Kubernetes service links inject `<SERVICE_NAME>_SERVICE_HOST`, `<SERVICE_NAME>_PORT`
/// and five more per Service in the namespace, and the service name is the *release* name — which
/// an image cannot know. A release called `portfolio` produces `PORTFOLIO_SERVICE_HOST` and
/// `PORTFOLIO_PORT` against a `PORTFOLIO_` prefix; a release called `staging-portfolio` produces
/// names that fall outside it entirely. No declaration written at build time is right for both.
///
/// So it belongs to whatever renders the deployment, which does know the release name, and the
/// answer is the same one that answers it at runtime: **set `enableServiceLinks: false`**. Service
/// links are a legacy mechanism, and leaving them on against a prefix that matches the release
/// name is not merely a validation nuisance — `PORTFOLIO_PORT` is a spelling of the key `port`,
/// so a service link would *supply* that key, from the environment layer, outranking the mounted
/// file. That is a live misconfiguration this document cannot fix and can only refuse to hide.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct External {
    /// Variables this image reads outside the loader's namespace, described well enough to be
    /// checked rather than merely tolerated.
    pub env: Vec<ExternalVar>,
    /// Patterns for variables that are nobody's business here. A trailing `*` matches any suffix.
    pub ignore: Vec<String>,
    /// What a validator should do with a variable matching neither.
    pub unknown: Unknown,
}

impl External {
    /// Nothing declared, nothing ignored, unknown variables refused.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a variable this image reads.
    #[must_use]
    pub fn var(mut self, var: ExternalVar) -> Self {
        self.env.push(var);
        self
    }

    /// Declare every variable in an iterator.
    #[must_use]
    pub fn vars(mut self, vars: impl IntoIterator<Item = ExternalVar>) -> Self {
        self.env.extend(vars);
        self
    }

    /// Ignore every variable matching `pattern`, which may end in `*`.
    ///
    /// For variables with no owner in this image — an operator's `TZ`, a base image's `PATH`, a
    /// platform's injected `KUBERNETES_*`. Prefer [`Self::var`] wherever the variable does have an
    /// owner: an ignored variable is one a chart can misspell freely.
    ///
    /// A pattern that reaches into the loader's namespace is refused by
    /// [`ContractBuilder::build`], including one that does not carry the prefix but subsumes it —
    /// `ignore("PORT*")` against a `PORTFOLIO_` prefix reads as a pattern about the external
    /// `PORT` and would exempt every configuration key the image has.
    #[must_use]
    pub fn ignore(mut self, pattern: impl Into<String>) -> Self {
        self.ignore.push(pattern.into());
        self
    }

    /// What a validator should do with a variable matching neither [`Self::env`] nor
    /// [`Self::ignore`]. Defaults to [`Unknown::Reject`].
    #[must_use]
    pub fn unknown(mut self, unknown: Unknown) -> Self {
        self.unknown = unknown;
        self
    }
}

/// One variable this image reads that the loader does not supply.
///
/// Every field mirrors [`Key`](super::Key) deliberately: a validator that has code for checking a
/// configuration key against a rendered value should need no second code path for `PORT`. Only the
/// spellings differ, and they differ by being absent — an external variable has no key path, no
/// `_FILE` form and no secrets-directory file name, because nothing in this crate reads it.
// `Eq`, unlike [`Key`](super::Key): every field here is a string or a flag. A key cannot derive it
// because its default is a figment `Value`, which can hold a float.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExternalVar {
    /// The variable's exact spelling, e.g. `PORT`.
    pub name: String,
    /// What reads it — `dioxus`, `tracing`, `the base image`. Prose, for whoever is asking why a
    /// chart is allowed to set this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// What it is for. Empty when nothing was said.
    pub docs: String,
    /// The type it takes, spelled as a Rust type — prose, saying what the reading side calls it.
    ///
    /// [`Self::constraint`] is what a validator acts on. This is not a vocabulary a consumer is
    /// expected to interpret, and a type this crate does not recognise leaves the constraint
    /// [`None`] rather than making one up.
    ///
    /// [`Key::ty`]: super::Key::ty
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
    /// The fixed set of values it accepts. Empty when it is not a choice.
    pub values: Vec<String>,
    /// What a value of this variable must be, as JSON Schema keywords, derived from [`Self::ty`]
    /// and [`Self::values`] exactly as [`Key::constraint`] is.
    ///
    /// **[`None`] means declared but unconstrained** — legitimate for an opaque string, and the
    /// difference a consumer must not miss: an entry with a constraint is checked like a
    /// configuration key, an entry without one is only checked to exist. That is nearer to
    /// [`External::ignore`] than the declaration reads, so a variable worth declaring is usually
    /// worth typing.
    ///
    /// Derived from [`Self::ty`] unless [`Self::constraint`](Self::constraint()) set one, which
    /// is the escape hatch for a domain type this crate cannot interpret.
    ///
    /// [`Key::constraint`]: super::Key::constraint
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub constraint: Option<Json>,
    /// What the **unparsed text** of this variable must be, as [`Key::text_constraint`] carries it
    /// for a configuration key.
    ///
    /// The one that matters for an external variable, because an environment variable is *only*
    /// ever text — nothing parses it on the way in the way figment's `Env` provider parses a
    /// prefixed one. A `PORT` declared `ty("u16")` is checkable against `"http"` because of this
    /// field and not because of [`Self::constraint`].
    ///
    /// [`Key::text_constraint`]: super::Key::text_constraint
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text_constraint: Option<Json>,
    /// How to read this variable's text, as [`Key::text_form`] carries it for a configuration key.
    ///
    /// [`Key::text_form`]: super::Key::text_form
    #[serde(default)]
    pub text_form: TextForm,
    /// What the image does when it is unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Whether the image fails without it.
    pub required: bool,
    /// Whether the value is a credential. A secret carries no default, on
    /// [`Key::secret`](super::Key::secret)'s reasoning: this document is published.
    pub secret: bool,
}

impl ExternalVar {
    /// A variable with nothing stated but its name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            owner: None,
            docs: String::new(),
            ty: None,
            values: Vec::new(),
            // Derived by `build`, for `Key::constraint`'s reason: one place decides what a type
            // means, so a variable's constraint cannot disagree with the key's for one spelling.
            constraint: None,
            text_constraint: None,
            text_form: TextForm::Unknown,
            default: None,
            required: false,
            secret: false,
        }
    }

    /// What reads it.
    #[must_use]
    pub fn owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    /// What it is for.
    #[must_use]
    pub fn docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = docs.into();
        self
    }

    /// The type it takes, spelled as a Rust type — `u16`, `String`, `bool`.
    #[must_use]
    pub fn ty(mut self, ty: impl Into<String>) -> Self {
        self.ty = Some(ty.into());
        self
    }

    /// The fixed set of values it accepts.
    #[must_use]
    pub fn values(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.values = values.into_iter().map(Into::into).collect();
        self
    }

    /// State the constraints outright, for a type [`Self::ty`] cannot express.
    ///
    /// [`ContractBuilder::build`] derives both constraints from the type token and leaves whatever
    /// is set here alone, so this is the escape hatch for a domain type — a duration, a
    /// connection string, anything whose shape the crate refuses to guess at. Pass the document
    /// form first and the text form second; pass [`None`] for either to keep the derived answer.
    ///
    /// Whatever is passed here is published verbatim and acted on by a validator, so the rule the
    /// rest of this module is written to is the caller's now: a constraint that rejects a value
    /// the image accepts stops a deployment that was correct.
    #[must_use]
    pub fn constraint(mut self, document: Option<Json>, text: Option<Json>) -> Self {
        if document.is_some() {
            self.constraint = document;
        }
        if text.is_some() {
            self.text_constraint = text;
            // A stated text constraint that left the form at its default would tell a consumer
            // "nothing certain is known" beside a pattern that says otherwise. `Text` is the
            // honest reading of a hand-written pattern: match it, and do not try to parse the
            // result into something this crate was not told about.
            self.text_form = TextForm::Text;
        }
        self
    }

    /// State how to read this variable's text, when [`Self::constraint`] gave it a shape the
    /// default reading is wrong for.
    ///
    /// [`TextForm::Integer`] beside a hand-written pattern is what makes a consumer parse the
    /// match and check [`Self::constraint`]'s bounds, rather than stopping at the pattern.
    #[must_use]
    pub fn text_form(mut self, form: TextForm) -> Self {
        self.text_form = form;
        self
    }

    /// What the image does when it is unset.
    #[must_use]
    pub fn default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// Mark it required: the image fails without it.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Mark it a credential.
    ///
    /// A secret with a default is refused by [`ContractBuilder::build`] whichever order the two
    /// were called in. Dropping the default here instead would make `.default(…).secret()` build
    /// and `.secret().default(…)` fail, which is one intent with two outcomes — and the caller
    /// writing either has a misunderstanding worth surfacing rather than silently repairing.
    #[must_use]
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
}

/// What a validator does with an environment variable no part of the contract accounts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Unknown {
    /// Fail. The default, on the reasoning the rest of this crate defaults on: a variable nobody
    /// reads is a mistake rather than a courtesy, and one that used to be read is a rename nobody
    /// finished.
    ///
    /// It is the default; it is not free. A pod carries variables no image asked for, and a
    /// contract adopting `Reject` has to account for all of them — including on a `scratch` image
    /// running one static binary, which is the case where it is most tempting to assume otherwise:
    ///
    /// - `HOSTNAME`, from the container runtime.
    /// - `KUBERNETES_SERVICE_HOST` and its four relatives, from the API server.
    /// - the service links, which need `enableServiceLinks: false` on the pod rather than anything
    ///   in this document — see [`External`] for why an image cannot declare them.
    ///
    /// The first two are what [`External::ignore`] is for. Reaching for [`Self::Warn`] instead
    /// gives up the whole gate to tolerate six names.
    #[default]
    Reject,
    /// Report it and carry on. For adopting the gate on a chart that has variables nobody has
    /// accounted for yet — a migration state, not a resting place.
    Warn,
    /// Say nothing. For an image whose environment is genuinely open, where the alternative is an
    /// ignore list that is never finished and therefore never trusted.
    Allow,
}

/// Assembles a [`Contract`]. Built by [`Schema::into_contract`].
///
/// The schema is moved in rather than borrowed: a generator builds one, converts it and is done,
/// so making the common path copy every key to satisfy an API shape nobody needs would be a cost
/// with no reader.
#[derive(Debug, Clone)]
pub struct ContractBuilder {
    schema: Schema,
    app: App,
    external: External,
    /// How the JSON Schema half renders. Not settable as a whole: see [`Self::closed`].
    json_schema: JsonSchema,
}

impl ContractBuilder {
    /// The surface outside the loader's namespace. Empty by default.
    #[must_use]
    pub fn external(mut self, external: External) -> Self {
        self.external = external;
        self
    }

    /// Whether a key the schema does not describe is an error. Defaults to `true`.
    ///
    /// Off for a service whose rendered document legitimately carries keys this contract does not
    /// describe. Prefer declaring those keys to relaxing the check: an unknown key is the defect
    /// the whole document exists to catch, and an open schema catches none of them.
    #[must_use]
    pub fn closed(mut self, closed: bool) -> Self {
        self.json_schema = self.json_schema.closed(closed);
        self
    }

    /// The JSON Schema half's `title`. Defaults to the app's name.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.json_schema = self.json_schema.title(title);
        self
    }

    /// Assemble the contract, checking the claims it is about to publish.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if the declared external surface is not one a validator could
    /// act on:
    ///
    /// - a variable whose name is not a name the environment can hold;
    /// - a variable carrying the loader's own prefix, which would exempt a real configuration key
    ///   from the check that owns it;
    /// - a variable declared twice, or colliding with a key's or the loader's own spelling;
    /// - an ignore pattern that is empty, or wildcarded anywhere but at its end;
    /// - a secret carrying a default, anywhere in the document.
    pub fn build(self) -> Result<Contract, Error> {
        let Self {
            schema,
            app,
            external,
            json_schema,
        } = self;

        validate_external(&schema, &external)?;
        validate_secrets(&schema, &external)?;

        // Derived where the caller said nothing, never over the top of what they did say:
        // `ExternalVar::constraint` is the only way to describe a type this crate cannot
        // interpret, and clobbering it would leave a domain type permanently uncheckable.
        let mut external = external;
        for var in &mut external.env {
            if var.constraint.is_none() {
                var.constraint =
                    json_schema::constraint(var.ty.as_deref(), &var.values).map(Json::Object);
            }
            let (form, text) = json_schema::text_constraint(var.ty.as_deref(), &var.values);
            if var.text_constraint.is_none() {
                var.text_form = form;
                var.text_constraint = text.map(Json::Object);
            }
        }

        let options = json_schema.or_title(format!("{} configuration", app.name));
        let rendered = json_schema::document(&schema, &options);

        Ok(Contract {
            terrace_contract: CONTRACT_VERSION,
            app,
            schema,
            json_schema: Json::Object(rendered),
            external,
        })
    }
}

impl Schema {
    /// This schema as the contract an image publishes. See [`Contract`].
    ///
    /// **The schema passed here is a claim about what *this image's binary* loads.** Pass what
    /// that binary loads — not the union across a workspace, which is what a generator producing
    /// every other rendering naturally has to hand. A contract built from the union asserts the
    /// runtime image reads a build-time credential no deployment supplies, and a chart believing
    /// it is a chart being told to mount something that does not exist. Nothing can check this:
    /// both schemas are well-formed and only the caller knows which binary is in the image.
    ///
    /// [`Schema::merge`] is for the other case, and it is a real one — several binaries in one
    /// image, or one document read by several. Merge the roots that are actually in the image.
    ///
    /// Consuming, because a generator has no use for the schema afterwards and the alternative is
    /// cloning every key to no end. [`Clone`] is there for the generator that does.
    #[must_use]
    pub fn into_contract(self, app: App) -> ContractBuilder {
        ContractBuilder {
            schema: self,
            app,
            external: External::new(),
            // draft-07 because that is the dialect Helm validates `values.schema.json` against
            // and the one every consumer of this document is already able to read; closed because
            // an unknown key in a rendered configuration is the defect this whole document exists
            // to catch, and an open schema catches none of them.
            //
            // Deliberately not settable as a whole. A caller writing the obvious
            // `.json_schema(JsonSchema::new().closed(false))` to relax one knob would silently
            // take the dialect back to 2020-12 with it — which validates fine on its own and
            // fails only when a pipeline pins a draft-07 engine, or when two contracts of one
            // document refuse to merge. `closed` and `title` are the two knobs that override
            // meaningfully, so they are the two that exist.
            json_schema: JsonSchema::new().meta_schema(DRAFT_07).closed(true),
        }
    }
}

impl Contract {
    /// The document, pretty-printed.
    ///
    /// Deterministic: the same schema and the same [`App`] produce byte-identical output, which is
    /// what lets the result be hashed into [`LABEL_SHA256`] and diffed in review. Keys keep
    /// declaration order and the JSON Schema half is ordered by `serde_json`'s own map; nothing
    /// here reads a clock, a hash seed or the environment.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if serialisation fails, which for this type means the JSON
    /// writer failed rather than the data being unrepresentable.
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self)
            .map_err(|e| Error::Invalid(format!("the contract could not be written as JSON: {e}")))
    }

    /// The image labels that make this contract discoverable, given where it was embedded.
    ///
    /// [`LABEL_SHA256`] is not among them: it is a property of the rendered bytes rather than of
    /// this value, and the build that writes those bytes is already the one that has them. Emit it
    /// alongside these.
    ///
    /// The point of the accessor is that the label *names* live in one place. A Dockerfile that
    /// spells one of them by hand is a Dockerfile that can spell it differently from the pipeline
    /// reading it, and the failure mode is a contract that is silently never found.
    #[must_use]
    pub fn labels(&self, path: &str) -> Vec<(&'static str, String)> {
        vec![
            (LABEL_VERSION, self.terrace_contract.to_string()),
            (LABEL_PATH, path.to_owned()),
            (LABEL_PREFIX, self.schema.dialect.prefix.clone()),
        ]
    }
}

/// Every environment spelling the loader itself claims, in one set.
///
/// Both halves of the collision check read this: an external variable may not shadow a key's
/// spelling, and the reason is not tidiness — a chart setting `PORTFOLIO_GITHUB__TOKEN` while the
/// contract declares it "external, owned by something else" is a key silently exempted from the
/// gate that would otherwise catch it being renamed.
///
/// [`Key::secrets_file`](super::Key::secrets_file) is deliberately absent: it is a file name in a
/// mounted directory, not a variable, so it cannot collide with one. A validator checking the
/// *files* a chart mounts wants that field, one per key, straight off
/// [`Contract::schema`]`.keys` — there is no set assembled here for it, because nothing in this
/// module has a reason to build one.
fn loader_spellings(schema: &Schema) -> impl Iterator<Item = &str> {
    schema
        .keys
        .iter()
        .flat_map(|key| {
            [
                key.env.as_deref(),
                key.env_file.as_deref(),
                // The secrets-directory name is not an environment variable and cannot collide
                // with one, so it is deliberately not here.
            ]
        })
        .flatten()
        .chain(schema.loader.iter().map(|var| var.env.as_str()))
}

/// Check that the declared external surface is one a validator could act on.
fn validate_external(schema: &Schema, external: &External) -> Result<(), Error> {
    let prefix = &schema.dialect.prefix;
    let mut seen = std::collections::BTreeSet::new();

    for var in &external.env {
        if !is_env_name(&var.name) {
            return Err(Error::Invalid(format!(
                "`{}` is declared as an external variable, but that is not a name an environment \
                 can hold: a name is a letter or underscore followed by letters, digits and \
                 underscores.",
                var.name
            )));
        }
        // The prefix is what tells a validator which variables this contract governs. A declared
        // external variable inside it would be governed and exempt at once, and the exemption
        // would win — so the two spaces are kept disjoint by construction rather than by care.
        if !prefix.is_empty() && var.name.starts_with(prefix.as_str()) {
            return Err(Error::Invalid(format!(
                "`{}` is declared as an external variable but carries the loader's own prefix \
                 `{prefix}`. Everything in that namespace is a configuration key; declaring one \
                 external would exempt it from the check that owns it.",
                var.name
            )));
        }
        if !seen.insert(var.name.as_str()) {
            return Err(Error::Invalid(format!(
                "`{}` is declared as an external variable twice; a consumer cannot tell which \
                 description is the one to check against.",
                var.name
            )));
        }
    }

    // Cheap because the loader half is walked once against a set that is already built: an
    // external declaration colliding with a spelling the loader reads is the same defect as the
    // prefix case, reached by the other door — a `reserve`d variable, or a renamed prefix.
    for spelling in loader_spellings(schema) {
        if seen.contains(spelling) {
            return Err(Error::Invalid(format!(
                "`{spelling}` is declared as an external variable, but the loader reads it \
                 itself. One of the two descriptions is wrong, and a consumer has no way to \
                 decide which."
            )));
        }
    }

    // Collected once and reused: the loader half is walked twice below and a pattern is checked
    // against all of it, which is the difference between this and the `var` check above — a name
    // either equals a spelling or it does not, while a pattern can cover one it does not equal.
    let spellings: Vec<&str> = loader_spellings(schema).collect();
    for pattern in &external.ignore {
        validate_pattern(pattern, prefix, &spellings)?;

        // An *exact* pattern naming a declared variable is the duplicate-declaration case in
        // different words: the contract says this variable is checked and that it is nobody's
        // business, and only the classification order decides which. A wildcard that happens to
        // cover one is deliberately left alone — `ignore("KUBERNETES_*")` beside a declared
        // `KUBERNETES_SERVICE_HOST` is an ordinary thing to write, and refusing it would make the
        // ordered list carry no weight.
        if seen.contains(pattern.as_str()) {
            return Err(Error::Invalid(format!(
                "`{pattern}` is both declared as an external variable and ignored. One says a \
                 chart's value for it is checked and the other says it is nobody's business; a \
                 contract cannot say both."
            )));
        }
    }

    Ok(())
}

/// Whether `name` is covered by `pattern`.
///
/// The whole matching rule, and it is a function rather than three inline characters because the
/// document tells every consumer to implement it: a trailing `*` matches any suffix, and anything
/// else is an exact name. Having the reference implementation in the crate is what a consumer can
/// check itself against, and it is what the refusals below are written in terms of, so the rule
/// that decides what a *validator* skips and the rule that decides what `build` *refuses* cannot
/// come apart.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(stem) => name.starts_with(stem),
        None => pattern == name,
    }
}

/// Check that an ignore pattern is one a consumer can match without inventing a glob dialect, and
/// that it covers nothing the loader reads.
///
/// Exactly one wildcard form is supported — a trailing `*` — because every consumer of this
/// document has to implement the matching, in whatever language it is written in, and a pattern
/// language is a place for two implementations to disagree about what is exempt from a security
/// check.
fn validate_pattern(pattern: &str, prefix: &str, spellings: &[&str]) -> Result<(), Error> {
    let wildcard = pattern.ends_with('*');
    let stem = pattern.strip_suffix('*').unwrap_or(pattern);

    if stem.is_empty() {
        return Err(Error::Invalid(if pattern.is_empty() {
            "an empty ignore pattern matches nothing and says nothing; remove it.".to_owned()
        } else {
            "`*` as an ignore pattern exempts the entire environment from checking. Use \
             `External::unknown(Unknown::Allow)`, which says so where a reader will look."
                .to_owned()
        }));
    }
    if stem.contains('*') {
        return Err(Error::Invalid(format!(
            "`{pattern}` wildcards somewhere other than its end. Only a trailing `*` is \
             supported, so that every consumer implementing this match implements the same one."
        )));
    }
    if !stem.chars().all(is_env_char) {
        return Err(Error::Invalid(format!(
            "`{pattern}` contains a character no environment variable name can hold."
        )));
    }

    // The `ExternalVar` case reached through the suppression list, and worse than it: a declared
    // variable at least names what it is, while a pattern exempts everything it happens to cover.
    //
    // Two ways in. A pattern *inside* the namespace — `PORTFOLIO_*`, `PORTFOLIO_GITHUB__TOKEN` —
    // and a wildcard pattern that does not carry the prefix but subsumes it: `ignore("PORT*")`
    // against `PORTFOLIO_` reads as a pattern about the external `PORT` and disables the whole
    // gate, one character from a spelling that is entirely correct.
    //
    // An *exact* pattern the prefix starts with is not either of those. `ignore("PORT")` matches
    // the name `PORT` and nothing else, and no key is spelled that.
    if !prefix.is_empty() && (stem.starts_with(prefix) || (wildcard && prefix.starts_with(stem))) {
        return Err(Error::Invalid(format!(
            "`{pattern}` ignores variables carrying the loader's prefix `{prefix}`. Everything in \
             that namespace is a configuration key, and an ignored key is one a chart may misspell \
             freely — which is the exemption `External::var` is refused for. If these are names a \
             platform injects rather than keys, they belong to whatever renders the deployment: \
             see `External` on service links."
        )));
    }

    // The prefix is not the whole namespace. A *key's* environment spelling is derived from the
    // prefix, so the check above covers every one of them — but a *loader* variable's spelling is
    // whatever the caller passed, and `config_var`, `secrets_dir_var` and `reserve` all take
    // arbitrary names. `secrets_dir_var("CREDENTIALS_DIR")` is in this crate's own README.
    //
    // Exempting one of those is worse than exempting a key: the variable naming the secrets
    // directory decides where *every* credential is read from, so a chart misspelling it loses
    // all of them at once, silently.
    if let Some(covered) = spellings
        .iter()
        .find(|spelling| pattern_matches(pattern, spelling))
    {
        return Err(Error::Invalid(format!(
            "`{pattern}` ignores `{covered}`, which the loader reads itself to decide what the \
             layers are. A variable a chart may misspell freely cannot be one the configuration \
             is loaded through."
        )));
    }

    Ok(())
}

/// Refuse to publish a credential.
///
/// [`Schema::with_defaults_from`] already drops a secret key's value, and [`ExternalVar::secret`]
/// already drops its own. This is the check that says so once, at the boundary the document
/// crosses — the point where "this file goes to a public registry" stops being an assumption a
/// reader has to make and becomes something the type refuses to violate.
fn validate_secrets(schema: &Schema, external: &External) -> Result<(), Error> {
    for key in &schema.keys {
        if key.secret && (key.default.is_some() || key.default_value.is_some()) {
            return Err(Error::Invalid(format!(
                "`{}` is marked secret but carries a default, and a contract is published. \
                 Nothing in this crate produces that pair; a schema built by hand can.",
                key.path
            )));
        }
    }
    for var in &external.env {
        if var.secret && var.default.is_some() {
            return Err(Error::Invalid(format!(
                "`{}` is marked secret but carries a default, and a contract is published.",
                var.name
            )));
        }
    }
    Ok(())
}

/// Whether `name` is a name the environment can hold.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(is_env_char)
}

/// Whether `c` may appear after the first character of an environment variable name.
fn is_env_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::pattern_matches;

    #[test]
    fn a_pattern_is_a_trailing_star_or_an_exact_name() {
        // The rule every consumer of this document has to reimplement, so the reference
        // implementation is worth pinning: anything looser here and `build`'s refusals stop
        // covering what a validator will actually skip.
        assert!(pattern_matches("KUBERNETES_*", "KUBERNETES_SERVICE_HOST"));
        assert!(pattern_matches("KUBERNETES_*", "KUBERNETES_"));
        assert!(pattern_matches("HOSTNAME", "HOSTNAME"));

        assert!(!pattern_matches("HOSTNAME", "HOSTNAMES"));
        assert!(!pattern_matches("HOSTNAME", "HOST"));
        assert!(!pattern_matches("KUBERNETES_*", "MY_KUBERNETES_PORT"));
    }

    #[test]
    fn a_bare_star_matches_everything() {
        // Never reachable through `build`, which refuses the pattern by name — but the matcher is
        // the thing a consumer copies, and it has to behave the way the sentence describing it
        // does rather than the way the refusal makes convenient.
        assert!(pattern_matches("*", "ANYTHING"));
        assert!(pattern_matches("*", ""));
    }
}
