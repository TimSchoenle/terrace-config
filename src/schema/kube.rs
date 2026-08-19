//! The cluster's half of the protocol: what a rendered Kubernetes object carries, and the check
//! that pairs it with the image's half.
//!
//! [`Contract::labels`] makes a contract discoverable from an *image*. That is enough for a
//! pipeline holding a digest and it is not enough for anything inside a cluster: a Kyverno
//! policy, a validating admission webhook or an init container looking at a live `ConfigMap`
//! holds a document and no way to tell that the image about to mount it is the image the
//! document was rendered for. The two are produced by repositories that never see each other,
//! and nothing in either one says they belong together.
//!
//! This module names the metadata that says so, and [`Pairing`] is the check that reads both
//! halves and refuses a mismatched pair.
//!
//! # Labels are what you select on. Annotations are what you read.
//!
//! That sentence decides every placement below, and it is forced by the platform rather than
//! chosen. A Kubernetes **label value** is at most 63 bytes and, unless empty, must match
//! `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?` — begins and ends alphanumeric, with `-`, `_`
//! and `.` between. Three of the values this crate already publishes are illegal under it:
//!
//! | Value | Why it cannot be a label value |
//! |---|---|
//! | `PORTFOLIO_`, a dialect prefix | trailing `_` |
//! | `/config/contract.json`, [`DEFAULT_PATH`] | `/` is not in the character class |
//! | `application/vnd.terrace.config-schema.v1+json`, [`ARTIFACT_TYPE`](super::ARTIFACT_TYPE) | `/` and `+` |
//!
//! A label **key** is looser and still bounded: an optional DNS-subdomain prefix of at most 253
//! bytes, a `/`, then a name of at most 63 bytes under the value rule. `dev.terrace.config` is a
//! legal prefix, which is why every key here can share it.
//!
//! Annotation keys follow that same key rule. Annotation **values are unconstrained** — the
//! whole `metadata.annotations` map is bounded at 256 KiB and nothing else is said about them.
//!
//! So a fact that must be *selected on* has to survive the value rule, and a fact that is merely
//! *read* does not. Everything below follows from that, and whoever next reaches for a
//! `dev.terrace.config/prefix` label will otherwise discover the rule at `kubectl apply` time,
//! on a value that is correct in every other sense.
//!
//! # The two targets are not stamped identically
//!
//! [`Target::Document`] is the `ConfigMap` — or the `Secret`, for the keys the
//! secrets-directory layer carries — and gets the label and all three annotations.
//! [`Target::Workload`] is the pod template, and gets the label and the image list alone:
//! [`ANNOTATION_DOCUMENT_KEY`] and [`ANNOTATION_FORMAT`] are properties of a document, and a pod
//! is not one.
//!
//! A pod carries the stamp at all because an admission webhook usually sees *only* the pod. With
//! the image list on it, the webhook can tell that the pod is this protocol's business and find
//! the images to check without walking ownership references back to whatever rendered it.
//!
//! # What this module does not do
//!
//! It renders no manifests. A chart is what stamps an object, and this crate never sees the
//! chart — so what is here is the names, the rules a value has to satisfy, the block to paste,
//! and the verification. [`Metadata::to_yaml`] exists for the same reason
//! [`Contract::to_dockerfile_labels`] does: hand-writing is unavoidable, so the honest answer is
//! to make it a copy-paste and then check the result.
//!
//! ```
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
//! let target = Target::document("config.toml", Format::Toml);
//! let metadata = contract.kube_metadata(
//!     &target,
//!     &["ghcr.io/you/portfolio@sha256:48e2c1e7a4c0d4e6b2f8a1c3d5e7f9a1b3c5d7e9f1a3c5d7e9f1a3c5d7e9f1a3"],
//! )?;
//!
//! // The block a Helm template pastes under `metadata:`.
//! print!("{}", metadata.to_yaml(2));
//! # Ok::<(), terrace_config::Error>(())
//! ```

use std::collections::BTreeMap;
use std::fmt;

use super::{Contract, DEFAULT_PATH, Error, LABEL_VERSION};

/// The key prefix every name in this module shares.
///
/// A DNS subdomain, which is what a key's prefix must be, and the same namespace the image
/// labels spell with dots instead of a `/`. The two spellings are not a choice either: an image
/// label is one flat string and a Kubernetes key is a prefix and a name, so
/// `dev.terrace.config.contract.version` and `dev.terrace.config/contract-version` are one fact
/// in the two shapes their platforms allow.
pub const NAMESPACE: &str = "dev.terrace.config";

/// The label carrying [`CONTRACT_VERSION`](super::CONTRACT_VERSION).
///
/// The entire label surface, and the value is the version stringified — digits, so always legal.
///
/// It answers the one question a cluster-side actor asks with a *selector*: does this object
/// participate in the protocol, and in which version of it. A policy matches it with
/// `matchExpressions: [{ key: dev.terrace.config/contract-version, operator: Exists }]` and
/// never has to enumerate the objects it governs — which is what makes the policy survive a
/// chart adding a second `ConfigMap`.
pub const LABEL_CONTRACT_VERSION: &str = "dev.terrace.config/contract-version";

