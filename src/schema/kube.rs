//! The Kubernetes half of the protocol: what a rendered object carries, and how it is paired
//! with the image that reads it.
//!
//! [`Contract::labels`](super::Contract::labels) puts three labels on an *image*, and that is
//! where the protocol stopped. Those labels are in the image config blob, which is a place a
//! Kyverno policy, a validating admission webhook or an initContainer inspecting a live
//! `ConfigMap` cannot see without resolving the pod's image and asking a registry. The object it
//! is holding — the document that is about to be mounted — says nothing about which contract it
//! was rendered against, so nothing in the cluster can tell a matched pair from a mismatched one.
//!
//! This module is the other half: the metadata a chart stamps onto the objects it renders, and
//! [`Pairing`], which takes both halves and asserts that they describe one configuration surface.
//!
//! # Labels are what you select on. Annotations are what you read.
//!
//! That sentence decides every placement below, and it is forced by the platform rather than
//! chosen. A Kubernetes **label value** must be 63 characters or fewer and, unless it is empty,
//! must match
//!
//! ```text
//! (([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?
//! ```
//!
//! — it begins and ends alphanumeric, with `-`, `_` and `.` allowed between. Every interesting
//! value this crate already publishes fails that rule:
//!
//! | Value | Why it is not a legal label value |
//! |---|---|
//! | `PORTFOLIO_` | trailing underscore |
//! | `/config/contract.json` | `/` is not in the character class |
//! | `application/vnd.terrace.config-schema.v1+json` | `/` and `+` |
//!
//! An **annotation** key follows the same rule a label key does, and an annotation **value** is
//! unconstrained — the whole `metadata.annotations` map is bounded at 256 KiB and nothing else is
//! said about it. So anything a cluster-side actor needs to *match on* has to be a label and has
//! to survive that character class, and everything else is an annotation.
//!
//! The reason to write this down rather than leave it implicit: the obvious next change to this
//! module is to promote the loader's prefix to a label, beside the image label that already
//! carries it. `PORTFOLIO_` cannot be a label value, and the way that is discovered otherwise is
//! at `kubectl apply` time, in whatever pipeline first renders a chart against a real cluster.
//!
//! # What gets stamped, and on what
//!
//! Two objects, and deliberately not the same stamp on both — see [`Target`]. The document object
//! carries everything; the pod template carries the label and the image list, because
//! `document-key` and `format` are properties of a document and a pod is not one.
//!
//! ```
//! # use std::collections::BTreeMap;
//! # use terrace_config::Terrace;
//! # use terrace_config::schema::{App, Describe, Leaf, Sink};
//! # use terrace_config::schema::kube::{Format, Target};
//! # struct Config;
//! # impl Describe for Config {
//! #     fn describe(sink: &mut Sink) {
//! #         sink.leaf(Leaf { name: "dist_dir", docs: "", ty: Some("String"), values: None,
//! #             aliases: &[], note: None, required: false, secret: false });
//! #     }
//! # }
//! let contract = Terrace::new("PORTFOLIO_")
//!     .schema::<Config>()
//!     .into_contract(App::new("portfolio"))
//!     .build()?;
//!
//! let metadata = contract.kube_metadata(
//!     &Target::document("config.toml", Format::Toml),
//!     &["ghcr.io/you/portfolio@sha256:48e259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a291807060504030"],
//! )?;
//!
//! // The block to paste into a Helm template, under `metadata:`.
//! print!("{}", metadata.to_yaml(2));
//! # Ok::<(), terrace_config::Error>(())
//! ```
//!
//! # What this module does not do
//!
//! It renders no manifests. A chart is the thing that knows which object is which, which images a
//! document is read by, and what else belongs in `metadata` — and it already has a templating
//! language for that. What it does not have is a place to look up how the protocol is spelled,
//! which is the gap these constants close: a chart hand-writing `dev.terrace.config/images` is a
//! chart that can hand-write it differently from the policy reading it, and the failure mode of
//! that is a pairing check that silently never runs.

use std::collections::BTreeMap;

use super::{Contract, DEFAULT_PATH, Error, LABEL_VERSION};

/// The DNS-subdomain prefix every key in this module carries.
///
/// The same string as the image labels' `dev.terrace.config.*` stem, and that is the whole reason
/// it is spelled with dots: one namespace, two carriers. An operator grepping a cluster for
/// `dev.terrace.config` finds the pods and the `ConfigMap`s, and an operator grepping an image
/// config blob for the same string finds the labels — rather than learning that the Kubernetes
/// side was named `terrace.dev` because it read better on the day it was written.
pub const NAMESPACE: &str = "dev.terrace.config";

/// The label carrying [`CONTRACT_VERSION`](super::CONTRACT_VERSION), stringified.
///
/// This is the entire label surface, and the value is always legal: a decimal integer begins and
/// ends alphanumeric and is nowhere near 63 characters.
///
/// It answers the one question a cluster-side actor asks with a *selector* — does this object
/// participate in the protocol, and in which version of it — which is what a Kyverno policy
/// matches on:
///
/// ```yaml
/// match:
///   any:
///     - resources:
///         kinds: [ConfigMap]
///         selector:
///           matchExpressions:
///             - key: dev.terrace.config/contract-version
///               operator: Exists
/// ```
pub const LABEL_CONTRACT_VERSION: &str = "dev.terrace.config/contract-version";

