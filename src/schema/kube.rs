//! The cluster-side half of the protocol: what a Kubernetes object carries, and the check that
//! pairs it with the image's own labels.
//!
//! [`Contract::labels`] puts three labels on an *image*, which is enough for a build pipeline
//! holding a digest. It reaches nothing inside a cluster. A Kyverno policy, a validating admission
//! webhook or an `initContainer` looking at a live `ConfigMap` holds an *object*, and an object
//! that says nothing about the contract it was rendered against is an object no policy can decide
//! about. This module is the stamp that object carries, and [`Pairing`] is the check that the stamp
//! and the image describe one configuration surface.
//!
//! # Labels are what you select on. Annotations are what you read.
//!
//! That sentence explains every placement below, and it is forced by the platform rather than
//! chosen. A Kubernetes **label value** must be 63 characters or fewer and, unless it is empty,
//! must begin and end with an alphanumeric character, with only `-`, `_` and `.` between them.
//! Every value the image half already publishes fails that rule:
//!
//! | Value | Why it is not a label value |
//! |---|---|
//! | `PORTFOLIO_` — the dialect prefix | ends in `_` |
//! | `/config/contract.json` — the contract path | `/` is not in the character set |
//! | `application/vnd.terrace.config-schema.v1+json` — [`ARTIFACT_TYPE`] | `/` and `+` |
//!
//! A label **key** is a different rule again: an optional DNS-subdomain prefix of 253 characters or
//! fewer, then `/`, then a name of 63 characters or fewer under the label-value character rule.
//! `dev.terrace.config` is a legal prefix, which is why the keys below can be spelled with the same
//! words the image labels use — with a `/` where the image labels have a `.`.
//!
//! Annotation keys follow the same key rule, and annotation **values are unconstrained**: the whole
//! `metadata.annotations` map is bounded at 256 KiB and nothing else applies. So anything that has
//! to be selected on is a label and has to survive that character rule, and everything else is an
//! annotation. There is no third option, and the next person to move a value across that line will
//! find out at `kubectl apply` time rather than here.
//!
//! # Two objects, stamped differently
//!
//! See [`Target`]. The document object gets everything; a pod template gets the label and the image
//! list, because a document key and a format are properties of a document and a pod is not one.
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
//!     .into_contract(App::new("portfolio").version("v2.5.0"))
//!     .build()?;
//!
//! let image = "ghcr.io/you/portfolio@sha256:\
//!              48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f";
//! let metadata = contract.kube_metadata(
//!     &Target::document("config.toml", Format::Toml),
//!     &[image],
//! )?;
//!
//! print!("{}", metadata.to_yaml(2));
//! # Ok::<(), terrace_config::Error>(())
//! ```
//!
//! # What this is not
//!
//! It is not a renderer. Nothing here writes a manifest, and [`Metadata::to_yaml`] emits a block to
//! paste rather than an object to apply — for [`Contract::to_dockerfile_labels`]'s reason, which is
//! that the chart is hand-written in a repository this crate never sees. What the crate owns is the
//! *names*, the character rules the values have to satisfy, and the check that the two halves
//! agree. Rendering is the chart's business, and a chart that spells a key by hand is a chart that
//! can spell it differently from the policy reading it.
//!
//! [`ARTIFACT_TYPE`]: super::ARTIFACT_TYPE

use std::collections::BTreeMap;

use super::{Contract, Error, LABEL_VERSION};

/// The DNS-subdomain prefix every key in this module carries.
///
/// Spelled with a `/` after it where the image labels spell a `.` — [`LABEL_VERSION`] is
/// `dev.terrace.config.contract.version` and [`LABEL_CONTRACT_VERSION`] is
/// `dev.terrace.config/contract-version` — because that is where the platform puts the boundary
/// between "who owns this key" and "what the key is". Same owner, same words, one separator that is
/// not a choice.
///
/// A constant because a policy selecting on the namespace — *every object participating in this
/// protocol* — needs the same string the stamp was written with, and a policy repository is not a
/// place where a typo announces itself.
pub const NAMESPACE: &str = "dev.terrace.config";

/// The label carrying [`CONTRACT_VERSION`](super::CONTRACT_VERSION), stringified.
///
/// The entire label surface. It answers the one question a cluster-side actor asks with a selector
/// — *does this object participate in the protocol, and in which version of it* — and its value is
/// a decimal integer, which is a legal label value for every integer there will ever be.
///
/// ```yaml
/// matchExpressions:
///   - key: dev.terrace.config/contract-version
///     operator: Exists
/// ```
pub const LABEL_CONTRACT_VERSION: &str = "dev.terrace.config/contract-version";

/// The annotation listing every image that reads this document, digest-pinned and comma-separated.
///
/// An annotation rather than a label for two independent reasons, either of which alone would
/// decide it. A digest reference carries `/`, `:` and `@`, none of which a label value may hold;
/// and the list is *multi-valued*, which no label value can be, because every separator a reader
/// might reach for — `,`, `/`, a space — is outside the label-value character set.
///
/// The crate cannot supply this value. A contract deliberately carries no digest — see
/// [`App`](super::App), where the omission is argued — so whatever renders the object passes the
/// references in, and the crate's job is to name the annotation, insist the references are pinned,
/// and refuse everything else.
pub const ANNOTATION_IMAGES: &str = "dev.terrace.config/images";

