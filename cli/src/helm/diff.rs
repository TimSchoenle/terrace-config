//! How a chart's vendored contracts changed against another revision, and what it costs.
//!
//! A refresh runs inside the pull request that repins the digest, so the new document and the bump
//! arrive together — which is the design's whole point, and also why the reviewer is looking at a
//! large reordered JSON diff instead of a sentence. This is the sentence: what the image gained,
//! what it dropped, what moved out from under the chart, and the smallest chart version bump that
//! set of findings justifies.
//!
//! The walk and the plumbing live here; every rule about what a difference *means* lives in
//! [`crate::diff`], which is why each rule is testable by calling it with two documents.
//!
//! # Offline, like every rule that reads a contract
//!
//! The old bytes come from version control and never from a registry, so this answers the same way
//! on a laptop with no network and on a re-run of an old commit, and it cannot disagree with the
//! committed file about what the previous revision said. The reader is a trait, so a test supplies
//! two documents rather than a repository.
//!
//! # The working tree is one of the two sides, deliberately
//!
//! Not the index and not the tip: a refresh writes files, and a contract for a newly declared
//! document is untracked until somebody adds it. Comparing against the tip would report nothing for
//! exactly the change most worth reporting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value as Json;

use crate::diff::{ContractDiff, Severity, diff_contract, observed_bump, suggest_version, worst};
use crate::error::Error;
use crate::report::{Report, error};

/// Where the other side of the comparison comes from.
///
/// A trait because the plumbing is the one part of this that needs a subprocess, and a rule that
/// needed one could not be tested by calling it. The implementation that shells out to version
/// control is [`Committed`]; a test supplies a map.
pub trait Revision {
    /// One file's bytes at that revision, or [`None`] when it did not exist there.
    fn read(&self, path: &str) -> Option<String>;

    /// Every vendored contract that existed at that revision, as repository-relative paths.
    fn contracts(&self, charts: &Path) -> BTreeSet<String>;

    /// How the revision should be named in a message.
    fn name(&self) -> &str;
}

/// Every contract a chart carries, and what the set of them implies for its version.
///
/// Both versions are reported, because the reviewer's real question is not "what bump does this
/// deserve" but "is the bump already in this branch big enough", and only the pair answers that.
#[derive(Debug, Clone)]
pub struct ChartDiff {
    /// The chart.
    pub chart: String,
    /// Each of its contracts, compared.
    pub contracts: Vec<ContractDiff>,
    /// Its version at the comparison revision.
    pub old_version: Option<String>,
    /// Its version in the working tree.
    pub new_version: Option<String>,
}

impl ChartDiff {
    /// The largest impact among its contracts.
    pub fn impact(&self) -> Severity {
        worst(self.contracts.iter().map(ContractDiff::impact))
    }

    /// The findings that set the impact — the "because" the suggestion is shown with.
    pub fn drivers(&self) -> Vec<&crate::diff::Change> {
        let level = self.impact();
        if level == Severity::None {
            return Vec::new();
        }
        self.contracts
            .iter()
            .flat_map(|contract| contract.changes.iter())
            .filter(|change| change.severity == level)
            .collect()
    }

    /// Whether the bump already in the branch is at least as large as the suggested one.
    ///
    /// [`None`] when it cannot be decided — a version this cannot read, or a chart whose file did
    /// not exist at the comparison revision.
    pub fn satisfied(&self) -> Option<bool> {
        if self.impact() == Severity::None {
            return Some(true);
        }
        let observed = observed_bump(self.old_version.as_deref(), self.new_version.as_deref())?;
        Some(observed >= self.impact())
    }

    /// This chart's diff as JSON.
    pub fn as_json(&self) -> Json {
        serde_json::json!({
            "chart": self.chart,
            "impact": self.impact().label(),
            // References rather than copies: every driver is already below, and a driver is an
            // added or removed declaration whose full entry is the largest object in the document.
            // A headline paragraph needs the sentence and the name, not the entry.
            "drivers": self.drivers().iter().map(|change| serde_json::json!({
                "area": change.area.label(),
                "kind": change.kind.label(),
                "subject": change.subject,
                "field": change.field,
                "message": change.message,
            })).collect::<Vec<_>>(),
            "version": {
                "old": self.old_version,
                "new": self.new_version,
                "bumped": self.old_version != self.new_version,
                "suggested": suggest_version(self.new_version.as_deref(), self.impact()),
                "satisfied": self.satisfied(),
            },
            "contracts": self.contracts.iter().map(ContractDiff::as_json).collect::<Vec<_>>(),
        })
    }
}