/// The annotation listing every image that reads this document, comma-separated.
///
/// Each reference is **digest-pinned**, and that is a hard requirement rather than a preference: a
/// tag can be moved after the pairing was checked, so a pairing keyed on a tag proves nothing
/// about the image that is actually running. An unpinned one is refused.
///
/// Comma-separated because a document may be read by several images — see
/// [`Target`] and the union case this exists for — and in declaration order, so the value is
/// byte-stable across renders of one chart.
///
/// This crate cannot supply the value. A [`Contract`] deliberately carries no digest: the digest
/// is what building the image *produces*, so a document naming it would have to be written after
/// the push, changing bytes that were already hashed. Whatever renders the object passes the
/// references in; this module's job is to name the annotation, validate what it is handed, and
/// refuse a reference that is not pinned.
pub const ANNOTATION_IMAGES: &str = "dev.terrace.config/images";

/// The annotation naming which key inside `data` is the configuration document.
///
/// A `ConfigMap` may carry several files — a `config.toml` beside an `nginx.conf` beside a
/// `.env` — and without this a validator picks one by guessing, which is a validator that reports
/// on the wrong file and passes. Must be a legal `ConfigMap` data key: `[-._a-zA-Z0-9]+`, and neither `.` nor `..`.
///
/// A property of a document, so a pod template does not carry it. See [`Target`].
pub const ANNOTATION_DOCUMENT_KEY: &str = "dev.terrace.config/document-key";

/// The annotation naming which parser reads the document.
///
/// `toml` today. It exists because a YAML or JSON document normalises to the same tree and every
/// gate is unchanged by which one it was — but only after something has parsed it, and a validator
/// has to know which parser to reach for before it can do that. Modelled as [`Format`], which
/// carries a fallback variant.
///
/// A property of a document, so a pod template does not carry it. See [`Target`].
pub const ANNOTATION_FORMAT: &str = "dev.terrace.config/format";

// There is deliberately no `app` label, and the reasoning is worth keeping because the obvious
// design has one — every other object in a chart carries `app.kubernetes.io/name`.
//
// A document may be read by *several* images, which is the union case: one rendered document, one
// prefix, several binaries, each `Describe` covering only the keys it consumes. So the value would
// have to be multi-valued, and a multi-valued label needs a separator — every plausible one is
// illegal in a label value, because the character class is alphanumeric plus `-`, `_` and `.` and
// a `,`, a `/` or a space is none of those.
//
// That is not a reason to pick a fourth separator. It is the platform saying the fact does not
// belong in a label: anything per-image or multi-valued is an annotation, and `ANNOTATION_IMAGES`
// is where this one went. The rule has no exceptions here.
//
// There is also deliberately no `prefix` label and no `contract-path` label, and this is the more
// tempting omission because both values are right there on the contract.
//
// Both are facts about *an image*, and the image already carries them —
// `dev.terrace.config.prefix` and `dev.terrace.config.contract.path` in its config blob, written
// by the build that produced it. Copying them onto a Kubernetes object creates a second spelling
// of a fact that already has one, which is the exact drift `Contract::verify_labels` exists to
// catch. Here there would be nothing to catch it: the object is rendered by a chart this crate
// never sees, from values this crate never reads, so a chart stamping a stale prefix would be
// stamping it against no authority at all. `PORTFOLIO_` and `/config/contract.json` are not legal
// label values either, so the copy could not even be a label — it would be a second annotation
// restating the image, and a validator reading it would have to decide which copy wins.
//
// A consumer that wants the prefix or the path reads them where they are written, off the image,
// which is a `crane config` away and is what `Pairing` does.

/// Which object is being stamped, and so what belongs on it.
///
/// Two targets rather than two constructors returning one type with fields silently absent,
/// because the difference is not a detail of the call — it is the whole reason a pod carries a
/// stamp at all.
///
/// - A **document object** — the `ConfigMap`, or the `Secret` behind a secrets directory — is the
///   thing being validated. It gets the label and all three annotations.
/// - A **workload pod template** — `spec.template.metadata` — gets the label and
///   [`ANNOTATION_IMAGES`] only. `document-key` and `format` describe a document, and a pod is not
///   one; stamping them there would be asserting something about an object that has no `data`.
///
/// A pod is stamped at all so that an admission webhook seeing *only* the pod — which is what an
/// admission webhook usually sees, since that is the object being admitted — can find the image
/// list without walking `ownerReferences` back to a workload and forward to a `ConfigMap`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The object holding the configuration document.
    Document {
        /// Which key inside `data` the document is. See [`ANNOTATION_DOCUMENT_KEY`].
        key: String,
        /// Which parser reads it. See [`ANNOTATION_FORMAT`].
        format: Format,
    },
    /// The workload's pod template.
    Workload,
}

impl Target {
    /// A document object holding `key`, parsed as `format`.
    #[must_use]
    pub fn document(key: impl Into<String>, format: Format) -> Self {
        Self::Document {
            key: key.into(),
            format,
        }
    }
}

/// Which parser reads a configuration document.
///
/// [`Self::Other`] is not decoration. A consumer deserialising this into a closed set makes one
/// unfamiliar value a failure of the *whole read*, so a chart that starts rendering YAML would
/// break every validator that only ever knew about TOML — including the ones checking documents
/// that are still TOML. With a fallback, an unfamiliar format is one document a validator declines
/// to parse, and it can say so.
///
/// Saying so is the obligation that comes with the fallback: **a check skipped because the format
/// was not recognised has to be reported as skipped.** A silently skipped check is
/// indistinguishable from a passing one, which is the failure this whole protocol exists to
/// prevent one level up.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// TOML, which is what this crate's file layers read.
    Toml,
    /// YAML.
    Yaml,
    /// JSON.
    Json,
    /// A format this build does not know, kept verbatim.
    Other(String),
}

