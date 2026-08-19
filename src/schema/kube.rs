//! The Kubernetes half of the protocol: what a rendered object carries, and the check that pairs
//! it with the image half.
//!
//! [`Contract::labels`] publishes three labels on an *image*, which is everything a pipeline
//! holding a digest needs. It is nothing at all to something inside the cluster. A Kyverno policy,
//! a validating admission webhook and an initContainer inspecting a mounted `ConfigMap` all hold an
//! **object**, not an image, and no field on that object says which image's contract the document
//! in it was rendered against. This module is the other end of that: the metadata a chart stamps
//! onto the object, and [`Pairing`], which asserts that the object and the running image describe
//! one configuration surface rather than two that happen to be deployed together.
//!
//! # Labels are what you select on. Annotations are what you read.
//!
//! That sentence is the whole design, and it is forced by the platform rather than chosen.
//!
//! A Kubernetes **label value** must be 63 characters or fewer and, unless empty, match
//! `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?` — it begins and ends alphanumeric, with `-`, `_`
//! and `.` in between. Every interesting value this crate already publishes on the image side
//! fails that rule:
//!
//! | Value | Why it is not a legal label value |
//! |---|---|
//! | `PORTFOLIO_` — the dialect prefix | trailing `_` |
//! | `/config/contract.json` — [`DEFAULT_PATH`](super::DEFAULT_PATH) | `/` is not in the set |
//! | `application/vnd.terrace.config-schema.v1+json` — [`ARTIFACT_TYPE`](super::ARTIFACT_TYPE) | `/` and `+` |
//!
//! A **label key** may carry a DNS-subdomain prefix of up to 253 characters followed by `/`, then a
//! name of up to 63 under the same character rule. `dev.terrace.config` is a legal prefix, so the
//! keys are never the problem — the values are.
//!
//! An **annotation key** follows the same key rule. Annotation *values* are unconstrained; only the
//! whole `metadata.annotations` map is bounded, at 256 KiB.
//!
//! So: the one fact a cluster-side actor selects on is a label, and everything else is an
//! annotation. Reversing that is not a style disagreement, it is a `kubectl apply` that fails —
//! and it fails at deploy time, on the chart, far from whatever decided to put a prefix in a label.
//!
//! # What is deliberately not here
//!
//! Two omissions, in the style of the ones on the image side, because the obvious design has both.
//!
//! **No `prefix` label and no `contract-path` label.** Both are facts about *an image*, and the
//! image already carries them — [`LABEL_PREFIX`](super::LABEL_PREFIX) and
//! [`LABEL_PATH`](super::LABEL_PATH), checked against the document by [`Contract::verify_labels`].
//! Copying them onto a Kubernetes object would create a second spelling of a fact that already has
//! one, which is the exact drift `verify_labels` exists to catch — except that here nothing could
//! catch it, because the object is rendered by a chart this crate never sees. A value with one
//! writer and one reader cannot disagree with itself; a value with two writers eventually does.
//!
//! **No `app` label.** A document may be read by several images — see the union case in
//! `docs/config-contract-plan.md` §4.3, where eight binaries share one document under one prefix —
//! so the field is inherently multi-valued, and a multi-valued label needs a separator. Every
//! plausible one (`,`, `/`, a space) is illegal in a label value. That is not a workaround waiting
//! to be found: **anything per-image or multi-valued is an annotation**, and the rule has no
//! exceptions here. [`ANNOTATION_IMAGES`] is where the image list lives, for exactly this reason.
//!
//! # Two targets, stamped differently
//!
//! [`Target`] is an enum rather than two constructors returning one type with fields silently
//! absent, because the difference is real and a caller should have to say which it means:
//!
//! | Object | Label | `images` | `document-key` | `format` |
//! |---|---|---|---|---|
//! | [`Target::Document`] — the `ConfigMap` or `Secret` | yes | yes | yes | yes |
//! | [`Target::Workload`] — `spec.template.metadata` | yes | yes | no | no |
//!
//! `document-key` and `format` are properties of a document, and a pod is not one. The pod is
//! stamped at all so that an admission webhook seeing only a pod — which is what an admission
//! webhook usually sees — can find the image list without walking ownership references back to the
//! object that mounts it.
//!
//! ```
//! # use std::collections::BTreeMap;
//! # use terrace_config::Terrace;
//! # use terrace_config::schema::{App, Describe, Leaf, Sink};
//! use terrace_config::schema::kube::{Format, Target};
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
//! let target = Target::document("config.toml", Format::Toml);
//! let metadata = contract.kube_metadata(
//!     &target,
//!     &["ghcr.io/you/portfolio@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d"],
//! )?;
//!
//! print!("{}", metadata.to_yaml(2));
//! # Ok::<(), terrace_config::Error>(())
//! ```
//!
//! # What this does not do
//!
//! It renders no manifests. This crate names the protocol and verifies both halves of it; the chart
//! that produces the `ConfigMap` and the `Deployment` is somebody else's, and [`Metadata::to_yaml`]
//! exists to be pasted into it for [`Contract::to_dockerfile_labels`]'s reason — hand-writing is
//! unavoidable, so the honest answer is to make it a copy-paste and then check the result.

use std::collections::BTreeMap;
use std::fmt;

use super::{Contract, Error, LABEL_VERSION};

/// The DNS-subdomain prefix every key in this module carries.
///
/// Shared with the image labels' `dev.terrace.config.*` namespace by design: one owner, one
/// spelling, whichever side of the protocol is being read. A cluster operator grepping for it finds
/// both halves.
pub const NAMESPACE: &str = "dev.terrace.config";

