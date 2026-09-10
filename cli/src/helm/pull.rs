//! Refreshing every vendored contract from the image its chart pins.
//!
//! The one thing here that talks to a container registry. Everything downstream reads the committed
//! file, so a registry outage cannot fail a pull request that changes no image, a re-run on a
//! six-month-old commit validates against what was true then, and the contract diff lands in the
//! *same* pull request as the digest bump — which is the whole point. A person reviewing a digest
//! bump sees the removed key next to the chart's failing gate.
//!
//! Per chart, per image a declaration names:
//!
//! 1. resolve the values path to `registry/repository:tag@digest`, normalising a registry-less
//!    Docker Hub name — a registry client reads a bare first segment as a hostname;
//! 2. verify the *image*'s signature against the expected signer identity;
//! 3. read the image config's labels, through the index when the pinned digest is one and requiring
//!    every platform to agree: the version label must name an envelope this build reads, and the
//!    prefix label is kept for step 5;
//! 4. read the document from **both carriers**, whichever are present — the OCI referrer attached
//!    to the digest, whose blob is checked against the digest its descriptor claims, and the file
//!    inside the image at the path the label names, read by walking the layer blobs. At least one
//!    must be there; if both are, they must be byte-identical;
//! 5. assert the image's prefix label equals the fetched document's dialect prefix: the label is
//!    what a consumer discovers the image by, and a label naming one namespace over a document
//!    describing another is an image whose two halves came from different builds;
//! 6. write the vendored file — the published bytes inside a `source` envelope recording which
//!    digest they were fetched for.
//!
//! # Why both carriers
//!
//! The format names the referrer as canonical and the embedded file as the fallback for registries
//! with no referrers API. Measured against a real image, the embedded file is the stronger of the
//! two: it lives in a layer whose digest is in a manifest whose digest is what the chart pins, so a
//! document read from it is provably *inside* the image — while an attached artifact is a separate
//! object that merely points at the same digest. The referrer is still tried first, because it is
//! one request rather than a layer walk. Neither is trusted alone when both exist.
//!
//! There is deliberately no path to "fetch it unverified". A contract that cannot be proven to
//! belong to the pinned digest is worse than none, because every gate downstream would trust it —
//! and the document is untrusted input until step 6 finishes: it is a JSON blob from a registry.
//!
//! The tie between a contract and an image is the *attachment*, not a field. The published document
//! carries no digest and cannot: a digest is what building an image produces, so a field holding it
//! would have to be written after the push, changing the bytes the signature was computed over.
//! Whatever comes back from asking a digest for its referrers belongs to that digest; step 6 records
//! which digest that was, for the offline staleness interlock to read.
//!
//! # The network is behind one seam
//!
//! [`Registry`] is the only thing that leaves this machine and every call goes through it, so
//! everything above is testable against recorded shapes — including the two things only a real image
//! revealed: the pinned digest being an index rather than a manifest, and the document arriving
//! through the embedded file rather than through an attachment.
//!
//! Shelling out to `oras` and `cosign` rather than linking a registry client is deliberate. Both are
//! already pinned and installed by a chart repository's own tooling; linking one would carry a TLS
//! stack, a credential-helper protocol and a signature-verification implementation into a binary
//! whose other twelve subcommands touch no network at all.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value as Json};
use sha2::{Digest, Sha256};

use crate::document::{CONTRACT_VERSION, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION};
use crate::error::Error;

use super::declaration::read_yaml;
use super::{DECLARATION, dig};

/// The artifact type a published contract is attached under.
pub const ARTIFACT_TYPE: &str = "application/vnd.terrace.config-schema.v1+json";

/// The OIDC issuer a hosted CI workflow signs under.
///
/// Paired with the signer identity, which names the workflow itself, so a signature from any other
/// workflow — in any other repository — is refused rather than accepted as "signed".
pub const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// Every call that leaves this machine, behind one seam.
pub trait Registry {
    /// The digest a reference resolves to, so a tag is never trusted twice.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the registry cannot be asked, or refuses.
    fn resolve(&self, reference: &str) -> Result<String, Error>;

    /// The referrers of one digest with one artifact type.
    ///
    /// # Errors
    /// As [`Self::resolve`].
    fn discover(&self, reference: &str, artifact_type: &str) -> Result<Vec<Json>, Error>;

    /// The labels on the image config blob, which is where discovery starts.
    ///
    /// # Errors
    /// As [`Self::resolve`].
    fn image_labels(&self, reference: &str) -> Result<BTreeMap<String, String>, Error>;

    /// One manifest, index or image.
    ///
    /// # Errors
    /// As [`Self::resolve`].
    fn manifest(&self, reference: &str) -> Result<Json, Error>;

    /// One blob, whole.
    ///
    /// # Errors
    /// As [`Self::resolve`].
    fn blob(&self, reference: &str) -> Result<Vec<u8>, Error>;

    /// Refuse anything not signed by the named identity.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the signature is absent, or belongs to somebody else.
    fn verify(&self, reference: &str, identity: &str) -> Result<(), Error>;

    /// The per-platform image manifests behind a digest, or the digest itself.
    ///
    /// A multi-architecture image is pushed as an index, and the index is what a chart pins — so the
    /// digest in a values file is usually *not* an image manifest and has no config blob to carry
    /// labels. The real manifests are one level down.
    ///
    /// Build attestations ride in the same index as entries with no platform; they are not images
    /// and carry none of this, so they are skipped.
    ///
    /// # Errors
    /// As [`Self::manifest`].
    fn platforms(&self, repository: &str, digest: &str) -> Result<Vec<String>, Error> {
        let manifest = self.manifest(&format!("{repository}@{digest}"))?;
        let Some(entries) = manifest
            .get("manifests")
            .and_then(Json::as_array)
            .filter(|held| !held.is_empty())
        else {
            return Ok(vec![digest.to_owned()]);
        };

        let found: Vec<String> = entries
            .iter()
            .filter(|entry| {
                !matches!(
                    entry
                        .get("platform")
                        .and_then(|platform| platform.get("os"))
                        .and_then(Json::as_str),
                    None | Some("unknown")
                )
            })
            .filter_map(|entry| entry.get("digest").and_then(Json::as_str))
            .map(str::to_owned)
            .collect();
        Ok(if found.is_empty() {
            vec![digest.to_owned()]
        } else {
            found
        })
    }
}

/// The two pinned binaries a chart repository already installs.
#[derive(Debug, Clone)]
pub struct Tools {
    oras: PathBuf,
    cosign: PathBuf,
}