impl Format {
    /// The annotation value, as [`ANNOTATION_FORMAT`] spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::Other(other) => other,
        }
    }

    /// Read an [`ANNOTATION_FORMAT`] value.
    ///
    /// Never fails — an unfamiliar value becomes [`Self::Other`], which is the point of the
    /// variant.
    ///
    /// No case folding, deliberately. The protocol spells these lower case and that is what
    /// [`Self::as_str`] emits, so `TOML` is a spelling nothing produces. Reading it as
    /// [`Self::Toml`] would quietly bless a second spelling and leave two of them in circulation;
    /// reading it as `Other("TOML")` tells a validator it does not recognise the value, which is
    /// true and is what makes somebody fix the chart.
    #[must_use]
    pub fn from_annotation(value: &str) -> Self {
        match value {
            "toml" => Self::Toml,
            "yaml" => Self::Yaml,
            "json" => Self::Json,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The labels and annotations one object carries, ready to render.
///
/// Both maps are [`BTreeMap`], never `HashMap`: this is output that gets pasted into a chart,
/// committed, and diffed in review, so it has to be byte-identical across runs and across
/// processes. A hash map's order is neither.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

impl Metadata {
    /// The labels, keyed by their full names.
    #[must_use]
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// The annotations, keyed by their full names.
    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// The block to paste into a Helm template, every line indented by `indent` spaces.
    ///
    /// Mirrors [`Contract::to_dockerfile_labels`](super::Contract::to_dockerfile_labels) and
    /// exists for the same reason: hand-writing is unavoidable — a chart's `metadata` is written
    /// in a templating language this crate has no part in — so the honest answer is to make
    /// hand-writing a copy-paste, and then to make the result checkable. That check is
    /// [`Contract::verify_kube_metadata`].
    ///
    /// Emits both mapping headers, so `indent` is the indentation of `metadata:`'s children:
    ///
    /// ```yaml
    /// metadata:
    ///   name: portfolio-config
    ///   labels:
    ///     dev.terrace.config/contract-version: "1"
    ///   annotations:
    ///     dev.terrace.config/images: "ghcr.io/you/portfolio@sha256:…"
    /// ```
    ///
    /// A chart that already emits its own `labels:` — from a `common.labels` helper, which most
    /// do — pastes the entries rather than the headers. Emitting the headers anyway is the right
    /// default because the output is then valid on its own and a reader can see what nests where;
    /// the alternative renders two bare entries that only mean something once you know where they
    /// were meant to go.
    ///
    /// Values are always double-quoted. `"1"` has to be, or YAML reads the contract version as an
    /// integer and Kubernetes refuses a label value that is not a string — which is a `helm
    /// install` failure rather than a rendering one, so it is found late.
    ///
    /// Ends with a newline.
    #[must_use]
    pub fn to_yaml(&self, indent: usize) -> String {
        let pad = " ".repeat(indent);
        let mut rendered = String::new();
        for (header, entries) in [("labels", &self.labels), ("annotations", &self.annotations)] {
            rendered.push_str(&pad);
            rendered.push_str(header);
            rendered.push_str(":\n");
            for (name, value) in entries {
                rendered.push_str(&pad);
                rendered.push_str("  ");
                // The keys are this module's own constants, and every one of them is a plain YAML
                // scalar — no leading indicator, no `: ` inside it — so they need no quoting.
                rendered.push_str(name);
                rendered.push_str(": \"");
                // A value carrying either of these would break the document rather than the
                // deployment, which is the worst way to find out. Neither occurs in an image
                // reference or a `ConfigMap` key; `Format::Other` is the one that could.
                for character in value.chars() {
                    if character == '"' || character == '\\' {
                        rendered.push('\\');
                    }
                    rendered.push(character);
                }
                rendered.push_str("\"\n");
            }
        }
        rendered
    }
}

impl Contract {
    /// The metadata a chart stamps onto one rendered object.
    ///
    /// `images` is every image that reads the document, digest-pinned, in declaration order — see
    /// [`ANNOTATION_IMAGES`] for why this crate cannot produce them and why a tag will not do.
    ///
    /// The emitted keys and label values are checked against the Kubernetes rules on the way out.
    /// Today that check cannot fail — every key is a constant in this module and the only label
    /// value is a decimal integer — and it is here precisely because that is a property of the
    /// current design rather than of the type system. The first label carrying anything derived
    /// from a contract is the change that breaks it, and this is what turns that into a caught
    /// error instead of an object the API server refuses. What is validated against *caller*
    /// input is the document key, the format and the references, above.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if `images` is empty, if any reference is not digest-pinned, if
    /// the document key is not a legal `ConfigMap` key, or if the format is empty.
    pub fn kube_metadata(&self, target: &Target, images: &[&str]) -> Result<Metadata, Error> {
        if images.is_empty() {
            return Err(Error::Invalid(format!(
                "no images were named for this object, so `{ANNOTATION_IMAGES}` would be empty. \
                 An object stamped with an empty image list claims to participate in the protocol \
                 and gives a validator nothing to pair it against — every membership test against \
                 an empty list fails. Name every image that reads this document."
            )));
        }
        for image in images {
            digest_ref(image)?;
        }

        let mut annotations = BTreeMap::new();
        annotations.insert(ANNOTATION_IMAGES.to_owned(), images.join(","));

        if let Target::Document { key, format } = target {
            configmap_key(key)?;
            if format.as_str().is_empty() {
                return Err(Error::Invalid(format!(
                    "this document's format is the empty string, so `{ANNOTATION_FORMAT}` would \
                     say nothing about which parser reads it. Name the format — `toml` is what \
                     this crate's file layers read."
                )));
            }
            annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), key.clone());
            annotations.insert(ANNOTATION_FORMAT.to_owned(), format.as_str().to_owned());
        }

        let mut labels = BTreeMap::new();
        labels.insert(
            LABEL_CONTRACT_VERSION.to_owned(),
            self.terrace_contract.to_string(),
        );

        for (name, value) in &labels {
            label_key(name)?;
            label_value(value)?;
        }
        for name in annotations.keys() {
            label_key(name)?;
        }

        Ok(Metadata {
            labels,
            annotations,
        })
    }

    /// Check that a rendered object carries the metadata this contract and target call for.
    ///
    /// `labels` and `annotations` are the object's `metadata.labels` and `metadata.annotations`.
    ///
    /// Keys **outside** the `dev.terrace.config` namespace are ignored, for the reason
    /// [`Contract::verify_labels`](super::Contract::verify_labels) ignores extra image labels: an
    /// object carries `app.kubernetes.io/*`, a `helm.sh/chart`, whatever an operator added, and
    /// none of it is this document's business.
    ///
    /// Keys **inside** the namespace that this target should not carry are refused, which is the
    /// one place this is stricter than the image-side check — and the difference is not
    /// inconsistency. `org.opencontainers.image.title` on an image belongs to somebody else;
    /// `dev.terrace.config/document-key` on a pod template belongs to *this* protocol and is
    /// simply wrong, as is `dev.terrace.config/image` on anything. Both are somebody spelling the
    /// protocol from memory, which is the failure these constants exist to prevent, and ignoring
    /// them would mean the misspelling is never reported by anything.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first key that is missing, wrong, malformed, or does
    /// not belong on this target.
    pub fn verify_kube_metadata(
        &self,
        target: &Target,
        labels: &BTreeMap<String, String>,
        annotations: &BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let expected = self.terrace_contract.to_string();
        match labels.get(LABEL_CONTRACT_VERSION) {
            Some(found) if found == &expected => {}
            Some(found) => {
                return Err(Error::Invalid(format!(
                    "the object's `{LABEL_CONTRACT_VERSION}` is `{found}`, and this contract's is \
                     `{expected}`. The two describe different versions of this protocol, so a \
                     validator reading the object would be applying rules the contract was not \
                     written against. Re-render the chart against this contract."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so nothing selecting on \
                     that label will ever see it and no cluster-side check will run on it. \
                     `Contract::kube_metadata` emits the block."
                )));
            }
        }

        let images = require(annotations, ANNOTATION_IMAGES)?;
        for image in images.split(',') {
            digest_ref(image.trim())?;
        }

        match target {
            Target::Document { key, format } => {
                let found = require(annotations, ANNOTATION_DOCUMENT_KEY)?;
                if found != key {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_DOCUMENT_KEY}` is `{found}`, and this document \
                         is `{key}`. A validator would read the wrong entry of `data` and report \
                         on a file nothing mounts. Stamp the key the document is actually under."
                    )));
                }
                configmap_key(found)?;

                let found = require(annotations, ANNOTATION_FORMAT)?;
                if found != format.as_str() {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_FORMAT}` is `{found}`, and this document is \
                         `{format}`. A validator would reach for the wrong parser, and a parse \
                         failure reads as a broken document rather than as a wrong annotation. \
                         Stamp the format the document is actually written in."
                    )));
                }
            }
            Target::Workload => {}
        }

        known_keys(labels, &[LABEL_CONTRACT_VERSION], "label")?;
        known_keys(annotations, target.annotations(), "annotation")
    }
}

