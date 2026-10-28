//! The two carriers of a published contract, and the proofs that tie one to a digest.
//!
//! Every function here takes the [`Registry`] seam rather than a client, so each rule (which
//! carrier answers, in which order the registry is asked, what a mismatch is called) is tested
//! against recorded shapes rather than against a live registry.

use std::collections::BTreeMap;
use std::io::Read;

use serde_json::Value as Json;

use super::{ARTIFACT_TYPE, Registry, hex_sha256};
use crate::document::{CONTRACT_VERSION, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION};
use crate::error::Error;

/// Fetches and verifies one image's contract. Returns the published bytes and their hash.
///
/// Two things are proven here, and they answer different questions. The blob is what its descriptor
/// says it is: a registry content-addresses its blobs, so hashing what arrived and comparing is the
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
                crate::text::quoted(path.unwrap_or_default())
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
            crate::text::quoted(declared.unwrap_or_default()),
            crate::text::quoted(described.unwrap_or_default())
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
/// Which of the two a producer attaches to is a real choice (the index is what a chart pins, a
/// platform manifest is what actually runs), and a consumer that only looked at one would report
/// "no contract" for an image that publishes one perfectly well. The index is tried first because
/// that is the digest everything else is tied to.
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
    if !registry.nested_referrers() {
        return Ok(Vec::new());
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
/// As [`fetch_referrer`].
pub fn fetch_attached(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
) -> Result<Option<Vec<u8>>, Error> {
    Ok(fetch_referrer(registry, repository, digest)?.map(|held| held.payload))
}

/// One contract attached to a digest: the referrer that carries it, and the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// The digest of the referrer manifest, which is what signing the attachment signs.
    pub digest: String,
    /// The document exactly as attached, proven to hash to its descriptor.
    pub payload: Vec<u8>,
}

/// The referrer carrying a digest's contract, or [`None`] if nothing is attached.
///
/// # Errors
/// [`Error::Invalid`] when there is more than one, when its manifest is not one layer, or when the
/// blob does not hash to the digest its descriptor claims.
pub fn fetch_referrer(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
) -> Result<Option<Attached>, Error> {
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
    Ok(Some(Attached {
        digest: attached.to_owned(),
        payload,
    }))
}

/// The document from inside the image, at the path the label names.
///
/// Read by walking the layer blobs from the top down, because that is the order a filesystem
/// resolves them: the last layer to write a path is the one the container sees. The first hit wins
/// and that platform's walk stops, so the usual case (the document copied in a late, tiny layer)
/// costs one small blob per platform rather than the whole image.
///
/// No container runtime and no local daemon: the images this validates may be built from scratch
/// with no shell, and requiring one for a step that otherwise needs two static binaries would put it
/// out of reach of the pull request that needs it most.
///
/// # Errors
/// As [`Registry::manifest`] and [`Registry::blob`], and [`Error::Invalid`] when the platforms of
/// one index do not embed the same file: different bytes, or the file on some and not others.
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

    // Every platform is read, not only the first that holds the file: a multi-platform image whose
    // platforms embed different documents, or where only some embed one, publishes a contract that
    // is wrong for part of the fleet, which is the same refusal `agreed_labels` makes for labels.
    let mut agreed: Option<(String, Option<Vec<u8>>)> = None;
    for platform in registry.platforms(repository, digest)? {
        let held = embedded_on(registry, repository, &platform, wanted)?;
        match &agreed {
            None => agreed = Some((platform, held)),
            Some((first, document)) if *document != held => {
                return Err(Error::Invalid(format!(
                    "{repository}@{digest} does not embed one document at `/{wanted}`: platforms \
                     {first} and {platform} hold {}",
                    if document.is_some() && held.is_some() {
                        "different files"
                    } else {
                        "it on one and not the other"
                    }
                )));
            }
            Some(_) => {}
        }
    }
    Ok(agreed.and_then(|(_, document)| document))
}

/// The file at `wanted` on one platform's image, walking its layers from the top down.
fn embedded_on(
    registry: &dyn Registry,
    repository: &str,
    platform: &str,
    wanted: &str,
) -> Result<Option<Vec<u8>>, Error> {
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
    Ok(None)
}