/// The label carrying [`Contract::terrace_contract`], and the entire label surface of this module.
///
/// It answers the one question a cluster-side actor asks with a selector — *does this object
/// participate in the protocol, and in which version of it* — and its value is the contract version
/// stringified, which is digits and therefore always a legal label value. A Kyverno policy matches
/// participants with:
///
/// ```yaml
/// matchExpressions:
///   - key: dev.terrace.config/contract-version
///     operator: Exists
/// ```
///
/// Everything else this module emits is an annotation. See the module documentation for why that
/// split is the platform's decision rather than this crate's.
pub const LABEL_CONTRACT_VERSION: &str = "dev.terrace.config/contract-version";

/// The annotation listing every image that reads this document, comma-separated.
///
/// **Digest-pinned, and that is a requirement rather than a preference.** A tag can be moved, so a
/// pairing keyed on a tag proves nothing about the image that is actually running — the same
/// reasoning that makes the registry artifact attach to a digest in
/// `docs/config-contract-plan.md` §3.3. [`Contract::kube_metadata`] refuses a reference without
/// `@sha256:` and 64 lowercase hex digits.
///
/// The list is plural because a document may be read by several images. It is an annotation rather
/// than a label for the same reason: a multi-valued label needs a separator, and every separator is
/// illegal in a label value.
///
/// This crate cannot supply the value. A contract deliberately carries no digest — the note on
/// [`App`](super::App) says why, and it comes to this: a digest is what the push *produces*, so
/// nothing written before the push can name it. The references are therefore passed in by whatever
/// renders the object, and this module's job is to name the annotation, validate what it is given,
/// and refuse anything unpinned.
pub const ANNOTATION_IMAGES: &str = "dev.terrace.config/images";

/// The annotation naming which key inside `data` is the configuration document.
///
/// A `ConfigMap` may carry several files — a `config.toml` beside a `logging.json` beside a script
/// somebody mounted — and without this a validator guesses, which on a two-key object is a coin
/// toss it will lose silently. The value must be a legal `ConfigMap` data key: `[-._a-zA-Z0-9]+`,
/// and neither `.` nor `..`.
///
/// Only on [`Target::Document`]. A pod is not a document.
pub const ANNOTATION_DOCUMENT_KEY: &str = "dev.terrace.config/document-key";

/// The annotation naming which parser reads the document.
///
/// `toml` today, and the field exists because it will not be `toml` forever: `source.format` is
/// already in the plan and a YAML or JSON document normalises to the same tree, leaving every gate
/// unchanged. A validator still has to know which parser to reach for, and guessing from a file
/// extension is guessing from a name a chart chose.
///
/// Only on [`Target::Document`]. See [`Format`] for why the enum has a fallback variant.
pub const ANNOTATION_FORMAT: &str = "dev.terrace.config/format";

/// The longest a label value, or the name half of a label key, may be.
const MAX_NAME: usize = 63;

/// The longest the DNS-subdomain prefix of a label key may be.
const MAX_PREFIX: usize = 253;

/// The longest a `ConfigMap` data key may be.
const MAX_DATA_KEY: usize = 253;

/// What separates the name from the digest in a pinned reference.
const DIGEST_MARKER: &str = "sha256:";

/// The number of hex digits in a SHA-256 digest.
const DIGEST_HEX: usize = 64;

/// What separates one image reference from the next in [`ANNOTATION_IMAGES`].
const IMAGE_SEPARATOR: char = ',';

/// Which parser reads the document a stamped object carries.
///
/// Modelled with a fallback variant for the reason [`CONTRACT_VERSION`](super::CONTRACT_VERSION)
/// gives at length and every enum in the contract document already follows: a consumer that folds
/// an unfamiliar value into a closed set makes one unknown token poison everything around it. Here
/// the value arrives as an annotation string rather than through `Deserialize`, and the failure
/// mode is the same shape — a validator meeting `yaml` from a newer producer should be able to say
/// "a format I do not know" about that one object, not fail to read the object at all.
///
/// `#[non_exhaustive]` says the same thing to a Rust caller and does nothing for anyone parsing the
/// annotation, which is why both are here.
///
/// Spellings are exact and are deliberately not folded: `yml` becomes [`Self::Other`] rather than
/// [`Self::Yaml`]. This crate writes the annotation and this crate reads it back, so two spellings
/// of one format would be two annotation values that compare unequal while meaning the same thing
/// — and [`Contract::verify_kube_metadata`] compares them for equality.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Format {
    /// TOML, which is every document this crate's loader reads today.
    #[default]
    Toml,
    /// YAML.
    Yaml,
    /// JSON.
    Json,
    /// A format this version does not know, carried verbatim.
    ///
    /// Not an error. A newer producer naming a parser this build has never heard of is exactly the
    /// case the fallback exists for, and a consumer that meets one should say it skipped the check
    /// rather than quietly treat the document as TOML.
    Other(String),
}

impl Format {
    /// The annotation value for this format.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::Other(other) => other,
        }
    }
}

