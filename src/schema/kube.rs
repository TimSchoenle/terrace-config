//! The Kubernetes half of the publication protocol: what a *rendered object* says about itself.
//!
//! [`Contract::labels`](super::Contract::labels) puts three labels on an **image**, which is
//! enough for anything holding a digest. It is not enough for anything standing in a cluster. A
//! Kyverno policy, a validating admission webhook or an initContainer looking at a live
//! `ConfigMap` holds the *document* and not the image, and has no way to tell that the bytes it is
//! about to mount are the ones the pod's image expects. This module is the other end of that
//! pairing: the metadata a chart stamps onto the object, and [`Pairing`], which is the one
//! function a cluster-side actor calls to assert that an image and a document describe one
//! configuration surface.
//!
//! # Labels are what you select on. Annotations are what you read.
//!
//! That sentence explains every placement below, and it is forced by the platform rather than
//! chosen. A Kubernetes **label value** must be at most 63 characters and, unless empty, must
//! begin and end alphanumeric with `-`, `_` and `.` between. The values this crate already emits
//! on an image are illegal under that rule and cannot be made legal without changing what they
//! mean:
//!
//! | Value | Why it cannot be a label value |
//! |---|---|
//! | `PORTFOLIO_` — the dialect prefix | trailing `_` |
//! | `/config/contract.json` — [`DEFAULT_PATH`](super::DEFAULT_PATH) | `/` |
//! | `application/vnd.terrace.config-schema.v1+json` — [`ARTIFACT_TYPE`](super::ARTIFACT_TYPE) | `/` and `+` |
//!
//! An **annotation** key follows the same key rule, and an annotation *value* is unconstrained —
//! the whole `metadata.annotations` map is bounded at 256 KiB and nothing else applies. So a fact
//! a selector has to match on is a label, a fact something reads after it has already found the
//! object is an annotation, and there is no third option. The next person to read this will want
//! to move the prefix into a label; it will pass review and fail at `kubectl apply`.
//!
//! # The shape
//!
//! One label, [`LABEL_CONTRACT_VERSION`], whose value is
//! [`CONTRACT_VERSION`](super::CONTRACT_VERSION) stringified — a small integer, so always legal.
//! It answers the one question worth a selector: does this object participate in the protocol, and
//! in which version of it.
//!
//! ```yaml
//! match:
//!   any:
//!     - resources:
//!         selector:
//!           matchExpressions:
//!             - key: dev.terrace.config/contract-version
//!               operator: Exists
//! ```
//!
//! Three annotations: [`ANNOTATION_IMAGES`], [`ANNOTATION_DOCUMENT_KEY`] and
//! [`ANNOTATION_FORMAT`]. Two objects get stamped and they are not stamped identically — see
//! [`Target`].
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
//! let stamp = contract.kube_metadata(
//!     &Target::document("config.toml", Format::Toml),
//!     &["ghcr.io/you/portfolio@sha256:48e259cb4c9d0e3f1a2b5c6d7e8f9012345678901234567890abcdefabcdef01"],
//! )?;
//!
//! print!("{}", stamp.to_yaml(2));
//! # Ok::<(), terrace_config::Error>(())
//! ```
//!
//! # What this module does not do
//!
//! It renders no manifests. A `ConfigMap` is a chart's business, and a crate that emitted one
//! would be guessing at a name, a namespace and a release. What it emits is the metadata block to
//! paste and the check to run against what a cluster actually holds — the same division as
//! [`Contract::to_dockerfile_labels`](super::Contract::to_dockerfile_labels) and
//! [`Contract::verify_labels`](super::Contract::verify_labels), for the same reason: hand-writing
//! is unavoidable, so make it a paste and then check the result.

use std::collections::BTreeMap;
use std::fmt;

use super::{Contract, Error, LABEL_VERSION};

/// The DNS-subdomain prefix every key in this module carries.
///
/// The same identity as the image labels' `dev.terrace.config.*` stem, spelled with the separator
/// each platform requires: Docker labels are conventionally reverse-DNS and dotted throughout,
/// while a Kubernetes key that carries a prefix must separate it from the name with `/`. Two
/// spellings of one namespace, because the platforms disagree — not two namespaces.
pub const NAMESPACE: &str = "dev.terrace.config";