/// One file out of one layer blob, or [`None`] if this layer does not hold it.
///
/// A layer this cannot read is not an error: the file may be in another one, and a document that is
/// nowhere is reported by the caller rather than here.
#[must_use]
pub fn read_from_layer(blob: &[u8], wanted: &str) -> Option<Vec<u8>> {
    // Streamed, so the walk stops at the entry it wants without holding the decompressed layer.
    let body: Box<dyn Read + '_> = if blob.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(blob))
    } else {
        Box::new(blob)
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

#[cfg(test)]
/// A registry that answers from recorded shapes, shared by every test that reads a carrier.
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use serde_json::{Value as Json, json};

    use crate::document::{LABEL_PATH, LABEL_PREFIX, LABEL_VERSION};
    use crate::error::Error;
    use crate::oci::{ARTIFACT_TYPE, Registry};

    pub(crate) const IMAGE: &str = "docker.io/demo/api";
    pub(crate) const INDEX: &str = "sha256:aa";
    pub(crate) const AMD64: &str = "sha256:bb";
    pub(crate) const ARM64: &str = "sha256:cc";
    pub(crate) const SIGNER: &str = "https://example.invalid/workflow";

    /// One published contract, small enough to read in a failure message.
    pub(crate) fn contract() -> Json {
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

    pub(crate) fn published() -> Vec<u8> {
        serde_json::to_vec(&contract()).expect("it serialises")
    }

    pub(crate) fn digest_of(payload: &[u8]) -> String {
        format!("sha256:{}", crate::oci::hex_sha256(payload))
    }

    /// A registry that answers from a recorded shape, and remembers what it was asked.
    ///
    /// Every rule above is about the *order* of the proofs and about which carrier answered, so what
    /// a double has to record is the calls rather than the bytes.
    #[derive(Default)]
    pub(crate) struct Recorded {
        pub(crate) labels: BTreeMap<String, BTreeMap<String, String>>,
        pub(crate) manifests: BTreeMap<String, Json>,
        pub(crate) blobs: BTreeMap<String, Vec<u8>>,
        pub(crate) referrers: BTreeMap<String, Vec<Json>>,
        pub(crate) verified: RefCell<Vec<String>>,
        pub(crate) asked: RefCell<Vec<String>>,
        pub(crate) refuse_signature: bool,
    }

    impl Recorded {
        /// The ordinary case: an index over two platforms, both carrying the same labels, with the
        /// document attached to the index.
        pub(crate) fn of(payload: &[u8]) -> Self {
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

        /// The same image carrying the document both ways: attached, and inside a layer.
        pub(crate) fn both(payload: &[u8]) -> Self {
            let attached = Self::of(payload);
            let mut held = Self::embedded_only(payload);
            held.referrers = attached.referrers;
            held.manifests.extend(
                attached
                    .manifests
                    .into_iter()
                    .filter(|(reference, _)| !held_platform(reference)),
            );
            held.blobs.extend(attached.blobs);
            held
        }

        /// The same image with nothing attached and the document inside a layer instead.
        pub(crate) fn embedded_only(payload: &[u8]) -> Self {
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
    pub(crate) fn tarball(path: &str, body: &[u8]) -> Vec<u8> {
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

    /// Whether a recorded reference is one of the two platform manifests, which [`Recorded::both`]
    /// keeps from the embedded shape because those are the manifests that carry the layer.
    fn held_platform(reference: &str) -> bool {
        reference == format!("{IMAGE}@{AMD64}") || reference == format!("{IMAGE}@{ARM64}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value as Json, json};

    use super::testing::{
        AMD64, ARM64, IMAGE, INDEX, Recorded, SIGNER, digest_of, published, tarball,
    };
    use super::{agreed_labels, fetch_attached, fetch_contract, fetch_embedded, read_from_layer};
    use crate::document::{LABEL_PREFIX, LABEL_VERSION};
    use crate::oci::{ARTIFACT_TYPE, parse_referrers};

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
    fn platforms_that_embed_different_documents_are_refused() {
        // A multi-platform image is one contract: a stale copy on one platform, or a copy on only
        // some of them, is a document that is wrong for part of the fleet.
        let payload = published();
        let stale = serde_json::to_vec(&json!({"terrace_contract": 1, "app": {"name": "old"}}))
            .expect("it serialises");
        for arm64 in [Some(stale), None] {
            let mut registry = Recorded::embedded_only(&payload);
            let layers = match &arm64 {
                Some(document) => {
                    registry.blobs.insert(
                        format!("{IMAGE}@sha256:stale"),
                        tarball("etc/api/contract.json", document),
                    );
                    json!({"layers": [{"digest": "sha256:stale"}]})
                }
                None => json!({"layers": []}),
            };
            registry
                .manifests
                .insert(format!("{IMAGE}@{ARM64}"), layers);
            let failure = fetch_embedded(&registry, IMAGE, INDEX, Some("/etc/api/contract.json"))
                .expect_err("the platforms disagree")
                .to_string();
            assert!(failure.contains("does not embed one document"), "{failure}");
            assert!(
                failure.contains(AMD64) && failure.contains(ARM64),
                "{failure}"
            );
        }
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
        assert_eq!(
            parse_referrers("{\"something\": 1}").expect("it reads"),
            Vec::<Json>::new()
        );
        assert!(parse_referrers("not json").is_err());
    }
}