impl Tools {
    /// Find both, from the environment or from the path.
    ///
    /// # Errors
    /// [`Error::Invalid`] naming whichever is missing, and how to supply it.
    pub fn found() -> Result<Self, Error> {
        let held = |name: &str, variable: &str| -> Result<PathBuf, Error> {
            if let Ok(named) = std::env::var(variable) {
                return Ok(PathBuf::from(named));
            }
            which(name).ok_or_else(|| {
                Error::Invalid(format!(
                    "{name} is not on PATH. Refreshing contracts is the only thing here that \
                     talks to a registry and it needs both oras and cosign; install the pinned \
                     versions, or set {variable}."
                ))
            })
        };
        Ok(Self {
            oras: held("oras", "ORAS_BIN")?,
            cosign: held("cosign", "COSIGN_BIN")?,
        })
    }

    fn run(binary: &Path, argv: &[&str]) -> Result<Vec<u8>, Error> {
        let name = binary
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("the tool");
        let output = Command::new(binary)
            .args(argv)
            .output()
            .map_err(|failure| Error::io(binary.display(), failure))?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        let detail = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        Err(Error::Invalid(format!(
            "{name} {} failed: {}",
            argv.join(" "),
            String::from_utf8_lossy(detail).trim()
        )))
    }
}

impl Registry for Tools {
    fn resolve(&self, reference: &str) -> Result<String, Error> {
        let out = Self::run(&self.oras, &["resolve", reference])?;
        Ok(String::from_utf8_lossy(&out).trim().to_owned())
    }

    fn discover(&self, reference: &str, artifact_type: &str) -> Result<Vec<Json>, Error> {
        let out = Self::run(
            &self.oras,
            &[
                "discover",
                "--format",
                "json",
                "--artifact-type",
                artifact_type,
                reference,
            ],
        )?;
        parse_referrers(&String::from_utf8_lossy(&out))
    }

    fn image_labels(&self, reference: &str) -> Result<BTreeMap<String, String>, Error> {
        let out = Self::run(&self.oras, &["manifest", "fetch-config", reference])?;
        let config: Json = serde_json::from_slice(&out)
            .map_err(|failure| Error::Invalid(format!("{reference}: {failure}")))?;
        Ok(config
            .get("config")
            .and_then(|held| held.get("Labels"))
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| {
                value.as_str().map(|value| (name.clone(), value.to_owned()))
            })
            .collect())
    }

    fn manifest(&self, reference: &str) -> Result<Json, Error> {
        let out = Self::run(&self.oras, &["manifest", "fetch", reference])?;
        serde_json::from_slice(&out)
            .map_err(|failure| Error::Invalid(format!("{reference}: {failure}")))
    }

    fn blob(&self, reference: &str) -> Result<Vec<u8>, Error> {
        Self::run(&self.oras, &["blob", "fetch", "--output", "-", reference])
    }

    fn verify(&self, reference: &str, identity: &str) -> Result<(), Error> {
        Self::run(
            &self.cosign,
            &[
                "verify",
                "--certificate-identity-regexp",
                identity,
                "--certificate-oidc-issuer",
                OIDC_ISSUER,
                reference,
            ],
        )?;
        Ok(())
    }
}

/// One executable on the path, however the platform spells one.
fn which(name: &str) -> Option<PathBuf> {
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .ok()
        .map(|held| held.split(';').map(str::to_lowercase).collect())
        .unwrap_or_default();
    for directory in std::env::split_paths(&std::env::var_os("PATH")?) {
        let bare = directory.join(name);
        if bare.is_file() {
            return Some(bare);
        }
        for extension in &extensions {
            let held = directory.join(format!("{name}{extension}"));
            if held.is_file() {
                return Some(held);
            }
        }
    }
    None
}

/// Read a referrers listing, whose top-level key has changed across releases.
///
/// # Errors
/// [`Error::Invalid`] when the output is not JSON at all.
pub fn parse_referrers(text: &str) -> Result<Vec<Json>, Error> {
    let document: Json = serde_json::from_str(text).map_err(|failure| {
        Error::Invalid(format!("the referrer listing is not JSON: {failure}"))
    })?;
    if let Json::Array(entries) = document {
        return Ok(entries);
    }
    for key in ["referrers", "manifests"] {
        if let Some(entries) = document.get(key).and_then(Json::as_array) {
            return Ok(entries.clone());
        }
    }
    Ok(Vec::new())
}

/// Fetch and verify one image's contract. Returns the published bytes and their hash.
///
/// Two things are proven here, and they answer different questions. The blob is what its descriptor
/// says it is — a registry content-addresses its blobs, so hashing what arrived and comparing is the
/// whole integrity check. And the image and the document belong together: the prefix label is how a
/// consumer discovers that the image publishes a contract at all, so a label naming one namespace
/// over a document describing another is an image whose two halves came from different builds, and
/// vendoring it would tie a chart to a contract for something else.
///
/// Neither is a fallback. A contract that cannot be proven to belong to the pinned digest is worse
/// than none, because every gate downstream would trust it.
///
/// # Errors
/// [`Error::Invalid`] with the sentence describing which of the proofs failed.
pub fn fetch_contract(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
    signer: &str,
) -> Result<(Vec<u8>, String), Error> {
    let pinned = format!("{repository}@{digest}");
    registry.verify(&pinned, signer)?;

    let labels = agreed_labels(registry, repository, digest)?;
    let Some(version) = labels.get(LABEL_VERSION) else {
        return Err(Error::Invalid(format!(
            "{pinned} carries no {LABEL_VERSION} label, so it publishes no contract"
        )));
    };
    if version != &CONTRACT_VERSION.to_string() {
        return Err(Error::Invalid(format!(
            "{pinned} publishes contract version {version}, and this build reads {CONTRACT_VERSION}"
        )));
    }

    let path = labels.get(LABEL_PATH).map(String::as_str);
    let attached = fetch_attached(registry, repository, digest)?;
    let embedded = fetch_embedded(registry, repository, digest, path)?;

    let payload = match (&attached, &embedded) {
        (None, None) => {
            return Err(Error::Invalid(format!(
                "{pinned} carries the contract labels but publishes no document: nothing is \
                 attached with artifact type {ARTIFACT_TYPE}, and there is no file at {} inside \
                 the image. The build set the labels without running the attach step or the copy \
                 that embeds the document.",
                crate::gate::quoted(path.unwrap_or_default())
            )));
        }
        (Some(attached), Some(embedded)) if attached != embedded => {
            return Err(Error::Invalid(format!(
                "{pinned} publishes two different documents: the artifact attached to the digest \
                 and the file at {} do not match, so the image and its attachment came from \
                 different builds",
                path.unwrap_or_default()
            )));
        }
        (Some(held), _) | (None, Some(held)) => held.clone(),
    };

    let document: Json = serde_json::from_slice(&payload).map_err(|failure| {
        Error::Invalid(format!(
            "{pinned} published a document that is not JSON: {failure}"
        ))
    })?;
    let described = document
        .get("schema")
        .and_then(|schema| schema.get("dialect"))
        .and_then(|dialect| dialect.get("prefix"))
        .and_then(Json::as_str);
    let declared = labels.get(LABEL_PREFIX).map(String::as_str);
    if declared != described {
        return Err(Error::Invalid(format!(
            "{pinned} is labelled {LABEL_PREFIX}={} but the contract attached to it describes the \
             namespace {}: the image and its contract came from different builds",
            crate::gate::quoted(declared.unwrap_or_default()),
            crate::gate::quoted(described.unwrap_or_default())
        )));
    }

    Ok((payload.clone(), hex_sha256(&payload)))
}