/// The label carrying [`CONTRACT_VERSION`](super::CONTRACT_VERSION).
///
/// The entire label surface, and deliberately so.
///
/// There is no `app` label. A document may be read by several images — that is the union case, and
/// it is the normal case for a workspace whose binaries share one rendered `ConfigMap` — so the
/// fact is multi-valued, and a multi-valued label needs a separator. Every plausible one is
/// illegal in a label value: `,`, `/` and a space are all outside the character class, and `.` and
/// `-` are already legal *inside* a name, so neither can separate. Anything per-image or
/// multi-valued is an annotation, and that rule has no exceptions here.
///
/// There is no `prefix` or `contract-path` label either, for a different reason. Both are facts
/// about **an image**, and the image already carries them as
/// [`LABEL_PREFIX`](super::LABEL_PREFIX) and [`LABEL_PATH`](super::LABEL_PATH). Copying them onto
/// a Kubernetes object would create a second spelling of a fact that already has one, which is
/// exactly the drift [`Contract::verify_labels`](super::Contract::verify_labels) exists to catch —
/// and here there would be nothing to catch it, because the object is rendered by a chart this
/// crate never sees. A consumer that wants the prefix reads it from the image it is already
/// inspecting.
pub const LABEL_CONTRACT_VERSION: &str = "dev.terrace.config/contract-version";

/// The annotation listing every image that reads this document, comma-separated.
///
/// **Digest-pinned, each of them.** This is a hard requirement rather than a preference, and it is
/// the same reasoning that makes the contract an OCI referrer of a digest rather than of a tag: a
/// tag can be moved, so a pairing keyed on one proves nothing about what is running. A reference
/// carrying no `@sha256:` is refused by [`Contract::kube_metadata`] rather than accepted and
/// checked leniently later.
///
/// This crate cannot supply the value. A contract deliberately carries no digest — the digest is
/// what building the image *produces*, so a document containing it must be written after the push,
/// changing bytes that were already hashed. So the value is passed in by whatever renders the
/// object, and this crate's job is to name the annotation, validate what it is given, and refuse
/// an unpinned reference.
///
/// Order is the caller's declaration order, preserved verbatim: a reader that diffs two renders of
/// one chart should see a reordered list as a change, because in a chart it is one.
pub const ANNOTATION_IMAGES: &str = "dev.terrace.config/images";

/// The annotation naming which key inside `data` is the configuration document.
///
/// A `ConfigMap` may carry several files, and a validator without this has to guess which one the
/// contract describes — a guess that is right until somebody adds a second key, and then silently
/// validates the wrong file.
///
/// Only on a [`Target::Document`]. A pod is not a document.
pub const ANNOTATION_DOCUMENT_KEY: &str = "dev.terrace.config/document-key";

/// The annotation naming how the document is written — `toml` today.
///
/// It exists because a YAML or JSON document normalises to the same tree and every gate is
/// unchanged by which one it was, so the *only* thing a validator needs in order to accept all
/// three is to know which parser to reach for. Publishing it now costs nothing and means the day
/// the first YAML document appears, nothing downstream has to guess or be redeployed.
///
/// Only on a [`Target::Document`], for the same reason as [`ANNOTATION_DOCUMENT_KEY`].
pub const ANNOTATION_FORMAT: &str = "dev.terrace.config/format";

/// The longest a Kubernetes label value, or the name half of a key, may be.
const MAX_NAME: usize = 63;

/// The longest a DNS subdomain may be — the prefix half of a key, and a `ConfigMap` data key.
const MAX_SUBDOMAIN: usize = 253;

/// How the configuration document is written.
///
/// Carries a fallback variant, and `#[non_exhaustive]` is not a substitute for it: a consumer
/// deserialising an annotation into a closed enum makes one unfamiliar value a parse error, and
/// `#[non_exhaustive]` says something to a Rust *caller* and nothing at all to `Deserialize`. The
/// same reasoning as [`CONTRACT_VERSION`](super::CONTRACT_VERSION)'s, one level down.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Format {
    /// TOML, which is what every layer of this loader reads today.
    Toml,
    /// YAML.
    Yaml,
    /// JSON.
    Json,
    /// A spelling this version does not know. Carried verbatim.
    Other(String),
}

impl Format {
    /// The annotation value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::Other(other) => other,
        }
    }

    /// Read an annotation value back.
    ///
    /// Matched exactly rather than case-insensitively, and a spelling that is not one of the three
    /// becomes [`Self::Other`] rather than an error. Accepting `TOML` as well as `toml` would put
    /// two spellings of one format into the wild, and the second one to appear is the one a
    /// hand-written policy will not have been written against.
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

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which of the two objects is being stamped.
///
/// They are not stamped identically, and modelling that as an enum rather than as two constructors
/// returning one type with fields silently absent is the point: a caller has to say which object
/// this is, and a reader of [`Contract::verify_kube_metadata`] can see that a `document-key` on a
/// pod is refused rather than tolerated.
///
/// - **The document object** — the `ConfigMap`, or the `Secret` behind a secrets-directory mount —
///   carries the label and all three annotations.
/// - **The workload pod template** (`spec.template.metadata`) carries the label and
///   [`ANNOTATION_IMAGES`] only. `document-key` and `format` are properties of a document, and a
///   pod is not one.
///
/// A pod carries the stamp at all so that an admission webhook seeing only the pod — which is what
/// an admission webhook usually sees — can reach the image list without walking ownership
/// references to find the object that was mounted into it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The object holding the document.
    Document {
        /// Which key inside `data` the document is. See [`ANNOTATION_DOCUMENT_KEY`].
        key: String,
        /// How it is written. See [`ANNOTATION_FORMAT`].
        format: Format,
    },
    /// The workload's pod template.
    Workload,
}