impl Target {
    /// The annotations an object of this target is allowed to carry, in this namespace.
    fn annotations(&self) -> &'static [&'static str] {
        match self {
            Self::Document { .. } => &[
                ANNOTATION_IMAGES,
                ANNOTATION_DOCUMENT_KEY,
                ANNOTATION_FORMAT,
            ],
            Self::Workload => &[ANNOTATION_IMAGES],
        }
    }
}

/// One entry of `map`, or an error naming what is missing.
fn require<'a>(map: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, Error> {
    map.get(name).map(String::as_str).ok_or_else(|| {
        Error::Invalid(format!(
            "the object carries no `{name}`, which is the annotation a validator reads to know \
             what it is looking at. `Contract::kube_metadata` emits the block."
        ))
    })
}

/// Refuse a key in this module's namespace that the target has no place for.
fn known_keys(map: &BTreeMap<String, String>, allowed: &[&str], kind: &str) -> Result<(), Error> {
    let prefix = format!("{NAMESPACE}/");
    for name in map.keys() {
        if !name.starts_with(&prefix) || allowed.contains(&name.as_str()) {
            continue;
        }
        return Err(Error::Invalid(format!(
            "the object carries a `{name}` {kind}, which this protocol does not define for this \
             object. Either it is a misspelling of one that is defined — the {kind}s here are \
             {} — or it is a fact about a different kind of object, such as a document key on a \
             pod template. Nothing reads it, so nothing would ever report it as wrong.",
            allowed.join(", ")
        )));
    }
    Ok(())
}

/// Both halves of the protocol, and the assertion that they describe one configuration surface.
///
/// This is the function a cluster-side actor calls, and the point of the whole module. Everything
/// above it is a way of writing something down; this is the thing that reads all of it back and
/// says whether the pod about to start and the document about to be mounted belong together.
///
/// The inputs are the two halves and the tie between them:
///
/// - the contract under test, and the path it was embedded at;
/// - what `crane config` or `docker inspect` reports for the running container's image under
///   `config.Labels`, and that image's digest-pinned reference;
/// - the mounted document object's `metadata.labels` and `metadata.annotations`.
///
/// ```no_run
/// # use std::collections::BTreeMap;
/// # use terrace_config::schema::Contract;
/// use terrace_config::schema::kube::Pairing;
///
/// # fn run(contract: &Contract, image_labels: &BTreeMap<String, String>,
/// #        object_labels: &BTreeMap<String, String>,
/// #        object_annotations: &BTreeMap<String, String>) -> Result<(), terrace_config::Error> {
/// Pairing::new(contract)
///     .image(image_labels, "ghcr.io/you/portfolio@sha256:48e2…")
///     .object(object_labels, object_annotations)
///     .check()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Pairing<'a> {
    contract: &'a Contract,
    path: &'a str,
    image: Option<ImageSide<'a>>,
    object: Option<ObjectSide<'a>>,
}