impl From<&str> for Format {
    fn from(value: &str) -> Self {
        match value {
            "toml" => Self::Toml,
            "yaml" => Self::Yaml,
            "json" => Self::Json,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Which kind of object is being stamped.
///
/// An enum rather than two constructors returning one type with fields silently absent: the two
/// stamps differ, and a caller that does not have to say which it means is a caller who will
/// eventually stamp a pod with a document's metadata and never find out. See the module
/// documentation for the table.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The object holding the document — a `ConfigMap`, or a `Secret` for the secrets-directory
    /// layer. Carries the label and all three annotations.
    Document {
        /// Which key inside `data` is the document. See [`ANNOTATION_DOCUMENT_KEY`].
        key: String,
        /// Which parser reads it. See [`ANNOTATION_FORMAT`].
        format: Format,
    },
    /// The workload's pod template — `spec.template.metadata`. Carries the label and
    /// [`ANNOTATION_IMAGES`] only.
    Workload,
}

impl Target {
    /// A document object holding `key`, in `format`.
    #[must_use]
    pub fn document(key: impl Into<String>, format: Format) -> Self {
        Self::Document {
            key: key.into(),
            format,
        }
    }
}

/// The `metadata.labels` and `metadata.annotations` one object should carry.
///
/// Both maps are [`BTreeMap`] rather than `HashMap`, for [`Contract::to_json`]'s reason: the output
/// has to be byte-stable so that a chart's rendered manifest can be diffed in review and so that
/// [`Self::to_yaml`] produces the same block twice. A hash order that changes per process would
/// make every render a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metadata {
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

impl Metadata {
    /// The labels, keyed as Kubernetes spells them.
    #[must_use]
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// The annotations, keyed as Kubernetes spells them.
    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// The block to paste into a Helm template, indented by `indent` spaces.
    ///
    /// Mirrors [`Contract::to_dockerfile_labels`], and for the same reason: a chart's `metadata:`
    /// block cannot be interpolated from a Rust API, so hand-writing it is unavoidable and the
    /// honest answer is to make it a paste and then check the result with
    /// [`Contract::verify_kube_metadata`].
    ///
    /// `indent` is where the `labels:` and `annotations:` keys sit; entries go two spaces deeper.
    /// Under a `metadata:` at column 0 that is `to_yaml(2)`; inside a `spec.template.metadata:` it
    /// is usually `to_yaml(8)`.
    ///
    /// Values are double-quoted so that `"1"` stays a string — YAML would otherwise read it as an
    /// integer, and a Kubernetes label value must be a string. Keys are *not* quoted, which is safe
    /// rather than lucky: every key here is validated as a label key, and that character set has no
    /// overlap with YAML's indicators.
    ///
    /// Ends with a newline, and an empty map contributes nothing rather than a dangling `labels:`
    /// with no entries under it — which is a YAML null, not an empty map, and merges into a
    /// template differently.
    #[must_use]
    pub fn to_yaml(&self, indent: usize) -> String {
        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);
        let mut rendered = String::new();

        for (block, entries) in [("labels", &self.labels), ("annotations", &self.annotations)] {
            if entries.is_empty() {
                continue;
            }
            rendered.push_str(&outer);
            rendered.push_str(block);
            rendered.push_str(":\n");
            for (key, value) in entries {
                rendered.push_str(&inner);
                rendered.push_str(key);
                rendered.push_str(": \"");
                // A value carrying either of these would break the document rather than the
                // deployment, which is the worst way to find out. Neither occurs in a value this
                // module emits — every one of them is validated first — so this is the same
                // belt-and-braces `to_dockerfile_labels` applies to a prefix and a path.
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
    /// The Kubernetes metadata an object should carry, given what it is and which images read it.
    ///
    /// `images` is every image that reads the document, each digest-pinned, in declaration order.
    /// It is passed in rather than derived because a contract deliberately carries no digest: the
    /// digest is what the push *produces*, so nothing written before the push can name it.
    ///
    /// See [`Metadata::to_yaml`] for the block to paste and [`Self::verify_kube_metadata`] for the
    /// check that a rendered object actually carries it.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if `images` is empty, repeats a reference, or holds one that is
    /// not digest-pinned; if [`Target::Document`]'s key is not a legal `ConfigMap` data key; or if
    /// its format is not a bare token. Each message names the value and what it must be instead.
    pub fn kube_metadata(&self, target: &Target, images: &[&str]) -> Result<Metadata, Error> {
        let mut labels = BTreeMap::new();
        // Digits, so this is a legal label value for every `u32` there is. Checked anyway rather
        // than argued: the check costs a branch, and it is what makes "everything this function
        // emits is Kubernetes-legal" a property of the code instead of a claim in a comment.
        let version = self.terrace_contract.to_string();
        label_value(&version).map_err(|error| {
            Error::Invalid(format!(
                "this contract's version `{version}` cannot be a Kubernetes label value: {error}"
            ))
        })?;
        labels.insert(key(LABEL_CONTRACT_VERSION)?, version);

        let mut annotations = BTreeMap::new();
        annotations.insert(key(ANNOTATION_IMAGES)?, join_images(images)?);

        if let Target::Document { key, format } = target {
            configmap_key(key).map_err(|error| {
                Error::Invalid(format!(
                    "`{key}` cannot be the `{ANNOTATION_DOCUMENT_KEY}` of a ConfigMap: {error} \
                     The annotation names a key inside `data`, so a value no `data` key could \
                     have is one no validator will ever find."
                ))
            })?;
            annotations.insert(self::key(ANNOTATION_DOCUMENT_KEY)?, key.clone());

            let format = format.as_str();
            // Held to the label-value rule even though annotation values are unconstrained. The
            // vocabulary is short tokens a validator dispatches on — `toml`, `yaml`, `json` — and
            // `Format::Other` exists for a token a later version adds, not for arbitrary text. It
            // does forbid a media type here, which is deliberate: a media type is a different
            // field, and holding this one to the stricter rule is what would let it become a
            // label later without breaking anyone already reading it.
            label_value(format).map_err(|error| {
                Error::Invalid(format!(
                    "`{format}` cannot be the `{ANNOTATION_FORMAT}` of a document: {error} The \
                     value names a parser, so it is a bare token like `toml` — a media type or a \
                     file name belongs in a field of its own."
                ))
            })?;
            annotations.insert(self::key(ANNOTATION_FORMAT)?, format.to_owned());
        }

        Ok(Metadata {
            labels,
            annotations,
        })
    }

    /// Check that a rendered object carries the metadata this contract expects for `target`.
    ///
    /// `labels` and `annotations` are the object's `metadata.labels` and `metadata.annotations`.
    /// Checking the **rendered object** rather than the chart is the whole value, for
    /// [`Self::verify_labels`]'s reason: a template diff cannot see a value that failed to
    /// interpolate, a block a base template overrode, or a stamp dropped on the one branch nobody
    /// rendered.
    ///
    /// Two rules about what is ignored, and they are not the same rule:
    ///
    /// - **Foreign keys are ignored.** An object carries `app.kubernetes.io/*`, whatever the chart
    ///   adds and whatever `kubectl` last wrote. None of that is this document's business, exactly
    ///   as [`Self::verify_labels`] ignores an image's `org.opencontainers.image.*`.
    /// - **A key in this crate's own namespace that `target` does not define is refused.** A pod
    ///   carrying `dev.terrace.config/document-key` was not stamped by a stranger — the only thing
    ///   that writes that key is a chart that copied the document block onto a workload, and
    ///   ignoring it would leave a validator reading a document key off an object with no document.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first key that is missing, wrong, malformed, or
    /// present on a target that does not define it.
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
                     `{expected}`. One of the two was rendered against a different version of the \
                     protocol, so a policy selecting on this label is reading a document whose \
                     shape it was not written for. Re-render the object from the contract the \
                     image publishes."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so nothing cluster-side can \
                     select it as a participant in this protocol — a Kyverno policy matching on \
                     `Exists` will not see it at all. `Contract::kube_metadata` produces the \
                     stamp and `Metadata::to_yaml` renders it."
                )));
            }
        }

        // Shape only. `verify_kube_metadata` holds a contract and a target, neither of which knows
        // which images a chart pinned — that is `Pairing`'s question, and it needs the running
        // image to ask it.
        let images = require(annotations, ANNOTATION_IMAGES, "annotation")?;
        for image in split_images(images) {
            digest_ref(image).map_err(|error| {
                Error::Invalid(format!(
                    "the object's `{ANNOTATION_IMAGES}` names `{image}`, which {error} A pairing \
                     keyed on a tag proves nothing, because a tag can be moved after the object \
                     was rendered."
                ))
            })?;
        }

        match target {
            Target::Document { key, format } => {
                let found = require(annotations, ANNOTATION_DOCUMENT_KEY, "annotation")?;
                if found != key {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_DOCUMENT_KEY}` is `{found}`, and this stamp was \
                         built for `{key}`. A validator would read the wrong entry out of `data`, \
                         or none at all."
                    )));
                }
                let expected = format.as_str();
                let found = require(annotations, ANNOTATION_FORMAT, "annotation")?;
                if found != expected {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_FORMAT}` is `{found}`, and this stamp was built \
                         for `{expected}`. A validator would reach for the wrong parser, and a \
                         document that fails to parse is indistinguishable from one that is wrong."
                    )));
                }
            }
            Target::Workload => {
                for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
                    if annotations.contains_key(name) {
                        return Err(Error::Invalid(format!(
                            "the workload carries `{name}`, which describes a document, and a pod \
                             is not one. The only thing that writes it here is a template that \
                             copied the document block onto a pod template; stamp the workload \
                             with `Target::Workload`, which carries the label and \
                             `{ANNOTATION_IMAGES}` alone."
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

/// The check a cluster-side actor runs: one image, one document object, one configuration surface.
///
/// This is the point of the module. Everything above produces or checks *one* side; this asserts
/// that the two sides describe the same thing, which is the assertion neither side can make alone.
/// A `ConfigMap` whose stamp is internally perfect and a pod whose image labels are internally
/// perfect still deploy a mismatch if the two were rendered a release apart.
///
/// The object under test is always the **document** object — the `ConfigMap` or `Secret` that is
/// actually mounted. A workload's stamp carries no document key or format, so there would be
/// nothing for checks 4 and 5 to read; use [`Contract::verify_kube_metadata`] for a pod.
///
/// ```
/// # use std::collections::BTreeMap;
/// # use terrace_config::Terrace;
/// # use terrace_config::schema::{App, DEFAULT_PATH, Describe, Leaf, Sink};
/// # use terrace_config::schema::kube::{Format, Pairing, Target};
/// # struct Config;
/// # impl Describe for Config {
/// #     fn describe(sink: &mut Sink) {
/// #         sink.leaf(Leaf { name: "dist_dir", docs: "", ty: Some("String"), values: None,
/// #             aliases: &[], note: None, required: false, secret: false });
/// #     }
/// # }
/// # let contract = Terrace::new("PORTFOLIO_").schema::<Config>()
/// #     .into_contract(App::new("portfolio")).build()?;
/// # let image = "ghcr.io/you/portfolio@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d";
/// # let target = Target::document("config.toml", Format::Toml);
/// # let metadata = contract.kube_metadata(&target, &[image])?;
/// # let image_labels: BTreeMap<String, String> = contract.labels(DEFAULT_PATH).into_iter()
/// #     .map(|(name, value)| (name.to_owned(), value)).collect();
/// Pairing::new(&contract, DEFAULT_PATH)
///     .image(image, &image_labels)
///     .object(metadata.labels(), metadata.annotations())
///     .check()?;
/// # Ok::<(), terrace_config::Error>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Pairing<'a> {
    contract: &'a Contract,
    path: &'a str,
    image: Option<ImageSide<'a>>,
    object: Option<ObjectSide<'a>>,
}

/// A running container's digest-pinned reference, and its image labels.
type ImageSide<'a> = (&'a str, &'a BTreeMap<String, String>);

/// One object's `metadata.labels` and `metadata.annotations`, in that order.
type ObjectSide<'a> = (&'a BTreeMap<String, String>, &'a BTreeMap<String, String>);

impl<'a> Pairing<'a> {
    /// The contract under test, and the path it was embedded at in the image.
    ///
    /// `path` is what [`LABEL_PATH`](super::LABEL_PATH) should say —
    /// [`DEFAULT_PATH`](super::DEFAULT_PATH) unless a build moved it. It is here rather than read
    /// out of the image's labels because it is half of what [`Contract::verify_labels`] checks: a
    /// path taken from the labels would agree with them by construction.
    #[must_use]
    pub fn new(contract: &'a Contract, path: &'a str) -> Self {
        Self {
            contract,
            path,
            image: None,
            object: None,
        }
    }

    /// The running container's image: its digest-pinned reference, and what `crane config` or
    /// `docker inspect` reports under `config.Labels`.
    #[must_use]
    pub fn image(mut self, reference: &'a str, labels: &'a BTreeMap<String, String>) -> Self {
        self.image = Some((reference, labels));
        self
    }

    /// The mounted document object's `metadata.labels` and `metadata.annotations`.
    #[must_use]
    pub fn object(
        mut self,
        labels: &'a BTreeMap<String, String>,
        annotations: &'a BTreeMap<String, String>,
    ) -> Self {
        self.object = Some((labels, annotations));
        self
    }

    /// Assert that the image and the object describe one configuration surface.
    ///
    /// Five checks, and the order is the order a failure is cheapest to read in:
    ///
    /// 1. the object's [`LABEL_CONTRACT_VERSION`] is present and equals this contract's version;
    /// 2. the image's own three labels agree with this contract — [`Contract::verify_labels`],
    ///    called rather than reimplemented, so the two sides cannot drift into two rules;
    /// 3. the object's version equals the image's [`LABEL_VERSION`];
    /// 4. the running image's reference is a **member** of the object's [`ANNOTATION_IMAGES`] —
    ///    membership, not equality, because one document may be read by several images;
    /// 5. [`ANNOTATION_DOCUMENT_KEY`] and [`ANNOTATION_FORMAT`] are present and well-formed.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first check that fails, both sides of it, and what to
    /// do about it. Also returns it when [`Self::image`] or [`Self::object`] was never called,
    /// which is a caller that built half a question.
    pub fn check(&self) -> Result<(), Error> {
        let Some((reference, image_labels)) = self.image else {
            return Err(Error::Invalid(
                "this pairing has no image to check against. Call `Pairing::image` with the \
                 running container's digest-pinned reference and the labels `crane config` \
                 reports for it."
                    .to_owned(),
            ));
        };
        let Some((object_labels, object_annotations)) = self.object else {
            return Err(Error::Invalid(
                "this pairing has no object to check. Call `Pairing::object` with the mounted \
                 document's `metadata.labels` and `metadata.annotations`."
                    .to_owned(),
            ));
        };

        // 1. The object participates, in this contract's version of the protocol.
        let expected = self.contract.terrace_contract.to_string();
        let object_version = match object_labels.get(LABEL_CONTRACT_VERSION) {
            Some(found) if found == &expected => found,
            Some(found) => {
                return Err(Error::Invalid(format!(
                    "the object's `{LABEL_CONTRACT_VERSION}` is `{found}`, and this contract's is \
                     `{expected}`. The document was rendered against a different version of the \
                     protocol than the contract being checked, so nothing below this line is \
                     comparing like with like. Re-render the object from the contract the image \
                     publishes."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so there is nothing to \
                     pair with the image: an object that does not declare the protocol cannot be \
                     shown to satisfy it. `Contract::kube_metadata` produces the stamp."
                )));
            }
        };

        // 2. The image agrees with the contract. Reused rather than restated — a second spelling
        //    of this rule is a second thing to keep in step with the first.
        self.contract.verify_labels(self.path, image_labels)?;

        // 3. The two sides agree with each other. Implied by 1 and 2 today, because both compare
        //    against this same contract, and stated anyway: it is the assertion the other two
        //    exist to support, and it is the one that still names the skew if either of them ever
        //    stops being asked against the same document.
        let image_version = require(image_labels, LABEL_VERSION, "label")?;
        if image_version != object_version {
            return Err(Error::Invalid(format!(
                "the object's `{LABEL_CONTRACT_VERSION}` is `{object_version}` and the image's \
                 `{LABEL_VERSION}` is `{image_version}`. The document and the program that reads \
                 it were built against two versions of this protocol; whichever is older is the \
                 one to re-render."
            )));
        }

        // 4. The running image is one of the images this document was rendered for. Membership
        //    rather than equality: several binaries may read one document — see
        //    `docs/config-contract-plan.md` §4.3 — and requiring equality would refuse every union
        //    the annotation is plural for.
        digest_ref(reference).map_err(|error| {
            Error::Invalid(format!(
                "the running image is given as `{reference}`, which {error} A pairing keyed on a \
                 tag proves nothing about what is running, because a tag can be moved."
            ))
        })?;
        let listed = require(object_annotations, ANNOTATION_IMAGES, "annotation")?;
        if !split_images(listed).any(|image| image == reference) {
            return Err(Error::Invalid(format!(
                "the running image `{reference}` is not among the images this document was \
                 rendered for: `{listed}`. Either the pod is mounting a document meant for a \
                 different image, or the chart pinned a new digest without re-rendering the \
                 object — the second is the common one, and it is the failure this check exists \
                 for."
            )));
        }

        // 5. The object says enough about its document for a validator to act on it.
        let key = require(object_annotations, ANNOTATION_DOCUMENT_KEY, "annotation")?;
        configmap_key(key).map_err(|error| {
            Error::Invalid(format!(
                "the object's `{ANNOTATION_DOCUMENT_KEY}` is `{key}`, which {error} No entry in \
                 `data` can be named that, so the annotation points at nothing."
            ))
        })?;
        let format = require(object_annotations, ANNOTATION_FORMAT, "annotation")?;
        label_value(format).map_err(|error| {
            Error::Invalid(format!(
                "the object's `{ANNOTATION_FORMAT}` is `{format}`, which {error} The value names \
                 the parser to read the document with, so it is a bare token like `toml`."
            ))
        })?;

        Ok(())
    }
}

/// One of this module's own keys, checked against the rule Kubernetes applies to it.
///
/// The keys are constants, so in a released build this cannot fail. It is here because that is a
/// property worth *holding* rather than asserting in a comment: the day one of them is edited into
/// something Kubernetes will not accept, the failure lands on whoever edited it instead of on a
/// `kubectl apply` in somebody else's cluster. It is also what makes the fuzz oracle's invariant —
/// everything [`Contract::kube_metadata`] emits is Kubernetes-legal — a fact about the code rather
/// than about the four string literals it happens to hold today.
fn key(name: &'static str) -> Result<String, Error> {
    label_key(name).map_err(|error| {
        Error::Invalid(format!("`{name}` is not a key Kubernetes accepts: {error}"))
    })?;
    Ok(name.to_owned())
}

/// Fetch one key, or say which one was missing and what kind of thing it is.
fn require<'a>(
    map: &'a BTreeMap<String, String>,
    name: &str,
    kind: &str,
) -> Result<&'a str, Error> {
    map.get(name).map(String::as_str).ok_or_else(|| {
        Error::Invalid(format!(
            "no `{name}` {kind} is present, and the check that reads it cannot be skipped quietly \
             — a check nobody ran is indistinguishable from one that passed."
        ))
    })
}

/// The references in an [`ANNOTATION_IMAGES`] value, in the order they were written.
///
/// Empty segments are kept rather than filtered, so that a value with a stray or doubled separator
/// fails the reference check with a message naming it instead of silently listing one image fewer.
fn split_images(value: &str) -> impl Iterator<Item = &str> {
    value.split(IMAGE_SEPARATOR)
}

/// Validate a list of image references and join it into an [`ANNOTATION_IMAGES`] value.
fn join_images(images: &[&str]) -> Result<String, Error> {
    if images.is_empty() {
        return Err(Error::Invalid(format!(
            "no images were given, so `{ANNOTATION_IMAGES}` would be empty — a stamp saying this \
             document is read by nothing, which no pairing can ever satisfy. Name every image \
             that reads it, digest-pinned."
        )));
    }

    for (index, image) in images.iter().enumerate() {
        digest_ref(image).map_err(|error| {
            Error::Invalid(format!(
                "the image `{image}` {error} `{ANNOTATION_IMAGES}` is what a cluster-side check \
                 pairs against the running container, and a tag can be moved after this object \
                 was rendered — so an unpinned reference is a pairing that proves nothing."
            ))
        })?;
        // A repeat is not harmless the way an extra label is. The list is written by whatever
        // renders the object, and one image named twice is far more likely to be a template that
        // meant to name two than a chart deliberately saying the same thing again.
        if images[..index].contains(image) {
            return Err(Error::Invalid(format!(
                "the image `{image}` is named twice in `{ANNOTATION_IMAGES}`. Each image that \
                 reads this document is named once; a repeat is usually a template that meant to \
                 name a second image and interpolated the first."
            )));
        }
    }

    Ok(images.join(&IMAGE_SEPARATOR.to_string()))
}

// -------------------------------------------------------------------------------------------
// The character rules, written out by hand
// -------------------------------------------------------------------------------------------
//
// By hand, and not with a regex crate. Four predicates over ASCII is not what a regex engine is
// for, and `Cargo.toml` argues every dependency it has — a crate pulled in for this would be the
// first one in that manifest with no argument behind it. Written out, each rule is also a place to
// say *which* Kubernetes rule it is, which a regex literal is not.
//
// Every message is phrased to complete the sentence "`{value}` …", so a caller can wrap it in the
// context it has without the two halves disagreeing about grammar.
//
// They return a `String` rather than an `Error`, which is the one place this layer departs from the
// rest of the module. Every caller wraps the result in an `Error::Invalid` of its own — naming the
// annotation, the object and the fix, none of which a character rule knows — and an `Error` here
// would be a second `Display` inside that one, rendering as "configuration error: the image `…`
// configuration error: is not digest-pinned". A fragment composes; a formatted error does not.

/// Whether `byte` is an ASCII letter or digit.
const fn is_alphanumeric(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// A Kubernetes label value: at most 63 characters matching
/// `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?`.
///
/// The empty string is legal, which the outer `?` in that expression says and which is easy to
/// read past. It is legal here too: refusing it would be this module inventing a rule the platform
/// does not have, and the two values that must never be empty — the contract version and the format
/// token — are refused by their callers for reasons of their own.
fn label_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > MAX_NAME {
        return Err(format!(
            "is {} characters long, and a Kubernetes label value is at most {MAX_NAME}.",
            value.len()
        ));
    }

    let bytes = value.as_bytes();
    // `first` and `last` rather than indexing: a multi-byte character yields a lead or
    // continuation byte here, which is not alphanumeric, so it is refused by the same branch that
    // refuses `-` at the edge. Both are correct — a label value is ASCII.
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return Err("is not a legal Kubernetes label value.".to_owned());
    };
    if !is_alphanumeric(first) || !is_alphanumeric(last) {
        return Err(
            "must begin and end with a letter or a digit to be a Kubernetes label value; `-`, `_` \
             and `.` are permitted only between them."
                .to_owned(),
        );
    }
    for &byte in bytes {
        if !is_alphanumeric(byte) && !matches!(byte, b'-' | b'_' | b'.') {
            return Err(format!(
                "carries `{}`, and a Kubernetes label value holds only letters, digits, `-`, `_` \
                 and `.`.",
                byte.escape_ascii()
            ));
        }
    }
    Ok(())
}

/// A Kubernetes label or annotation key: an optional DNS-subdomain prefix of at most 253
/// characters, a `/`, then a name of at most 63 under the label-value character rule.
///
/// The name half is required and, unlike a value, may not be empty — `dev.terrace.config/` names
/// nothing.
fn label_key(key: &str) -> Result<(), String> {
    let (prefix, name) = match key.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, key),
    };

    if let Some(prefix) = prefix {
        if prefix.len() > MAX_PREFIX {
            return Err(format!(
                "has a prefix of {} characters, and the DNS subdomain before the `/` is at most \
                 {MAX_PREFIX}.",
                prefix.len()
            ));
        }
        dns_subdomain(prefix)?;
    }

    if name.is_empty() {
        return Err(
            "names nothing after its `/`; a Kubernetes key is a prefix and a name, and only the \
             prefix is optional."
                .to_owned(),
        );
    }
    label_value(name)
}