impl Target {
    /// The object holding the document, under `key`.
    #[must_use]
    pub fn document(key: impl Into<String>, format: Format) -> Self {
        Self::Document {
            key: key.into(),
            format,
        }
    }
}

/// The metadata one object carries, ready to render.
///
/// Every key and value in here has been checked against the Kubernetes character rules by
/// [`Contract::kube_metadata`], which is the only way to obtain one. That is what the private
/// fields are for: this type is a small proof, and a caller able to insert an arbitrary pair would
/// be able to produce a `Metadata` that `kubectl apply` refuses.
///
/// `BTreeMap` rather than `HashMap`, so [`Self::to_yaml`] is byte-identical across runs and a
/// generated block can be committed and diffed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

impl Metadata {
    /// What belongs under `metadata.labels`. Never empty.
    #[must_use]
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// What belongs under `metadata.annotations`. Never empty.
    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// The block to paste into a Helm template, every line indented by `indent` spaces.
    ///
    /// Mirrors [`Contract::to_dockerfile_labels`](super::Contract::to_dockerfile_labels), and for
    /// the same reason: a template's `metadata:` block is hand-written whatever this crate does, so
    /// the useful thing is to make writing it a paste and checking it a function call.
    ///
    /// Emits a `labels:` map and an `annotations:` map, in that order, each entry indented two
    /// further. Both are always non-empty, so neither is ever rendered as a bare key with no
    /// mapping under it. Ends with a newline.
    ///
    /// Keys are plain scalars, which is how a Kubernetes manifest is normally written and legal
    /// because a validated key can contain neither `: ` nor a leading indicator. Values are
    /// double-quoted so that `"1"` is a string rather than an integer — a label value is a string
    /// to the API server, and a template that lets YAML decide otherwise fails at apply time.
    #[must_use]
    pub fn to_yaml(&self, indent: usize) -> String {
        let outer = " ".repeat(indent);
        let inner = " ".repeat(indent + 2);

        let mut rendered = String::new();
        for (block, entries) in [("labels", &self.labels), ("annotations", &self.annotations)] {
            rendered.push_str(&outer);
            rendered.push_str(block);
            rendered.push_str(":\n");
            for (key, value) in entries {
                rendered.push_str(&inner);
                rendered.push_str(key);
                rendered.push_str(": \"");
                // Neither character can occur in a value this type holds — the validators refuse
                // both. Escaped anyway, because the failure of getting it wrong is a manifest that
                // parses as something else rather than a manifest that fails to parse.
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
    /// The Kubernetes metadata for one object, given every image that reads the document.
    ///
    /// `images` is in declaration order and each entry must be digest-pinned — see
    /// [`ANNOTATION_IMAGES`] for why a tag is refused rather than tolerated.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if `images` is empty or holds a reference that is not
    /// digest-pinned, if a [`Target::Document`]'s key is not a legal `ConfigMap` data key, or if
    /// its format is not a legal annotation value. Every message names the offending value and
    /// what a legal one looks like.
    pub fn kube_metadata(&self, target: &Target, images: &[&str]) -> Result<Metadata, Error> {
        // Checked rather than assumed, even though a `u32` rendered as decimal is always legal and
        // this can never fire today. The claim this module makes is that everything it emits is
        // legal, and the place to enforce a claim is where the value is produced — not in a test
        // that describes the version constant as it stands.
        let version = self.terrace_contract.to_string();
        label_key(LABEL_CONTRACT_VERSION)?;
        label_value(&version)?;

        if images.is_empty() {
            return Err(Error::Invalid(format!(
                "no images were given for `{ANNOTATION_IMAGES}`, so the stamp would say that this \
                 document is read by nothing. Pass every image that reads it, digest-pinned; a \
                 document genuinely read by no image needs no stamp at all."
            )));
        }
        for image in images {
            digest_ref(image)?;
        }
        let mut annotations = BTreeMap::new();
        annotations.insert(ANNOTATION_IMAGES.to_owned(), images.join(","));

        if let Target::Document { key, format } = target {
            configmap_key(key)?;
            annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), key.clone());

            // A format is an annotation value, so the platform would accept anything. It is held
            // to the label rule anyway: the value is a short machine-written token, and keeping it
            // one leaves the door open to selecting on it later without a migration.
            let format = format.as_str();
            if format.is_empty() {
                return Err(Error::Invalid(format!(
                    "the document format is empty, so `{ANNOTATION_FORMAT}` would say nothing \
                     about which parser reads this document. Use `Format::Toml`, or the spelling \
                     of whatever writes it."
                )));
            }
            label_value(format)?;
            annotations.insert(ANNOTATION_FORMAT.to_owned(), format.to_owned());
        }

        for key in annotations.keys() {
            label_key(key)?;
        }

        let mut labels = BTreeMap::new();
        labels.insert(LABEL_CONTRACT_VERSION.to_owned(), version);

        Ok(Metadata {
            labels,
            annotations,
        })
    }

    /// Check the metadata a live object actually carries.
    ///
    /// `labels` and `annotations` are `metadata.labels` and `metadata.annotations` as the API
    /// server reports them. Extra entries are ignored — an object carries `app.kubernetes.io/*`
    /// and whatever the chart's own conventions add, and none of that is this document's business
    /// — with one exception: a [`Target::Workload`] carrying `document-key` or `format` is refused
    /// rather than ignored, because those are the two annotations a template can only have grown
    /// by copying a document's stamp onto a pod.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first entry that is missing, disagrees with this
    /// contract, or is malformed.
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
                     `{expected}`. The object was rendered against a different generation of this \
                     protocol than the contract being checked; re-render the chart, or check \
                     against the contract the running image actually publishes."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so nothing selecting on \
                     this protocol will ever see it. `Contract::kube_metadata` produces the block, \
                     and `Metadata::to_yaml` renders it for a Helm template."
                )));
            }
        }

        match annotations.get(ANNOTATION_IMAGES) {
            Some(images) if !images.is_empty() => {
                for image in images.split(',') {
                    digest_ref(image)?;
                }
            }
            Some(_) => {
                return Err(Error::Invalid(format!(
                    "the object's `{ANNOTATION_IMAGES}` is empty, which claims this document is \
                     read by nothing. An empty list is not the same as an absent annotation and \
                     neither is checkable; list every image that reads it, digest-pinned."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{ANNOTATION_IMAGES}`, so nothing can tell which image \
                     this document belongs to and the pairing cannot be checked at all."
                )));
            }
        }

        match target {
            Target::Document { key, format } => {
                verify_document_annotation(annotations, ANNOTATION_DOCUMENT_KEY, key.as_str())?;
                verify_document_annotation(annotations, ANNOTATION_FORMAT, format.as_str())?;
                // Both values are held to the same rules `kube_metadata` writes them under, so a
                // `Target` a caller assembled by hand cannot make this verifier accept a stamp the
                // generator would have refused.
                configmap_key(key)?;
                label_value(format.as_str())?;
            }
            Target::Workload => {
                for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
                    if let Some(found) = annotations.get(name) {
                        return Err(Error::Invalid(format!(
                            "this is a workload's pod template and it carries `{name}` = \
                             `{found}`. A pod is not a document, so a template that stamps one \
                             was written by copying a `ConfigMap`'s block — and the copy will \
                             keep saying `{found}` after the document stops. Drop both \
                             `{ANNOTATION_DOCUMENT_KEY}` and `{ANNOTATION_FORMAT}` from the pod \
                             template; the label and `{ANNOTATION_IMAGES}` are what belongs there."
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

/// One annotation a [`Target::Document`] must carry, with the value the caller expects.
fn verify_document_annotation(
    annotations: &BTreeMap<String, String>,
    name: &str,
    expected: &str,
) -> Result<(), Error> {
    match annotations.get(name) {
        Some(found) if found == expected => Ok(()),
        Some(found) => Err(Error::Invalid(format!(
            "the object's `{name}` is `{found}`, and the document being checked is `{expected}`. \
             A validator following the annotation would read a different file from the one this \
             check was given, and pass."
        ))),
        None => Err(Error::Invalid(format!(
            "the object carries no `{name}`, so a validator has to guess which key in `data` is \
             the configuration document. Stamp it with `Contract::kube_metadata`."
        ))),
    }
}

/// One image, one document, and the assertion that they describe one configuration surface.
///
/// This is the function a cluster-side actor calls. Everything else in this module exists to
/// produce or validate one of its inputs.
///
/// ```no_run
/// # use std::collections::BTreeMap;
/// # use terrace_config::schema::{Contract, DEFAULT_PATH};
/// # use terrace_config::schema::kube::{Format, Pairing, Target};
/// # fn demo(contract: &Contract, image_labels: &BTreeMap<String, String>,
/// #         object_labels: &BTreeMap<String, String>,
/// #         object_annotations: &BTreeMap<String, String>) -> Result<(), terrace_config::Error> {
/// Pairing::new(contract, DEFAULT_PATH)
///     .image("ghcr.io/you/portfolio@sha256:48e2…", image_labels)
///     .object(
///         &Target::document("config.toml", Format::Toml),
///         object_labels,
///         object_annotations,
///     )
///     .check()
/// # }
/// ```
pub struct Pairing<'a> {
    contract: &'a Contract,
    contract_path: &'a str,
    image: Option<(&'a str, &'a BTreeMap<String, String>)>,
    object: Option<Stamped<'a>>,
}

/// The object half of a [`Pairing`], as the builder collected it.
struct Stamped<'a> {
    target: &'a Target,
    labels: &'a BTreeMap<String, String>,
    annotations: &'a BTreeMap<String, String>,
}

impl<'a> Pairing<'a> {
    /// The contract under test, and the path it was embedded at in the image.
    ///
    /// `contract_path` is what [`LABEL_PATH`](super::LABEL_PATH) should say —
    /// [`DEFAULT_PATH`](super::DEFAULT_PATH) unless the build put it elsewhere.
    #[must_use]
    pub fn new(contract: &'a Contract, contract_path: &'a str) -> Self {
        Self {
            contract,
            contract_path,
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

    /// The mounted document object, or the pod template: which it is, and its `metadata`.
    #[must_use]
    pub fn object(
        mut self,
        target: &'a Target,
        labels: &'a BTreeMap<String, String>,
        annotations: &'a BTreeMap<String, String>,
    ) -> Self {
        self.object = Some(Stamped {
            target,
            labels,
            annotations,
        });
        self
    }

    /// Assert that all of it describes one configuration surface.
    ///
    /// Five checks, in the order that makes the *first* failure the informative one:
    ///
    /// 1. the image's own labels agree with this contract —
    ///    [`Contract::verify_labels`](super::Contract::verify_labels), not a second copy of it;
    /// 2. the object's `contract-version` agrees with the image's;
    /// 3. the object's `contract-version` agrees with this contract, and its `document-key` and
    ///    `format` are present and well-formed — [`Contract::verify_kube_metadata`];
    /// 4. the running image's reference is a **member** of the object's image list — membership
    ///    rather than equality, because a document read by several binaries is the normal case;
    /// 5. every reference in that list, including the running one, is digest-pinned.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming both sides of the first disagreement, or naming the
    /// builder step that was never called.
    pub fn check(&self) -> Result<(), Error> {
        let Some((reference, image_labels)) = self.image else {
            return Err(Error::Invalid(
                "no image was given to this pairing, so there is nothing to check the document \
                 against. Call `Pairing::image` with the running container's digest-pinned \
                 reference and the labels from its config blob."
                    .to_owned(),
            ));
        };
        let Some(Stamped {
            target,
            labels: object_labels,
            annotations: object_annotations,
        }) = self.object
        else {
            return Err(Error::Invalid(
                "no object was given to this pairing, so there is nothing to check the image \
                 against. Call `Pairing::object` with the mounted document's metadata."
                    .to_owned(),
            ));
        };

        // (1) The image half. Reused rather than reimplemented: a second copy of these three
        // comparisons is a second place for the label names to be spelled, which is the failure
        // that constants exist to prevent.
        self.contract
            .verify_labels(self.contract_path, image_labels)?;

        // (2) Redundant with (3) as a *check* — `verify_labels` has just established that the
        // image's version equals this contract's — and here for the message. An object saying `1`
        // against an image saying `2` is a chart and a build a generation apart, and telling an
        // operator "the object disagrees with the contract" sends them to the wrong repository.
        if let Some(found) = object_labels.get(LABEL_CONTRACT_VERSION)
            && let Some(image_version) = image_labels.get(LABEL_VERSION)
            && found != image_version
        {
            return Err(Error::Invalid(format!(
                "the object's `{LABEL_CONTRACT_VERSION}` is `{found}` and the image's \
                 `{LABEL_VERSION}` is `{image_version}`. The document and the image that reads it \
                 were produced against different generations of this protocol; roll the chart and \
                 the image forward together, or pin the image back to one the chart was rendered \
                 for."
            )));
        }

        // (3) The object half.
        self.contract
            .verify_kube_metadata(target, object_labels, object_annotations)?;

        // (4) Membership.
        digest_ref(reference).map_err(|error| {
            Error::Invalid(format!(
                "the reference given for the running image is not usable as evidence: {error} A \
                 pairing keyed on anything a tag can move is not a pairing."
            ))
        })?;
        let listed = object_annotations
            .get(ANNOTATION_IMAGES)
            .map(String::as_str)
            .unwrap_or_default();
        if listed
            .split(',')
            .any(|member| same_image(member, reference))
        {
            return Ok(());
        }
        Err(Error::Invalid(format!(
            "the running image is `{reference}`, and the document's `{ANNOTATION_IMAGES}` lists \
             `{listed}`. This pod is mounting a document that was never rendered for it: either \
             the chart's image list is missing this component, or the pod is running a digest the \
             chart no longer pins."
        )))
    }
}

/// Whether two digest-pinned references name the same image.
///
/// Compared on the repository and the digest, so `repo:v2.5.0@sha256:…` and `repo@sha256:…` are
/// one image. Both spellings are produced in practice — a chart writes the first and a pod's
/// `status.containerStatuses[].imageID` reports something closer to the second — and a check that
/// insisted on one of them would be a check nothing in a real cluster could pass.
///
/// The repository is compared too, rather than the digest alone. Two references to one digest
/// through two registries are the same *bytes*, but the annotation is a statement about which
/// images a chart deploys, and a mirror it never named is not one of them.
fn same_image(left: &str, right: &str) -> bool {
    match (reference_parts(left), reference_parts(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// The repository and digest of a pinned reference, or [`None`] if it is not one.
fn reference_parts(reference: &str) -> Option<(&str, &str)> {
    let (name, digest) = reference.rsplit_once('@')?;
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    if name.is_empty() {
        return None;
    }
    // A `:` whose remainder carries no `/` is a tag; one whose remainder does is a registry port,
    // as in `localhost:5000/portfolio`.
    let repository = match name.rsplit_once(':') {
        Some((repository, tag)) if !repository.is_empty() && !tag.contains('/') => repository,
        _ => name,
    };
    Some((repository, hex))
}

/// Refuse a key Kubernetes will not accept on `metadata.labels` or `metadata.annotations`.
///
/// An optional DNS-subdomain prefix of at most 253 characters, then `/`, then a name of at most 63
/// under the same rule as a label value. Written out by hand rather than as a regular expression:
/// four predicates do not justify this manifest's first unargued dependency, and every one of them
/// is a character-class walk that reads more plainly than the pattern would.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the key and the rule it broke.
fn label_key(value: &str) -> Result<(), Error> {
    let (prefix, name) = match value.split_once('/') {
        Some((prefix, name)) => (Some(prefix), name),
        None => (None, value),
    };

    if name.contains('/') {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes key: a key carries at most one `/`, separating an \
             optional DNS-subdomain prefix from the name."
        )));
    }

    if let Some(prefix) = prefix {
        if prefix.is_empty() || prefix.len() > MAX_SUBDOMAIN {
            return Err(Error::Invalid(format!(
                "`{value}` is not a legal Kubernetes key: the prefix before `/` must be a DNS \
                 subdomain of 1 to {MAX_SUBDOMAIN} characters, and this one is {} long.",
                prefix.len()
            )));
        }
        if !is_dns_subdomain(prefix) {
            return Err(Error::Invalid(format!(
                "`{value}` is not a legal Kubernetes key: the prefix before `/` must be a DNS \
                 subdomain — lower-case alphanumerics, `-` and `.`, beginning and ending \
                 alphanumeric, with no empty label between two dots."
            )));
        }
    }

    if name.is_empty() {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes key: the name after `/` is empty."
        )));
    }
    if name.len() > MAX_NAME {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes key: the name is {} characters and the limit is \
             {MAX_NAME}.",
            name.len()
        )));
    }
    if !is_name(name) {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes key: the name must begin and end alphanumeric, \
             with `-`, `_` and `.` allowed between."
        )));
    }
    Ok(())
}

