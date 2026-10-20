//! Refreshing every vendored contract from the image its chart pins.
//!
//! The one chart command that talks to a container registry. Everything downstream reads the committed
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
//! Fetching and proving the document is [`crate::oci`]'s; this module is what a chart repository
//! adds to it: resolving a values path to a pinned reference, the vendored file's `source`
//! envelope, and the offline staleness interlock that envelope feeds.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json};

use crate::error::Error;
use crate::oci::{Registry, fetch_contract};

pub use super::declaration::normalized_image;
use super::declaration::read_yaml;
use super::{DECLARATION, dig};

/// The vendored file for one fetched document: the published bytes inside a `source` envelope.
///
/// `source.sha256` is over the document as published — the same value the registry addressed the
/// blob by, minus the prefix. It is deliberately *not* recomputable from this file: the published
/// bytes do not survive being re-serialised into the wrapper. That is a provenance record for a
/// later networked run to check, not an offline integrity check, and the offline trust anchor is
/// that this file was reviewed in the pull request that introduced it — which is the pull request
/// that also carries the digest bump, by design.
///
/// # Errors
/// [`Error::Invalid`] when the payload is not a JSON document; `path` names it in the message.
///
/// # Panics
/// Never: the wrapper is built here out of owned values a moment before it is serialised.
pub fn vendored_text(
    path: &Path,
    image: &str,
    digest: &str,
    payload: &[u8],
    sha256: &str,
    fetched: &str,
) -> Result<String, Error> {
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
    Ok(format!(
        "{}\n",
        serde_json::to_string_pretty(&Json::Object(wrapper))
            .expect("a wrapper of owned values serialises")
    ))
}

/// Write the wrapper [`vendored_text`] renders, and report whether the file changed.
///
/// # Errors
/// As [`vendored_text`], and [`Error::Io`] when the file cannot be written.
pub fn write_vendored(
    path: &Path,
    image: &str,
    digest: &str,
    payload: &[u8],
    sha256: &str,
    fetched: &str,
) -> Result<bool, Error> {
    let rendered = vendored_text(path, image, digest, payload, sha256, fetched)?;
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
pub fn same_contract(previous: &str, rendered: &str) -> bool {
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

/// One image's contract, fetched and proven to belong to the digest it was asked for.
///
/// The unit [`refresh_chart`] writes per declared image, held in memory: a caller that has to show
/// what it would write before writing it — a scaffold's `--diff` — needs the wrapper's text without
/// the file, and must never have a way to come by one that skipped the proofs. So the fields are
/// private to this crate and [`fetch_pinned`] is the only way to hold one: a value of this type
/// *is* the statement that the signature, the labels and the carriers were checked.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The image, registry and repository, in the fully qualified form.
    pub(crate) image: String,
    /// The digest the contract was proven against.
    pub(crate) digest: String,
    /// The document exactly as published.
    pub(crate) payload: Vec<u8>,
    /// The hash of the payload, as a descriptor spells it.
    pub(crate) sha256: String,
}

impl Fetched {
    /// The image, registry and repository, in the fully qualified form.
    pub fn image(&self) -> &str {
        &self.image
    }

    /// The digest the contract was proven against.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// The published document, parsed.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the payload is not JSON, which [`fetch_contract`] has already
    /// refused — so in practice never.
    pub fn contract(&self) -> Result<Json, Error> {
        serde_json::from_slice(&self.payload).map_err(|failure| {
            Error::Invalid(format!(
                "{}@{} published a document that is not JSON: {failure}",
                self.image, self.digest
            ))
        })
    }

    /// The vendored file's text, as [`vendored_text`] renders it for `path`.
    ///
    /// # Errors
    /// As [`vendored_text`].
    pub fn vendored(&self, path: &Path, fetched: &str) -> Result<String, Error> {
        vendored_text(
            path,
            &self.image,
            &self.digest,
            &self.payload,
            &self.sha256,
            fetched,
        )
    }
}

/// Fetch the contract one image block pins, through every proof [`fetch_contract`] makes.
///
/// `image` is the block as a values file spells it — `registry`, `repository`, `tag` — with
/// `app_version` standing in for an empty tag, as a chart's own `appVersion` does. Every refresh
/// goes through here, so a contract fetched for a chart that does not exist yet is the one a later
/// refresh of the same pin would write. A tag with no digest is resolved to one
/// first; the proofs are then made against the digest, never against the tag.
///
/// # Errors
/// As [`reference_for`], [`Registry::resolve`] and [`fetch_contract`].
pub fn fetch_pinned(
    registry: &dyn Registry,
    image: &Json,
    app_version: Option<&str>,
    signer: &str,
) -> Result<Fetched, Error> {
    let (normalized, reference, digest) = reference_for(image, app_version)?;
    let digest = match digest {
        Some(held) => held,
        // Resolving a tag is a second network call and a second answer; a chart that pins by
        // digest never reaches this, so it is a fallback rather than the path.
        None => registry.resolve(&reference)?,
    };
    let (payload, sha256) = fetch_contract(registry, &normalized, &digest, signer)?;
    Ok(Fetched {
        image: normalized,
        digest,
        payload,
        sha256,
    })
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

            let held = fetch_pinned(registry, image, app_version, signer)?;
            if write_vendored(
                &target,
                &held.image,
                &held.digest,
                &held.payload,
                &held.sha256,
                fetched,
            )? {
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
    use serde_json::{Value as Json, json};

    use super::{normalized_image, now, reference_for, write_vendored};
    use crate::oci::carriers::testing::{AMD64, IMAGE, INDEX, contract, published};

    // -- what a chart's values say ------------------------------------------------------------

    #[test]
    fn a_bare_docker_hub_name_is_qualified_before_it_reaches_a_client() {
        // A registry client reads a bare first segment as a hostname and tries to resolve it by
        // DNS; signature verification normalises and it does not, so passing the chart's spelling
        // around would have the two disagree about which image was verified.
        assert_eq!(normalized_image("", "demo/api"), "docker.io/demo/api");
        assert_eq!(normalized_image("ghcr.io", "demo/api"), "ghcr.io/demo/api");
        assert_eq!(normalized_image("", "redis"), "docker.io/library/redis");
    }

    #[test]
    fn a_registry_written_into_the_repository_is_kept_rather_than_put_under_docker_hub() {
        // The helm-charts convention: `registry` stays empty and a non-Docker-Hub repository is
        // written fully qualified, which is what the image template renders and the runtime pulls.
        assert_eq!(normalized_image("", "ghcr.io/org/app"), "ghcr.io/org/app");
        assert_eq!(
            normalized_image("", "registry:5000/app"),
            "registry:5000/app"
        );
        assert_eq!(normalized_image("", "localhost/app"), "localhost/app");
        let image = serde_json::json!({"registry": "", "repository": "ghcr.io/org/app", "tag": "v1@sha256:ab"});
        let (normalized, reference, digest) = reference_for(&image, None).expect("it resolves");
        assert_eq!(normalized, "ghcr.io/org/app");
        assert_eq!(reference, "ghcr.io/org/app:v1@sha256:ab");
        assert_eq!(digest.as_deref(), Some("sha256:ab"));
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
        let sha256 = crate::oci::hex_sha256(&payload);
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
        let sha256 = crate::oci::hex_sha256(&payload);
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
        let sha256 = crate::oci::hex_sha256(&payload);
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