/// The image half of a [`Pairing`]: what its config blob says, and which image it is.
///
/// The two travel together because neither is worth much alone — labels with no reference cannot
/// be pinned to a running container, and a reference with no labels says nothing about a contract.
#[derive(Debug, Clone, Copy)]
struct ImageSide<'a> {
    labels: &'a BTreeMap<String, String>,
    reference: &'a str,
}

/// The object half of a [`Pairing`]: the mounted document's own metadata, both maps.
#[derive(Debug, Clone, Copy)]
struct ObjectSide<'a> {
    labels: &'a BTreeMap<String, String>,
    annotations: &'a BTreeMap<String, String>,
}

impl<'a> Pairing<'a> {
    /// A pairing against `contract`, embedded at
    /// [`DEFAULT_PATH`].
    #[must_use]
    pub fn new(contract: &'a Contract) -> Self {
        Self {
            contract,
            path: DEFAULT_PATH,
            image: None,
            object: None,
        }
    }

    /// Where the contract is embedded in the image, if not
    /// [`DEFAULT_PATH`].
    ///
    /// The value the image's `dev.terrace.config.contract.path` label is checked against.
    #[must_use]
    pub fn embedded_at(mut self, path: &'a str) -> Self {
        self.path = path;
        self
    }

    /// The running container's image: its config-blob labels, and its digest-pinned reference.
    #[must_use]
    pub fn image(mut self, labels: &'a BTreeMap<String, String>, reference: &'a str) -> Self {
        self.image = Some(ImageSide { labels, reference });
        self
    }

    /// The mounted document object's `metadata.labels` and `metadata.annotations`.
    #[must_use]
    pub fn object(
        mut self,
        labels: &'a BTreeMap<String, String>,
        annotations: &'a BTreeMap<String, String>,
    ) -> Self {
        self.object = Some(ObjectSide {
            labels,
            annotations,
        });
        self
    }

    /// Check that all of it describes one configuration surface.
    ///
    /// Five questions, in this order, because each one makes the next worth asking:
    ///
    /// 1. the object's [`LABEL_CONTRACT_VERSION`] is present and is this contract's;
    /// 2. the image's own three labels agree with this contract —
    ///    [`Contract::verify_labels`](super::Contract::verify_labels), run from the cluster side;
    /// 3. the object's contract version equals the image's;
    /// 4. the running image is a **member** of the object's [`ANNOTATION_IMAGES`];
    /// 5. [`ANNOTATION_DOCUMENT_KEY`] and [`ANNOTATION_FORMAT`] are present and well-formed.
    ///
    /// Question 3 looks redundant after 1 and 2, and is not. Both compare against *this* contract,
    /// so both pass whenever the caller fetched the right document — and the case worth catching
    /// is the one where they did not: a policy holding a contract that matches neither side, or
    /// matching one side by coincidence. Asking the object and the image about each other directly
    /// is the only question whose answer does not depend on the contract being the right one.
    ///
    /// Question 4 is membership rather than equality because a document may be read by several
    /// images. A container is correct if it is *one of* the readers; requiring it to be the only
    /// one would refuse every union deployment.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming both sides of the first disagreement, and what to do
    /// about it. Also returns it if the image or the object was never supplied.
    pub fn check(&self) -> Result<(), Error> {
        let ImageSide {
            labels: image_labels,
            reference,
        } = self.image.ok_or_else(|| {
            Error::Invalid(
                "this pairing has no image, so there is nothing to pair the object against. Pass \
                 the running container's `config.Labels` and its digest-pinned reference to \
                 `Pairing::image`."
                    .to_owned(),
            )
        })?;
        let ObjectSide {
            labels: object_labels,
            annotations: object_annotations,
        } = self.object.ok_or_else(|| {
            Error::Invalid(
                "this pairing has no object, so there is nothing to pair the image against. Pass \
                 the mounted document's `metadata.labels` and `metadata.annotations` to \
                 `Pairing::object`."
                    .to_owned(),
            )
        })?;

        // 1. The object participates, in the version this contract is written in.
        let expected = self.contract.terrace_contract.to_string();
        let object_version = object_labels.get(LABEL_CONTRACT_VERSION).ok_or_else(|| {
            Error::Invalid(format!(
                "the mounted object carries no `{LABEL_CONTRACT_VERSION}`, so nothing says it was \
                 rendered against a contract at all. A validator cannot tell an unstamped object \
                 from one whose stamp was lost; both have to be refused. Stamp it with \
                 `Contract::kube_metadata`."
            ))
        })?;
        if object_version != &expected {
            return Err(Error::Invalid(format!(
                "the mounted object's `{LABEL_CONTRACT_VERSION}` is `{object_version}`, and this \
                 contract's is `{expected}`. The object was rendered against a different version \
                 of this protocol than the one being checked. Re-render the chart, or check \
                 against the contract the object was rendered from."
            )));
        }

        // 2. The image agrees with the contract. Reused rather than restated: this is the same
        //    question the build asked before the push, asked again from the far side, and two
        //    implementations of it would be two chances to disagree about what a label means.
        self.contract.verify_labels(self.path, image_labels)?;

        // 3. The object and the image agree with each other, without going through the contract.
        let image_version = image_labels.get(LABEL_VERSION).ok_or_else(|| {
            Error::Invalid(format!(
                "the running image carries no `{LABEL_VERSION}`, so it does not publish a config \
                 contract and there is nothing for the mounted object to pair with. Either the \
                 wrong image is running, or its build never emitted the label block."
            ))
        })?;
        if image_version != object_version {
            return Err(Error::Invalid(format!(
                "the running image's `{LABEL_VERSION}` is `{image_version}`, and the mounted \
                 object's `{LABEL_CONTRACT_VERSION}` is `{object_version}`. The image and the \
                 document it is about to read were produced against different versions of this \
                 protocol — a chart and an image that were rolled out separately. Roll the two \
                 forward together."
            )));
        }

        // 4. The running image is one of the document's declared readers.
        let running = reference_identity(reference)?;
        let images = require(object_annotations, ANNOTATION_IMAGES)?;
        let mut members = Vec::new();
        let mut reads_it = false;
        for member in images.split(',') {
            let member = member.trim();
            // Every member is validated even once a match is found, so that an unpinned reference
            // beside a correct one is still reported. A list that is half-pinned is a list whose
            // next rollout silently stops proving anything.
            digest_ref(member)?;
            reads_it |= reference_identity(member)? == running;
            members.push(member);
        }
        if !reads_it {
            return Err(Error::Invalid(format!(
                "the running image is `{reference}`, and the mounted object declares its readers \
                 as `{}`. A document is validated against the contracts of the images that read \
                 it, so a container reading one it is not listed in is being checked against \
                 somebody else's configuration surface — or is not listed because the chart \
                 forgot it. Add it to `{ANNOTATION_IMAGES}`, or mount the document its own images \
                 declare.",
                members.join(", ")
            )));
        }

        // 5. The document says which entry of `data` it is, and how to parse it.
        self.document_is_named()
    }