/// The annotation listing every image that reads this document: comma-separated, each
/// digest-pinned, in declaration order.
///
/// Digest-pinned is a requirement rather than a preference, and it is the reasoning that already
/// keeps a contract attached to a digest rather than to a tag. A tag can be moved after the
/// object was stamped, so a pairing keyed on one proves nothing about the image that actually
/// runs; `digest_ref` is where an unpinned reference is refused.
///
/// The crate cannot supply this value. A contract deliberately carries no digest — see the note
/// on [`App`](super::App) — because a digest is what building the image *produces*, so whatever
/// renders the object passes it in, and this crate's job is to name the annotation, check the
/// value, and refuse a reference that cannot bear the weight put on it.
pub const ANNOTATION_IMAGES: &str = "dev.terrace.config/images";

/// The annotation naming which key inside `data` is the configuration document.
///
/// A `ConfigMap` may carry several files — a document, a logging profile, a template fragment —
/// and without this a validator picks one by guessing. Must be a key the object can actually
/// hold: letters, digits, `-`, `_` and `.`, and neither `.` nor `..`.
pub const ANNOTATION_DOCUMENT_KEY: &str = "dev.terrace.config/document-key";

/// The annotation naming how the document is spelled — `toml` today.
///
/// It exists because a YAML or JSON document normalises to the same tree and leaves every gate
/// unchanged, which makes the format the one thing a validator cannot derive from the contract:
/// it has to know which parser to reach for before it has a tree to check at all.
pub const ANNOTATION_FORMAT: &str = "dev.terrace.config/format";

// There is deliberately no `dev.terrace.config/prefix` and no `dev.terrace.config/contract-path`
// here, and the reasoning is worth keeping because both read as obvious companions to the
// version.
//
// They are facts about **an image**, and the image already carries them —
// `dev.terrace.config.prefix` and `dev.terrace.config.contract.path`, in a config blob that is
// one request away for anything that can already name the image. Copying them onto a Kubernetes
// object creates a second spelling of a fact that already has one, and a second spelling is a
// place to drift: which is exactly what [`Contract::verify_labels`] exists to catch on the image
// side. Here there would be nothing to catch it. The object is rendered by a chart this crate
// never sees, so no build-time check can compare the two, and the copy would be believed
// precisely when it has gone stale.
//
// Neither could be a label value in any case — `PORTFOLIO_` ends in an underscore and
// `/config/contract.json` carries slashes — so the only shape either could take here is an
// annotation, which is not selectable and therefore buys nothing over the image label it would
// be duplicating.

// There is deliberately no `app` label either, and that one is refused by the design rather than
// by the character rule.
//
// A document may be read by *several* images — the union case, where one `ConfigMap` is mounted
// by a web binary and a worker binary built from the same tree — so the honest field is a list.
// A label value cannot hold one: every plausible separator (`,`, `/`, a space) is outside the
// character class, and a label that could hold only the single-image case is a label that
// silently stops being true the moment a second image is added. Anything per-image or
// multi-valued is an annotation, and that rule has no exceptions here — it is the whole reason
// [`ANNOTATION_IMAGES`] is one.

/// How the configuration document is spelled.
///
/// Carries a fallback variant for the reason [`CONTRACT_VERSION`](super::CONTRACT_VERSION)
/// spells out at length: a consumer reading this into a closed enum makes one unfamiliar value
/// poison the whole object, and `#[non_exhaustive]` says that to a Rust *caller* while doing
/// nothing at all for anything parsing an annotation string.
///
/// The canonical spellings are lower case and matching is exact, so `TOML` reads back as
/// [`Self::Other`] rather than as [`Self::Toml`]. That is not pedantry: a consumer that guessed
/// at the case would be guessing at the parser, and the obligation this protocol puts on a
/// consumer runs the other way — **say that a form was not recognised rather than skipping the
/// check quietly**, because a silently skipped check is indistinguishable from a passing one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Format {
    /// The document is TOML. The only format the loader reads today.
    #[default]
    Toml,
    /// The document is YAML.
    Yaml,
    /// The document is JSON.
    Json,
    /// A spelling this version of the crate does not know.
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which kind of object is being stamped.
///
/// An enum rather than two constructors returning the same type with fields silently absent. A
/// pod template carrying a document key is a chart that pasted the `ConfigMap`'s block into the
/// wrong template, and a shape that leaves the two indistinguishable is the shape that lets that
/// through — see [`Contract::verify_kube_metadata`], which refuses it by name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The object holding the document: a `ConfigMap`, or a `Secret` for the keys the
    /// secrets-directory layer carries. Gets the label and all three annotations.
    Document {
        /// The key inside `data` that is the configuration document.
        key: String,
        /// How that document is spelled.
        format: Format,
    },
    /// The workload's pod template, at `spec.template.metadata`. Gets the label and the image
    /// list alone.
    Workload,
}