/// The annotation naming which key of the object's `data` map is the configuration document.
///
/// A `ConfigMap` may carry several files, and a validator without this annotation has to guess
/// which one the contract describes. Guessing is the quiet failure this whole protocol exists to
/// remove: the wrong document validates cleanly against a schema it was never meant for, or fails
/// against one that does not describe it, and either answer is delivered with full confidence.
pub const ANNOTATION_DOCUMENT_KEY: &str = "dev.terrace.config/document-key";

/// The annotation naming the document's syntax, so a validator knows which parser to reach for.
///
/// `toml` is the only value this crate produces today. The annotation exists anyway, because the
/// design it implements already promises YAML and JSON documents that normalise to the same tree —
/// and a validator inferring the syntax from the [`ANNOTATION_DOCUMENT_KEY`] file extension would
/// be inferring it from a name a chart chose. See [`Format`].
pub const ANNOTATION_FORMAT: &str = "dev.terrace.config/format";

// There is deliberately no label or annotation carrying the loader's prefix, and none carrying the
// path the contract is embedded at. Both are facts about an *image*, and the image already
// publishes both — `dev.terrace.config.prefix` and `dev.terrace.config.contract.path`, checked
// against the document by `Contract::verify_labels`.
//
// Copying either onto a Kubernetes object creates a second spelling of a fact that already has one,
// which is exactly the drift `verify_labels` exists to catch. Here there would be nothing to catch
// it: the object is rendered by a chart in a repository this crate never sees, so the second
// spelling is written by hand, by somebody who is not looking at the first. A consumer that wants
// the prefix reads the image's config blob, which it is already reading for the version.
//
// There is deliberately no `app` label either, and that one is not a matter of taste. A document
// may be read by *several* images — the union case, which is why `ANNOTATION_IMAGES` is a list —
// so the value would have to be multi-valued, and a multi-valued label needs a separator. Every
// plausible separator (`,`, `/`, a space) is outside the character set a label value may hold.
// Anything per-image or multi-valued is an annotation, and that rule has no exceptions here.

/// What separates the references in [`ANNOTATION_IMAGES`].
///
/// A constant because both halves of the protocol have to agree on it: the crate joins with it and
/// a policy splits on it, and it is the one character a reference may not contain.
const LIST_SEPARATOR: &str = ",";

/// The longest a Kubernetes label value — or the name half of a key — may be.
const MAX_LABEL_VALUE_LEN: usize = 63;

/// The longest a DNS subdomain may be: a label key's prefix, and a `ConfigMap` data key.
const MAX_DNS_SUBDOMAIN_LEN: usize = 253;

/// The digest a pinned reference has to carry, and the length of what follows it.
const DIGEST_SEPARATOR: &str = "@sha256:";
const DIGEST_HEX_LEN: usize = 64;

/// The syntax of the document a stamped object carries.
///
/// Modelled with a fallback variant rather than as a closed set, for the reason spelled out on
/// [`CONTRACT_VERSION`](super::CONTRACT_VERSION): a consumer deserialising an unfamiliar value into
/// a closed enum makes that one value poison the whole object, and this value is a hint about which
/// parser to reach for rather than an assertion the rest of the stamp depends on. A reader that
/// does not recognise a format should **say** it skipped the document check — a silently skipped
/// check is indistinguishable from a passing one.
///
/// `#[non_exhaustive]` says the same thing to a Rust caller and does nothing at all for a
/// deserialiser, which is why both are here.
///
/// [`Self::parse`] is the constructor, and it never yields [`Self::Other`] for a name that has a
/// variant. Writing `Other("toml".to_owned())` by hand is possible and is the caller's own doing;
/// nothing in this module compares two [`Format`] values, so it changes no decision here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// TOML — what every layer in this crate reads.
    Toml,
    /// YAML.
    Yaml,
    /// JSON.
    Json,
    /// A syntax this version of the crate has no variant for.
    Other(String),
}

impl Format {
    /// The format an annotation value names.
    ///
    /// Case-sensitive. The annotation is written by a chart and read by a policy, and a rule that
    /// folds case is a rule two implementations in two languages can fold differently — so `TOML`
    /// is [`Self::Other`] rather than a second blessed spelling of [`Self::Toml`].
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "toml" => Self::Toml,
            "yaml" => Self::Yaml,
            "json" => Self::Json,
            other => Self::Other(other.to_owned()),
        }
    }

    /// This format as it is spelled in [`ANNOTATION_FORMAT`].
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

/// Which kind of object is being stamped.
///
/// An enum rather than two constructors returning one type with fields silently absent, because the
/// two stamps are not one stamp with something left out — they carry different claims, and a caller
/// who has to remember which fields apply to which object is a caller who will one day put a
/// document key on a pod.
///
/// | | [`LABEL_CONTRACT_VERSION`] | [`ANNOTATION_IMAGES`] | [`ANNOTATION_DOCUMENT_KEY`] | [`ANNOTATION_FORMAT`] |
/// |---|---|---|---|---|
/// | [`Self::Document`] | yes | yes | yes | yes |
/// | [`Self::Workload`] | yes | yes | no | no |
///
/// A pod carries the stamp at all — rather than being reached through the document it mounts —
/// because an admission webhook usually sees *only* the pod. Making it walk ownership references to
/// find the image list would make the check depend on objects that may not exist yet, at the one
/// moment the API server is waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The object holding the rendered configuration — a `ConfigMap`, or a `Secret` for the
    /// secrets-directory layer.
    Document {
        /// Which key of the object's `data` map is the document. See [`ANNOTATION_DOCUMENT_KEY`].
        key: String,
        /// The document's syntax. See [`ANNOTATION_FORMAT`].
        format: Format,
    },
    /// The pod template of the workload that mounts the document — `spec.template.metadata` of a
    /// `Deployment`, `StatefulSet` or `DaemonSet`.
    Workload,
}