/// Refuse a value Kubernetes will not accept on `metadata.labels`.
///
/// At most 63 characters and, unless empty, beginning and ending alphanumeric with `-`, `_` and
/// `.` between. This is the rule that decides the whole shape of this module — see the module
/// documentation for the three values this crate already emits that fail it.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the value and the rule it broke.
fn label_value(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > MAX_NAME {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes label value: it is {} characters and the limit is \
             {MAX_NAME}. A value this long is an annotation, whose values are unconstrained.",
            value.len()
        )));
    }
    if !is_name(value) {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal Kubernetes label value: it must begin and end alphanumeric, \
             with `-`, `_` and `.` allowed between — so no `/`, no `+`, and no trailing `_`. A \
             value that cannot satisfy that is an annotation, whose values are unconstrained."
        )));
    }
    Ok(())
}

/// Refuse a name a `ConfigMap` or `Secret` cannot use as a key in `data`.
///
/// Alphanumerics, `-`, `_` and `.`, at most 253 characters, and neither `.` nor `..` — the two the
/// API server refuses outright, because a projected volume would have to write a file named for a
/// directory entry that already means something.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the key and the rule it broke.
fn configmap_key(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::Invalid(
            "the document key is empty, so nothing names which entry in `data` the configuration \
             document is."
                .to_owned(),
        ));
    }
    if value == "." || value == ".." {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal `ConfigMap` data key: `.` and `..` are refused by the API \
             server, because a projected volume cannot write a file named for a directory entry \
             that already means something."
        )));
    }
    if value.len() > MAX_SUBDOMAIN {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal `ConfigMap` data key: it is {} characters and the limit is \
             {MAX_SUBDOMAIN}.",
            value.len()
        )));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return Err(Error::Invalid(format!(
            "`{value}` is not a legal `ConfigMap` data key: only alphanumerics, `-`, `_` and `.` \
             are allowed — a key with a `/` in it names a path, and `data` is flat."
        )));
    }
    Ok(())
}

