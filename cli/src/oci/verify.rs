//! Holding a built image to the document its build produced, read from the image itself.
//!
//! `pull` asks a registry what a chart's pinned image publishes and trusts nothing it cannot prove.
//! This asks the same questions from the other end, in the build that has just produced the image:
//! the document is known, and what is unknown is whether the image carries it. So every carrier is
//! compared with the document, not merely with each other:
//!
//! - the labels, read from every platform's config, must be the ones the document renders;
//! - the file at the labelled path inside the image must be the document, byte for byte;
//! - a referrer attached to the digest, if there is one, must be the document, byte for byte;
//! - at least one of the two carriers must be there.
//!
//! Byte for byte, not as JSON: the build holds the very file it copied into the image and attached
//! to it, so any difference at all is a step that copied something else. The format promises a
//! byte-stable document precisely so that this comparison is free.

use std::fmt;

use super::{Registry, agreed_labels, fetch_attached, fetch_embedded};
use crate::document::Contract;
use crate::error::Error;
use crate::render::image::{LabelFault, check_labels};

/// One way a built image fails to publish the document its build produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageFault {
    /// The signature is absent, or belongs to an identity other than the one required.
    Unsigned {
        /// What the verifier said.
        reason: String,
    },
    /// A contract label is missing or carries another value.
    Label(LabelFault),
    /// Neither carrier is present: nothing is attached, and there is no file at the path.
    NoCarrier {
        /// The in-image path that was read.
        path: String,
    },
    /// The file inside the image is not the document.
    EmbeddedDiffers {
        /// The in-image path that was read.
        path: String,
    },
    /// The artifact attached to the digest is not the document.
    AttachedDiffers,
}

impl fmt::Display for ImageFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsigned { reason } => write!(f, "the signature does not verify: {reason}"),
            Self::Label(fault) => write!(f, "{fault}"),
            Self::NoCarrier { path } => write!(
                f,
                "the image publishes no document: nothing is attached to the digest, and there is \
                 no file at `{path}` inside it. Copy the document into the image, attach it, or \
                 both."
            ),
            Self::EmbeddedDiffers { path } => write!(
                f,
                "the file at `{path}` inside the image is not this document byte for byte, so the \
                 build copied a different file into the image than the one it was checked against"
            ),
            Self::AttachedDiffers => write!(
                f,
                "the artifact attached to the digest is not this document byte for byte, so the \
                 attach step published a different file than the one the image was checked against"
            ),
        }
    }
}

/// Every way the image at `repository@digest` fails to publish `document`, in the order checked.
///
/// `document` is the file exactly as the build produced it; `contract` is the same file, parsed.
/// `path` is where the build put the document inside the image, which is also what the path label
/// must say. With a `signer`, the image's signature is verified first and nothing else is read when
/// it fails: an image whose provenance is unproven has nothing further worth checking.
///
/// An empty result is an image that carries the document, its labels, and at least one carrier.
///
/// # Errors
/// [`Error::Invalid`] when the registry cannot be read, when the platforms of one index disagree
/// about their labels or the file they embed, or when an attached carrier is malformed — more than
/// one referrer, a manifest that is not one layer, or a blob that does not hash to its descriptor.
/// Each of those leaves the question unanswerable rather than answered no.
pub fn verify_image(
    registry: &dyn Registry,
    repository: &str,
    digest: &str,
    document: &[u8],
    contract: &Contract,
    path: &str,
    signer: Option<&str>,
) -> Result<Vec<ImageFault>, Error> {
    let pinned = format!("{repository}@{digest}");
    if let Some(identity) = signer
        && let Err(failure) = registry.verify(&pinned, identity)
    {
        return Ok(vec![ImageFault::Unsigned {
            reason: failure.to_string(),
        }]);
    }

    let labels = agreed_labels(registry, repository, digest)?;
    let mut faults: Vec<ImageFault> = check_labels(contract, path, &labels)
        .into_iter()
        .map(ImageFault::Label)
        .collect();

    let attached = fetch_attached(registry, repository, digest)?;
    let embedded = fetch_embedded(registry, repository, digest, Some(path))?;
    if attached.is_none() && embedded.is_none() {
        faults.push(ImageFault::NoCarrier {
            path: path.to_owned(),
        });
    }
    if embedded.as_deref().is_some_and(|held| held != document) {
        faults.push(ImageFault::EmbeddedDiffers {
            path: path.to_owned(),
        });
    }
    if attached.as_deref().is_some_and(|held| held != document) {
        faults.push(ImageFault::AttachedDiffers);
    }
    Ok(faults)
}