/// The contract labels, read through the index when the pinned digest is one.
///
/// Every platform of one image is built from one recipe and must therefore carry the same labels.
/// They are all read and required to agree rather than trusting the first: two architectures
/// disagreeing about which namespace the binary reads is a build that produced two different
/// programs under one tag, and vendoring either one would tie the chart to a contract that is wrong
/// for half its nodes.
///
/// # Errors
/// [`Error::Invalid`] naming the two platforms that disagree.
pub fn agreed_labels(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
) -> Result<BTreeMap<String, String>, Error> {
    let mut agreed: Option<(String, BTreeMap<String, String>)> = None;
    for platform in registry.platforms(repository, digest)? {
        let labels: BTreeMap<String, String> = registry
            .image_labels(&format!("{repository}@{platform}"))?
            .into_iter()
            .filter(|(name, _)| name.starts_with("dev.terrace.config."))
            .collect();
        match &agreed {
            None => agreed = Some((platform, labels)),
            Some((first, held)) if held != &labels => {
                return Err(Error::Invalid(format!(
                    "{repository}@{digest} carries different contract labels per platform: \
                     {first} says {} and {platform} says {}",
                    Json::from_iter(held.clone()),
                    Json::from_iter(labels)
                )));
            }
            Some(_) => {}
        }
    }
    Ok(agreed.map(|(_, labels)| labels).unwrap_or_default())
}

/// The contract attached to a digest, looked for on the index and on each platform.
///
/// Which of the two a producer attaches to is a real choice — the index is what a chart pins, a
/// platform manifest is what actually runs — and a consumer that only looked at one would report "no
/// contract" for an image that publishes one perfectly well. The index is tried first because that
/// is the digest everything else is tied to.
///
/// # Errors
/// As [`Registry::discover`].
pub fn discover_contract(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
) -> Result<Vec<Json>, Error> {
    let referrers = registry.discover(&format!("{repository}@{digest}"), ARTIFACT_TYPE)?;
    if !referrers.is_empty() {
        return Ok(referrers);
    }
    for platform in registry.platforms(repository, digest)? {
        if platform == digest {
            continue;
        }
        let referrers = registry.discover(&format!("{repository}@{platform}"), ARTIFACT_TYPE)?;
        if !referrers.is_empty() {
            return Ok(referrers);
        }
    }
    Ok(Vec::new())
}

/// The document attached to a digest as an OCI referrer, or [`None`] if nothing is.
///
/// # Errors
/// [`Error::Invalid`] when there is more than one, when its manifest is not one layer, or when the
/// blob does not hash to the digest its descriptor claims.
pub fn fetch_attached(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
) -> Result<Option<Vec<u8>>, Error> {
    let referrers = discover_contract(registry, repository, digest)?;
    let Some(first) = referrers.first() else {
        return Ok(None);
    };
    if referrers.len() > 1 {
        return Err(Error::Invalid(format!(
            "{repository}@{digest} has {} referrers of type {ARTIFACT_TYPE}, expected one",
            referrers.len()
        )));
    }

    let attached = first
        .get("digest")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let manifest = registry.manifest(&format!("{repository}@{attached}"))?;
    let layers: Vec<&Json> = manifest
        .get("layers")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .collect();
    if layers.len() != 1 {
        return Err(Error::Invalid(format!(
            "the contract attached to {repository}@{digest} has {} layers, expected one",
            layers.len()
        )));
    }

    let claimed = layers[0]
        .get("digest")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let payload = registry.blob(&format!("{repository}@{claimed}"))?;
    let actual = hex_sha256(&payload);
    if claimed != format!("sha256:{actual}") {
        return Err(Error::Invalid(format!(
            "the contract attached to {repository}@{digest} hashes to sha256:{actual}, but its \
             descriptor claims {claimed}"
        )));
    }
    Ok(Some(payload))
}