    /// The document half of question 5, split out so [`Self::check`] reads as its five steps.
    ///
    /// Runs after membership: a document key that is well-formed but belongs to a document the
    /// running image never reads is not the interesting failure, and reporting it first would
    /// send somebody to fix the wrong annotation.
    fn document_is_named(&self) -> Result<(), Error> {
        let Some(ObjectSide { annotations, .. }) = self.object else {
            return Ok(());
        };
        configmap_key(require(annotations, ANNOTATION_DOCUMENT_KEY)?)?;
        let format = require(annotations, ANNOTATION_FORMAT)?;
        if format.is_empty() {
            return Err(Error::Invalid(format!(
                "the mounted object's `{ANNOTATION_FORMAT}` is empty, so nothing says which \
                 parser reads the document. A validator that guesses reports a parse failure on a \
                 file that is perfectly valid in the format it is actually written in. Stamp the \
                 format."
            )));
        }
        Ok(())
    }
}

/// Refuse a key Kubernetes would refuse: an optional DNS-subdomain prefix of 253 characters or
/// fewer, a `/`, then a name of 63 characters or fewer.
///
/// The name obeys the same character class a label value does, except that it may not be empty.
///
/// # Errors
/// Returns [`Error::Invalid`] naming which half is wrong and why.
fn label_key(value: &str) -> Result<(), Error> {
    let (prefix, name) = match value.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, value),
    };

    if name.contains('/') {
        return Err(Error::Invalid(format!(
            "`{value}` carries more than one `/`, and a Kubernetes key has at most one — the \
             separator between its DNS-subdomain prefix and its name."
        )));
    }

    if let Some(prefix) = prefix {
        dns_subdomain(prefix).map_err(|reason| {
            Error::Invalid(format!(
                "the prefix of `{value}` is not a DNS subdomain: {reason}. Kubernetes refuses the \
                 object rather than the key, so nothing carrying it can be applied at all."
            ))
        })?;
    }

    if name.is_empty() {
        return Err(Error::Invalid(format!(
            "`{value}` has an empty name after its `/`, and a Kubernetes key must name something."
        )));
    }
    name_segment(name).map_err(|reason| {
        Error::Invalid(format!(
            "the name of `{value}` is not a legal Kubernetes key: {reason}. Kubernetes refuses \
             the object rather than the key, so nothing carrying it can be applied at all."
        ))
    })
}

/// Refuse a label value Kubernetes would refuse.
///
/// 63 characters or fewer and, unless empty, matching
/// `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?` — alphanumeric at both ends, with `-`, `_` and
/// `.` allowed between.
///
/// The empty string is legal, and is why this is a separate rule from [`label_key`]'s name half.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the character or the length that breaks the rule.
fn label_value(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }
    name_segment(value).map_err(|reason| {
        Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes label value: {reason}. This is the constraint \
             that decides what can be a label at all — a value that is not selectable has to be \
             an annotation instead."
        ))
    })
}

/// The shared character rule: 63 characters or fewer, alphanumeric at both ends, `-`, `_` and `.`
/// between.
///
/// Written by hand rather than as a regular expression. Four predicates do not justify a
/// dependency in a manifest that argues for every one it has, and the rule is three conditions
/// once it is not being read out of a character class.
fn name_segment(value: &str) -> Result<(), String> {
    if value.len() > 63 {
        return Err(format!(
            "it is {} characters and the limit is 63",
            value.len()
        ));
    }
    let legal = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
    if let Some(bad) = value.chars().find(|c| !legal(*c)) {
        return Err(format!(
            "`{bad}` is not one of the characters allowed here, which are ASCII letters, digits, \
             `-`, `_` and `.`"
        ));
    }
    // Checked after the character class so that a value failing both is reported by the character,
    // which is the more actionable half: `PORTFOLIO_` ends illegally and `a/b` contains an illegal
    // character, and being told about the `/` is what explains the fix.
    let ends_well = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    if !ends_well(value.chars().next()) || !ends_well(value.chars().next_back()) {
        return Err(
            "it must begin and end with an ASCII letter or digit, and this begins or ends with \
             `-`, `_` or `.`"
                .to_owned(),
        );
    }
    Ok(())
}