/// What a run of [`collect`] found.
#[derive(Debug)]
pub struct Diffed {
    /// One entry per chart carrying a contract at either revision.
    pub charts: Vec<ChartDiff>,
    /// Files that could not be read as a vendored contract, on either side.
    pub report: Report,
}

impl Diffed {
    /// The largest impact anywhere.
    pub fn impact(&self) -> Severity {
        worst(self.charts.iter().map(ChartDiff::impact))
    }

    /// Whether anything differs at all.
    pub fn changed(&self) -> bool {
        self.impact() != Severity::None
    }
}

/// Every chart carrying a contract at either revision, and the diff of each of its contracts.
///
/// Contracts are found by walking the *files* rather than by reading a declaration. The unit being
/// compared is the vendored document, and a document whose declaration was deleted is one of the
/// cases most worth reporting — reading the declaration to find the files would hide exactly that.
///
/// # Errors
/// [`Error::Io`] when the chart tree cannot be walked.
pub fn collect(
    charts: &Path,
    revision: &dyn Revision,
    only: Option<&str>,
) -> Result<Diffed, Error> {
    let mut report = Report::new();
    let old_paths = revision.contracts(charts);
    let new_paths = vendored_paths(charts)?;

    let mut charted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for spec in old_paths.union(&new_paths) {
        let Some(chart) = chart_of(spec) else {
            continue;
        };
        if only.is_some_and(|named| named != chart) {
            continue;
        }
        charted
            .entry(chart.to_owned())
            .or_default()
            .push(spec.clone());
    }

    let mut found = Vec::new();
    for (chart, specs) in charted {
        let chart_yaml = charts.join(&chart).join("Chart.yaml");
        let diff = ChartDiff {
            old_version: chart_version(revision.read(&spec_of(&chart_yaml, charts)).as_deref()),
            new_version: chart_version(std::fs::read_to_string(&chart_yaml).ok().as_deref()),
            contracts: specs
                .iter()
                .filter_map(|spec| compare(&chart, spec, charts, revision, &mut report))
                .collect(),
            chart,
        };
        found.push(diff);
    }

    Ok(Diffed {
        charts: found,
        report,
    })
}

/// One contract at both revisions, or [`None`] when neither side can be read as one.
///
/// A document that is not readable is reported and skipped rather than raised, matching what every
/// other rule here does: one broken file must not hide the state of the rest. Nothing beyond "it is
/// an object with a contract in it" is asserted, because reporting an envelope nothing could
/// validate is this tool's job rather than its failure mode.
fn compare(
    chart: &str,
    spec: &str,
    charts: &Path,
    revision: &dyn Revision,
    report: &mut Report,
) -> Option<ContractDiff> {
    let root = repository_root(charts);
    let old = parse(
        revision.read(spec).as_deref(),
        &format!("{spec} at {}", revision.name()),
        report,
    );
    let new = parse(
        std::fs::read_to_string(root.join(spec)).ok().as_deref(),
        spec,
        report,
    );
    if old.is_none() && new.is_none() {
        return None;
    }
    let name = Path::new(spec)
        .file_stem()
        .map_or_else(String::new, |held| held.to_string_lossy().into_owned());
    Some(diff_contract(
        chart,
        &name,
        spec,
        old.as_ref(),
        new.as_ref(),
    ))
}

/// One vendored document, or [`None`] with the reason reported.
fn parse(text: Option<&str>, origin: &str, report: &mut Report) -> Option<Json> {
    let text = text?;
    match serde_json::from_str::<Json>(text) {
        Err(failure) => {
            report.add(origin, error(format!("is not valid JSON: {failure}")));
            None
        }
        Ok(document) if document.get("contract").is_some_and(Json::is_object) => Some(document),
        Ok(_) => {
            report.add(
                origin,
                error("is not a vendored contract: no `contract` object"),
            );
            None
        }
    }
}

