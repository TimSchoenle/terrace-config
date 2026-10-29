//! Writing the configuration half of a new chart: the files its image's contract decides.
//!
//! A chart in a repository of this shape is two things stacked. Underneath is a chassis (a values
//! block per concern, a handful of templates, a pod spec wired to a library chart), which is the
//! same in every chart of that repository and belongs to it: the repository keeps it as a template
//! and copies it. On top is the part that is about *this* image: the settings it reads, what each
//! one accepts, which of them are credentials, and how a chart value reaches each one. That half is
//! written by reading a contract, and reading a 400 KB contract by hand is how a key gets missed.
//!
//! This writes the second half, into a chart directory the repository's own template copy may
//! already have populated:
//!
//! | file | what it is |
//! |---|---|
//! | `contracts/<document>.json` | the image's contract, fetched and verified exactly as a refresh does |
//! | `values.yaml` | the image pin, a block per setting with the marker that binds it, then the chassis |
//! | `templates/_helpers.tpl` | the partials that project the values into the document and the Secret |
//! | `config-contract.yaml` | the declaration every contract gate reads, enrolling the round-trip suites |
//! | `ci/test-values.yaml` | the credentials the render guard demands, and nothing else |
//! | `tests/configmap_test.yaml` | a first suite, for a person to extend |
//!
//! For an image that publishes no contract (`--no-contract`) the same files are written in their
//! plain form, and `config-contract.yaml` only when the image is first-party, as
//! [`owes_a_declaration`] decides.
//!
//! # The placeholders a contract decides
//!
//! A repository's workload template has to name things only the contract knows: the environment
//! prefix the loader owns, the separator in a secrets-file name, the `ConfigMap` key the document
//! is rendered under. The template spells them as `%%PREFIX%%`, `%%ENV_PREFIX%%`, `%%SEPARATOR%%`
//! and `%%DOCUMENT_KEY%%`, a form Helm never reads, so the Go template braces around them survive,
//! and this fills them in every file of the chart, from the dialect the fetched contract declares.
//! Every other `%%NAME%%` is the repository's to substitute when it copies its template, and one
//! still present when this runs is refused: a placeholder nobody replaced would reach the chart as
//! literal text and render as itself.
//!
//! That split is the seam between the two halves. The repository's copy needs only the chart's name
//! and the facts it was given; everything learnt from the contract is filled here, so a copy can be a
//! `sed` and nothing in it has to read JSON.
//!
//! # Never a second path into `contracts/`
//!
//! The contract is fetched through [`super::pull`], and only through it: [`Source::Contract`] holds a
//! [`Fetched`], which nothing but [`super::pull::fetch_pinned`] constructs, so a contract this writes
//! has been through the same signature verification, the same label agreement and the same envelope
//! as every contract a refresh has ever written. A contract that arrived some other way would be
//! trusted by every gate downstream.
//!
//! # It replaces nothing
//!
//! Every file is planned in memory before one is written, and a file that is already there and says
//! something else is refused rather than overwritten: a chart that has a declaration is a chart
//! somebody configured, and [`super::adopt`] is what writes into one. A file already holding
//! exactly what would be written is left alone, so a run over a chart this has just written owes
//! nothing; the vendored contract counts as unchanged when only its fetch time moved, as a refresh
//! counts it.
//!
//! # What is left for a person
//!
//! Printed by [`Init::closing`] as an ordered list rather than left to be discovered: the description
//! on each grouping block, because a contract describes keys and says nothing about the blocks a
//! chart groups them into; the `unbound` reasons, each defensible and none considered; and, where
//! the image publishes no default for a key it requires, the value that had to be invented.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Value as Json, json};

use crate::classify::matches_ignore;
use crate::error::Error;
use crate::union::union_contracts;