/// The DNS-subdomain rule a key's prefix obeys: 253 characters or fewer, dot-separated labels of
/// lower-case alphanumerics and `-`, each beginning and ending alphanumeric.
fn dns_subdomain(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("it is empty".to_owned());
    }
    if value.len() > 253 {
        return Err(format!(
            "it is {} characters and the limit is 253",
            value.len()
        ));
    }
    for label in value.split('.') {
        if label.is_empty() {
            return Err(
                "it has an empty label, from a leading, trailing or doubled `.`".to_owned(),
            );
        }
        if label.len() > 63 {
            return Err(format!(
                "the label `{label}` is {} characters and the limit is 63",
                label.len()
            ));
        }
        let legal = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if let Some(bad) = label.chars().find(|c| !legal(*c)) {
            return Err(format!(
                "`{bad}` is not allowed in a DNS label, which takes lower-case ASCII letters, \
                 digits and `-`"
            ));
        }
        let ends_well = |c: Option<char>| c.is_some_and(|c: char| c.is_ascii_alphanumeric());
        if !ends_well(label.chars().next()) || !ends_well(label.chars().next_back()) {
            return Err(format!(
                "the label `{label}` must begin and end with a letter or digit"
            ));
        }
    }
    Ok(())
}

/// Refuse a `ConfigMap` data key Kubernetes would refuse.
///
/// `[-._a-zA-Z0-9]+`, capped at 253 characters, and neither `.` nor `..` — those two name a
/// directory rather than a file, and a `ConfigMap` key becomes a file name when the object is
/// mounted as a volume.
///
/// # Errors
/// Returns [`Error::Invalid`] naming what is wrong with the key.
fn configmap_key(value: &str) -> Result<(), Error> {
    let refuse = |reason: &str| {
        Err(Error::Invalid(format!(
            "`{value}` is not a legal `ConfigMap` data key: {reason}. The key names an entry of \
             `data`, and becomes a file name when the object is mounted as a volume."
        )))
    };

    if value.is_empty() {
        return refuse("it is empty");
    }
    if value.len() > 253 {
        return refuse(&format!(
            "it is {} characters and the limit is 253",
            value.len()
        ));
    }
    if value == "." || value == ".." {
        return refuse("it names a directory rather than a file");
    }
    let legal = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.';
    if let Some(bad) = value.chars().find(|c| !legal(*c)) {
        return refuse(&format!(
            "`{bad}` is not allowed, and the characters that are are ASCII letters, digits, `-`, \
             `_` and `.`"
        ));
    }
    Ok(())
}

/// Refuse an image reference that is not pinned to a digest.
///
/// The reference must carry `@sha256:` followed by exactly 64 lower-case hexadecimal characters,
/// at the end, with a non-empty name before it.
///
/// A tag is refused rather than tolerated, and this is the one validation here that is about the
/// protocol rather than about Kubernetes. A tag can be moved after a pairing was checked and
/// before the pod is rescheduled, so a pairing keyed on a tag is a claim about whatever the tag
/// pointed at when somebody last looked — which is to say, no claim at all.
///
/// What is deliberately *not* checked is whether the name is a well-formed OCI repository, let
/// alone one that resolves. A registry is the authority on both and will say so; restating its
/// grammar here would be a second opinion that can drift from it, and the only property a pairing
/// actually rests on is the one above.
///
/// # Errors
/// Returns [`Error::Invalid`] saying which part is missing or malformed.
fn digest_ref(value: &str) -> Result<(), Error> {
    reference_identity(value).map(|_| ())
}

/// The `(name, digest)` a reference denotes, with any tag between them dropped.
///
/// Two references name the same image when both halves match. The tag is dropped rather than
/// compared because `repo:v2.5.0@sha256:48e2…` and `repo@sha256:48e2…` are the same image by
/// definition — the digest is what resolves — and both spellings occur in practice: a chart pins
/// `image.tag: v2.5.0@sha256:48e2…` for the benefit of whoever reads the values file, while a
/// pod's `status.containerStatuses[].imageID` carries no tag at all. Comparing the full strings
/// would refuse a pairing that is correct, and refuse it with a message pointing at a digest that
/// visibly matches.
fn reference_identity(value: &str) -> Result<(&str, &str), Error> {
    let refuse = |reason: &str| {
        Err(Error::Invalid(format!(
            "`{value}` is not a digest-pinned image reference: {reason}. A pairing is only worth \
             checking against an image that cannot change under it, and a tag can be moved after \
             the check and before the pod is rescheduled. Pin it as \
             `registry/repository@sha256:<64 hex characters>`."
        )))
    };

    if value.contains(',') {
        return refuse("it contains a `,`, which is what separates one reference from the next");
    }
    let Some((name, digest)) = value.split_once('@') else {
        return refuse("it carries no `@sha256:` digest");
    };
    if name.is_empty() {
        return refuse("it names no repository before its `@`");
    }
    if digest.contains('@') {
        return refuse("it carries more than one `@`");
    }
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return refuse("its digest is not a `sha256:`");
    };
    if hex.len() != 64 {
        return refuse(&format!(
            "its digest is {} characters and a `sha256` is 64",
            hex.len()
        ));
    }
    // Lower case as well as hexadecimal. A registry treats `SHA256:48E2…` as a different string
    // from the digest it serves, so an upper-case spelling is one that resolves nowhere — and
    // accepting it here would let a membership test miss on two references that look identical.
    if let Some(bad) = hex
        .chars()
        .find(|c| !c.is_ascii_hexdigit() || c.is_ascii_uppercase())
    {
        return refuse(&format!(
            "`{bad}` is not a lower-case hexadecimal character, and a digest is written in those"
        ));
    }

    // The tag, if the reference carries one, sits between the last `:` and the `@` — and only if
    // that `:` comes after the last `/`, since a registry may carry a port.
    let bare = match name.rsplit_once(':') {
        Some((repository, _))
            if !repository.is_empty() && !name[repository.len()..].contains('/') =>
        {
            repository
        }
        _ => name,
    };
    Ok((bare, hex))
}