/// Refuse an image reference a tag could move out from under.
///
/// # Errors
/// Returns [`Error::Invalid`] naming the reference and what a pinned one looks like.
fn digest_ref(value: &str) -> Result<(), Error> {
    if value.contains(',') {
        return Err(Error::Invalid(format!(
            "the image reference `{value}` contains a comma, which is what separates entries in \
             `{ANNOTATION_IMAGES}`. One reference carrying the separator makes the list mean \
             something other than what was written."
        )));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(Error::Invalid(format!(
            "the image reference `{value}` contains whitespace. `{ANNOTATION_IMAGES}` is a list \
             with no padding around its separator, so a reference with a space in it is a \
             reference a comparison will never match."
        )));
    }
    if reference_parts(value).is_none() {
        return Err(Error::Invalid(format!(
            "the image reference `{value}` is not digest-pinned. It must carry \
             `@sha256:` followed by 64 lower-case hexadecimal characters — a tag can be moved \
             after the chart was rendered, so a pairing keyed on one proves nothing about what is \
             actually running."
        )));
    }
    Ok(())
}

/// The shared character rule: begins and ends alphanumeric, `-`, `_` and `.` between.
///
/// Empty is *not* accepted here; the two callers differ on whether empty is legal and each says so
/// itself.
fn is_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && last.is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' || *b == b'.')
}