impl Target {
    /// The object holding the document.
    #[must_use]
    pub fn document(key: impl Into<String>, format: Format) -> Self {
        Self::Document {
            key: key.into(),
            format,
        }
    }

    /// The workload's pod template.
    #[must_use]
    pub fn workload() -> Self {
        Self::Workload
    }
}

/// The labels and annotations one object carries. Built by [`Contract::kube_metadata`].
///
/// [`BTreeMap`], never a hash map, for the reason every other rendering in this crate is
/// ordered: the output is meant to be pasted, committed and diffed, and a map that iterates in a
/// seeded order makes a diff out of two identical stamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

impl Metadata {
    /// What belongs under `metadata.labels`.
    #[must_use]
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// What belongs under `metadata.annotations`.
    ///
    /// Kubernetes bounds the whole map at 256 KiB, and this crate cannot see the rest of it: a
    /// long enough image list is refused by the API server rather than here.
    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// The block to paste into a Helm template, indented by `indent` spaces.
    ///
    /// `indent` is where the two mapping keys land, so `to_yaml(2)` drops straight under a
    /// `metadata:` at the left margin and `to_yaml(8)` under a pod template's. Entries sit two
    /// spaces further in.
    ///
    /// Deterministic: the same metadata produces the same bytes on every call, because both maps
    /// are ordered.
    ///
    /// **Values are quoted and keys are not**, which is not a matter of taste. A label value is
    /// a string and this one's is `1`; left bare, YAML reads it as an integer and the API server
    /// refuses the object with a type error naming neither this protocol nor the chart line that
    /// produced it. The keys are constants of this crate and carry nothing YAML gives meaning
    /// to, so quoting them would only make the block read as foreign in a template where every
    /// neighbouring key is bare.
    ///
    /// Ends with a newline, so whatever follows needs no separator.
    #[must_use]
    pub fn to_yaml(&self, indent: usize) -> String {
        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);
        let mut rendered = String::new();

