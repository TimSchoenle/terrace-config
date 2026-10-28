//! Reading an image's published contract from a container registry, and attaching one to it.
//!
//! Two commands talk to a registry, and they ask it the same questions from opposite ends: `pull`
//! reads the contract a chart's pinned image publishes and vendors it, and `image verify --image`
//! reads the contract a build has just published and holds it to the document the build produced.
//! Both go through this module, so the producer and the consumer cannot disagree about where a
//! contract is found or what makes one belong to a digest. Nothing here knows what a chart is: it
//! sits beside the core, below `helm`, so a producer's build links it without the chart gates.
//!
//! A published contract has two carriers, and [`carriers`] reads both:
//!
//! - the OCI referrer attached to the digest under [`ARTIFACT_TYPE`], whose blob is checked against
//!   the digest its descriptor claims;
//! - the file inside the image at the path the `dev.terrace.config.contract.path` label names, read
//!   by walking the layer blobs.
//!
//! At least one must be present, and if both are they must be byte-identical. The format names the
//! referrer as canonical and the embedded file as the fallback for registries with no referrers API.
//! Measured against a real image, the embedded file is the stronger of the two: it lives in a layer
//! whose digest is in a manifest whose digest is what a chart pins, so a document read from it is
//! provably *inside* the image, while an attached artifact is a separate object that merely points
//! at the same digest. The referrer is still tried first, because it is one request rather than a
//! layer walk. Neither is trusted alone when both exist.
//!
//! # The network is behind one seam
//!
//! [`Registry`] is the only thing that leaves this machine and every read goes through it, so
//! everything above is testable against recorded shapes, including the two things only a real image
//! revealed: the pinned digest being an index rather than a manifest, and the document arriving
//! through the embedded file rather than through an attachment.
//!
//! Shelling out to `oras` and `cosign` rather than linking a registry client is deliberate. Both are
//! already pinned and installed by the repositories that call this; linking one would carry a TLS
//! stack, a credential-helper protocol and a signature-verification implementation into a binary
//! whose other subcommands touch no network at all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::error::Error;

pub mod carriers;
pub mod verify;

pub use carriers::{
    Attached, agreed_labels, discover_contract, fetch_attached, fetch_contract, fetch_embedded,
    fetch_referrer, read_from_layer,
};
pub use verify::{ImageFault, Reference, verify_image};

/// The artifact type a published contract is attached under.
pub const ARTIFACT_TYPE: &str = "application/vnd.terrace.config-schema.v1+json";

/// The OIDC issuer a hosted CI workflow signs under.
///
/// Paired with the signer identity, which names the workflow itself, so a signature from any other
/// workflow, in any other repository, is refused rather than accepted as "signed".
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

    /// Refuses anything not signed by the named identity.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the signature is absent, or belongs to somebody else.
    fn verify(&self, reference: &str, identity: &str) -> Result<(), Error>;

    /// Whether the referrers of a platform manifest one index down can be asked for.
    ///
    /// A registry answers for any digest it holds. An OCI image layout cannot: a manifest below the
    /// top-level index is found by digest alone, with no media type to say it is a manifest, so
    /// `oras` refuses to list its referrers. A referrer in a layout is recorded against a digest
    /// its `index.json` names, which is where `image attach` puts one, so nothing is lost by not
    /// asking.
    fn nested_referrers(&self) -> bool {
        true
    }

    /// The per-platform image manifests behind a digest, or the digest itself.
    ///
    /// A multi-architecture image is pushed as an index, and the index is what a chart pins, so the
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

/// The two pinned binaries every registry call is delegated to.
///
/// `oras` reads and attaches; `cosign` only verifies a signature, so a caller that verifies none (
/// `image verify` without `--signer`, or anything reading an OCI image layout, which carries no
/// signature to verify) runs without it.
#[derive(Debug, Clone)]
pub struct Tools {
    oras: PathBuf,
    cosign: Option<PathBuf>,
    layout: bool,
}

impl Tools {
    /// Finds both, from the environment or from the path.
    ///
    /// # Errors
    /// [`Error::Invalid`] naming whichever is missing, and how to supply it.
    pub fn found() -> Result<Self, Error> {
        Ok(Self {
            oras: required("oras", "ORAS_BIN")?,
            cosign: Some(required("cosign", "COSIGN_BIN")?),
            layout: false,
        })
    }

    /// Finds `oras`, and `cosign` only if it is there.
    ///
    /// For a caller that may verify no signature at all. [`Registry::verify`] on the result fails,
    /// naming `cosign`, when it was not found, so a signature is never skipped for want of the tool.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `oras` is missing.
    pub fn reading() -> Result<Self, Error> {
        Ok(Self {
            oras: required("oras", "ORAS_BIN")?,
            cosign: located("cosign", "COSIGN_BIN"),
            layout: false,
        })
    }

    /// Reads every reference as an OCI image layout (a directory or a tarball, followed by `:tag`
    /// or `@digest`) rather than as a registry address.
    ///
    /// This is how an image is checked after it is built and before it is pushed: `docker buildx
    /// build --output type=oci` writes one, and so does `docker save` from Docker 25 on. A layout
    /// carries no signature, so [`Registry::verify`] refuses on one rather than reporting success.
    #[must_use]
    pub fn in_layout(mut self) -> Self {
        self.layout = true;
        self
    }