impl Target {
    /// A document object whose `data` carries the configuration at `key`, in `format`.
    #[must_use]
    pub fn document(key: impl Into<String>, format: Format) -> Self {
        Self::Document {
            key: key.into(),
            format,
        }
    }
}

/// The `metadata` a stamped object carries, ready to be merged into whatever else a chart sets.
///
/// [`BTreeMap`] rather than a hash map, for [`Contract::to_json`]'s reason: the output is pasted
/// into a template, rendered, committed and diffed, so an iteration order that depended on a hash
/// seed would make two runs of one chart produce two manifests that are not byte-comparable.
//
// No `#[non_exhaustive]`, deliberately, and it is the one type here that goes without: the fields
// are private, so nothing outside this crate can construct one or match on one, and the attribute
// would announce a restriction the privacy has already made total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

impl Metadata {
    /// The `metadata.labels` entries this stamp contributes.
    ///
    /// Only this protocol's own keys. Whatever else the object carries — `app.kubernetes.io/*`, an
    /// operator's own selectors — is the chart's, and nothing here has an opinion about it.
    #[must_use]
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// The `metadata.annotations` entries this stamp contributes.
    #[must_use]
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }

    /// The block to paste into a Helm template, every line indented by `indent` spaces.
    ///
    /// Mirrors [`Contract::to_dockerfile_labels`] and exists for its reason: the manifest is
    /// hand-written in a repository this crate never sees, so hand-writing the stamp is
    /// unavoidable, and the honest answer is to make it a paste and then check the result — which
    /// is what [`Contract::verify_kube_metadata`] and [`Pairing`] are for.
    ///
    /// Keys and values are both double-quoted. Quoting the key is not decoration, since a key
    /// carries a `/`; quoting the value is what keeps `contract-version: "1"` a string rather than
    /// the integer a YAML parser would otherwise hand the API server, which refuses it.
    ///
    /// Ends with a newline, so a following line in the template needs no separator.
    ///
    /// ```text
    /// labels:
    ///   "dev.terrace.config/contract-version": "1"
    /// annotations:
    ///   "dev.terrace.config/document-key": "config.toml"
    ///   "dev.terrace.config/format": "toml"
    ///   "dev.terrace.config/images": "ghcr.io/you/portfolio@sha256:48e2…"
    /// ```
    #[must_use]
    pub fn to_yaml(&self, indent: usize) -> String {
        let pad = " ".repeat(indent);
        let mut rendered = String::new();
        for (heading, entries) in [("labels", &self.labels), ("annotations", &self.annotations)] {
            if entries.is_empty() {
                continue;
            }
            rendered.push_str(&pad);
            rendered.push_str(heading);
            rendered.push_str(":\n");
            for (key, value) in entries {
                rendered.push_str(&pad);
                rendered.push_str("  ");
                push_quoted(&mut rendered, key);
                rendered.push_str(": ");
                push_quoted(&mut rendered, value);
                rendered.push('\n');
            }
        }
        rendered
    }
}

/// Append `text` as a double-quoted YAML scalar.
///
/// Only `"` and `\` need escaping inside a double-quoted scalar, and neither can occur in anything
/// this module emits — every value has been through one of the validators below first. The escaping
/// is here anyway, on `Contract::to_dockerfile_labels`'s reasoning: a value that broke the *syntax*
/// rather than the deployment would be found by whoever pasted the block, at the worst moment to
/// find it.
fn push_quoted(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
}

impl Contract {
    /// The Kubernetes stamp for one object, given every image that reads its document.
    ///
    /// `images` is passed in rather than derived, because a contract carries no digest and must not
    /// — see [`App`](super::App). Each reference has to be **digest-pinned**: a tag can be moved
    /// after the object was rendered, so a pairing keyed on one proves nothing about the image that
    /// is actually running.
    ///
    /// Deterministic: the same contract, target and images produce byte-identical output, which is
    /// what lets a rendered manifest be diffed.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] if the stamp it would produce is not one Kubernetes accepts, or
    /// not one a policy could act on:
    ///
    /// - `images` is empty, so nothing pairs the object with a container;
    /// - a reference that is not digest-pinned, or whose digest is not 64 lower-case hexadecimal
    ///   characters;
    /// - the same reference listed twice;
    /// - a [`Target::Document`] whose key is not one a `ConfigMap` can hold;
    /// - a [`Format`] that is not a plain token.
    pub fn kube_metadata(&self, target: &Target, images: &[&str]) -> Result<Metadata, Error> {
        let mut labels = BTreeMap::new();
        let mut annotations = BTreeMap::new();

        // The keys are constants, so this can only fail if one of them is edited into something
        // Kubernetes refuses. Checked here rather than only in a unit test, for the reason
        // `verify_labels` checks a built image rather than a Dockerfile: a check that runs where
        // the value is emitted cannot be edited away by a change somewhere else in the file.
        label_key(LABEL_CONTRACT_VERSION)?;
        let version = self.terrace_contract.to_string();
        label_value(&version)?;
        labels.insert(LABEL_CONTRACT_VERSION.to_owned(), version);

        label_key(ANNOTATION_IMAGES)?;
        annotations.insert(ANNOTATION_IMAGES.to_owned(), image_list(images)?);

        if let Target::Document { key, format } = target {
            label_key(ANNOTATION_DOCUMENT_KEY)?;
            configmap_key(key)?;
            annotations.insert(ANNOTATION_DOCUMENT_KEY.to_owned(), key.clone());

            label_key(ANNOTATION_FORMAT)?;
            document_format(format)?;
            annotations.insert(ANNOTATION_FORMAT.to_owned(), format.as_str().to_owned());
        }

        Ok(Metadata {
            labels,
            annotations,
        })
    }