        for (heading, entries) in [("labels", &self.labels), ("annotations", &self.annotations)] {
            rendered.push_str(&outer);
            rendered.push_str(heading);
            rendered.push_str(":\n");
            for (key, value) in entries {
                rendered.push_str(&inner);
                rendered.push_str(key);
                rendered.push_str(": \"");
                // Neither can occur in anything this module emits — every value is checked
                // against a tighter rule first — but a value that broke the block rather than
                // the deployment would be the worst way to find that out.
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
    /// The Kubernetes metadata an object carrying this contract's document should be stamped
    /// with.
    ///
    /// `images` is every image that reads the document, digest-pinned, in the order they should
    /// be listed. It is passed in rather than derived because a contract deliberately names no
    /// image at all: see [`ANNOTATION_IMAGES`].
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if the stamp would not be one the API server accepts, or would
    /// not be one a validator could act on:
    ///
    /// - `images` is empty, which stamps an object that nothing claims to read;
    /// - an image reference is not digest-pinned, or is not a reference this annotation can
    ///   carry;
    /// - the document key is not one a `ConfigMap` can hold under `data`;
    /// - the format is empty or carries whitespace, so nothing could dispatch a parser on it.
    pub fn kube_metadata(&self, target: &Target, images: &[&str]) -> Result<Metadata, Error> {
        let mut labels = BTreeMap::new();
        let mut annotations = BTreeMap::new();

        put_label(
            &mut labels,
            LABEL_CONTRACT_VERSION,
            self.terrace_contract.to_string(),
        )?;

        if images.is_empty() {
            return Err(Error::Invalid(format!(
                "no image was named for `{ANNOTATION_IMAGES}`, so this object would say that \
                 nothing reads it. The annotation exists so that a validator holding a running \
                 pod can decide whether this document is that pod's configuration, and with no \
                 members it can decide nothing. Name every image that reads the document."
            )));
        }
        for image in images {
            digest_ref(image)?;
        }
        put_annotation(&mut annotations, ANNOTATION_IMAGES, images.join(","))?;

        if let Target::Document { key, format } = target {
            configmap_key(key)?;
            put_annotation(&mut annotations, ANNOTATION_DOCUMENT_KEY, key.clone())?;

            format_token(format.as_str())?;
            put_annotation(
                &mut annotations,
                ANNOTATION_FORMAT,
                format.as_str().to_owned(),
            )?;
        }

        Ok(Metadata {
            labels,
            annotations,
        })
    }

    /// Check that a rendered object carries the stamp this contract and target call for.
    ///
    /// The producer's check, and the counterpart of [`Self::verify_labels`]: run it in the chart
    /// repository's CI over the manifests the chart actually rendered, where a failure costs a
    /// re-render instead of a rejected deployment. [`Pairing`] is the consumer's check and asks
    /// a different question — see there.
    ///
    /// Extra labels and annotations are ignored, exactly as [`Self::verify_labels`] ignores an
    /// image's `org.opencontainers.image.*`. Every object in a real chart carries
    /// `app.kubernetes.io/*`, a checksum annotation and whatever the release tooling adds, and
    /// none of it is this document's business.
    ///
    /// What is *not* ignored is one of this protocol's own annotations on a [`Target::Workload`].
    /// That is not a stranger's key: it is a claim from this protocol on an object that cannot
    /// support it, which means the `ConfigMap`'s template block was pasted into the pod
    /// template — and the image list beside it is then the document's rather than the workload's,
    /// which nothing downstream can tell.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first key that is missing, wrong, or present where
    /// this target cannot carry it.
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
                    "the object's `{LABEL_CONTRACT_VERSION}` is `{found}`, and this contract's \
                     is `{expected}`. One of the two was written against a different version of \
                     this protocol, and a policy selecting on the label would either govern this \
                     object under the wrong rules or never find it at all."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so nothing selecting on \
                     that label will ever see it — and a policy that governs this document then \
                     passes every deployment of it by never matching. \
                     `Contract::kube_metadata` emits the block."
                )));
            }
        }

        // Read for its refusals rather than for the list it returns: a member that is not
        // digest-pinned is a hole in the protocol for whoever runs *that* image, whichever image
        // this call happens to be about.
        image_list(annotations)?;

        match target {
            Target::Document { key, format } => {
                let found = annotation(annotations, ANNOTATION_DOCUMENT_KEY)?;
                configmap_key(found)?;
                if found != key {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_DOCUMENT_KEY}` is `{found}` and this stamp \
                         was rendered for `{key}`. A validator reads the document at the key the \
                         annotation names, so the two disagreeing means it checks a file nobody \
                         mounts, or the pod mounts a file nobody checked."
                    )));
                }

                let found = annotation(annotations, ANNOTATION_FORMAT)?;
                format_token(found)?;
                if found != format.as_str() {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_FORMAT}` is `{found}` and this stamp was \
                         rendered for `{format}`. The format decides which parser reads the \
                         document, and a validator reaching for the wrong one reports the \
                         document as malformed rather than reporting the annotation."
                    )));
                }
            }
            Target::Workload => {
                for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
                    if annotations.contains_key(name) {
                        return Err(Error::Invalid(format!(
                            "this pod template carries `{name}`, which describes a document — \
                             and a pod is not one. The block belongs on the object holding the \
                             document, so a pod carrying it is a chart that pasted the wrong \
                             template block, and the `{ANNOTATION_IMAGES}` beside it is then the \
                             document's list rather than this workload's."
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

/// The check a cluster-side actor runs: do the image and the document describe one configuration
/// surface?
///
/// This is the point of the module. Everything above names things; this reads both halves and
/// refuses a pair that does not belong together, which is the admission webhook's question asked
/// once, in the one place that holds the contract.
///
/// The five inputs are all required and there is no way to build a [`Pairing`] without them.
/// A pairing missing a half is not a pairing, and a check that quietly skipped the image side
/// would pass for every deployment while proving nothing. The one thing with a default is where
/// the contract was embedded, because [`DEFAULT_PATH`] is a real default rather than an
/// assumption — see [`Self::embedded_at`].
///
/// # What it checks
///
/// 1. the object's [`LABEL_CONTRACT_VERSION`] is present and equals this contract's version;
/// 2. the image's own three labels agree with this contract — [`Contract::verify_labels`] run
///    verbatim rather than reimplemented, so the two sides cannot drift into two spellings of
///    one rule;
/// 3. the object's version equals the image's [`LABEL_VERSION`];
/// 4. the running image is a **member** of the object's [`ANNOTATION_IMAGES`] — membership
///    rather than equality, because one document may be read by several images;
/// 5. [`ANNOTATION_DOCUMENT_KEY`] and [`ANNOTATION_FORMAT`] are present and well-formed.
///
/// Check 3 reads as redundant beside 1 and 2 and is not. Those two compare each side against the
/// *contract in hand*, and the contract in hand is whichever one the caller fetched; 3 compares
/// the two sides against each other, and is what catches a caller that fetched the contract for
/// a different image than the one running.
///
/// # It pairs a document, not a pod
///
/// `labels` and `annotations` are the mounted **document** object's, because check 5 is about a
/// document and a pod template carries neither annotation. A webhook that sees only the pod
/// reads [`ANNOTATION_IMAGES`] off it to learn that the pod is this protocol's business at all,
/// then fetches the `ConfigMap` it mounts and runs this.
#[derive(Debug, Clone, Copy)]
pub struct Pairing<'a> {
    contract: &'a Contract,
    image: &'a str,
    image_labels: &'a BTreeMap<String, String>,
    labels: &'a BTreeMap<String, String>,
    annotations: &'a BTreeMap<String, String>,
    path: &'a str,
}

impl<'a> Pairing<'a> {
    /// The contract, the running container's digest-pinned image and its `config.Labels`, and
    /// the mounted document object's `metadata.labels` and `metadata.annotations`.
    ///
    /// `image_labels` is what `crane config` or `docker inspect` reports under `config.Labels`,
    /// unfiltered.
    #[must_use]
    pub fn new(
        contract: &'a Contract,
        image: &'a str,
        image_labels: &'a BTreeMap<String, String>,
        labels: &'a BTreeMap<String, String>,
        annotations: &'a BTreeMap<String, String>,
    ) -> Self {
        Self {
            contract,
            image,
            image_labels,
            labels,
            annotations,
            path: DEFAULT_PATH,
        }
    }

    /// Where the contract is embedded in the image's filesystem. Defaults to [`DEFAULT_PATH`].
    ///
    /// Only the image's [`LABEL_PATH`](super::LABEL_PATH) is checked against it — nothing here
    /// reads the file — so this is what a build that embedded the document elsewhere passes, and
    /// passing nothing is right for every build that took the default.
    #[must_use]
    pub fn embedded_at(mut self, path: &'a str) -> Self {
        self.path = path;
        self
    }

    /// Run the five checks.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first disagreement, both sides of it, and what to
    /// do about it.
    pub fn check(&self) -> Result<(), Error> {
        // 1. The object participates, and in this version of the protocol.
        let object_version = self.labels.get(LABEL_CONTRACT_VERSION).ok_or_else(|| {
            Error::Invalid(format!(
                "the mounted object carries no `{LABEL_CONTRACT_VERSION}`, so it does not claim \
                 to be a document any contract describes. Either the chart never stamped it — \
                 `Contract::kube_metadata` emits the block — or this pod is mounting a \
                 `ConfigMap` that belongs to something else."
            ))
        })?;
        let expected = self.contract.terrace_contract.to_string();
        if object_version != &expected {
            return Err(Error::Invalid(format!(
                "the mounted object's `{LABEL_CONTRACT_VERSION}` is `{object_version}` and this \
                 contract's is `{expected}`. The two were written against different versions of \
                 this protocol, and the older side cannot be assumed to mean what the newer one \
                 says: refuse the pair rather than checking it under rules only one of them \
                 agreed to."
            )));
        }

        // 2. The image agrees with the contract. `verify_labels` verbatim: the image half of
        //    this protocol has one implementation, and a second one here would be a second thing
        //    to keep in step with the labels a build actually writes.
        self.contract.verify_labels(self.path, self.image_labels)?;

        // 3. The two sides agree with each other, which neither 1 nor 2 can establish: both of
        //    those compare against the contract in hand, and the contract in hand is whatever
        //    the caller managed to fetch.
        let image_version = self.image_labels.get(LABEL_VERSION).ok_or_else(|| {
            Error::Invalid(format!(
                "the running image carries no `{LABEL_VERSION}`, so nothing ties it to a config \
                 contract at all. An image in this protocol carries the label; one that does not \
                 was either not built by this toolchain or lost the label to a base-image \
                 override."
            ))
        })?;
        if image_version != object_version {
            return Err(Error::Invalid(format!(
                "the mounted object's `{LABEL_CONTRACT_VERSION}` is `{object_version}` and the \
                 running image's `{LABEL_VERSION}` is `{image_version}`. The document and the \
                 image were produced against different versions of this protocol, so the chart \
                 and the build are out of step: re-render the chart against the contract the \
                 image publishes, or roll the image forward."
            )));
        }

        // 4. The running image is one of the images the document says read it.
        digest_ref(self.image)?;
        let images = image_list(self.annotations)?;
        if !images.contains(&self.image) {
            return Err(Error::Invalid(format!(
                "the running image is `{}`, and the mounted object's `{ANNOTATION_IMAGES}` \
                 lists `{}`. This pod is mounting a document rendered for other images, so \
                 nothing has checked its configuration against what this binary reads. Add the \
                 image to the annotation if it does read this document, and mount the right \
                 object if it does not.",
                self.image,
                images.join(", ")
            )));
        }

        // 5. The document is one a validator can go and read.
        let key = annotation(self.annotations, ANNOTATION_DOCUMENT_KEY)?;
        configmap_key(key)?;
        let format = annotation(self.annotations, ANNOTATION_FORMAT)?;
        format_token(format)?;

        Ok(())
    }
}

/// Put one label in, checking both halves against the rules the API server applies.
///
/// Every key this module emits is a constant, so the key check can only ever fire on a constant
/// somebody edited — which is exactly when it is worth firing, and it costs less than a test
/// that has to remember to enumerate them.
fn put_label(map: &mut BTreeMap<String, String>, key: &str, value: String) -> Result<(), Error> {
    label_key(key)?;
    label_value(&value)?;
    map.insert(key.to_owned(), value);
    Ok(())
}

/// Put one annotation in, checking the key.
///
/// The value is deliberately unchecked here. Annotation values are unconstrained, and what makes
/// each of these three usable is a rule of its own — `digest_ref`, `configmap_key`,
/// `format_token` — applied by the caller, which is the only place that knows which value it is
/// holding.
fn put_annotation(
    map: &mut BTreeMap<String, String>,
    key: &str,
    value: String,
) -> Result<(), Error> {
    label_key(key)?;
    map.insert(key.to_owned(), value);
    Ok(())
}

/// One annotation, or a refusal naming what is missing.
fn annotation<'a>(annotations: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, Error> {
    annotations.get(name).map(String::as_str).ok_or_else(|| {
        Error::Invalid(format!(
            "the object carries no `{name}`. Every annotation in this protocol exists because a \
             validator would otherwise have to guess the answer, and a guess that happens to be \
             right is the failure worth avoiding. `Contract::kube_metadata` emits the block."
        ))
    })
}

/// The members of [`ANNOTATION_IMAGES`], each checked.
///
/// Members are trimmed before they are read. What this crate emits carries no spaces, and a
/// hand-written or templated list separated with `, ` is common enough that refusing it would
/// fail deployments over whitespace rather than over a pairing — while a *reference* still
/// carrying whitespace after the trim is refused by `digest_ref` like any other malformed one.
fn image_list(annotations: &BTreeMap<String, String>) -> Result<Vec<&str>, Error> {
    let raw = annotation(annotations, ANNOTATION_IMAGES)?;
    let images: Vec<&str> = raw.split(',').map(str::trim).collect();

    if images.iter().all(|image| image.is_empty()) {
        return Err(Error::Invalid(format!(
            "the object's `{ANNOTATION_IMAGES}` names no image, so it says that nothing reads \
             this document — and a validator holding a running pod could then decide nothing \
             about it. Name every image that reads the document."
        )));
    }
    for image in &images {
        digest_ref(image)?;
    }
    Ok(images)
}

/// Whether `value` is a key the API server accepts for a label or an annotation.
///
/// An optional DNS-subdomain prefix of at most 253 bytes, a `/`, then a name of at most 63 bytes
/// under `label_value`'s rule — except that a name, unlike a value, may not be empty.
///
/// Written out by hand rather than as a regular expression, deliberately: four predicates that
/// are each a character class and a length would be the first dependency in this crate's
/// manifest that nothing else in it argues for.
///
/// # Errors
/// Returns [`Error::Invalid`] naming which half of the key is wrong and what the rule is.
fn label_key(value: &str) -> Result<(), Error> {
    let (prefix, name) = match value.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, value),
    };

    if let Some(prefix) = prefix
        && (prefix.is_empty() || prefix.len() > 253 || !is_dns_subdomain(prefix))
    {
        return Err(Error::Invalid(format!(
            "`{value}` is not a key Kubernetes accepts: the part before the `/` is a DNS \
             subdomain of at most 253 characters — lower-case alphanumeric labels separated by \
             dots, each beginning and ending alphanumeric — and `{prefix}` is not one."
        )));
    }
    if name.contains('/') {
        return Err(Error::Invalid(format!(
            "`{value}` carries more than one `/`, and a key is at most a prefix, a `/` and a \
             name."
        )));
    }
    if name.is_empty() {
        return Err(Error::Invalid(format!(
            "`{value}` has no name after its prefix, and the name is the half that says what the \
             key means."
        )));
    }
    label_value(name).map_err(|_| {
        Error::Invalid(format!(
            "`{value}` is not a key Kubernetes accepts: the part after the `/` is at most 63 \
             characters, begins and ends alphanumeric, and carries only `-`, `_` and `.` between."
        ))
    })
}

/// Whether `value` is a value the API server accepts for a **label**.
///
/// At most 63 bytes and, unless empty, `(([A-Za-z0-9][-A-Za-z0-9_.]*)?[A-Za-z0-9])?`. The bound
/// is a byte count because that is what the API server counts, and every character the class
/// admits is one byte — so a value over the limit in bytes and under it in characters is already
/// refused for carrying the multi-byte character.
///
/// This is the rule the whole module is arranged around; the module documentation lists the
/// three values this crate publishes that fail it.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the value and the rule.
fn label_value(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 63 {
        return Err(Error::Invalid(format!(
            "`{value}` is {} bytes, and a Kubernetes label value is at most 63.",
            value.len()
        )));
    }

    let legal = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    let bounded = value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());

    if legal && bounded {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "`{value}` is not a Kubernetes label value: a value begins and ends with a letter or \
             a digit and carries only `-`, `_` and `.` between them. A value that has to hold \
             anything else — a path, a media type, a prefix ending in `_` — belongs in an \
             annotation, which is unconstrained and gives up being selectable in exchange."
        )))
    }
}