/// A DNS subdomain: dot-separated labels of at most 63 lowercase alphanumerics and `-`, each
/// beginning and ending alphanumeric.
fn dns_subdomain(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("has an empty prefix before its `/`, which is not a DNS subdomain.".to_owned());
    }
    for label in value.split('.') {
        if label.is_empty() {
            return Err("has an empty segment in the DNS subdomain before its `/`.".to_owned());
        }
        if label.len() > MAX_NAME {
            return Err(format!(
                "has a {}-character segment in the DNS subdomain before its `/`, and each segment \
                 is at most {MAX_NAME}.",
                label.len()
            ));
        }
        let bytes = label.as_bytes();
        let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
            return Err(
                "has a segment that is not a DNS label in the prefix before its `/`.".to_owned(),
            );
        };
        if !first.is_ascii_lowercase() && !first.is_ascii_digit()
            || !last.is_ascii_lowercase() && !last.is_ascii_digit()
        {
            return Err(
                "has a DNS segment before its `/` that does not begin and end with a lowercase \
                 letter or a digit."
                    .to_owned(),
            );
        }
        for &byte in bytes {
            if !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-' {
                return Err(format!(
                    "carries `{}` in the DNS subdomain before its `/`, which holds only lowercase \
                     letters, digits, `-` and `.`.",
                    byte.escape_ascii()
                ));
            }
        }
    }
    Ok(())
}