    /// Check that an object carries the stamp this contract expects.
    ///
    /// `labels` and `annotations` are the object's own `metadata.labels` and
    /// `metadata.annotations`. Anything this protocol did not put there is ignored — an object
    /// carries `app.kubernetes.io/*`, a chart's own selectors and whatever an operator added,
    /// exactly as [`Contract::verify_labels`] ignores the `org.opencontainers.image.*` labels
    /// around the three it checks.
    ///
    /// The one exception is [`Target::Workload`], where a document key or a format is **refused**
    /// rather than ignored. Those are not foreign keys this check has no business with; they are
    /// this protocol's own keys on an object that is not a document, claiming a document key that
    /// nothing verified against the document actually mounted. That is the second spelling this
    /// module refuses everywhere else, so it is refused here too.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first entry that is missing, wrong, or present on an
    /// object that must not carry it.
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
                     `{expected}`. One of the two was written against a different version of the \
                     protocol; re-render the chart from the contract the image publishes."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the object carries no `{LABEL_CONTRACT_VERSION}`, so nothing selecting on \
                     this protocol can see it and no policy can decide about it. \
                     `Contract::kube_metadata` emits the block a chart stamps on."
                )));
            }
        }

        // Read for its refusals: every member has to be pinned, whoever wrote the annotation. An
        // unpinned member is the one defect that makes the whole pairing decorative.
        images(annotations)?;

        match target {
            Target::Document { key, format } => {
                let found = require(annotations, ANNOTATION_DOCUMENT_KEY)?;
                configmap_key(found)?;
                if found != key {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_DOCUMENT_KEY}` is `{found}`, and the document \
                         being checked is `{key}`. A validator would read one entry of the `data` \
                         map and check it against the description of another."
                    )));
                }

                let found = require(annotations, ANNOTATION_FORMAT)?;
                document_format(&Format::parse(found))?;
                if found != format.as_str() {
                    return Err(Error::Invalid(format!(
                        "the object's `{ANNOTATION_FORMAT}` is `{found}`, and this document is \
                         `{}`. A validator would parse it with the wrong reader and report every \
                         key in the contract as missing.",
                        format.as_str()
                    )));
                }
            }
            Target::Workload => {
                for name in [ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT] {
                    if annotations.contains_key(name) {
                        return Err(Error::Invalid(format!(
                            "this object carries `{name}` and it is a workload rather than a \
                             document. A pod has no document key and no format, so the value here \
                             is a second spelling of a fact nothing checked against the document \
                             that is actually mounted. Remove it; `Target::Document` stamps the \
                             object that does have one."
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

/// The check a cluster-side actor runs: that an image, a document object and a contract describe
/// **one** configuration surface.
///
/// Five pairings, every one of which can silently come apart:
///
/// 1. the object's [`LABEL_CONTRACT_VERSION`] is present and matches the contract;
/// 2. the image's own three labels agree with the contract — [`Contract::verify_labels`], reused
///    rather than reimplemented, so the two halves of the protocol cannot drift apart;
/// 3. the object's version and the image's [`LABEL_VERSION`] agree;
/// 4. the running image is a **member** of the object's [`ANNOTATION_IMAGES`] — membership rather
///    than equality, because a document read by several binaries lists all of them;
/// 5. the document key and format are present and well-formed.
///
/// Numbered as the protocol states them, not as they run: 3 is checked before 2, because both
/// compare a version against the contract and only the earlier one can name the object and the
/// running container in the same sentence. The source says so where it happens.
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
/// # let reference = "ghcr.io/you/portfolio@sha256:\
/// #                  48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f";
/// # let image_labels: BTreeMap<String, String> = contract.labels(DEFAULT_PATH).into_iter()
/// #     .map(|(name, value)| (name.to_owned(), value)).collect();
/// let target = Target::document("config.toml", Format::Toml);
/// # let stamp = contract.kube_metadata(&target, &[reference])?;
/// // `stamp` is what the chart rendered onto the `ConfigMap`.
/// Pairing::new(&contract)
///     .image(reference, DEFAULT_PATH, &image_labels)
///     .object(&target, stamp.labels(), stamp.annotations())
///     .check()?;
/// # Ok::<(), terrace_config::Error>(())
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Pairing<'a> {
    contract: &'a Contract,
    image: Option<Image<'a>>,
    object: Option<Object<'a>>,
}

/// The image half of a [`Pairing`], as a running container reports it.
#[derive(Debug, Clone)]
struct Image<'a> {
    reference: &'a str,
    path: &'a str,
    labels: &'a BTreeMap<String, String>,
}

/// The object half of a [`Pairing`], as the API server holds it.
#[derive(Debug, Clone)]
struct Object<'a> {
    target: &'a Target,
    labels: &'a BTreeMap<String, String>,
    annotations: &'a BTreeMap<String, String>,
}

impl<'a> Pairing<'a> {
    /// A pairing against `contract`, with neither half supplied yet.
    #[must_use]
    pub fn new(contract: &'a Contract) -> Self {
        Self {
            contract,
            image: None,
            object: None,
        }
    }

    /// The running container's image: its digest-pinned reference, the path the contract was
    /// embedded at, and what `crane config` or `docker inspect` reports under `config.Labels`.
    ///
    /// `reference` is what the runtime reports — `status.containerStatuses[].imageID` — rather than
    /// what the pod spec asked for, which may be a tag. `path` is what
    /// [`Contract::verify_labels`] compares [`LABEL_PATH`](super::LABEL_PATH) against, and
    /// [`DEFAULT_PATH`](super::DEFAULT_PATH) is what a build that said nothing used.
    #[must_use]
    pub fn image(
        mut self,
        reference: &'a str,
        path: &'a str,
        labels: &'a BTreeMap<String, String>,
    ) -> Self {
        self.image = Some(Image {
            reference,
            path,
            labels,
        });
        self
    }

    /// The mounted document object: what it is, and its `metadata.labels` and
    /// `metadata.annotations`.
    #[must_use]
    pub fn object(
        mut self,
        target: &'a Target,
        labels: &'a BTreeMap<String, String>,
        annotations: &'a BTreeMap<String, String>,
    ) -> Self {
        self.object = Some(Object {
            target,
            labels,
            annotations,
        });
        self
    }

    /// Run every check.
    ///
    /// # Errors
    /// Returns [`Error::Invalid`] naming the first pairing that does not hold, both of its sides
    /// and what to do about it — or saying which half was never supplied, if [`Self::image`] or
    /// [`Self::object`] was not called.
    pub fn check(&self) -> Result<(), Error> {
        let image = self.image.as_ref().ok_or_else(|| {
            Error::Invalid(
                "this pairing has no image half, so there is nothing to pair the object with. \
                 Call `Pairing::image` with the running container's digest-pinned reference and \
                 the labels its config blob carries."
                    .to_owned(),
            )
        })?;
        let object = self.object.as_ref().ok_or_else(|| {
            Error::Invalid(
                "this pairing has no object half, so there is nothing to pair the image with. \
                 Call `Pairing::object` with the mounted document's labels and annotations."
                    .to_owned(),
            )
        })?;

        // Checks 1 and 5. First, because a stamp that is not well-formed makes every comparison
        // below an answer about a value nobody should have been reading.
        self.contract
            .verify_kube_metadata(object.target, object.labels, object.annotations)?;

        // Check 3, and it runs *before* check 2 deliberately. Both orders detect a skew — the two
        // sides are each compared against the contract, so check 2 would refuse the image's label
        // too — but only this order can name the two things whoever is holding the alert actually
        // has. A rolled image beside a `ConfigMap` nothing re-rendered is a skew between the object
        // and the container, and being told twice that one of them disagrees with a document that
        // is not in front of you is the worse half of the same fact. Run after check 2, this would
        // be unreachable rather than merely later.
        let object_version = object
            .labels
            .get(LABEL_CONTRACT_VERSION)
            .map_or("", String::as_str);
        match image.labels.get(LABEL_VERSION) {
            Some(image_version) if image_version == object_version => {}
            Some(image_version) => {
                return Err(Error::Invalid(format!(
                    "the object's `{LABEL_CONTRACT_VERSION}` is `{object_version}` and the running \
                     image's `{LABEL_VERSION}` is `{image_version}`. The two were built against \
                     different versions of this protocol: re-render the chart from the contract \
                     this image publishes, or roll the image forward to the one the chart was \
                     rendered against."
                )));
            }
            None => {
                return Err(Error::Invalid(format!(
                    "the running image carries no `{LABEL_VERSION}`, so nothing can pair it with \
                     an object claiming `{LABEL_CONTRACT_VERSION}` `{object_version}`. Either this \
                     image publishes no contract, or its labels were lost in the build — \
                     `Contract::to_dockerfile_labels` emits the block a Dockerfile pastes."
                )));
            }
        }

        // Check 2, reusing the image half rather than restating it: a second implementation of that
        // comparison is a second place for the two halves of the protocol to disagree. It is the
        // one that reaches the prefix and the embedded path, which the object deliberately does not
        // carry — see the omissions argued above the constants.
        self.contract.verify_labels(image.path, image.labels)?;

        // Check 4: membership, not equality. A document read by several binaries lists all of them,
        // so equality would refuse the union case the annotation exists for.
        digest_ref(image.reference)?;
        let listed = images(object.annotations)?;
        if !listed.contains(&image.reference) {
            return Err(Error::Invalid(format!(
                "the running image is `{}`, and the object's `{ANNOTATION_IMAGES}` lists `{}`. The \
                 document was rendered for a set of images this container is not in, so nothing \
                 here describes the configuration it is about to read. Either the chart pins a \
                 digest the workload does not run, or an image was rolled without re-rendering the \
                 document.",
                image.reference,
                listed.join(", ")
            )));
        }

        Ok(())
    }
}

/// The value [`ANNOTATION_IMAGES`] carries for `images`, refusing what a pairing cannot rest on.
fn image_list(images: &[&str]) -> Result<String, Error> {
    if images.is_empty() {
        return Err(Error::Invalid(format!(
            "no images were given for `{ANNOTATION_IMAGES}`, so nothing would pair this object \
             with a container. The annotation names every image that reads the document, and an \
             object with an empty list is one no policy can decide about."
        )));
    }

    let mut seen: Vec<&str> = Vec::with_capacity(images.len());
    for reference in images {
        digest_ref(reference)?;
        if seen.contains(reference) {
            return Err(Error::Invalid(format!(
                "`{reference}` is listed twice in `{ANNOTATION_IMAGES}`. The annotation is the set \
                 of images that read this document, and a repeated member says nothing the first \
                 one did not."
            )));
        }
        seen.push(reference);
    }

    Ok(seen.join(LIST_SEPARATOR))
}

/// The references [`ANNOTATION_IMAGES`] lists, refusing a list a pairing cannot rest on.
fn images(annotations: &BTreeMap<String, String>) -> Result<Vec<&str>, Error> {
    let value = require(annotations, ANNOTATION_IMAGES)?;
    let listed: Vec<&str> = value.split(LIST_SEPARATOR).collect();
    for reference in &listed {
        digest_ref(reference)?;
    }
    Ok(listed)
}

/// One annotation, or an error naming what a validator cannot do without it.
fn require<'a>(annotations: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, Error> {
    annotations.get(name).map(String::as_str).ok_or_else(|| {
        Error::Invalid(format!(
            "the object carries no `{name}`, so its stamp is incomplete and a validator reading it \
             would have to guess. `Contract::kube_metadata` emits every entry together, which is \
             what makes a partial stamp a rendering bug rather than a policy decision."
        ))
    })
}

/// Whether a [`Format`] is a token a policy can branch on.
///
/// Annotation values are unconstrained by the platform, so this narrowing is the crate's own and
/// worth saying why: the value names a *parser*, it is compared for equality by implementations in
/// other languages, and the label-value rule is the one character rule everything else in this
/// module is already written to. A format needing more than that is a format whose name is doing
/// something other than naming a parser.
fn document_format(format: &Format) -> Result<(), Error> {
    let value = format.as_str();
    if value.is_empty() {
        return Err(Error::Invalid(format!(
            "the document format is empty, and `{ANNOTATION_FORMAT}` is what tells a validator \
             which parser to reach for. Name the syntax — `toml`, `yaml`, `json`."
        )));
    }
    if label_value(value).is_err() {
        return Err(Error::Invalid(format!(
            "`{value}` is not a name `{ANNOTATION_FORMAT}` can carry. A format names a parser and \
             is compared for equality by policies written in other languages, so it is held to the \
             label-value character rule even though annotation values are not: at most \
             {MAX_LABEL_VALUE_LEN} letters, digits, `-`, `_` and `.`, beginning and ending with a \
             letter or a digit."
        )));
    }
    Ok(())
}

/// Whether `value` is a key Kubernetes accepts on `metadata.labels` or `metadata.annotations`.
///
/// An optional DNS-subdomain prefix of at most 253 characters, then `/`, then a name of at most 63
/// characters under [`label_value`]'s character rule. Written out by hand rather than as a regular
/// expression: four predicates do not justify the first unargued dependency in a manifest that
/// argues every one it has.
fn label_key(value: &str) -> Result<(), Error> {
    let name = match value.split_once('/') {
        None => value,
        Some((prefix, name)) => {
            if name.contains('/') {
                return Err(Error::Invalid(format!(
                    "`{value}` is not a key Kubernetes accepts: a key carries at most one `/`, \
                     separating the DNS-subdomain prefix that says who owns it from the name."
                )));
            }
            if dns_subdomain(prefix).is_err() {
                return Err(Error::Invalid(format!(
                    "`{value}` is not a key Kubernetes accepts: the part before the `/` is a DNS \
                     subdomain, so it is at most {MAX_DNS_SUBDOMAIN_LEN} characters of lower-case \
                     letters, digits, `-` and `.`, with each dot-separated part beginning and \
                     ending with a letter or a digit."
                )));
            }
            name
        }
    };

    if name.is_empty() {
        return Err(Error::Invalid(format!(
            "`{value}` is not a key Kubernetes accepts: the part after the `/` is the name, and it \
             may not be empty."
        )));
    }
    if label_value(name).is_err() {
        return Err(Error::Invalid(format!(
            "`{value}` is not a key Kubernetes accepts: its name is held to the same rule as a \
             label value — at most {MAX_LABEL_VALUE_LEN} letters, digits, `-`, `_` and `.`, \
             beginning and ending with a letter or a digit."
        )));
    }
    Ok(())
}

/// Whether `value` is a DNS subdomain: the prefix half of a label key.
fn dns_subdomain(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::Invalid(
            "a key's prefix may not be empty; write the name on its own, with no `/`.".to_owned(),
        ));
    }
    for part in value.split('.') {
        let legal = !part.is_empty()
            && part.len() <= MAX_LABEL_VALUE_LEN
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !part.starts_with('-')
            && !part.ends_with('-');
        if !legal {
            return Err(Error::Invalid(format!(
                "`{value}` is not a DNS subdomain: each dot-separated part is one to \
                 {MAX_LABEL_VALUE_LEN} lower-case letters, digits and `-`, beginning and ending \
                 with a letter or a digit."
            )));
        }
    }
    // Reached only once every part is ASCII, so the count is the same in bytes and in characters.
    if value.len() > MAX_DNS_SUBDOMAIN_LEN {
        return Err(Error::Invalid(format!(
            "a key's prefix is at most {MAX_DNS_SUBDOMAIN_LEN} characters, and this one is {}.",
            value.len()
        )));
    }
    Ok(())
}

/// Whether `value` is one Kubernetes accepts as a label value.
///
/// At most 63 characters and, unless it is empty, beginning and ending with an alphanumeric
/// character, with only `-`, `_` and `.` between. The empty string being legal is the one part of
/// this rule that reads like an oversight and is not — it is how a label is used as a pure marker.
/// No value this module emits is empty, and every caller that would refuse one refuses it by name,
/// where the message can say what is missing rather than that a character rule was broken.
fn label_value(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }
    if let Some(illegal) = value
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.'))
    {
        return Err(Error::Invalid(format!(
            "`{value}` contains `{illegal}`, which a Kubernetes label value may not hold: only \
             letters, digits, `-`, `_` and `.` are accepted. A value carrying `/`, `:` or `+` — a \
             path, an image reference, a media type — belongs in an annotation."
        )));
    }
    let ends_are_alphanumeric = value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
    if !ends_are_alphanumeric {
        return Err(Error::Invalid(format!(
            "`{value}` does not begin and end with a letter or a digit, which a Kubernetes label \
             value must. This is the rule a loader prefix such as `PORTFOLIO_` breaks, and why \
             anything shaped like one is an annotation here."
        )));
    }
    // Reached only once the value is ASCII, so the count is the same in bytes and in characters.
    if value.len() > MAX_LABEL_VALUE_LEN {
        return Err(Error::Invalid(format!(
            "`{value}` is {} characters, and a Kubernetes label value is at most \
             {MAX_LABEL_VALUE_LEN}. A value that does not fit is one the API server refuses on \
             apply, so it cannot be a label; carry it as an annotation instead.",
            value.len()
        )));
    }
    Ok(())
}

/// Whether `value` is a key a `ConfigMap` or `Secret` can hold in its `data` map.
///
/// Letters, digits, `-`, `_` and `.`, and neither `.` nor `..` — a projected volume writes each key
/// as a file, and those two are the names a directory already has.
fn configmap_key(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::Invalid(format!(
            "the document key is empty, and `{ANNOTATION_DOCUMENT_KEY}` is what tells a validator \
             which entry of the object's `data` map to read. Name the key — `config.toml`."
        )));
    }
    if value == "." || value == ".." {
        return Err(Error::Invalid(format!(
            "`{value}` is not a key a `ConfigMap` can hold: a projected volume writes each key as \
             a file, and `.` and `..` are the two names a directory already has."
        )));
    }
    if let Some(illegal) = value
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.'))
    {
        return Err(Error::Invalid(format!(
            "`{value}` contains `{illegal}`, which a `ConfigMap` data key may not hold: only \
             letters, digits, `-`, `_` and `.` are accepted. A key carrying a `/` is a path, and a \
             `data` map has no directories in it."
        )));
    }
    // Reached only once the key is ASCII, so the count is the same in bytes and in characters.
    if value.len() > MAX_DNS_SUBDOMAIN_LEN {
        return Err(Error::Invalid(format!(
            "`{value}` is {} characters, and a `ConfigMap` data key is at most \
             {MAX_DNS_SUBDOMAIN_LEN}.",
            value.len()
        )));
    }
    Ok(())
}

/// Whether `value` is an image reference a pairing can be keyed on.
///
/// **Not a reference parser**, and deliberately not one: a registry, a port, a namespace and a
/// repository have a grammar this crate has no business restating, and getting it subtly wrong
/// would refuse deployments that are correct. What it checks is the one property the whole pairing
/// rests on — that the reference names *bytes* rather than a name somebody can repoint.
fn digest_ref(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Err(Error::Invalid(format!(
            "an empty image reference cannot be paired with anything. `{ANNOTATION_IMAGES}` lists \
             every image that reads the document, each pinned to a digest."
        )));
    }
    if value.contains(LIST_SEPARATOR) {
        return Err(Error::Invalid(format!(
            "`{value}` contains `{LIST_SEPARATOR}`, which is what separates the references in \
             `{ANNOTATION_IMAGES}`. A reference carrying one would split into two members that \
             name nothing."
        )));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(Error::Invalid(format!(
            "`{value}` contains whitespace. `{ANNOTATION_IMAGES}` is compared member by member \
             with no trimming, so a reference with a space around it never matches the image a \
             runtime reports."
        )));
    }

    let Some((name, digest)) = value.split_once(DIGEST_SEPARATOR) else {
        return Err(Error::Invalid(format!(
            "`{value}` is not digest-pinned. A tag can be moved after the object was rendered, so \
             a pairing keyed on one proves nothing about the image that is actually running. Write \
             `{value}@sha256:` and the {DIGEST_HEX_LEN} hexadecimal characters that \
             `status.containerStatuses[].imageID` reports."
        )));
    };
    if name.is_empty() || name.contains('@') {
        return Err(Error::Invalid(format!(
            "`{value}` names no repository before its digest, so there is nothing for the digest \
             to be a digest *of*."
        )));
    }
    if digest.len() != DIGEST_HEX_LEN || !digest.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    {
        return Err(Error::Invalid(format!(
            "`{value}` carries a `sha256:` that is not {DIGEST_HEX_LEN} lower-case hexadecimal \
             characters. A digest that is not one is a digest no registry resolves, so the pairing \
             would fail at the one moment it had to hold."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ANNOTATION_DOCUMENT_KEY, ANNOTATION_FORMAT, ANNOTATION_IMAGES, Format,
        LABEL_CONTRACT_VERSION, NAMESPACE, configmap_key, digest_ref, document_format, label_key,
        label_value,
    };

    /// A digest of the right shape, so that a test about something else is not accidentally a test
    /// about the digest rule.
    const DIGEST: &str = "sha256:48e259cb0e5f4b3a6d1c8f97a2b4e6d0c3a5f7192b4d6e8a0c2e4f6a8b0d2e4f";

    #[test]
    fn every_key_this_module_publishes_is_one_kubernetes_accepts() {
        // The constants *are* the protocol. A typo in one is a compile error nowhere, and the
        // failure it produces is an object the API server refuses on apply — in a chart repository,
        // days later, with nothing pointing back here.
        for key in [
            LABEL_CONTRACT_VERSION,
            ANNOTATION_IMAGES,
            ANNOTATION_DOCUMENT_KEY,
            ANNOTATION_FORMAT,
        ] {
            label_key(key).unwrap_or_else(|error| panic!("`{key}`: {error}"));
            assert!(
                key.starts_with(&format!("{NAMESPACE}/")),
                "`{key}` is outside this protocol's namespace, so a selector on the namespace \
                 would not find the objects carrying it"
            );
        }
    }

    #[test]
    fn a_label_key_carries_at_most_one_slash_and_a_dns_subdomain_before_it() {
        assert!(label_key("dev.terrace.config/contract-version").is_ok());
        assert!(label_key("contract-version").is_ok());

        assert!(label_key("dev.terrace.config/a/b").is_err());
        assert!(label_key("/contract-version").is_err());
        assert!(label_key("dev.terrace.config/").is_err());
        assert!(label_key("dev..config/version").is_err());
        assert!(label_key("-dev.config/version").is_err());
        // Upper case is legal in the *name* and illegal in the prefix. That asymmetry is the one
        // part of the key rule a reader is most likely to get wrong from memory.
        assert!(label_key("Dev.Terrace.Config/version").is_err());
        assert!(label_key("dev.terrace.config/Version").is_ok());
    }

    #[test]
    fn a_label_value_is_precisely_what_the_image_labels_are_not() {
        // The three values the image half publishes, and the reason not one of them is a label.
        assert!(label_value("PORTFOLIO_").is_err());
        assert!(label_value("/config/contract.json").is_err());
        assert!(label_value("application/vnd.terrace.config-schema.v1+json").is_err());

        assert!(label_value("1").is_ok());
        assert!(label_value("v2.5.0").is_ok());
        // The empty value is legal, and every caller here still refuses one by name.
        assert!(label_value("").is_ok());
        assert!(label_value(&"a".repeat(63)).is_ok());
        assert!(label_value(&"a".repeat(64)).is_err());
        assert!(label_value(".leading").is_err());
        assert!(label_value("trailing.").is_err());
    }

    #[test]
    fn a_document_key_is_a_name_a_projected_volume_can_write() {
        assert!(configmap_key("config.toml").is_ok());
        assert!(configmap_key("00-base.toml").is_ok());
        assert!(configmap_key("_").is_ok());
        // A leading dot is legal: `..data` is the entry a projected volume reserves, and it is
        // reserved by being that exact name rather than by beginning with a dot.
        assert!(configmap_key(".hidden.toml").is_ok());

        assert!(configmap_key("").is_err());
        assert!(configmap_key(".").is_err());
        assert!(configmap_key("..").is_err());
        assert!(configmap_key("conf/ig.toml").is_err());
        assert!(configmap_key("config toml").is_err());
        assert!(configmap_key(&"a".repeat(254)).is_err());
    }

    #[test]
    fn a_reference_that_is_not_pinned_is_refused() {
        assert!(digest_ref(&format!("ghcr.io/you/portfolio@{DIGEST}")).is_ok());
        assert!(digest_ref(&format!("portfolio@{DIGEST}")).is_ok());
        assert!(digest_ref(&format!("ghcr.io:5000/you/portfolio:v2.5.0@{DIGEST}")).is_ok());

        // The one that matters: a tag can be repointed after the object was rendered.
        let error = digest_ref("ghcr.io/you/portfolio:v2.5.0").expect_err("refused");
        assert!(error.to_string().contains("moved"), "{error}");

        assert!(digest_ref("").is_err());
        assert!(digest_ref(&format!("@{DIGEST}")).is_err());
        assert!(digest_ref(&format!("a@b@{DIGEST}")).is_err());
        assert!(digest_ref("portfolio@sha256:abc").is_err());
        assert!(digest_ref(&format!("portfolio@{}", DIGEST.to_uppercase())).is_err());
        assert!(digest_ref(&format!("portfolio@{DIGEST},other@{DIGEST}")).is_err());
        assert!(digest_ref(&format!(" portfolio@{DIGEST}")).is_err());
        // `sha512` is a digest algorithm and not this one. Nothing in the protocol says how two
        // references under different algorithms compare, so only the one is accepted.
        assert!(digest_ref("portfolio@sha512:0123456789abcdef").is_err());
    }

    #[test]
    fn an_unfamiliar_format_is_kept_rather_than_refused() {
        assert_eq!(Format::parse("toml"), Format::Toml);
        assert_eq!(Format::parse("hcl"), Format::Other("hcl".to_owned()));
        assert_eq!(Format::Other("hcl".to_owned()).as_str(), "hcl");
        // Not folded: `TOML` is a spelling nothing emits, and reading it as `Toml` would bless a
        // second one that two implementations could then disagree about.
        assert_eq!(Format::parse("TOML"), Format::Other("TOML".to_owned()));

        assert!(document_format(&Format::Toml).is_ok());
        assert!(document_format(&Format::Other("hcl".to_owned())).is_ok());
        assert!(document_format(&Format::Other(String::new())).is_err());
        assert!(document_format(&Format::Other("to ml".to_owned())).is_err());
        assert!(document_format(&Format::Other("text/toml".to_owned())).is_err());
    }
}