/// Whether `value` is a key a `ConfigMap` or `Secret` can hold under `data`.
///
/// `[-._a-zA-Z0-9]+`, at most 253 bytes, and neither `.` nor `..` — the two that name a
/// directory entry rather than a file, and so collide with the mount point itself.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the key and the rule.
fn configmap_key(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::Invalid(format!(
            "an empty `{ANNOTATION_DOCUMENT_KEY}` names no entry in the object's `data`, so a \
             validator has nothing to read. Name the key the document is under."
        )));
    }
    if value == "." || value == ".." {
        return Err(Error::Invalid(format!(
            "`{value}` cannot be a key in a `ConfigMap`'s `data`: it names a directory entry \
             rather than a file, and the directory is the volume the document is mounted \
             through."
        )));
    }
    if value.len() > 253 {
        return Err(Error::Invalid(format!(
            "`{value}` is {} bytes, and a key in a `ConfigMap`'s `data` is at most 253.",
            value.len()
        )));
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "`{value}` cannot be a key in a `ConfigMap`'s `data`: a key carries only letters, \
             digits, `-`, `_` and `.`. One with a `/` in it reads as a path and is refused by \
             the API server rather than projected as a nested file."
        )))
    }
}

/// Whether `value` names one exact image and cannot be made to name another.
///
/// The digest is what makes the pairing mean anything. A reference by tag names whatever the tag
/// points at *now*, so an object stamped with one carries a claim that can be made false after
/// it was written, by somebody who never touched the chart — and the pairing would still pass,
/// against a binary reading different keys.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the reference and what a digest-pinned one looks like.
fn digest_ref(value: &str) -> Result<(), Error> {
    let Some((name, digest)) = value.split_once('@') else {
        return Err(Error::Invalid(format!(
            "`{value}` is not digest-pinned, so pairing a document with it proves nothing: a tag \
             can be moved after the object was stamped, and the image that then runs is one this \
             contract was never checked against. Pin it as `name@sha256:<64 hex digits>`, which \
             is already to hand — it is the digest the deployment resolves."
        )));
    };

    if name.is_empty() {
        return Err(Error::Invalid(format!(
            "`{value}` carries a digest and no image name, so there is nothing to fetch and \
             check against it."
        )));
    }
    // The list is comma-separated, and whitespace is what a template that interpolated nothing
    // leaves behind. Either would split one reference into two when the annotation is read back.
    if name.contains(['@', ',']) || name.chars().any(char::is_whitespace) {
        return Err(Error::Invalid(format!(
            "`{value}` is not a reference `{ANNOTATION_IMAGES}` can carry: a `,` separates its \
             members and whitespace is what a template that interpolated nothing leaves behind, \
             so either one would split a single reference into two."
        )));
    }

    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(Error::Invalid(format!(
            "`{value}` carries a digest this protocol does not read. `sha256:` is what a \
             registry serves and what a deployment resolves; a reference is pinned as \
             `name@sha256:<64 hex digits>`."
        )));
    };
    if hex.len() != 64 || !hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(Error::Invalid(format!(
            "`{value}` is not a digest-pinned reference: a `sha256:` digest is exactly 64 \
             lower-case hexadecimal characters. Upper case is a different string to a registry, \
             so it would never match rather than matching loosely."
        )));
    }

    Ok(())
}