/// The document from inside the image, at the path the label names.
///
/// Read by walking the layer blobs from the top down, because that is the order a filesystem
/// resolves them: the last layer to write a path is the one the container sees. The first hit wins
/// and the walk stops, so the usual case — the document copied in a late, tiny layer — costs one
/// small blob rather than the whole image.
///
/// No container runtime and no local daemon: the images this validates may be built from scratch
/// with no shell, and requiring one for a step that otherwise needs two static binaries would put it
/// out of reach of the pull request that needs it most.
///
/// # Errors
/// As [`Registry::manifest`] and [`Registry::blob`].
pub fn fetch_embedded(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
    path: Option<&str>,
) -> Result<Option<Vec<u8>>, Error> {
    let Some(wanted) = path
        .map(|held| held.trim_start_matches('/'))
        .filter(|held| !held.is_empty())
    else {
        return Ok(None);
    };

    for platform in registry.platforms(repository, digest)? {
        let manifest = registry.manifest(&format!("{repository}@{platform}"))?;
        let layers: Vec<&Json> = manifest
            .get("layers")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .collect();
        for layer in layers.iter().rev() {
            let held = layer
                .get("digest")
                .and_then(Json::as_str)
                .unwrap_or_default();
            let blob = registry.blob(&format!("{repository}@{held}"))?;
            if let Some(found) = read_from_layer(&blob, wanted) {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

/// One file out of one layer blob, or [`None`] if this layer does not hold it.
///
/// A layer this cannot read is not an error: the file may be in another one, and a document that is
/// nowhere is reported by the caller rather than here.
#[must_use]
pub fn read_from_layer(blob: &[u8], wanted: &str) -> Option<Vec<u8>> {
    let mut plain: Vec<u8> = Vec::new();
    let body: &[u8] = if blob.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(blob)
            .read_to_end(&mut plain)
            .ok()?;
        &plain
    } else {
        blob
    };

    let mut archive = tar::Archive::new(body);
    for entry in archive.entries().ok()? {
        let mut entry = entry.ok()?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().ok()?.to_string_lossy().replace('\\', "/");
        if path.trim_start_matches("./") != wanted {
            continue;
        }
        let mut found = Vec::new();
        entry.read_to_end(&mut found).ok()?;
        return Some(found);
    }
    None
}

/// One payload's hash, spelled the way a descriptor spells it.
fn hex_sha256(payload: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut held = Sha256::new();
    held.update(payload);
    held.finalize().iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Write the wrapper, and report whether the file changed.
///
/// `source.sha256` is over the document as published — the same value the registry addressed the
/// blob by, minus the prefix. It is deliberately *not* recomputable from this file: the published
/// bytes do not survive being re-serialised into the wrapper. That is a provenance record for a
/// later networked run to check, not an offline integrity check, and the offline trust anchor is
/// that this file was reviewed in the pull request that introduced it — which is the pull request
/// that also carries the digest bump, by design.
///
/// # Errors
/// [`Error::Invalid`] when the payload is not a JSON document, [`Error::Io`] when it cannot be
/// written.
///
/// # Panics
/// Never: the wrapper is built here out of owned values a moment before it is serialised.
pub fn write_vendored(
    path: &Path,
    image: &str,
    digest: &str,
    payload: &[u8],
    sha256: &str,
    fetched: &str,
) -> Result<bool, Error> {
    let contract: Json = serde_json::from_slice(payload)
        .map_err(|failure| Error::Invalid(format!("{}: {failure}", path.display())))?;
    let mut source = Map::new();
    source.insert("image".to_owned(), Json::String(image.to_owned()));
    source.insert("digest".to_owned(), Json::String(digest.to_owned()));
    source.insert("sha256".to_owned(), Json::String(sha256.to_owned()));
    source.insert("fetched".to_owned(), Json::String(fetched.to_owned()));
    let mut wrapper = Map::new();
    wrapper.insert("source".to_owned(), Json::Object(source));
    wrapper.insert("contract".to_owned(), contract);
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&Json::Object(wrapper))
            .expect("a wrapper of owned values serialises")
    );

    if let Ok(previous) = std::fs::read_to_string(path)
        && same_contract(&previous, &rendered)
    {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
    }
    std::fs::write(path, rendered).map_err(|e| Error::io(path.display(), e))?;
    Ok(true)
}

/// Whether two wrappers differ in anything but when they were fetched.
///
/// `fetched` moves on every run by construction. Rewriting the file for it alone would put a commit
/// on every scheduled run — noise in exactly the place a real contract change is supposed to stand
/// out.
fn same_contract(previous: &str, rendered: &str) -> bool {
    let held = |text: &str| -> Option<Json> {
        let mut document: Json = serde_json::from_str(text).ok()?;
        if let Some(source) = document.get_mut("source").and_then(Json::as_object_mut) {
            source.remove("fetched");
        }
        Some(document)
    };
    match (held(previous), held(rendered)) {
        (Some(old), Some(new)) => old == new,
        _ => false,
    }
}

/// A chart's spelling of an image, in the fully qualified form.
///
/// A chart writes a bare Docker Hub name because that is what a templating engine and a container
/// CLI accept; a registry client reads a bare first segment as a hostname and tries to resolve it by
/// DNS. Signature verification normalises and the registry client does not, so passing the chart's
/// spelling around would have the two disagree about which image was verified.
#[must_use]
pub fn normalized_image(registry: &str, repository: &str) -> String {
    format!(
        "{}/{repository}",
        if registry.is_empty() {
            "docker.io"
        } else {
            registry
        }
    )
}

/// `(repository, tagged reference, inline digest)` for one image block.
///
/// # Errors
/// [`Error::Invalid`] when the block names no repository.
pub fn reference_for(
    image: &Json,
    app_version: Option<&str>,
) -> Result<(String, String, Option<String>), Error> {
    let text = |name: &str| image.get(name).and_then(Json::as_str).unwrap_or_default();
    let repository = text("repository");
    if repository.is_empty() {
        return Err(Error::Invalid("image block has no `repository`".to_owned()));
    }
    let tag = match text("tag") {
        "" => app_version.unwrap_or_default(),
        held => held,
    };

    let normalized = normalized_image(text("registry"), repository);
    let reference = if tag.is_empty() {
        normalized.clone()
    } else {
        format!("{normalized}:{tag}")
    };
    let digest = tag.split_once('@').map(|(_, held)| held.to_owned());
    Ok((normalized, reference, digest))
}

/// What one chart's refresh did.
#[derive(Debug, Default)]
pub struct Refreshed {
    /// Vendored files whose bytes moved.
    pub changed: Vec<PathBuf>,
    /// Vendored files that were already current.
    pub unchanged: Vec<PathBuf>,
    /// Charts that could not be refreshed, and why.
    pub problems: Vec<String>,
}

/// Refresh every contract one chart declares.
///
/// # Errors
/// [`Error::Invalid`] when the chart's own files cannot be read, or an image cannot be proven to
/// publish the contract vendored for it, [`Error::Io`] when a file cannot be written.
pub fn refresh_chart(
    chart_dir: &Path,
    registry: &dyn Registry,
    signer: &str,
    fetched: &str,
    into: &mut Refreshed,
) -> Result<(), Error> {
    let read = |name: &str| -> Result<Json, Error> {
        let path = chart_dir.join(name);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
        read_yaml(&text, &path)
    };
    let (declaration, values, meta) = (
        read(DECLARATION)?,
        read("values.yaml")?,
        read("Chart.yaml")?,
    );
    let app_version = meta.get("appVersion").and_then(Json::as_str);

    let mut seen: Vec<PathBuf> = Vec::new();
    for document in declaration
        .get("documents")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        for entry in document
            .get("images")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let contract = entry
                .get("contract")
                .and_then(Json::as_str)
                .unwrap_or_default();
            let target = chart_dir.join(contract);
            if seen.contains(&target) {
                continue;
            }
            seen.push(target.clone());

            let at = entry
                .get("values")
                .and_then(Json::as_str)
                .unwrap_or_default();
            let Some(image) = dig(&values, at).filter(|held| held.is_object()) else {
                return Err(Error::Invalid(format!(
                    "{}: values path {} resolves to no image block",
                    chart_dir.join(DECLARATION).display(),
                    crate::gate::quoted(at)
                )));
            };

            let (normalized, reference, digest) = reference_for(image, app_version)?;
            let digest = match digest {
                Some(held) => held,
                // Resolving a tag is a second network call and a second answer; a chart that pins
                // by digest never reaches this, so it is a fallback rather than the path.
                None => registry.resolve(&reference)?,
            };

            let (payload, sha256) = fetch_contract(registry, &normalized, &digest, signer)?;
            if write_vendored(&target, &normalized, &digest, &payload, &sha256, fetched)? {
                into.changed.push(target);
            } else {
                into.unchanged.push(target);
            }
        }
    }
    Ok(())
}