    /// Attaches a document to a digest as a referrer of [`ARTIFACT_TYPE`], and returns the digest
    /// of the attached artifact.
    ///
    /// `document` is attached under its file name, read from its own directory: `oras` refuses an
    /// absolute or parent-relative path by default, and the name is what the artifact's title
    /// annotation records.
    ///
    /// # Errors
    /// [`Error::Invalid`] when `document` names no file, or `oras` fails or prints no digest.
    pub fn attach(&self, reference: &str, document: &Path) -> Result<String, Error> {
        let (Some(directory), Some(name)) = (
            document.parent(),
            document.file_name().and_then(std::ffi::OsStr::to_str),
        ) else {
            return Err(Error::Invalid(format!(
                "{} names no file to attach",
                document.display()
            )));
        };
        let directory = if directory.as_os_str().is_empty() {
            Path::new(".")
        } else {
            directory
        };
        let file = format!("{name}:application/json");
        // `oras` runs in the document's directory, so a layout's path (relative to this process,
        // not to the document) is made absolute first. A registry reference is no path.
        let reference = if self.layout {
            let (path, pinned) = reference.rsplit_once('@').unwrap_or((reference, ""));
            let path = std::path::absolute(path).map_err(|e| Error::io(path, e))?;
            if pinned.is_empty() {
                path.display().to_string()
            } else {
                format!("{}@{pinned}", path.display())
            }
        } else {
            reference.to_owned()
        };
        let reference = reference.as_str();
        let out = self.oras(
            Some(directory),
            &[
                "attach",
                "--artifact-type",
                ARTIFACT_TYPE,
                "--format",
                "json",
                reference,
                &file,
            ],
        )?;
        let printed: Json = serde_json::from_slice(&out).map_err(|failure| {
            Error::Invalid(format!(
                "oras attach printed something that is not JSON: {failure}"
            ))
        })?;
        printed
            .get("digest")
            .and_then(Json::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "oras attach printed no digest for the artifact it attached to {reference}"
                ))
            })
    }

    /// One `oras` call, reading the reference (always the last argument) as a layout when asked.
    fn oras(&self, directory: Option<&Path>, argv: &[&str]) -> Result<Vec<u8>, Error> {
        let mut argv = argv.to_vec();
        if self.layout {
            argv.insert(argv.len().saturating_sub(1), "--oci-layout");
        }
        run(&self.oras, directory, &argv)
    }
}

/// A binary from the named environment variable, else from the path.
fn located(name: &str, variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|held| !held.is_empty())
        .map(PathBuf::from)
        .or_else(|| which(name))
}

/// As [`located`], and an error naming how to supply the binary when it is not found.
fn required(name: &str, variable: &str) -> Result<PathBuf, Error> {
    located(name, variable).ok_or_else(|| {
        Error::Invalid(format!(
            "{name} is not on PATH, and every registry call here is delegated to it; install the \
             pinned version, or set {variable}."
        ))
    })
}

/// Runs one delegated tool and returns what it printed, or its complaint as the error.
fn run(binary: &Path, directory: Option<&Path>, argv: &[&str]) -> Result<Vec<u8>, Error> {
    let name = binary
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("the tool");
    let mut command = Command::new(binary);
    command.args(argv);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command
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

impl Registry for Tools {
    fn nested_referrers(&self) -> bool {
        !self.layout
    }

    fn resolve(&self, reference: &str) -> Result<String, Error> {
        let out = self.oras(None, &["resolve", reference])?;
        Ok(String::from_utf8_lossy(&out).trim().to_owned())
    }

    fn discover(&self, reference: &str, artifact_type: &str) -> Result<Vec<Json>, Error> {
        let out = self.oras(
            None,
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
        // The manifest, then its config blob, rather than `oras manifest fetch-config`: in an OCI
        // image layout a manifest one index down is found by digest alone, with no media type to
        // say it is an image manifest, and `fetch-config` then refuses it. `docker save` writes
        // exactly that shape, so the two-step read is what makes a saved image checkable at all.
        let Some((repository, _)) = reference.rsplit_once('@') else {
            return Err(Error::Invalid(format!(
                "{reference}: labels are read by digest, and this reference names none"
            )));
        };
        let manifest = self.manifest(reference)?;
        let Some(config) = manifest
            .get("config")
            .and_then(|held| held.get("digest"))
            .and_then(Json::as_str)
        else {
            return Err(Error::Invalid(format!(
                "{reference} is not an image manifest: it names no config blob"
            )));
        };
        let out = self.blob(&format!("{repository}@{config}"))?;
        if config != format!("sha256:{}", hex_sha256(&out)) {
            return Err(Error::Invalid(format!(
                "the config blob of {reference} does not hash to {config}"
            )));
        }
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
        let out = self.oras(None, &["manifest", "fetch", reference])?;
        serde_json::from_slice(&out)
            .map_err(|failure| Error::Invalid(format!("{reference}: {failure}")))
    }

    fn blob(&self, reference: &str) -> Result<Vec<u8>, Error> {
        self.oras(None, &["blob", "fetch", "--output", "-", reference])
    }

    fn verify(&self, reference: &str, identity: &str) -> Result<(), Error> {
        if self.layout {
            return Err(Error::Invalid(format!(
                "{reference} is read from an OCI image layout, which carries no signature to \
                 verify; verify the pushed image instead"
            )));
        }
        let Some(cosign) = &self.cosign else {
            return Err(Error::Invalid(format!(
                "cosign is not on PATH, so the signature on {reference} cannot be verified; \
                 install the pinned version, or set COSIGN_BIN"
            )));
        };
        run(
            cosign,
            None,
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

/// Reads a referrers listing, whose top-level key differs between `oras` releases.
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

/// One payload's hash, spelled the way a descriptor spells it.
pub fn hex_sha256(payload: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut held = Sha256::new();
    held.update(payload);
    held.finalize().iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}
