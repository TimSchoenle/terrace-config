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
mod tests;