/// Refresh every chart that declares a contract, or one of them.
///
/// Every chart is attempted before this returns: one image that cannot be reached must not hide the
/// state of the rest.
///
/// # Errors
/// [`Error::Io`] when the chart tree cannot be walked. A chart that could not be refreshed is a
/// problem on the result rather than an error.
pub fn refresh(
    charts: &Path,
    only: Option<&str>,
    registry: &dyn Registry,
    signer: &str,
    fetched: &str,
) -> Result<Refreshed, Error> {
    let mut refreshed = Refreshed::default();
    for chart_dir in super::declaration::chart_dirs(charts)? {
        let name = chart_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned();
        if only.is_some_and(|wanted| wanted != name) || !chart_dir.join(DECLARATION).is_file() {
            continue;
        }
        if let Err(failure) = refresh_chart(&chart_dir, registry, signer, fetched, &mut refreshed) {
            refreshed.problems.push(format!("{name}: {failure}"));
        }
    }
    Ok(refreshed)
}

/// The moment a refresh happened, spelled the way a vendored file records it.
#[must_use]
pub fn now() -> String {
    civil(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |held| held.as_secs()),
    )
}

/// One count of seconds since the epoch, as a calendar instant.
///
/// Written out rather than taken from a crate: this is the only place here that needs a calendar,
/// what it produces is a provenance record rather than an input to any rule, and the algorithm is a
/// dozen lines of integer arithmetic with no time zones and no leap seconds in it.
fn civil(seconds: u64) -> String {
    let (days, rest) = (
        i64::try_from(seconds / 86_400).unwrap_or(0),
        seconds % 86_400,
    );
    let era_days = days + 719_468;
    let era = era_days.div_euclid(146_097);
    let day_of_era = era_days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// A copy of a chart tree that `--check` may write into, removed when it goes out of scope.
///
/// `--check` answers whether the committed bytes are the ones the pinned digest publishes, and the
/// only honest way to answer it is to fetch and write and compare. Writing into the chart's own tree
/// would make the answer true by construction; writing into a copy leaves the working tree exactly
/// as it was, whatever the answer turns out to be.
///
/// Only the files a refresh reads are copied — the declaration, the values, the chart metadata and
/// the vendored contracts — because that is what keeps a check over ten charts from copying ten
/// template trees to compare fifteen JSON files.
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    /// Take a copy.
    ///
    /// # Errors
    /// [`Error::Io`] when the tree cannot be read, or the copy cannot be written.
    pub fn of(charts: &Path) -> Result<Self, Error> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let at = std::env::temp_dir().join(format!(
            "terrace-pull-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&at);

        for chart_dir in super::declaration::chart_dirs(charts)? {
            let Some(name) = chart_dir.file_name() else {
                continue;
            };
            let into = at.join(name);
            for file in [DECLARATION, "values.yaml", "Chart.yaml"] {
                copy_into(&chart_dir.join(file), &into.join(file))?;
            }
            let contracts = chart_dir.join("contracts");
            let Ok(entries) = std::fs::read_dir(&contracts) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(named) = entry.path().file_name().map(std::ffi::OsStr::to_os_string)
                else {
                    continue;
                };
                copy_into(&entry.path(), &into.join("contracts").join(named))?;
            }
        }
        Ok(Self(at))
    }

    /// The copied chart tree.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One file into the copy, skipped when it is not there.
fn copy_into(from: &Path, to: &Path) -> Result<(), Error> {
    if !from.is_file() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
    }
    std::fs::copy(from, to).map_err(|e| Error::io(from.display(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use serde_json::{Value as Json, json};

    use super::{
        ARTIFACT_TYPE, Registry, agreed_labels, fetch_attached, fetch_contract, normalized_image,
        now, parse_referrers, read_from_layer, reference_for, write_vendored,
    };
    use crate::document::{LABEL_PATH, LABEL_PREFIX, LABEL_VERSION};
    use crate::error::Error;

    const IMAGE: &str = "docker.io/demo/api";
    const INDEX: &str = "sha256:aa";
    const AMD64: &str = "sha256:bb";
    const ARM64: &str = "sha256:cc";
    const SIGNER: &str = "https://example.invalid/workflow";

    /// One published contract, small enough to read in a failure message.
    fn contract() -> Json {
        json!({
            "terrace_contract": 1,
            "app": {"name": "api", "version": "1.0.0"},
            "schema": {
                "schema_version": 2,
                "dialect": {
                    "prefix": "API_",
                    "nesting_separator": "__",
                    "indirection_suffix": "_FILE",
                },
                "loader": [],
                "keys": [],
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        })
    }

    fn published() -> Vec<u8> {
        serde_json::to_vec(&contract()).expect("it serialises")
    }

    fn digest_of(payload: &[u8]) -> String {
        format!("sha256:{}", super::hex_sha256(payload))
    }

    /// A registry that answers from a recorded shape, and remembers what it was asked.
    ///
    /// Every rule above is about the *order* of the proofs and about which carrier answered, so what
    /// a double has to record is the calls rather than the bytes.
    #[derive(Default)]
    struct Recorded {
        labels: BTreeMap<String, BTreeMap<String, String>>,
        manifests: BTreeMap<String, Json>,
        blobs: BTreeMap<String, Vec<u8>>,
        referrers: BTreeMap<String, Vec<Json>>,
        verified: RefCell<Vec<String>>,
        asked: RefCell<Vec<String>>,
        refuse_signature: bool,
    }

    impl Recorded {
        /// The ordinary case: an index over two platforms, both carrying the same labels, with the
        /// document attached to the index.
        fn of(payload: &[u8]) -> Self {
            let labels: BTreeMap<String, String> = [
                (LABEL_VERSION.to_owned(), "1".to_owned()),
                (LABEL_PATH.to_owned(), "/etc/api/contract.json".to_owned()),
                (LABEL_PREFIX.to_owned(), "API_".to_owned()),
            ]
            .into_iter()
            .collect();

            let attached = "sha256:dd";
            Self {
                labels: [
                    (format!("{IMAGE}@{AMD64}"), labels.clone()),
                    (format!("{IMAGE}@{ARM64}"), labels),
                ]
                .into_iter()
                .collect(),
                manifests: [
                    (
                        format!("{IMAGE}@{INDEX}"),
                        json!({"manifests": [
                            {"digest": AMD64, "platform": {"os": "linux", "architecture": "amd64"}},
                            {"digest": ARM64, "platform": {"os": "linux", "architecture": "arm64"}},
                            {"digest": "sha256:ee", "platform": {"os": "unknown"}},
                        ]}),
                    ),
                    (format!("{IMAGE}@{AMD64}"), json!({"layers": []})),
                    (format!("{IMAGE}@{ARM64}"), json!({"layers": []})),
                    (
                        format!("{IMAGE}@{attached}"),
                        json!({"layers": [{"digest": digest_of(payload)}]}),
                    ),
                ]
                .into_iter()
                .collect(),
                blobs: [(format!("{IMAGE}@{}", digest_of(payload)), payload.to_vec())]
                    .into_iter()
                    .collect(),
                referrers: [(
                    format!("{IMAGE}@{INDEX}"),
                    vec![json!({"digest": attached, "artifactType": ARTIFACT_TYPE})],
                )]
                .into_iter()
                .collect(),
                ..Self::default()
            }
        }

        /// The same image with nothing attached and the document inside a layer instead.
        fn embedded_only(payload: &[u8]) -> Self {
            let mut held = Self::of(payload);
            held.referrers.clear();
            let layer = "sha256:layer";
            let tarball = tarball("etc/api/contract.json", payload);
            held.manifests.insert(
                format!("{IMAGE}@{AMD64}"),
                json!({"layers": [{"digest": layer}]}),
            );
            held.manifests.insert(
                format!("{IMAGE}@{ARM64}"),
                json!({"layers": [{"digest": layer}]}),
            );
            held.blobs.insert(format!("{IMAGE}@{layer}"), tarball);
            held
        }
    }

    impl Registry for Recorded {
        fn resolve(&self, reference: &str) -> Result<String, Error> {
            self.asked.borrow_mut().push(format!("resolve {reference}"));
            Ok(INDEX.to_owned())
        }

        fn discover(&self, reference: &str, artifact_type: &str) -> Result<Vec<Json>, Error> {
            self.asked
                .borrow_mut()
                .push(format!("discover {reference} {artifact_type}"));
            Ok(self.referrers.get(reference).cloned().unwrap_or_default())
        }

        fn image_labels(&self, reference: &str) -> Result<BTreeMap<String, String>, Error> {
            self.asked.borrow_mut().push(format!("labels {reference}"));
            Ok(self.labels.get(reference).cloned().unwrap_or_default())
        }

        fn manifest(&self, reference: &str) -> Result<Json, Error> {
            self.asked
                .borrow_mut()
                .push(format!("manifest {reference}"));
            self.manifests
                .get(reference)
                .cloned()
                .ok_or_else(|| Error::Invalid(format!("{reference}: no such manifest")))
        }

        fn blob(&self, reference: &str) -> Result<Vec<u8>, Error> {
            self.asked.borrow_mut().push(format!("blob {reference}"));
            self.blobs
                .get(reference)
                .cloned()
                .ok_or_else(|| Error::Invalid(format!("{reference}: no such blob")))
        }

        fn verify(&self, reference: &str, identity: &str) -> Result<(), Error> {
            self.verified.borrow_mut().push(reference.to_owned());
            self.asked.borrow_mut().push(format!("verify {reference}"));
            if self.refuse_signature {
                return Err(Error::Invalid(format!(
                    "{reference} is not signed by {identity}"
                )));
            }
            Ok(())
        }
    }

    /// One uncompressed tar holding one file, which is all a layer needs to be for these.
    fn tarball(path: &str, body: &[u8]) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let mut builder = tar::Builder::new(Vec::new());
        builder
            .append_data(&mut header, path, body)
            .expect("a tar entry is appendable");
        builder.into_inner().expect("a tar closes")
    }

    // -- what the registry is asked, and in what order ---------------------------------------

    #[test]
    fn the_signature_is_checked_before_anything_is_read() {
        // A contract that cannot be proven to belong to the pinned digest is worse than none,
        // because every gate downstream would trust it. So nothing is read until it is proven.
        let registry = Recorded::of(&published());
        fetch_contract(&registry, IMAGE, INDEX, SIGNER).expect("the image publishes one");
        assert_eq!(
            registry.asked.borrow().first().map(String::as_str),
            Some(format!("verify {IMAGE}@{INDEX}").as_str())
        );
    }

    #[test]
    fn an_unsigned_image_is_refused_and_nothing_else_is_asked() {
        let registry = Recorded {
            refuse_signature: true,
            ..Recorded::of(&published())
        };
        assert!(fetch_contract(&registry, IMAGE, INDEX, SIGNER).is_err());
        assert_eq!(registry.asked.borrow().len(), 1, "{:?}", registry.asked);
    }

    #[test]
    fn the_document_comes_back_with_the_hash_the_registry_addressed_it_by() {
        let payload = published();
        let registry = Recorded::of(&payload);
        let (found, sha256) =
            fetch_contract(&registry, IMAGE, INDEX, SIGNER).expect("the image publishes one");
        assert_eq!(found, payload);
        assert_eq!(format!("sha256:{sha256}"), digest_of(&payload));
    }

    #[test]
    fn a_blob_that_does_not_hash_to_its_descriptor_is_refused() {
        // A registry content-addresses its blobs, so hashing what arrived and comparing is the
        // whole integrity check.
        let mut registry = Recorded::of(&published());
        registry.blobs.insert(
            format!("{IMAGE}@{}", digest_of(&published())),
            b"{}".to_vec(),
        );
        let failure = fetch_attached(&registry, IMAGE, INDEX)
            .expect_err("the blob is not what its descriptor claims")
            .to_string();
        assert!(failure.contains("descriptor claims"), "{failure}");
    }

    // -- the two carriers --------------------------------------------------------------------

    #[test]
    fn the_document_inside_the_image_is_read_when_nothing_is_attached() {
        // The stronger of the two carriers: it lives in a layer whose digest is in a manifest whose
        // digest is what the chart pins, so it is provably *inside* the image.
        let payload = published();
        let registry = Recorded::embedded_only(&payload);
        let (found, _) =
            fetch_contract(&registry, IMAGE, INDEX, SIGNER).expect("the image publishes one");
        assert_eq!(found, payload);
    }

    #[test]
    fn an_image_publishing_neither_carrier_is_refused_with_both_named() {
        let mut registry = Recorded::embedded_only(&published());
        registry.blobs.clear();
        registry
            .manifests
            .insert(format!("{IMAGE}@{AMD64}"), json!({"layers": []}));
        registry
            .manifests
            .insert(format!("{IMAGE}@{ARM64}"), json!({"layers": []}));
        let failure = fetch_contract(&registry, IMAGE, INDEX, SIGNER)
            .expect_err("it publishes nothing")
            .to_string();
        assert!(failure.contains(ARTIFACT_TYPE), "{failure}");
        assert!(failure.contains("/etc/api/contract.json"), "{failure}");
    }

    #[test]
    fn two_carriers_that_disagree_are_refused_rather_than_reconciled() {
        // The image and its attachment came from different builds, and picking either would tie a
        // chart to a document the other half of the image does not implement.
        let payload = published();
        let mut registry = Recorded::of(&payload);
        let layer = "sha256:layer";
        let other = serde_json::to_vec(&json!({"terrace_contract": 1, "app": {"name": "other"}}))
            .expect("it serialises");
        registry.manifests.insert(
            format!("{IMAGE}@{AMD64}"),
            json!({"layers": [{"digest": layer}]}),
        );
        registry.manifests.insert(
            format!("{IMAGE}@{ARM64}"),
            json!({"layers": [{"digest": layer}]}),
        );
        registry.blobs.insert(
            format!("{IMAGE}@{layer}"),
            tarball("etc/api/contract.json", &other),
        );

        let failure = fetch_contract(&registry, IMAGE, INDEX, SIGNER)
            .expect_err("the two carriers differ")
            .to_string();
        assert!(failure.contains("two different documents"), "{failure}");
    }

    #[test]
    fn a_gzipped_layer_is_read_and_an_unreadable_one_is_skipped() {
        use std::io::Write as _;

        let payload = published();
        let plain = tarball("etc/api/contract.json", &payload);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&plain).expect("it compresses");
        let compressed = encoder.finish().expect("it finishes");

        assert_eq!(
            read_from_layer(&compressed, "etc/api/contract.json").as_deref(),
            Some(payload.as_slice())
        );
        // Not an error: the file may be in another layer, and a document that is nowhere is
        // reported by the caller rather than here.
        assert!(read_from_layer(b"not an archive at all", "etc/api/contract.json").is_none());
        assert!(read_from_layer(&plain, "etc/api/other.json").is_none());
    }

    #[test]
    fn a_path_written_with_a_leading_dot_is_the_same_path() {
        // Layer tarballs spell an absolute path either way, and a walk that only matched one would
        // report "no document" for an image that carries one.
        let payload = published();
        let held = tarball("./etc/api/contract.json", &payload);
        assert_eq!(
            read_from_layer(&held, "etc/api/contract.json").as_deref(),
            Some(payload.as_slice())
        );
    }

    // -- the labels --------------------------------------------------------------------------

    #[test]
    fn every_platform_of_one_image_has_to_agree_about_its_labels() {
        // Two architectures disagreeing about which namespace the binary reads is a build that
        // produced two different programs under one tag, and vendoring either would tie the chart
        // to a contract that is wrong for half its nodes.
        let mut registry = Recorded::of(&published());
        registry.labels.insert(
            format!("{IMAGE}@{ARM64}"),
            [(LABEL_PREFIX.to_owned(), "OTHER_".to_owned())]
                .into_iter()
                .collect(),
        );
        let failure = agreed_labels(&registry, IMAGE, INDEX)
            .expect_err("the platforms disagree")
            .to_string();
        assert!(failure.contains("per platform"), "{failure}");
    }

    #[test]
    fn the_labels_are_read_through_the_index_the_chart_actually_pins() {
        // The digest in a values file is usually an index and has no config blob of its own.
        let registry = Recorded::of(&published());
        let labels = agreed_labels(&registry, IMAGE, INDEX).expect("they agree");
        assert_eq!(labels.get(LABEL_PREFIX).map(String::as_str), Some("API_"));
        assert!(
            registry
                .asked
                .borrow()
                .iter()
                .any(|held| held == &format!("labels {IMAGE}@{AMD64}")),
            "{:?}",
            registry.asked
        );
    }

    #[test]
    fn an_image_labelled_for_another_namespace_is_refused() {
        // The label is what a consumer discovers the image by, so a label naming one namespace over
        // a document describing another is an image whose two halves came from different builds.
        let mut registry = Recorded::of(&published());
        registry.labels.insert(
            format!("{IMAGE}@{AMD64}"),
            [
                (LABEL_VERSION.to_owned(), "1".to_owned()),
                (LABEL_PREFIX.to_owned(), "OTHER_".to_owned()),
            ]
            .into_iter()
            .collect(),
        );
        registry.labels.insert(
            format!("{IMAGE}@{ARM64}"),
            registry.labels[&format!("{IMAGE}@{AMD64}")].clone(),
        );
        let failure = fetch_contract(&registry, IMAGE, INDEX, SIGNER)
            .expect_err("the two halves disagree")
            .to_string();
        assert!(failure.contains("different builds"), "{failure}");
    }

    #[test]
    fn an_image_publishing_another_envelope_version_is_refused() {
        let mut registry = Recorded::of(&published());
        for platform in [AMD64, ARM64] {
            registry
                .labels
                .get_mut(&format!("{IMAGE}@{platform}"))
                .expect("the platform")
                .insert(LABEL_VERSION.to_owned(), "99".to_owned());
        }
        let failure = fetch_contract(&registry, IMAGE, INDEX, SIGNER)
            .expect_err("this build does not read it")
            .to_string();
        assert!(failure.contains("contract version 99"), "{failure}");
    }

    #[test]
    fn an_image_with_no_version_label_publishes_no_contract() {
        let mut registry = Recorded::of(&published());
        for platform in [AMD64, ARM64] {
            registry
                .labels
                .get_mut(&format!("{IMAGE}@{platform}"))
                .expect("the platform")
                .remove(LABEL_VERSION);
        }
        let failure = fetch_contract(&registry, IMAGE, INDEX, SIGNER)
            .expect_err("it publishes none")
            .to_string();
        assert!(failure.contains("publishes no contract"), "{failure}");
    }

    // -- discovery ---------------------------------------------------------------------------

    #[test]
    fn an_attachment_on_a_platform_is_found_when_the_index_carries_none() {
        // Which of the two a producer attaches to is a real choice, and a consumer that looked at
        // only one would report "no contract" for an image that publishes one perfectly well.
        let payload = published();
        let mut registry = Recorded::of(&payload);
        let attached = registry
            .referrers
            .remove(&format!("{IMAGE}@{INDEX}"))
            .expect("the index carried it");
        registry
            .referrers
            .insert(format!("{IMAGE}@{ARM64}"), attached);
        assert_eq!(
            fetch_attached(&registry, IMAGE, INDEX).expect("it is found"),
            Some(payload)
        );
    }

    #[test]
    fn more_than_one_attachment_is_refused_rather_than_picked_between() {
        let mut registry = Recorded::of(&published());
        registry.referrers.insert(
            format!("{IMAGE}@{INDEX}"),
            vec![
                json!({"digest": "sha256:dd"}),
                json!({"digest": "sha256:ff"}),
            ],
        );
        let failure = fetch_attached(&registry, IMAGE, INDEX)
            .expect_err("there are two")
            .to_string();
        assert!(failure.contains("expected one"), "{failure}");
    }

    #[test]
    fn a_referrer_listing_is_read_whichever_key_the_client_used() {
        // The top-level key has changed across releases of the client this shells out to.
        let entry = json!({"digest": "sha256:dd"});
        for text in [
            serde_json::to_string(&json!([entry])).expect("it serialises"),
            serde_json::to_string(&json!({"referrers": [entry]})).expect("it serialises"),
            serde_json::to_string(&json!({"manifests": [entry]})).expect("it serialises"),
        ] {
            assert_eq!(parse_referrers(&text).expect("it reads").len(), 1, "{text}");
        }
        assert!(
            parse_referrers("{\"something\": 1}")
                .expect("it reads")
                .is_empty()
        );
        assert!(parse_referrers("not json").is_err());
    }

    // -- what a chart's values say ------------------------------------------------------------

    #[test]
    fn a_bare_docker_hub_name_is_qualified_before_it_reaches_a_client() {
        // A registry client reads a bare first segment as a hostname and tries to resolve it by
        // DNS; signature verification normalises and it does not, so passing the chart's spelling
        // around would have the two disagree about which image was verified.
        assert_eq!(normalized_image("", "demo/api"), "docker.io/demo/api");
        assert_eq!(normalized_image("ghcr.io", "demo/api"), "ghcr.io/demo/api");
    }

    #[test]
    fn a_digest_pinned_in_the_tag_is_taken_rather_than_resolved() {
        let image = json!({"repository": "demo/api", "tag": format!("v1@{INDEX}")});
        let (repository, reference, digest) =
            reference_for(&image, None).expect("the block names a repository");
        assert_eq!(repository, IMAGE);
        assert_eq!(reference, format!("{IMAGE}:v1@{INDEX}"));
        assert_eq!(digest.as_deref(), Some(INDEX));
    }

    #[test]
    fn a_block_with_no_tag_falls_back_to_the_charts_app_version() {
        let image = json!({"repository": "demo/api"});
        let (_, reference, digest) = reference_for(&image, Some("v2")).expect("it reads");
        assert_eq!(reference, format!("{IMAGE}:v2"));
        assert!(digest.is_none(), "nothing pinned it");
        assert!(reference_for(&json!({}), None).is_err());
    }

    // -- what is written ----------------------------------------------------------------------

    /// A throwaway file, removed when it goes out of scope.
    struct Vendored(std::path::PathBuf);

    impl Vendored {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir().join(format!(
                "terrace-vendored-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&at);
            Self(at)
        }

        fn path(&self) -> std::path::PathBuf {
            self.0.join("contracts").join("api.json")
        }

        fn read(&self) -> Json {
            serde_json::from_str(&std::fs::read_to_string(self.path()).expect("it was written"))
                .expect("it is JSON")
        }
    }

    impl Drop for Vendored {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_wrapper_records_which_digest_the_bytes_were_fetched_for() {
        // Which is what the offline staleness interlock reads: the tie between a contract and an
        // image is the attachment, and this is where that fact is written down.
        let held = Vendored::new();
        let payload = published();
        let sha256 = super::hex_sha256(&payload);
        assert!(
            write_vendored(
                &held.path(),
                IMAGE,
                INDEX,
                &payload,
                &sha256,
                "2026-01-01T00:00:00Z"
            )
            .expect("it writes")
        );
        let written = held.read();
        assert_eq!(written["source"]["image"], json!(IMAGE));
        assert_eq!(written["source"]["digest"], json!(INDEX));
        assert_eq!(written["source"]["sha256"], json!(sha256));
        assert_eq!(written["contract"], contract());
    }

    #[test]
    fn a_file_that_differs_only_in_when_it_was_fetched_is_left_alone() {
        // `fetched` moves on every run by construction, and rewriting the file for it alone would
        // put a commit on every scheduled run — noise in exactly the place a real contract change
        // is supposed to stand out.
        let held = Vendored::new();
        let payload = published();
        let sha256 = super::hex_sha256(&payload);
        let write = |fetched: &str| {
            write_vendored(&held.path(), IMAGE, INDEX, &payload, &sha256, fetched)
                .expect("it writes")
        };
        assert!(write("2026-01-01T00:00:00Z"));
        assert!(!write("2026-06-01T12:00:00Z"));
        assert_eq!(
            held.read()["source"]["fetched"],
            json!("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn a_moved_digest_rewrites_the_file() {
        let held = Vendored::new();
        let payload = published();
        let sha256 = super::hex_sha256(&payload);
        assert!(
            write_vendored(&held.path(), IMAGE, INDEX, &payload, &sha256, "t").expect("it writes")
        );
        assert!(
            write_vendored(&held.path(), IMAGE, AMD64, &payload, &sha256, "t").expect("it writes")
        );
        assert_eq!(held.read()["source"]["digest"], json!(AMD64));
    }

    #[test]
    fn the_moment_a_refresh_happened_is_spelled_the_way_a_vendored_file_records_it() {
        let held = now();
        assert_eq!(held.len(), 20, "{held}");
        assert!(held.ends_with('Z'), "{held}");
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
                .expect("it compiles")
                .is_match(&held),
            "{held}"
        );
    }

    #[test]
    fn the_calendar_this_carries_agrees_with_a_known_instant() {
        // Hand-rolled, so it is worth pinning against dates a reader can check: the epoch itself,
        // a leap day, and a year a naive rule would get wrong.
        assert_eq!(super::civil(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::civil(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(super::civil(1_767_225_599), "2025-12-31T23:59:59Z");
    }
}