/// A `ConfigMap` or `Secret` data key: `[-._a-zA-Z0-9]+`, and neither `.` nor `..`.
///
/// The two excluded spellings are the reason this is not simply the character rule: `.` and `..`
/// are legal *characters* all the way through and illegal *names*, because a `ConfigMap` projects
/// its keys as file names into a volume and neither of those is a file.
fn configmap_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("is empty, and a ConfigMap data key names an entry.".to_owned());
    }
    if key.len() > MAX_DATA_KEY {
        return Err(format!(
            "is {} characters long, and a ConfigMap data key is at most {MAX_DATA_KEY}.",
            key.len()
        ));
    }
    if key == "." || key == ".." {
        return Err(format!(
            "is `{key}`, which a ConfigMap refuses as a data key: its keys are projected as file \
             names into a volume, and that is a directory entry rather than a file."
        ));
    }
    for &byte in key.as_bytes() {
        if !is_alphanumeric(byte) && !matches!(byte, b'-' | b'.' | b'_') {
            return Err(format!(
                "carries `{}`, and a ConfigMap data key holds only letters, digits, `-`, `.` and \
                 `_`.",
                byte.escape_ascii()
            ));
        }
    }
    Ok(())
}

/// A digest-pinned image reference: a name, `@sha256:`, and 64 lowercase hex digits.
///
/// The name half is checked only for what would break the annotation or hide a pin — a separator,
/// whitespace, a control character, emptiness. Reference grammar is the registry's business and
/// restating it here would be this module holding an opinion about names it never resolves; what it
/// does hold an opinion about is that the reference names *one* build and can survive a
/// comma-separated list.
fn digest_ref(reference: &str) -> Result<(), String> {
    if reference.is_empty() {
        return Err("is empty, so it names no image.".to_owned());
    }
    if reference.contains(IMAGE_SEPARATOR) {
        return Err(format!(
            "carries a `{IMAGE_SEPARATOR}`, which is what separates one reference from the next \
             in this annotation — a reference containing one could never be found in the list \
             again."
        ));
    }
    if let Some(byte) = reference
        .bytes()
        .find(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(format!(
            "carries `{}`, and an image reference holds no whitespace or control characters.",
            byte.escape_ascii()
        ));
    }

    let Some((name, digest)) = reference.split_once('@') else {
        return Err(
            "is not digest-pinned: it carries no `@sha256:…`. A tag can be moved after an object \
             is rendered, so only a digest names the build that is actually running."
                .to_owned(),
        );
    };
    if name.is_empty() {
        return Err("has no name before its `@`, so it pins a digest to nothing.".to_owned());
    }

    let Some(hex) = digest.strip_prefix(DIGEST_MARKER) else {
        return Err(format!(
            "pins `{digest}`, and this protocol reads `{DIGEST_MARKER}` digests. An algorithm \
             nothing here can verify is not a pin."
        ));
    };
    if hex.len() != DIGEST_HEX {
        return Err(format!(
            "pins a digest of {} hex digits, and a SHA-256 digest is {DIGEST_HEX}.",
            hex.len()
        ));
    }
    if let Some(byte) = hex
        .bytes()
        .find(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(byte))
    {
        return Err(format!(
            "pins a digest carrying `{}`, and a hex digest is `0`–`9` and `a`–`f`. An upper-case \
             digest is a different string to every registry that compares them.",
            byte.escape_ascii()
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::{
        ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, LABEL_CONTRACT_VERSION,
        MAX_NAME, configmap_key, digest_ref, label_key, label_value,
    };

    /// A digest-pinned reference, spelled once.
    const PINNED: &str = "ghcr.io/you/portfolio@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d";

    #[test]
    fn the_keys_this_module_emits_are_legal_kubernetes_keys() {
        for key in [
            LABEL_CONTRACT_VERSION,
            ANNOTATION_IMAGES,
            ANNOTATION_DOCUMENT_KEY,
            ANNOTATION_FORMAT,
        ] {
            label_key(key).unwrap_or_else(|error| panic!("`{key}` {error}"));
        }
    }

    #[test]
    fn a_label_value_begins_and_ends_alphanumeric() {
        for legal in ["1", "toml", "a", "a.b_c-d", "0", &"a".repeat(MAX_NAME), ""] {
            assert!(label_value(legal).is_ok(), "`{legal}` was refused");
        }
        // The three the image side publishes, every one of them illegal here — which is the whole
        // reason this module exists rather than reusing `Contract::labels`.
        for illegal in [
            "PORTFOLIO_",
            "/config/contract.json",
            "application/vnd.terrace.config-schema.v1+json",
            ".leading",
            "trailing-",
            "has space",
            &"a".repeat(MAX_NAME + 1),
            "ünïcode",
        ] {
            assert!(label_value(illegal).is_err(), "`{illegal}` was accepted");
        }
    }

    #[test]
    fn a_label_key_takes_a_dns_prefix_and_a_name() {
        for legal in ["dev.terrace.config/format", "format", "a1.b2/c-d_e.f"] {
            assert!(label_key(legal).is_ok(), "`{legal}` was refused");
        }
        for illegal in [
            "dev.terrace.config/",
            "/format",
            "DEV.TERRACE.CONFIG/format",
            "dev..config/format",
            "a/b/c",
            "dev.terrace.config/-format",
        ] {
            assert!(label_key(illegal).is_err(), "`{illegal}` was accepted");
        }
    }

    #[test]
    fn a_configmap_key_is_a_file_name_and_not_a_directory_entry() {
        for legal in ["config.toml", "a", "_x-y.z", "00-base.toml"] {
            assert!(configmap_key(legal).is_ok(), "`{legal}` was refused");
        }
        // `.` and `..` carry only legal characters and are still refused, which is why the rule is
        // not the character class alone.
        for illegal in ["", ".", "..", "a/b", "a b", "a:b"] {
            assert!(configmap_key(illegal).is_err(), "`{illegal}` was accepted");
        }
    }

    #[test]
    fn only_a_digest_pins_an_image() {
        assert!(digest_ref(PINNED).is_ok());
        assert!(digest_ref("portfolio@sha256:{}").is_err());

        for illegal in [
            "",
            "ghcr.io/you/portfolio",
            "ghcr.io/you/portfolio:v2.5.0",
            // The digest is there and the algorithm is not one this protocol reads.
            "ghcr.io/you/portfolio@sha512:48e259cb",
            // Right algorithm, wrong length.
            "ghcr.io/you/portfolio@sha256:48e259cb",
            // Right length, upper case — a different string to every registry that compares them.
            "ghcr.io/you/portfolio@sha256:48E259CB4D7C1F0A2B3E5D6C7A8B9E0F1A2B3C4D5E6F7A8B9C0D1E2F3A4B5C6D",
            // No name before the `@`.
            "@sha256:48e259cb4d7c1f0a2b3e5d6c7a8b9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d",
        ] {
            assert!(digest_ref(illegal).is_err(), "`{illegal}` was accepted");
        }
    }

    #[test]
    fn a_reference_that_would_break_the_list_is_refused() {
        // The separator, which would make the reference unfindable in the annotation it is about
        // to be joined into.
        let comma = format!("{PINNED},{PINNED}");
        assert!(digest_ref(&comma).is_err());
        assert!(digest_ref(" ghcr.io/you/p@sha256:x").is_err());
        assert!(digest_ref("ghcr.io/you\n/p@sha256:x").is_err());
    }
}