use super::DECLARATION;
use super::adopt::posix;
use super::declaration::parse_declaration;
use super::files;
use super::patch::{created, replaced, unified};
use super::pull::{Fetched, same_contract};
use super::scaffold::chart::{
    CONTRACTS, Surface, render_ci_values, render_declaration, render_helpers, render_opt_out,
    render_unit_test, render_values, undescribed,
};
use super::scaffold::{WIDTH, check_chart_name, env_prefix, invented, wrap};

/// The document key a chart renders its configuration under when nothing else is asked for.
pub const DEFAULT_DOCUMENT_KEY: &str = "config.toml";

/// The image a new chart pins, as its `values.yaml` spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The repository, exactly as written: the registry-less form for Docker Hub, fully qualified
    /// elsewhere.
    pub repository: String,
    /// The tag, with the digest after an `@` when it was given one.
    pub tag: String,
}

impl Pin {
    /// Reads `REPOSITORY:TAG[@DIGEST]`.
    ///
    /// The tag is required. A digest is not, because the refresh resolves a tag, but a chart pinned
    /// by tag alone carries a mutable reference, and the document gate refuses to validate against
    /// a contract that cannot be tied to a digest, so the digest is what should be given.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a reference with no repository or no tag.
    pub fn parse(reference: &str) -> Result<Self, Error> {
        let (named, digest) = match reference.split_once('@') {
            Some((named, digest)) => (named, Some(digest)),
            None => (reference, None),
        };
        let slash = named.rfind('/').map_or(0, |at| at + 1);
        let split = named[slash..]
            .rfind(':')
            .map(|at| (&named[..slash + at], &named[slash + at + 1..]));
        let refuse = || {
            Error::Invalid(format!(
                "{}: expected REPOSITORY:TAG, optionally followed by @DIGEST — the image as \
                 values.yaml pins it, ideally `timschoenle/foo:v1.2.3@sha256:...`",
                crate::text::quoted(reference)
            ))
        };
        let Some((repository, tag)) = split.filter(|(repository, tag)| {
            !repository.is_empty() && !tag.is_empty() && digest.is_none_or(|held| !held.is_empty())
        }) else {
            return Err(refuse());
        };
        Ok(Self {
            repository: repository.to_owned(),
            tag: digest.map_or_else(|| tag.to_owned(), |digest| format!("{tag}@{digest}")),
        })
    }

    /// The pin as the image block of a values file, which is what a refresh resolves.
    pub fn block(&self) -> Json {
        json!({"registry": "", "repository": self.repository, "tag": self.tag})
    }
}

/// Whether the coverage gate will demand a declaration from a chart pinning `repository`.
///
/// The gate's own question, asked with the same pattern matcher: a chart pinning a first-party
/// image owes a declaration (a real one, or an explicit opt-out with a reason), and a chart pinning
/// a third-party image owes nothing, because nobody publishes a contract for it and a file that
/// could only say "no" is a file nobody should be asked to write. An empty list makes nothing
/// first-party.
pub fn owes_a_declaration(first_party: &[String], repository: &str) -> bool {
    first_party
        .iter()
        .any(|pattern| matches_ignore(pattern, repository))
}

/// Where a chart's configuration surface comes from.
#[derive(Debug, Clone)]
pub enum Source<'a> {
    /// The image publishes a contract, fetched and verified, and the moment it was fetched.
    Contract {
        /// The contract and its provenance.
        fetched: &'a Fetched,
        /// When it was fetched, as the vendored file records it.
        at: &'a str,
    },
    /// The image publishes none.
    Plain {
        /// The reason the image publishes no contract, for the opt-out a first-party image owes;
        /// [`None`] for an image that owes no declaration.
        opt_out: Option<&'a str>,
    },
}

/// What a new chart is, apart from the contract.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// The chart's name, which is also its directory under the chart tree.
    pub chart: &'a str,
    /// The image it pins.
    pub pin: &'a Pin,
    /// The configuration document's name in the declaration.
    pub document: &'a str,
    /// The `ConfigMap` key the document is rendered under.
    pub document_key: &'a str,
    /// The values every chart of the repository shares, appended after the configuration surface.
    pub chassis: &'a str,
}