/// An image reference split into what a registry is asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The registry and repository, or the OCI image layout's path, with no tag or digest.
    pub repository: String,
    /// The digest, when the reference pins one.
    pub digest: Option<String>,
    /// The reference as given, which is what a tag is resolved through.
    pub given: String,
}

impl Reference {
    /// Split a reference, `REPOSITORY[:TAG][@DIGEST]`.
    ///
    /// A registry reference must be fully qualified — `docker.io/library/redis:7`, not `redis:7`.
    /// A registry client reads a bare first segment as a host name, so `redis:7` would be looked up
    /// as the host `redis`; refusing it here says so instead of reporting an unreachable host. An
    /// OCI image layout's path is taken as written.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the reference is empty, carries a digest that is not `algorithm:hex`,
    /// or is a registry reference that is not fully qualified.
    pub fn parse(reference: &str, layout: bool) -> Result<Self, Error> {
        let given = reference.trim();
        let (named, digest) = match given.split_once('@') {
            Some((named, digest)) => (named, Some(digest)),
            None => (given, None),
        };
        if let Some(digest) = digest
            && !digest.split_once(':').is_some_and(|(algorithm, hex)| {
                !algorithm.is_empty()
                    && !hex.is_empty()
                    && hex.chars().all(|held| held.is_ascii_hexdigit())
            })
        {
            return Err(Error::Invalid(format!(
                "`{given}` pins `{digest}`, which is not a digest: one is `sha256:` and hex"
            )));
        }
        // A tag is the part after the last `:` that follows the last `/`, so a registry port or a
        // drive letter earlier in the name is not mistaken for one.
        let slash = named.rfind('/').map_or(0, |at| at + 1);
        let repository = match named[slash..].rfind(':') {
            Some(at) => &named[..slash + at],
            None => named,
        };
        if repository.is_empty() {
            return Err(Error::Invalid(format!("`{given}` names no repository")));
        }
        if !layout {
            let first = repository.split('/').next().unwrap_or_default();
            let qualified = repository.contains('/')
                && (first.contains('.') || first.contains(':') || first == "localhost");
            if !qualified {
                return Err(Error::Invalid(format!(
                    "`{given}` is not fully qualified, and a registry client would read `{first}` \
                     as a host name. Name the registry: `docker.io/library/{given}` for an \
                     official Docker Hub image, `docker.io/{given}` for any other on Docker Hub."
                )));
            }
        }
        Ok(Self {
            repository: repository.to_owned(),
            digest: digest.map(str::to_owned),
            given: given.to_owned(),
        })
    }