/// Whether `value` is a DNS subdomain: dot-separated labels, each beginning and ending
/// alphanumeric, lower case, with `-` between.
///
/// Note the difference from [`is_name`], which is what makes them two functions: a subdomain is
/// lower case and admits no `_`, while a label's *name* half admits both.
fn is_dns_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|label| {
            let bytes = label.as_bytes();
            let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
                return false;
            };
            is_subdomain_byte(*first)
                && is_subdomain_byte(*last)
                && bytes.iter().all(|b| is_subdomain_byte(*b) || *b == b'-')
        })
}

/// The character class a DNS label admits at its edges, and everywhere but for `-`.
fn is_subdomain_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::{
        LABEL_CONTRACT_VERSION, NAMESPACE, configmap_key, digest_ref, is_dns_subdomain, label_key,
        label_value, reference_parts, same_image,
    };

    const DIGEST: &str = "sha256:48e259cb4c9d0e3f1a2b5c6d7e8f9012345678901234567890abcdefabcdef01";

    #[test]
    fn the_keys_this_module_emits_are_keys_kubernetes_accepts() {
        for key in [
            LABEL_CONTRACT_VERSION,
            super::ANNOTATION_IMAGES,
            super::ANNOTATION_DOCUMENT_KEY,
            super::ANNOTATION_FORMAT,
        ] {
            label_key(key).expect("every key this module emits must be legal");
            assert!(key.starts_with(NAMESPACE));
        }
        assert!(is_dns_subdomain(NAMESPACE));
    }

    /// The three values the image half already publishes, each of which is why the Kubernetes half
    /// puts the same fact in an annotation or nowhere at all.
    #[test]
    fn the_image_labels_values_are_illegal_as_kubernetes_label_values() {
        for value in [
            "PORTFOLIO_",
            "/config/contract.json",
            "application/vnd.terrace.config-schema.v1+json",
        ] {
            label_value(value).expect_err("this is the constraint the whole module is shaped by");
        }
    }

    #[test]
    fn a_label_value_is_bounded_and_alphanumeric_at_both_ends() {
        label_value("").expect("an empty label value is legal");
        label_value("1").expect("legal");
        label_value("a-b_c.d").expect("legal");
        label_value(&"a".repeat(63)).expect("exactly at the limit");

        label_value(&"a".repeat(64)).expect_err("one over the limit");
        label_value("-leading").expect_err("must begin alphanumeric");
        label_value("trailing.").expect_err("must end alphanumeric");
        label_value("has space").expect_err("space is outside the class");
    }

    #[test]
    fn a_key_is_a_bounded_prefix_and_a_bounded_name() {
        label_key("simple").expect("a name with no prefix is legal");
        label_key("app.kubernetes.io/name").expect("legal");
        label_key(&format!("{}/name", "a".repeat(253))).expect("exactly at the prefix limit");

        label_key(&format!("{}/name", "a".repeat(254))).expect_err("one over the prefix limit");
        label_key("a/b/c").expect_err("at most one `/`");
        label_key("prefix/").expect_err("the name may not be empty");
        label_key("/name").expect_err("the prefix may not be empty");
        label_key("Prefix.Example/name").expect_err("a subdomain is lower case");
        label_key("under_score.example/name").expect_err("a subdomain admits no `_`");
        label_key("double..dot/name").expect_err("no empty label between two dots");
        label_key(&format!("prefix.example/{}", "a".repeat(64))).expect_err("the name is bounded");
    }

    #[test]
    fn a_configmap_key_is_flat_and_is_neither_dot_nor_dotdot() {
        configmap_key("config.toml").expect("legal");
        configmap_key("_underscore-and.dots").expect("legal");

        configmap_key("").expect_err("empty names nothing");
        configmap_key(".").expect_err("refused by the API server");
        configmap_key("..").expect_err("refused by the API server");
        configmap_key("conf.d/config.toml").expect_err("`data` is flat");
        configmap_key(&"a".repeat(254)).expect_err("bounded at a DNS subdomain's length");
    }

    #[test]
    fn a_reference_is_only_evidence_when_a_tag_cannot_move_it() {
        digest_ref(&format!("ghcr.io/you/portfolio@{DIGEST}")).expect("pinned");
        digest_ref(&format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST}")).expect("pinned, and tagged");
        digest_ref(&format!("localhost:5000/portfolio@{DIGEST}")).expect("a registry port");

        digest_ref("ghcr.io/you/portfolio:v2.5.0").expect_err("a tag can be moved");
        digest_ref(&format!("ghcr.io/you/portfolio@{DIGEST},x"))
            .expect_err("a comma is the list separator");
        digest_ref(&format!("ghcr.io/you/portfolio @{DIGEST}")).expect_err("whitespace");
        digest_ref(&format!("@{DIGEST}")).expect_err("no repository");
        digest_ref("ghcr.io/you/portfolio@sha256:abc").expect_err("a truncated digest");
        digest_ref(&format!("ghcr.io/you/portfolio@sha256:{}", "AB".repeat(32)))
            .expect_err("upper case is not the spelling a registry produces");
        digest_ref(&format!("ghcr.io/you/portfolio@sha512:{}", "a".repeat(64)))
            .expect_err("only `sha256` today");
    }

    /// The two spellings a chart and a running pod produce for one image differ by the tag, and a
    /// membership check that insisted on one of them would be one nothing in a cluster could pass.
    #[test]
    fn a_tagged_and_an_untagged_reference_to_one_digest_are_one_image() {
        let tagged = format!("ghcr.io/you/portfolio:v2.5.0@{DIGEST}");
        let plain = format!("ghcr.io/you/portfolio@{DIGEST}");
        assert!(same_image(&tagged, &plain));

        // A mirror is the same bytes and a different statement about what the chart deploys.
        let mirror = format!("mirror.example/you/portfolio@{DIGEST}");
        assert!(!same_image(&plain, &mirror));

        let other = format!("ghcr.io/you/portfolio@sha256:{}", "b".repeat(64));
        assert!(!same_image(&plain, &other));
        assert!(!same_image("ghcr.io/you/portfolio:v2.5.0", &plain));
    }

    #[test]
    fn a_registry_port_is_not_read_as_a_tag() {
        let reference = format!("localhost:5000/portfolio@{DIGEST}");
        assert_eq!(
            reference_parts(&reference).map(|(repository, _)| repository),
            Some("localhost:5000/portfolio")
        );
    }
}