/// The `version` out of a chart's own file, or [`None`] when there is not one to read.
fn chart_version(text: Option<&str>) -> Option<String> {
    let parsed: Json = serde_norway::from_str(text?).ok()?;
    parsed.get("version")?.as_str().map(str::to_owned)
}

/// Every vendored contract in the working tree, as repository-relative paths.
fn vendored_paths(charts: &Path) -> Result<BTreeSet<String>, Error> {
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(charts).map_err(|e| Error::io(charts.display(), e))? {
        let chart = entry.map_err(|e| Error::io(charts.display(), e))?.path();
        let contracts = chart.join("contracts");
        if !contracts.is_dir() {
            continue;
        }
        for held in std::fs::read_dir(&contracts).map_err(|e| Error::io(contracts.display(), e))? {
            let path = held.map_err(|e| Error::io(contracts.display(), e))?.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
                found.insert(spec_of(&path, charts));
            }
        }
    }
    Ok(found)
}

/// A repository-relative, forward-slashed path — the only spelling version control accepts.
fn spec_of(path: &Path, charts: &Path) -> String {
    let root = repository_root(charts);
    let relative = path.strip_prefix(&root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

/// The directory the chart tree sits in, which every path is spelled relative to.
fn repository_root(charts: &Path) -> std::path::PathBuf {
    charts
        .parent()
        .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf)
}

/// The chart one contract path belongs to.
fn chart_of(spec: &str) -> Option<&str> {
    let mut parts = spec.rsplit('/');
    parts.next()?;
    parts.next()?;
    parts.next()
}

// ------------------------------------------------------------------------------------------
// The plumbing
// ------------------------------------------------------------------------------------------

/// A revision read out of version control, by shelling out to it.
///
/// The one part of this that needs a subprocess, and the reason [`Revision`] is a trait: every rule
/// above is a function of two documents, and none of them should need a repository to be tested.
pub struct Committed {
    reference: String,
    commit: String,
    root: std::path::PathBuf,
}

impl Committed {
    /// Resolve one reference against the working directory.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is no such revision, or when this is not a repository.
    pub fn resolve(reference: &str) -> Result<Self, Error> {
        Self::resolve_in(Path::new("."), reference)
    }

    /// Resolve one reference against a named checkout.
    ///
    /// Separate from [`Self::resolve`] because a comparison harness legitimately points at a
    /// repository that is not the one it is running in, and threading a directory through is a
    /// smaller thing to carry than a changed working directory every later call would inherit.
    ///
    /// # Errors
    /// [`Error::Invalid`] when there is no such revision, or when this is not a repository.
    pub fn resolve_in(directory: &Path, reference: &str) -> Result<Self, Error> {
        let root = run(&["rev-parse", "--show-toplevel"], directory)?;
        let root = std::path::PathBuf::from(root.trim());
        let commit = run(
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
            &root,
        )
        .map_err(|_| {
            Error::Invalid(format!(
                "{reference}: no such revision. Name one that exists, or fetch the default branch                  if this is a shallow checkout."
            ))
        })?;
        Ok(Self {
            reference: reference.to_owned(),
            commit: commit.trim().to_owned(),
            root,
        })
    }

    /// The commit the reference resolved to, for a report that has to be reproducible.
    pub fn commit(&self) -> &str {
        &self.commit
    }
}

impl Revision for Committed {
    fn read(&self, path: &str) -> Option<String> {
        run(&["show", &format!("{}:{path}", self.commit)], &self.root).ok()
    }

    fn contracts(&self, charts: &Path) -> BTreeSet<String> {
        let pattern = format!(
            "{}/*/contracts/*.json",
            spec_of(charts, charts.parent().unwrap_or(charts))
        );
        let listed = run(
            &["ls-tree", "-r", "--name-only", &self.commit, "--", &pattern],
            &self.root,
        );
        listed
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn name(&self) -> &str {
        &self.reference
    }
}

/// One version-control command, with its output.
fn run(arguments: &[&str], directory: &Path) -> Result<String, Error> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .map_err(|failure| {
            Error::Invalid(format!(
                "git is not runnable, and the other side of this comparison comes from it: \
                 {failure}"
            ))
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Invalid(format!(
            "git {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use serde_json::json;

    use super::{ChartDiff, Revision, chart_of, chart_version};
    use crate::diff::{Severity, diff_contract};

    /// A revision that is a map, which is what makes every rule above testable without a
    /// repository.
    struct Stored(BTreeMap<String, String>);

    impl Revision for Stored {
        fn read(&self, path: &str) -> Option<String> {
            self.0.get(path).cloned()
        }
        fn contracts(&self, _charts: &Path) -> BTreeSet<String> {
            self.0.keys().cloned().collect()
        }
        fn name(&self) -> &'static str {
            "stored"
        }
    }

    fn vendored(app_version: &str) -> serde_json::Value {
        json!({
            "source": {"image": "i", "digest": "sha256:0", "sha256": "a", "fetched": "t"},
            "contract": {
                "terrace_contract": 1,
                "app": {"name": "x", "version": app_version},
                "schema": {"schema_version": 2, "dialect": {}, "loader": [], "keys": []},
                "json_schema": {},
                "external": {"env": [], "ignore": [], "unknown": "reject"},
            },
        })
    }

    fn chart(old: Option<&str>, new: Option<&str>, impact: Severity) -> ChartDiff {
        let contracts = if impact == Severity::None {
            Vec::new()
        } else {
            vec![diff_contract("c", "app", "p", None, Some(&vendored("1")))]
        };
        ChartDiff {
            chart: "c".to_owned(),
            contracts,
            old_version: old.map(str::to_owned),
            new_version: new.map(str::to_owned),
        }
    }

    #[test]
    fn the_revision_is_a_trait_so_a_rule_needs_no_repository() {
        let stored = Stored(BTreeMap::from([(
            "charts/c/contracts/app.json".to_owned(),
            vendored("1").to_string(),
        )]));
        assert!(stored.read("charts/c/contracts/app.json").is_some());
        assert!(stored.read("charts/c/contracts/gone.json").is_none());
        assert_eq!(stored.contracts(Path::new("charts")).len(), 1);
    }

    #[test]
    fn a_chart_is_read_from_the_path_the_contract_sits_at() {
        assert_eq!(
            chart_of("charts/tankovault/contracts/api.json"),
            Some("tankovault")
        );
        assert_eq!(chart_of("api.json"), None);
    }

    #[test]
    fn a_version_this_cannot_read_is_absent_rather_than_guessed() {
        assert_eq!(
            chart_version(Some("version: 1.2.3\n")).as_deref(),
            Some("1.2.3")
        );
        assert_eq!(chart_version(Some("version: [1]\n")), None);
        assert_eq!(chart_version(Some("{{{")), None);
        assert_eq!(chart_version(None), None);
    }

    #[test]
    fn the_bump_already_in_the_branch_is_what_decides_whether_it_is_enough() {
        // The reviewer's question is not "what bump does this deserve" but "is the one already
        // here big enough", so only the pair answers it.
        let unmoved = chart(Some("1.0.0"), Some("1.0.0"), Severity::Minor);
        assert_eq!(unmoved.satisfied(), Some(false));

        let moved = chart(Some("1.0.0"), Some("1.1.0"), Severity::Minor);
        assert_eq!(moved.satisfied(), Some(true));

        // A chart whose file did not exist at the comparison revision cannot be decided.
        let arrived = chart(None, Some("1.0.0"), Severity::Minor);
        assert_eq!(arrived.satisfied(), None);
    }

    #[test]
    fn a_chart_with_nothing_to_report_is_always_satisfied() {
        assert_eq!(
            chart(Some("1.0.0"), Some("1.0.0"), Severity::None).satisfied(),
            Some(true)
        );
    }

    #[test]
    fn the_drivers_are_the_findings_that_set_the_impact() {
        let held = chart(Some("1.0.0"), Some("1.0.0"), Severity::Minor);
        let drivers = held.drivers();
        assert_eq!(drivers.len(), 1);
        assert_eq!(drivers[0].severity, held.impact());
    }
}