    /// The digest this reference names, resolving a tag through the registry when it pins none.
    ///
    /// # Errors
    /// As [`Registry::resolve`].
    pub fn digest(&self, registry: &dyn Registry) -> Result<String, Error> {
        match &self.digest {
            Some(held) => Ok(held.clone()),
            None => registry.resolve(&self.given),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageFault, Reference, verify_image};
    use crate::document::{Contract, LABEL_PATH};
    use crate::oci::carriers::testing::{IMAGE, INDEX, Recorded, SIGNER, published};

    const PATH: &str = "/etc/api/contract.json";

    fn contract() -> Contract {
        Contract::from_json(&String::from_utf8(published()).expect("UTF-8"))
            .expect("the recorded document reads")
    }

    fn verified(registry: &Recorded, document: &[u8], signer: Option<&str>) -> Vec<ImageFault> {
        verify_image(registry, IMAGE, INDEX, document, &contract(), PATH, signer)
            .expect("the recorded registry answers")
    }

    #[test]
    fn an_image_carrying_the_document_has_no_fault_whichever_carriers_it_uses() {
        for registry in [
            Recorded::of(&published()),
            Recorded::embedded_only(&published()),
            Recorded::both(&published()),
        ] {
            assert_eq!(verified(&registry, &published(), Some(SIGNER)), Vec::new());
        }
    }

    #[test]
    fn a_document_the_image_does_not_carry_is_named_for_each_carrier() {
        let registry = Recorded::of(&published());
        let mut other = published();
        other.push(b'\n');
        assert_eq!(
            verified(&registry, &other, None),
            vec![ImageFault::AttachedDiffers]
        );

        let registry = Recorded::embedded_only(&published());
        assert_eq!(
            verified(&registry, &other, None),
            vec![ImageFault::EmbeddedDiffers {
                path: PATH.to_owned()
            }]
        );
    }

    #[test]
    fn an_image_with_neither_carrier_is_a_fault_not_a_pass() {
        let mut registry = Recorded::of(&published());
        registry.referrers.clear();
        assert_eq!(
            verified(&registry, &published(), None),
            vec![ImageFault::NoCarrier {
                path: PATH.to_owned()
            }]
        );
    }

    #[test]
    fn a_wrong_label_is_reported_beside_the_carriers() {
        let mut registry = Recorded::embedded_only(&published());
        for labels in registry.labels.values_mut() {
            labels.insert(LABEL_PATH.to_owned(), "/elsewhere.json".to_owned());
        }
        let faults = verified(&registry, &published(), None);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(matches!(faults[0], ImageFault::Label(_)), "{faults:?}");
    }

    #[test]
    fn an_unsigned_image_is_refused_before_anything_else_is_read() {
        let registry = Recorded {
            refuse_signature: true,
            ..Recorded::of(&published())
        };
        let faults = verified(&registry, &published(), Some(SIGNER));
        assert!(
            matches!(faults[..], [ImageFault::Unsigned { .. }]),
            "{faults:?}"
        );
        assert_eq!(registry.asked.borrow().len(), 1, "{:?}", registry.asked);
    }

    #[test]
    fn without_a_signer_no_signature_is_asked_for() {
        let registry = Recorded::of(&published());
        verified(&registry, &published(), None);
        assert!(registry.verified.borrow().is_empty());
    }

    #[test]
    fn a_reference_splits_into_repository_and_digest() {
        let held = Reference::parse("ghcr.io/org/app:v1@sha256:ab", false).expect("it parses");
        assert_eq!(held.repository, "ghcr.io/org/app");
        assert_eq!(held.digest.as_deref(), Some("sha256:ab"));

        let held = Reference::parse("localhost:5000/app:v1", false).expect("a port is no tag");
        assert_eq!(held.repository, "localhost:5000/app");
        assert_eq!(held.digest, None);

        let held = Reference::parse("build/layout.tar:v1", true).expect("a layout path");
        assert_eq!(held.repository, "build/layout.tar");
    }

    #[test]
    fn an_unqualified_or_malformed_reference_is_refused() {
        for reference in [
            "redis:7",
            "demo/api:v1",
            "",
            "ghcr.io/org/app@latest",
            "ghcr.io/org/app@sha256:xyz",
        ] {
            assert!(
                Reference::parse(reference, false).is_err(),
                "{reference} should be refused"
            );
        }
        assert!(
            Reference::parse("demo/api:v1", true).is_ok(),
            "a layout path is as written"
        );
    }
}