/// What one file would become.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// It does not exist and is written.
    Create,
    /// It exists, and these placeholders in it are filled.
    Fill(Vec<&'static str>),
    /// It already holds what would be written.
    Unchanged,
}

/// One file, as it is and as it would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The file.
    pub path: PathBuf,
    /// What happens to it.
    pub action: Action,
    /// Its text now, when it exists.
    pub before: Option<String>,
    /// Its text after.
    pub after: String,
}

/// A planned init: every file it would write, and what is left for a person afterwards.
#[derive(Debug, Clone)]
pub struct Init {
    /// The chart directory.
    pub chart_dir: PathBuf,
    /// The configuration surface the files were rendered from.
    pub surface: Surface,
    /// Every file, generated ones first in the order they are written, then the filled ones.
    pub changes: Vec<Change>,
    /// The grouping blocks written with a placeholder description, as values paths.
    pub undescribed: Vec<String>,
    /// The values chosen here because the image publishes no default for a key it requires.
    pub invented: Vec<String>,
}

/// The placeholders this fills, and the ones it refuses to find left over.
const FILLED: [&str; 4] = ["DOCUMENT_KEY", "ENV_PREFIX", "PREFIX", "SEPARATOR"];

/// Directories of a chart that hold no template text: resolved dependencies and vendored contracts.
const NOT_TEMPLATES: [&str; 2] = ["charts", CONTRACTS];

/// Plan the configuration half of one new chart.
///
/// Nothing is written: the returned plan is what `--check`, `--diff` and `--write` all read, so the
/// diff a reviewer is shown is the edit that would be made.
///
/// # Errors
/// [`Error::Invalid`] for a chart name Helm would refuse; a contract that declares no nesting
/// separator or whose keys cannot all become chart values; a first-party image with no contract and
/// no reason; a placeholder the repository's copy left unsubstituted; and a file already there that
/// says something else. [`Error::Io`] when the chart directory cannot be read.
pub fn plan(charts: &Path, request: &Request<'_>, source: &Source<'_>) -> Result<Init, Error> {
    let Request {
        chart,
        pin,
        document,
        document_key,
        chassis,
    } = request;
    check_chart_name(chart)?;
    let chart_dir = charts.join(chart);
    let contract_file = format!("{CONTRACTS}/{document}.json");
    let contract_path = chart_dir.join(&contract_file);

    let (surface, vendored) = match source {
        Source::Contract { fetched, at } => {
            let label = posix(&contract_path);
            let union = union_contracts(&[(label.clone(), fetched.contract()?)])?;
            let surface = Surface::from_contract(&union)
                .map_err(|failure| Error::Invalid(format!("{label}: {failure}")))?;
            (surface, Some(fetched.vendored(&contract_path, at)?))
        }
        Source::Plain { .. } => (Surface::Plain, None),
    };
    let plan = surface.plan();

    let mut generated: Vec<(PathBuf, String)> = Vec::new();
    if let Some(text) = vendored {
        generated.push((contract_path.clone(), text));
    }
    generated.push((
        chart_dir.join("values.yaml"),
        render_values(
            chart,
            &surface,
            &pin.repository,
            &pin.tag,
            chassis,
            document_key,
        )?,
    ));
    generated.push((
        chart_dir.join("templates").join("_helpers.tpl"),
        render_helpers(chart, &surface)?,
    ));
    match source {
        Source::Contract { .. } => {
            let declaration = render_declaration(chart, document, document_key, plan)?;
            // Read back through the reader every gate uses. A file written here that no reader
            // accepts fails on the contributor's first run rather than here, with a message about
            // a file they did not write by hand.
            parse_declaration(&declaration, &chart_dir)?;
            generated.push((chart_dir.join(DECLARATION), declaration));
        }
        Source::Plain {
            opt_out: Some(reason),
        } => {
            let declaration = render_opt_out(&pin.repository, reason);
            parse_declaration(&declaration, &chart_dir)?;
            generated.push((chart_dir.join(DECLARATION), declaration));
        }
        Source::Plain { opt_out: None } => {}
    }
    generated.push((
        chart_dir.join("ci").join("test-values.yaml"),
        render_ci_values(chart, &surface)?,
    ));
    if surface.contracted() {
        generated.push((
            chart_dir.join("tests").join("configmap_test.yaml"),
            render_unit_test(chart, document_key, plan)?,
        ));
    }

    let mut changes = classify(&chart_dir, &contract_path, generated)?;
    let written: Vec<PathBuf> = changes.iter().map(|change| change.path.clone()).collect();
    changes.extend(fills(
        &chart_dir,
        &written,
        &placeholders(chart, &surface, document_key),
    )?);

    Ok(Init {
        undescribed: undescribed(plan)?,
        invented: invented(plan)
            .into_iter()
            .map(|item| item.values_path.clone())
            .collect(),
        chart_dir,
        surface,
        changes,
    })
}

