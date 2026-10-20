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
use super::declaration::{load_declaration, read_yaml};
use super::files::{Planned, inside, write_all};
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
/// The file is replaced by a rename (see [`super::files`]), so no reader sees half a wrapper, and
/// `path` must resolve inside its own parent directory. [`refresh_chart`] confines it to the chart
/// instead, and refuses a contract that is itself a symbolic link.
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
    let parent = match path.parent() {
        Some(held) if !held.as_os_str().is_empty() => held,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
    write_vendored_in(parent, path, image, digest, payload, sha256, fetched)
}

/// [`write_vendored`], with `path` checked against `chart` again at the write itself.
///
/// The check that ran before the fetch is not enough on its own: the write re-resolves `path`
/// against `chart` and creates the directories it needs only after that, so a directory turned into
/// a link in the meantime is refused rather than followed.
fn write_vendored_in(
    chart: &Path,
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
    write_all(&[Planned {
        chart: chart.to_path_buf(),
        path: path.to_path_buf(),
        text: rendered,
    }])?;
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
    // The typed reader, not the raw file: a declaration that leaves `images` or a contract path to
    // its defaults (spec/helm/v1/DEFAULTS.md) still names an image, and reading the raw keys would
    // refresh nothing for it without a word.
    let Some(declaration) = load_declaration(chart_dir)? else {
        return Ok(());
    };
    let (values, meta) = (read("values.yaml")?, read("Chart.yaml")?);
    let app_version = meta.get("appVersion").and_then(Json::as_str);

    // One contract file per resolved target. Every entry is checked before a repeat is skipped, so
    // a second document's bad values path is still refused, and two different images written into
    // one file are refused rather than left to whichever ran last.
    let mut seen: Vec<(PathBuf, String, &str)> = Vec::new();
    for entry in declaration
        .documents
        .iter()
        .flat_map(|document| &document.images)
    {
        let at = entry.values.as_str();
        let Some(image) = dig(&values, at).filter(|held| held.is_object()) else {
            return Err(Error::Invalid(format!(
                "{}: values path {} resolves to no image block",
                chart_dir.join(DECLARATION).display(),
                crate::gate::quoted(at)
            )));
        };
        let (_, reference, _) = reference_for(image, app_version)?;
        // The declaration reader already refuses `..` and absolute paths; this catches a symbolic
        // link inside the chart that points out of it, before anything is fetched or created.
        let declared = chart_dir.join(&entry.contract);
        let resolved = inside(chart_dir, &declared)?;
        // A link at the contract itself would hand the write to whatever it names, an in-chart
        // `values.yaml` included. The file is pull's alone, so it must be a plain file.
        if std::fs::symlink_metadata(&declared).is_ok_and(|held| held.file_type().is_symlink()) {
            return Err(Error::Invalid(format!(
                "{}: contract {} is a symbolic link; `pull` writes the vendored contract there, so \
                 it must be a plain file or absent",
                chart_dir.join(DECLARATION).display(),
                crate::gate::quoted(&entry.contract)
            )));
        }
        if let Some((_, first, by)) = seen.iter().find(|(held, _, _)| *held == resolved) {
            if *first == reference {
                continue;
            }
            return Err(Error::Invalid(format!(
                "{}: contract {} is vendored from {} (at {}) and from {} (at {}); one file holds \
                 one image's contract",
                chart_dir.join(DECLARATION).display(),
                crate::gate::quoted(&entry.contract),
                crate::gate::quoted(first),
                crate::gate::quoted(by),
                crate::gate::quoted(&reference),
                crate::gate::quoted(at)
            )));
        }
        seen.push((resolved.clone(), reference, at));

        let held = fetch_pinned(registry, image, app_version, signer)?;
        if write_vendored_in(
            chart_dir,
            &resolved,
            &held.image,
            &held.digest,
            &held.payload,
            &held.sha256,
            fetched,
        )? {
            into.changed.push(declared);
        } else {
            into.unchanged.push(declared);
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
    use crate::oci::carriers::testing::{
        AMD64, IMAGE, INDEX, Recorded, SIGNER, contract, digest_of, published,
    };

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
    fn a_declaration_that_leaves_its_image_to_the_defaults_is_still_refreshed() {
        // The slim declaration names no `images` at all: the image is the one at `image`, and its
        // contract is `contracts/<document>.json`. Reading the raw file found no image here and
        // reported nothing changed, which is a refresh that silently never happens.
        let chart = chart_with(&[
            (super::DECLARATION, "documents:\n  - name: api\n"),
            CHART,
            VALUES,
        ]);
        let registry = Recorded::of(&published());
        let mut refreshed = super::Refreshed::default();
        super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut refreshed)
            .expect("it refreshes");
        assert_eq!(refreshed.changed, vec![chart.path()]);
        assert_eq!(chart.read()["contract"], contract());
    }

    /// A chart directory holding the given files, removed with the guard.
    fn chart_with(files: &[(&str, &str)]) -> Vendored {
        let chart = Vendored::new();
        std::fs::create_dir_all(&chart.0).expect("the chart directory is created");
        for (name, text) in files {
            std::fs::write(chart.0.join(name), text).expect("the chart file is written");
        }
        chart
    }

    const CHART: (&str, &str) = ("Chart.yaml", "name: api\nversion: 1.0.0\n");
    const VALUES: (&str, &str) = (
        "values.yaml",
        "image:\n  repository: demo/api\n  tag: v1@sha256:aa\n",
    );

    #[test]
    fn a_contract_two_documents_share_is_fetched_once_and_then_left_alone() {
        let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: image\n        contract: contracts/api.json\n";
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
        let registry = Recorded::of(&published());

        let mut first = super::Refreshed::default();
        super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut first).expect("it refreshes");
        assert_eq!(first.changed, vec![chart.path()]);
        assert_eq!(
            first.unchanged,
            Vec::<std::path::PathBuf>::new(),
            "the second document names the same file"
        );

        let mut second = super::Refreshed::default();
        super::refresh_chart(&chart.0, &registry, SIGNER, "later", &mut second)
            .expect("it refreshes");
        assert_eq!(second.changed, Vec::<std::path::PathBuf>::new());
        assert_eq!(second.unchanged, vec![chart.path()]);
    }

    #[test]
    fn a_values_path_that_holds_no_image_block_is_refused_by_name() {
        let declaration = "documents:\n  - name: api\n    images:\n      - values: sidecar\n";
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
        let registry = Recorded::of(&published());
        let failure = super::refresh_chart(
            &chart.0,
            &registry,
            SIGNER,
            "t",
            &mut super::Refreshed::default(),
        )
        .expect_err("there is no image at `sidecar`");
        assert!(failure.to_string().contains("sidecar"), "{failure}");
        assert_eq!(
            *registry.asked.borrow(),
            Vec::<String>::new(),
            "nothing was fetched"
        );
    }

    #[test]
    fn a_second_document_sharing_a_contract_still_has_its_values_path_checked() {
        let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: sidecar\n        contract: contracts/api.json\n";
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
        let registry = Recorded::of(&published());
        let failure = super::refresh_chart(
            &chart.0,
            &registry,
            SIGNER,
            "t",
            &mut super::Refreshed::default(),
        )
        .expect_err("there is no image at `sidecar`");
        assert!(failure.to_string().contains("sidecar"), "{failure}");
    }

    #[test]
    fn two_images_vendored_into_one_contract_file_are_refused() {
        let declaration = "documents:\n  - name: api\n    images:\n      - values: image\n        \
                           contract: contracts/api.json\n  - name: worker\n    images:\n      \
                           - values: sidecar\n        contract: ./contracts/api.json\n";
        let values = (
            "values.yaml",
            "image:\n  repository: demo/api\n  tag: v1@sha256:aa\nsidecar:\n  repository: \
             demo/worker\n  tag: v1@sha256:bb\n",
        );
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, values]);
        let registry = Recorded::of(&published());
        let failure = super::refresh_chart(
            &chart.0,
            &registry,
            SIGNER,
            "t",
            &mut super::Refreshed::default(),
        )
        .expect_err("one file cannot hold two images' contracts");
        let message = failure.to_string();
        assert!(
            message.contains("demo/api") && message.contains("demo/worker"),
            "{message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_contracts_link_that_leaves_the_chart_is_refused_before_anything_is_fetched() {
        let declaration = "documents:\n  - name: api\n";
        let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
        let outside = Vendored::new();
        std::fs::create_dir_all(&outside.0).expect("a directory");
        std::fs::write(outside.0.join("api.json"), "untouched\n").expect("a file");
        std::os::unix::fs::symlink(&outside.0, chart.0.join("contracts")).expect("a link");
        let registry = Recorded::of(&published());

        let failure = super::refresh_chart(
            &chart.0,
            &registry,
            SIGNER,
            "t",
            &mut super::Refreshed::default(),
        )
        .expect_err("the link points out of the chart");

        assert!(
            failure.to_string().contains("outside the chart"),
            "{failure}"
        );
        assert_eq!(
            std::fs::read_to_string(outside.0.join("api.json")).expect("readable"),
            "untouched\n"
        );
        assert_eq!(
            *registry.asked.borrow(),
            Vec::<String>::new(),
            "nothing was fetched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_contract_is_refused_whether_or_not_it_points_anywhere() {
        let declaration = "documents:\n  - name: api\n";
        for (target, existing) in [("planted.json", false), ("values.yaml", true)] {
            let chart = chart_with(&[(super::DECLARATION, declaration), CHART, VALUES]);
            std::fs::create_dir_all(chart.0.join("contracts")).expect("a directory");
            std::os::unix::fs::symlink(chart.0.join(target), chart.path()).expect("a link");
            let registry = Recorded::of(&published());

            let failure = super::refresh_chart(
                &chart.0,
                &registry,
                SIGNER,
                "t",
                &mut super::Refreshed::default(),
            )
            .expect_err("the contract is a link");

            assert!(
                failure.to_string().contains("symbolic link"),
                "{target}: {failure}"
            );
            assert_eq!(
                chart.0.join(target).exists(),
                existing,
                "{target}: nothing was created through the link"
            );
            assert_eq!(
                std::fs::read_to_string(chart.0.join("values.yaml")).expect("readable"),
                VALUES.1,
                "{target}: the values file is untouched"
            );
            assert_eq!(
                *registry.asked.borrow(),
                Vec::<String>::new(),
                "{target}: nothing was fetched"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_write_itself_refuses_a_directory_that_became_a_link_after_the_check() {
        let chart = chart_with(&[CHART]);
        let outside = Vendored::new();
        std::fs::create_dir_all(&outside.0).expect("a directory");
        std::os::unix::fs::symlink(&outside.0, chart.0.join("contracts")).expect("a link");
        let payload = br#"{"version":1}"#;

        let failure = super::write_vendored_in(
            &chart.0,
            &chart.path(),
            IMAGE,
            INDEX,
            payload,
            &digest_of(payload),
            "t",
        )
        .expect_err("the link points out of the chart");

        assert!(
            failure.to_string().contains("outside the chart"),
            "{failure}"
        );
        assert!(!outside.0.join("api.json").exists(), "nothing was written");
    }

    #[test]
    fn a_chart_without_a_declaration_is_not_refreshed() {
        let chart = chart_with(&[CHART, VALUES]);
        let registry = Recorded::of(&published());
        let mut refreshed = super::Refreshed::default();
        super::refresh_chart(&chart.0, &registry, SIGNER, "t", &mut refreshed)
            .expect("there is nothing to do");
        assert_eq!(refreshed.changed, Vec::<std::path::PathBuf>::new());
        assert_eq!(refreshed.unchanged, Vec::<std::path::PathBuf>::new());
        assert_eq!(
            *registry.asked.borrow(),
            Vec::<String>::new(),
            "nothing was fetched"
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