#[cfg(test)]
mod tests {
    use super::{
        Format, configmap_key, digest_ref, dns_subdomain, label_key, label_value,
        reference_identity,
    };

    /// A digest that is legal in every way, so a test about something else is only about that.
    const DIGEST: &str = "sha256:48e259cb1b0f1a1d3b0f6c0a2e5d4c3b2a1908f7e6d5c4b3a291807060504030";

    #[test]
    fn a_label_value_is_alphanumeric_at_both_ends_and_short() {
        for legal in ["1", "a", "toml", "v2.5.0", "a-b_c.d", &"a".repeat(63), ""] {
            assert!(label_value(legal).is_ok(), "`{legal}` is legal");
        }
        // The two the crate already publishes, and cannot publish as labels.
        for illegal in [
            "PORTFOLIO_",
            "/config/contract.json",
            "application/vnd.terrace.config-schema.v1+json",
            "-leading",
            "trailing.",
            "a,b",
            "a b",
        ] {
            assert!(label_value(illegal).is_err(), "`{illegal}` is illegal");
        }
        assert!(
            label_value(&"a".repeat(64)).is_err(),
            "64 is over the limit"
        );
    }

    #[test]
    fn a_label_key_takes_one_optional_dns_prefix_and_one_name() {
        for legal in [
            "dev.terrace.config/contract-version",
            "app.kubernetes.io/name",
            "bare-name",
        ] {
            assert!(label_key(legal).is_ok(), "`{legal}` is legal");
        }
        for illegal in [
            "dev.terrace.config/",
            "/name",
            "a/b/c",
            "DEV.TERRACE.CONFIG/name",
            "dev..config/name",
        ] {
            assert!(label_key(illegal).is_err(), "`{illegal}` is illegal");
        }
    }

    #[test]
    fn a_dns_subdomain_is_lower_case_and_has_no_empty_labels() {
        assert!(dns_subdomain("dev.terrace.config").is_ok());
        assert!(dns_subdomain("a").is_ok());
        for illegal in ["", ".a", "a.", "a..b", "A", "a_b", "-a"] {
            assert!(dns_subdomain(illegal).is_err(), "`{illegal}` is illegal");
        }
    }

    #[test]
    fn a_configmap_key_is_a_file_name_and_not_a_directory() {
        for legal in ["config.toml", "a", "-_.", "00-base.yaml"] {
            assert!(configmap_key(legal).is_ok(), "`{legal}` is legal");
        }
        // `.` and `..` are the two the character class admits and a volume mount cannot.
        for illegal in ["", ".", "..", "a/b", "a b", "a:b"] {
            assert!(configmap_key(illegal).is_err(), "`{illegal}` is illegal");
        }
    }

    #[test]
    fn a_reference_without_a_digest_is_refused() {
        assert!(digest_ref(&format!("ghcr.io/you/portfolio@{DIGEST}")).is_ok());
        for illegal in [
            "ghcr.io/you/portfolio",
            "ghcr.io/you/portfolio:v2.5.0",
            "ghcr.io/you/portfolio@sha256:short",
            "ghcr.io/you/portfolio@md5:48e2",
            "@sha256:48e2",
            &format!("ghcr.io/you/portfolio@{}", DIGEST.to_uppercase()),
            &format!("a@{DIGEST},b@{DIGEST}"),
        ] {
            assert!(digest_ref(illegal).is_err(), "`{illegal}` is illegal");
        }
    }

    #[test]
    fn a_tag_beside_a_digest_names_the_same_image_as_the_digest_alone() {
        let tagged = format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST}");
        let bare = format!("ghcr.io/you/portfolio@{DIGEST}");
        assert_eq!(
            reference_identity(&tagged).expect("legal"),
            reference_identity(&bare).expect("legal"),
        );

        // A registry port is a `:` that is not a tag, and dropping it would make two different
        // registries look like one.
        let ported = format!("registry.local:5000/you/portfolio@{DIGEST}");
        assert_eq!(
            reference_identity(&ported).expect("legal").0,
            "registry.local:5000/you/portfolio",
        );
    }

    #[test]
    fn an_unfamiliar_format_is_kept_rather_than_refused() {
        assert_eq!(Format::from_annotation("toml"), Format::Toml);
        assert_eq!(Format::from_annotation("toml").as_str(), "toml");
        // Not folded: `TOML` is a spelling nothing emits, and reading it as `Toml` would bless a
        // second one.
        assert_eq!(
            Format::from_annotation("TOML"),
            Format::Other("TOML".to_owned())
        );
        assert_eq!(
            Format::from_annotation("hcl"),
            Format::Other("hcl".to_owned())
        );
        assert_eq!(Format::from_annotation("hcl").as_str(), "hcl");
    }
}