/// What each generated file would become, against what is on disk.
///
/// Every conflict is collected before one is raised, so a chart that already has a configuration is
/// refused once, naming every file, rather than once per run.
///
/// # Errors
/// [`Error::Invalid`] naming every file already there that says something else; [`Error::Io`] when
/// one cannot be read.
fn classify(
    chart_dir: &Path,
    contract_path: &Path,
    generated: Vec<(PathBuf, String)>,
) -> Result<Vec<Change>, Error> {
    let mut changes = Vec::new();
    let mut conflicts = Vec::new();
    for (path, after) in generated {
        let before = read_if_present(&path)?;
        let action = match &before {
            None => Action::Create,
            Some(held) if *held == after => Action::Unchanged,
            Some(held) if path == contract_path && same_contract(held, &after) => Action::Unchanged,
            Some(_) => {
                conflicts.push(posix(&path));
                continue;
            }
        };
        changes.push(Change {
            path,
            action,
            before,
            after,
        });
    }
    if conflicts.is_empty() {
        return Ok(changes);
    }
    Err(Error::Invalid(format!(
        "{}: already holds {} that init would write differently: {}. Init writes the \
         configuration of a chart that has none, and replaces nothing — `adopt` writes what an \
         existing chart is owed",
        posix(chart_dir),
        if conflicts.len() == 1 {
            "a file"
        } else {
            "files"
        },
        conflicts.join(", ")
    )))
}

/// A file's text, or [`None`] when there is no file.
fn read_if_present(path: &Path) -> Result<Option<String>, Error> {
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|e| Error::io(path.display(), e))
}

/// The value each filled placeholder takes.
///
/// From the contract's dialect for a contracted chart. A plain chart's image declares no dialect,
/// so the chart name stands in (`netcup-offer-bot` as `NETCUP_OFFER_BOT_`), which is a guess, and
/// only ever reaches the prose of a template that has no loader to describe.
fn placeholders(chart: &str, surface: &Surface, document_key: &str) -> Vec<(&'static str, String)> {
    let (prefix, separator) = match surface {
        Surface::Contracted { dialect, .. } => {
            (dialect.prefix.clone(), dialect.nesting_separator.clone())
        }
        Surface::Plain => (
            format!("{}_", chart.replace('-', "_").to_uppercase()),
            "__".to_owned(),
        ),
    };
    vec![
        ("DOCUMENT_KEY", document_key.to_owned()),
        ("ENV_PREFIX", env_prefix(&prefix).to_owned()),
        ("PREFIX", prefix),
        ("SEPARATOR", separator),
    ]
}

/// A `%%NAME%%` placeholder, compiled once.
fn placeholder() -> &'static Regex {
    static HELD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    HELD.get_or_init(|| Regex::new(r"%%([A-Z][A-Z0-9_]*)%%").expect("a literal pattern compiles"))
}