/// Whether `value` is a format a validator could dispatch a parser on.
///
/// Deliberately not a closed set — see [`Format::Other`]. What is refused is a value nothing
/// could act on at all: an empty one, or one carrying whitespace, which is what a template that
/// interpolated nothing leaves behind and which reads as *present* to anything checking only
/// that the key is there.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the annotation and what it is for.
fn format_token(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(Error::Invalid(format!(
            "`{value}` cannot be a `{ANNOTATION_FORMAT}`: the annotation says which parser reads \
             the document, so a value that is empty or carries whitespace is present without \
             saying anything — and present is what a validator checks for. `toml`, `yaml` and \
             `json` are the spellings this crate knows."
        )));
    }
    Ok(())
}

/// Whether `value` is a DNS subdomain, which is what a key's prefix must be.
///
/// Dot-separated labels, each non-empty, each beginning and ending alphanumeric, each carrying
/// only lower-case alphanumerics and `-` between. Lower case is not a nicety: the API server
/// refuses an upper-case prefix outright rather than folding it, which is why the image labels
/// and these keys are spelled apart.
fn is_dns_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|label| {
            let alphanumeric = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
            !label.is_empty()
                && label.chars().all(|c| alphanumeric(c) || c == '-')
                && label.chars().next().is_some_and(alphanumeric)
                && label.chars().next_back().is_some_and(alphanumeric)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, LABEL_CONTRACT_VERSION,
        configmap_key, digest_ref, format_token, label_key, label_value,
    };

    /// A digest-shaped tail, so the reference cases below read as references rather than as hex.
    const DIGEST: &str = "sha256:48e2c1e7a4c0d4e6b2f8a1c3d5e7f9a1b3c5d7e9f1a3c5d7e9f1a3c5d7e9f1a3";

    #[test]
    fn every_key_this_module_emits_is_one_the_api_server_accepts() {
        // These four are the entire key surface, and a key the API server refuses fails at
        // `kubectl apply` — after the chart rendered, after CI passed, on a stamp that is correct
        // in every other sense.
        for key in [
            LABEL_CONTRACT_VERSION,
            ANNOTATION_IMAGES,
            ANNOTATION_DOCUMENT_KEY,
            ANNOTATION_FORMAT,
        ] {
            label_key(key).expect("a key this module emits");
        }
    }

    #[test]
    fn a_key_is_at_most_a_prefix_a_slash_and_a_name() {
        assert!(label_key("contract-version").is_ok());
        assert!(label_key(&format!("{}/{}", "a".repeat(253), "b".repeat(63))).is_ok());

        assert!(label_key("").is_err());
        assert!(label_key("dev.terrace.config/").is_err());
        assert!(label_key("/contract-version").is_err());
        assert!(label_key("dev.terrace.config/a/b").is_err());
        assert!(label_key(&format!("{}/name", "a".repeat(254))).is_err());
        assert!(label_key(&format!("dev.terrace.config/{}", "a".repeat(64))).is_err());
        // A prefix is a DNS subdomain, and the API server refuses upper case rather than folding
        // it — which is why the image labels and these keys are spelled apart at all.
        assert!(label_key("Dev.Terrace.Config/contract-version").is_err());
        assert!(label_key("dev..config/contract-version").is_err());
        assert!(label_key("dev.config-/contract-version").is_err());
    }

    #[test]
    fn a_label_value_begins_and_ends_alphanumeric() {
        assert!(label_value("").is_ok());
        assert!(label_value("1").is_ok());
        assert!(label_value("v2.5.0").is_ok());
        assert!(label_value("a-b_c.d").is_ok());
        assert!(label_value(&"a".repeat(63)).is_ok());

        // The three this crate already publishes, and the reason the prefix and the contract
        // path stayed on the image rather than following the version onto the object.
        assert!(label_value("PORTFOLIO_").is_err());
        assert!(label_value("/config/contract.json").is_err());
        assert!(label_value("application/vnd.terrace.config-schema.v1+json").is_err());

        assert!(label_value(&"a".repeat(64)).is_err());
        assert!(label_value("-leading").is_err());
        assert!(label_value(".trailing.").is_err());
        // Multi-byte characters are outside the class, so the byte-count bound never has to
        // decide a case the character class has not already refused.
        assert!(label_value("ünïcode").is_err());
    }

    #[test]
    fn a_document_key_names_a_file_rather_than_a_directory() {
        assert!(configmap_key("config.toml").is_ok());
        assert!(configmap_key("00-base.toml").is_ok());
        assert!(configmap_key("_hidden").is_ok());
        assert!(configmap_key(&"a".repeat(253)).is_ok());
        // `...` is an ordinary file name. Only the two that name directory entries are refused,
        // and widening that to "anything all dots" would refuse a legal key.
        assert!(configmap_key("...").is_ok());

        assert!(configmap_key("").is_err());
        assert!(configmap_key(".").is_err());
        assert!(configmap_key("..").is_err());
        assert!(configmap_key("etc/config.toml").is_err());
        assert!(configmap_key("config toml").is_err());
        assert!(configmap_key(&"a".repeat(254)).is_err());
    }

    #[test]
    fn only_a_digest_pins_an_image() {
        assert!(digest_ref(&format!("ghcr.io/you/portfolio@{DIGEST}")).is_ok());
        // A tag beside the digest is ordinary and harmless: the digest is what is compared.
        assert!(digest_ref(&format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST}")).is_ok());
        assert!(digest_ref(&format!("portfolio@{DIGEST}")).is_ok());

        assert!(digest_ref("ghcr.io/you/portfolio:v2.5.0").is_err());
        assert!(digest_ref("ghcr.io/you/portfolio").is_err());
        assert!(digest_ref(&format!("@{DIGEST}")).is_err());
        assert!(digest_ref("ghcr.io/you/portfolio@sha256:").is_err());
        assert!(digest_ref("ghcr.io/you/portfolio@md5:abc").is_err());
        // 63 and 65, either side of the only length a `sha256` digest has.
        assert!(digest_ref(&format!("p@sha256:{}", "a".repeat(63))).is_err());
        assert!(digest_ref(&format!("p@sha256:{}", "a".repeat(65))).is_err());
        // Upper case is a different string to a registry, and `g` is not hexadecimal.
        assert!(digest_ref(&format!("p@sha256:{}", "A".repeat(64))).is_err());
        assert!(digest_ref(&format!("p@sha256:{}", "g".repeat(64))).is_err());
        // Either would split one reference into two when the annotation is read back.
        assert!(digest_ref(&format!("gh cr.io/p@{DIGEST}")).is_err());
        assert!(digest_ref(&format!("ghcr.io/a,b@{DIGEST}")).is_err());
    }

    #[test]
    fn a_format_says_something_or_it_is_refused() {
        assert!(format_token("toml").is_ok());
        // Not a closed set: an unfamiliar format is a value a consumer must report as
        // unrecognised, not one this crate refuses to let a chart write.
        assert!(format_token("hcl").is_ok());

        assert!(format_token("").is_err());
        assert!(format_token(" ").is_err());
        assert!(format_token("to ml").is_err());
    }
}