/// Every file of the chart that holds one of [`FILLED`], filled.
///
/// Walks the whole chart rather than a list of template names, because which files carry a
/// placeholder is the repository's template's business. Skips the directories that hold no template
/// text, the files this run generates, and anything that is not UTF-8.
///
/// # Errors
/// [`Error::Invalid`] for a value that would break a line, or a `%%NAME%%` left over that this
/// does not fill; [`Error::Io`] when the chart cannot be walked.
fn fills(
    chart_dir: &Path,
    generated: &[PathBuf],
    values: &[(&'static str, String)],
) -> Result<Vec<Change>, Error> {
    if let Some((name, _)) = values
        .iter()
        .find(|(_, value)| value.contains(['\n', '\r']))
    {
        return Err(Error::Invalid(format!(
            "the value for %%{name}%% spans lines, so filling it would reshape the template it \
             lands in"
        )));
    }

    let mut changes = Vec::new();
    for path in files_under(chart_dir)? {
        if generated.contains(&path) {
            continue;
        }
        let Ok(before) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut after = before.clone();
        let mut filled: Vec<&'static str> = Vec::new();
        for (name, value) in values {
            let marker = format!("%%{name}%%");
            if after.contains(&marker) {
                after = after.replace(&marker, value);
                filled.push(name);
            }
        }
        if let Some(left) = placeholder().find(&after) {
            return Err(Error::Invalid(format!(
                "{}: a template placeholder was not substituted: {}. Init fills {} from the \
                 contract; every other placeholder is the template copy's to substitute",
                posix(&path),
                left.as_str(),
                FILLED
                    .iter()
                    .map(|name| format!("%%{name}%%"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if !filled.is_empty() {
            changes.push(Change {
                path,
                action: Action::Fill(filled),
                before: Some(before),
                after,
            });
        }
    }
    Ok(changes)
}

/// Every regular file under a chart, sorted, outside [`NOT_TEMPLATES`]. Symbolic links are not
/// followed: a link out of the chart is not the chart's text to fill.
fn files_under(chart_dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    if !chart_dir.is_dir() {
        return Ok(found);
    }
    let mut pending = vec![chart_dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries =
            std::fs::read_dir(&directory).map_err(|e| Error::io(directory.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(directory.display(), e))?;
            let kind = entry
                .file_type()
                .map_err(|e| Error::io(entry.path().display(), e))?;
            let path = entry.path();
            if kind.is_dir() {
                let top = directory == chart_dir;
                let skipped = NOT_TEMPLATES
                    .iter()
                    .any(|name| top && entry.file_name() == *name);
                if !skipped {
                    pending.push(path);
                }
            } else if kind.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

impl Init {
    /// Whether anything would be written.
    pub fn owed(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.action != Action::Unchanged)
    }

    /// The plan, one line per file, for a person deciding whether to run `--write`.
    pub fn report(&self) -> String {
        let shown = posix(&self.chart_dir);
        if !self.owed() {
            return format!(
                "==> {shown}: nothing to write; every file init writes already says what it would \
                 write\n"
            );
        }
        let mut out = format!("==> {shown}: the {} form\n", self.surface.form());
        for change in &self.changes {
            let path = posix(&change.path);
            let _ = match &change.action {
                Action::Create => writeln!(out, "    create     {path}"),
                Action::Fill(names) => writeln!(
                    out,
                    "    fill       {path} ({})",
                    names
                        .iter()
                        .map(|name| format!("%%{name}%%"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Action::Unchanged => writeln!(out, "    unchanged  {path}"),
            };
        }
        out
    }

    /// Every change as a unified diff, `patch -p0` style: a new file against `/dev/null`, a filled
    /// one against itself.
    pub fn diff(&self) -> String {
        self.changes
            .iter()
            .map(|change| {
                let path = posix(&change.path);
                match (&change.action, &change.before) {
                    (Action::Unchanged, _) => String::new(),
                    (_, Some(before)) => unified(&path, &path, &replaced(before, &change.after)),
                    (_, None) => unified("/dev/null", &path, &created(&change.after)),
                }
            })
            .collect()
    }

    /// Writes every file that changes, creating the directories it needs, all of them or none.
    ///
    /// # Errors
    /// [`Error::Io`] when a directory or a file cannot be written, and [`Error::Invalid`] when one
    /// resolves outside the chart. No file has changed, unless putting one back failed too, as
    /// [`files::write_all`] describes.
    pub fn write(&self) -> Result<(), Error> {
        let planned: Vec<files::Planned> = self
            .changes
            .iter()
            .filter(|change| change.action != Action::Unchanged)
            .map(|change| files::Planned {
                chart: self.chart_dir.clone(),
                path: change.path.clone(),
                text: change.after.clone(),
            })
            .collect();
        if planned.is_empty() {
            return Ok(());
        }
        // The chart directory is the boundary every file is held inside, so it has to exist
        // before anything can be resolved against it.
        std::fs::create_dir_all(&self.chart_dir)
            .map_err(|e| Error::io(self.chart_dir.display(), e))?;
        files::write_all(&planned)
    }

    /// The ordered list of what a person still has to do, once the files are written.
    ///
    /// Only the steps the contract decides. What the repository's own half leaves (its chart
    /// description, its README prose, the recipes that generate its schema and its README) is the
    /// repository's to say, after this.
    pub fn closing(&self) -> String {
        let shown = posix(&self.chart_dir);
        let mut out = format!("\n==> {shown}: configuration written\n\n");
        let plan = self.surface.plan();
        let mut steps: Vec<String> = Vec::new();

        if self.surface.contracted() {
            let _ = writeln!(
                out,
                "    {} setting(s) surfaced as chart values, {} credential(s) delivered as files, \
                 {} key(s) written off\n",
                plan.projected.len(),
                plan.credentials.len(),
                plan.written_off.len()
            );
            steps.push(
                "Add whatever the workload needs beyond a Deployment — a Service, an Ingress, a \
                 PodMonitor, a volume. None of that is in the contract."
                    .to_owned(),
            );
            if !self.undescribed.is_empty() {
                steps.push(format!(
                    "Replace the placeholder description on each grouping block in \
                     {shown}/values.yaml. A contract describes keys, not the blocks a chart groups \
                     them into, so each of these carries a `TODO` that the README generator will \
                     publish as it stands: {}.",
                    self.undescribed.join(", ")
                ));
            }
            if !self.invented.is_empty() {
                steps.push(format!(
                    "Replace the values this had to invent. These keys are required and their image \
                     publishes no default, so the value written is merely one the constraint \
                     accepts: {}.",
                    self.invented.join(", ")
                ));
            }
            if !plan.credentials.is_empty() || !plan.written_off.is_empty() {
                steps.push(format!(
                    "Read the `unbound` reasons in {shown}/{DECLARATION}. Each is defensible and \
                     none is considered."
                ));
            }
        } else {
            out.push_str(
                "    No configuration surface: this image publishes no contract, so the chart is \
                 the chassis and the workload alone.\n\n",
            );
            steps.push(
                "Write the chart's own values, one `@schema` block each, and the templates that \
                 consume them — a ConfigMap, a Secret, a Service, whatever this image needs. \
                 `templates/_helpers.tpl` is where anything computed belongs."
                    .to_owned(),
            );
            steps.push(
                "If this image ever publishes a configuration contract, run `terrace-contract helm \
                 init` without `--no-contract` against a scratch copy and take the generated \
                 surface across."
                    .to_owned(),
            );
        }

        for (number, step) in steps.iter().enumerate() {
            let wrapped = wrap(step, "", WIDTH - 7);
            for (index, line) in wrapped.iter().enumerate() {
                let _ = if index == 0 {
                    writeln!(out, "    {}. {line}", number + 1)
                } else {
                    writeln!(out, "       {line}")
                };
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
