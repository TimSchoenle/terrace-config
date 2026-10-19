//! Writing the configuration half of a new chart: the files its image's contract decides.
//!
//! A chart in a repository of this shape is two things stacked. Underneath is a chassis — a values
//! block per concern, a handful of templates, a pod spec wired to a library chart — which is the same
//! in every chart of that repository and belongs to it: the repository keeps it as a template and
//! copies it. On top is the part that is about *this* image: the settings it reads, what each one
//! accepts, which of them are credentials, and how a chart value reaches each one. That half is
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
//! | `config-contract.yaml` | the declaration every contract gate reads |
//! | `contract-tests.yaml` | the round-trip enrolment |
//! | `ci/test-values.yaml` | the credentials the render guard demands, and nothing else |
//! | `tests/configmap_test.yaml` | a first suite, for a person to extend |
//!
//! For an image that publishes no contract (`--no-contract`) the same files are written in their
//! plain form, and `config-contract.yaml` only when the image is first-party — see
//! [`owes_a_declaration`].
//!
//! # The placeholders a contract decides
//!
//! A repository's workload template has to name things only the contract knows: the environment
//! prefix the loader owns, the separator in a secrets-file name, the `ConfigMap` key the document is
//! rendered under. The template spells them as `%%PREFIX%%`, `%%ENV_PREFIX%%`, `%%SEPARATOR%%` and
//! `%%DOCUMENT_KEY%%` — a form Helm never reads, so the Go template braces around them survive — and
//! this fills them in every file of the chart, from the dialect the fetched contract declares. Every
//! other `%%NAME%%` is the repository's to substitute when it copies its template, and one still
//! present when this runs is refused: a placeholder nobody replaced would reach the chart as
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
//! something else is refused rather than overwritten — a chart that has a declaration is a chart
//! somebody configured, and [`super::adopt`] is what writes into one. A file already holding exactly
//! what would be written is left alone, so a run over a chart this has just written owes nothing;
//! the vendored contract counts as unchanged when only its fetch time moved, as a refresh counts it.
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
use super::patch::{created, replaced, unified};
use super::pull::{Fetched, same_contract};
use super::scaffold::chart::{
    CONTRACTS, Surface, render_ci_values, render_declaration, render_enrolment, render_helpers,
    render_opt_out, render_unit_test, render_values, undescribed,
};
use super::scaffold::{WIDTH, check_chart_name, env_prefix, invented, wrap};
use super::suites::{ENROLMENT, parse_enrolment};

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
    /// Read `REPOSITORY:TAG[@DIGEST]`.
    ///
    /// The tag is required. A digest is not, because the refresh resolves a tag — but a chart pinned
    /// by tag alone carries a mutable reference, and the document gate refuses to validate against a
    /// contract that cannot be tied to a digest, so the digest is what should be given.
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
/// image owes a declaration — a real one, or an explicit opt-out with a reason — and a chart pinning
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
            let declaration =
                render_declaration(chart, document, &contract_file, document_key, plan)?;
            let enrolment = render_enrolment(chart, document, plan)?;
            // Read back through the readers every gate uses. A file written here that no reader
            // accepts fails on the contributor's first run rather than here, with a message about
            // a file they did not write by hand.
            parse_declaration(&declaration, &chart_dir)?;
            parse_enrolment(&enrolment, &chart_dir)?;
            generated.push((chart_dir.join(DECLARATION), declaration));
            generated.push((chart_dir.join(ENROLMENT), enrolment));
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
/// From the contract's dialect for a contracted chart. A plain chart's image declares no dialect, so
/// the chart name stands in — `netcup-offer-bot` as `NETCUP_OFFER_BOT_` — which is a guess, and only
/// ever reaches the prose of a template that has no loader to describe.
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

    /// Write every file that changes, creating the directories it needs.
    ///
    /// # Errors
    /// [`Error::Io`] when a directory or a file cannot be written.
    pub fn write(&self) -> Result<(), Error> {
        for change in &self.changes {
            if change.action == Action::Unchanged {
                continue;
            }
            if let Some(parent) = change.path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
            }
            std::fs::write(&change.path, &change.after)
                .map_err(|e| Error::io(change.path.display(), e))?;
        }
        Ok(())
    }

    /// The ordered list of what a person still has to do, once the files are written.
    ///
    /// Only the steps the contract decides. What the repository's own half leaves — its chart
    /// description, its README prose, the recipes that generate its schema and its README — is the
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
                plan.secrets.len(),
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
            if !plan.secrets.is_empty() || !plan.written_off.is_empty() {
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
mod tests {
    //! The planner, against a chart directory on disk and a registry double.
    //!
    //! What the renderers write is [`super::super::scaffold::chart`]'s suite. These are about the
    //! part only a run against a directory shows: which files are created and which are filled, that
    //! a second run owes nothing, that nothing is ever overwritten, that a placeholder nobody
    //! substituted is refused, and that the contract reaches the chart only through the fetch that
    //! verifies it — the double answers exactly what [`fetch_pinned`] asks, so a contract that
    //! skipped a proof would not be served.

    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use serde_json::{Value as Json, json};

    use super::{Action, Init, Pin, Request, Source, owes_a_declaration, plan};
    use crate::document::{LABEL_PREFIX, LABEL_VERSION};
    use crate::error::Error;
    use crate::helm::pull::{ARTIFACT_TYPE, Fetched, Registry, fetch_pinned, hex_sha256};

    const IMAGE: &str = "org/app:v1.0.0@sha256:0123";
    const ATTACHED: &str = "sha256:attached";

    /// A registry serving one contract, attached to the pinned digest, with a valid signature.
    struct Serving {
        payload: Vec<u8>,
    }

    impl Registry for Serving {
        fn resolve(&self, reference: &str) -> Result<String, Error> {
            Err(Error::Invalid(format!(
                "{reference}: the tests pin a digest"
            )))
        }

        fn discover(&self, reference: &str, _: &str) -> Result<Vec<Json>, Error> {
            Ok(if reference.ends_with(ATTACHED) {
                Vec::new()
            } else {
                vec![json!({"digest": ATTACHED, "artifactType": ARTIFACT_TYPE})]
            })
        }

        fn image_labels(&self, _: &str) -> Result<BTreeMap<String, String>, Error> {
            Ok([
                (LABEL_VERSION.to_owned(), "1".to_owned()),
                (LABEL_PREFIX.to_owned(), "APP_".to_owned()),
            ]
            .into_iter()
            .collect())
        }

        fn manifest(&self, reference: &str) -> Result<Json, Error> {
            Ok(if reference.ends_with(ATTACHED) {
                json!({"layers": [{"digest": format!("sha256:{}", hex_sha256(&self.payload))}]})
            } else {
                json!({"layers": []})
            })
        }

        fn blob(&self, _: &str) -> Result<Vec<u8>, Error> {
            Ok(self.payload.clone())
        }

        fn verify(&self, _: &str, _: &str) -> Result<(), Error> {
            Ok(())
        }
    }

    fn key(path: &str, overrides: &Json) -> Json {
        let spelt = path.replace('.', "__");
        let mut fields = json!({
            "path": path,
            "env": format!("APP_{}", spelt.to_uppercase()),
            "env_file": format!("APP_{}_FILE", spelt.to_uppercase()),
            "secrets_file": spelt,
            "docs": format!("Documentation for {path}."),
            "constraint": {"type": "string"},
            "text_form": "text",
            "default": null,
            "default_value": null,
            "required": false,
            "secret": false,
            "reserved": false,
        });
        for (name, value) in overrides.as_object().expect("an object") {
            fields[name] = value.clone();
        }
        fields
    }

    /// The contract every contracted case fetches: a setting, a required credential and a required
    /// key with no published default, under one grouping block.
    fn fetched() -> Fetched {
        let contract = json!({
            "terrace_contract": 1,
            "app": {"name": "app", "version": "1.0.0"},
            "schema": {
                "schema_version": 2,
                "dialect": {
                    "prefix": "APP_",
                    "nesting_separator": "__",
                    "indirection_suffix": "_FILE",
                },
                "loader": [],
                "keys": [
                    key("a.b", &json!({})),
                    key("a.port", &json!({"required": true, "text_form": "integer",
                                          "constraint": {"type": "integer", "minimum": 1}})),
                    key("a.token", &json!({"secret": true, "required": true})),
                ],
            },
            "json_schema": {},
            "external": {"env": [], "ignore": [], "unknown": "reject"},
        });
        let registry = Serving {
            payload: serde_json::to_vec(&contract).expect("it serialises"),
        };
        let pin = Pin::parse(IMAGE).expect("a pin");
        fetch_pinned(&registry, &pin.block(), None, "signer").expect("the double serves it")
    }

    /// A chart tree in the temporary directory, removed when dropped.
    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "terrace-init-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a chart tree");
            Self(root)
        }

        /// One file under `charts/app/`, as a repository's template copy would leave it.
        fn copied(&self, relative: &str, text: &str) -> &Self {
            let path = self.chart().join(relative);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, text).expect("a file");
            self
        }

        fn chart(&self) -> PathBuf {
            self.0.join("app")
        }

        fn read(&self, relative: &str) -> String {
            std::fs::read_to_string(self.chart().join(relative)).expect("the file exists")
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const DEPLOYMENT: &str = "\
env:
  - name: %%PREFIX%%CONFIG
    value: {{ .Values.configMount.configDir }}/%%DOCUMENT_KEY%%
{{- include \"common.fileConfig.env\" (dict \"prefix\" \"%%ENV_PREFIX%%\") }}
";

    fn run(tree: &Tree, source: &Source<'_>) -> Result<Init, Error> {
        let pin = Pin::parse(IMAGE).expect("a pin");
        plan(
            &tree.0,
            &Request {
                chart: "app",
                pin: &pin,
                document: "app",
                document_key: "config.toml",
                chassis: "",
            },
            source,
        )
    }

    /// Every change, as `(path under the chart, action)`.
    fn actions(init: &Init) -> Vec<(String, Action)> {
        init.changes
            .iter()
            .map(|change| {
                let relative = change
                    .path
                    .strip_prefix(&init.chart_dir)
                    .expect("under the chart");
                (
                    relative.to_string_lossy().replace('\\', "/"),
                    change.action.clone(),
                )
            })
            .collect()
    }

    fn created(paths: &[&str]) -> Vec<(String, Action)> {
        paths
            .iter()
            .map(|path| ((*path).to_owned(), Action::Create))
            .collect()
    }

    // --------------------------------------------------------------------------------------
    // The pin and the first-party question
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_pin_is_a_repository_and_a_tag_with_an_optional_digest() {
        let parse = |reference: &str| {
            let pin = Pin::parse(reference).expect("a pin");
            (pin.repository, pin.tag)
        };
        assert_eq!(parse("org/app:v1"), ("org/app".into(), "v1".into()));
        assert_eq!(
            parse("org/app:v1@sha256:ab"),
            ("org/app".into(), "v1@sha256:ab".into())
        );
        // A registry with a port: the colon before the last slash is not the tag's.
        assert_eq!(
            parse("registry.local:5000/org/app:8.0"),
            ("registry.local:5000/org/app".into(), "8.0".into())
        );
    }

    #[test]
    fn a_pin_without_a_tag_is_refused() {
        for reference in [
            "org/app",
            "org/app@sha256:ab",
            "registry.local:5000/org/app",
            ":v1",
            "org/app:",
            "org/app:v1@",
            "",
        ] {
            assert!(Pin::parse(reference).is_err(), "{reference:?} was accepted");
        }
    }

    #[test]
    fn a_first_party_image_owes_a_declaration() {
        let patterns = ["org/*".to_owned(), "ghcr.io/org/*".to_owned()];
        assert!(owes_a_declaration(&patterns, "org/app"));
        assert!(owes_a_declaration(&patterns, "ghcr.io/org/app"));
        assert!(!owes_a_declaration(&patterns, "library/redis"));
    }

    #[test]
    fn an_empty_list_makes_nothing_first_party() {
        assert!(!owes_a_declaration(&[], "org/app"));
    }

    // --------------------------------------------------------------------------------------
    // The contracted form
    // --------------------------------------------------------------------------------------

    #[test]
    fn every_file_is_planned_and_the_copy_is_filled_from_the_contract() {
        let tree = Tree::new();
        tree.copied("Chart.yaml", "name: app\n")
            .copied("templates/deployment.yaml", DEPLOYMENT);
        let fetched = fetched();
        let init = run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-01-01T00:00:00Z",
            },
        )
        .expect("it plans");

        let mut expected = created(&[
            "contracts/app.json",
            "values.yaml",
            "templates/_helpers.tpl",
            "config-contract.yaml",
            "contract-tests.yaml",
            "ci/test-values.yaml",
            "tests/configmap_test.yaml",
        ]);
        expected.push((
            "templates/deployment.yaml".to_owned(),
            Action::Fill(vec!["DOCUMENT_KEY", "ENV_PREFIX", "PREFIX"]),
        ));
        assert_eq!(actions(&init), expected);
        assert!(init.owed());
        assert_eq!(init.undescribed, ["a"]);
        assert_eq!(init.invented, ["a.port"]);

        // Planning wrote nothing.
        assert!(!tree.chart().join("values.yaml").exists());
        assert_eq!(tree.read("templates/deployment.yaml"), DEPLOYMENT);

        init.write().expect("it writes");
        assert_eq!(
            tree.read("templates/deployment.yaml"),
            DEPLOYMENT
                .replace("%%PREFIX%%", "APP_")
                .replace("%%DOCUMENT_KEY%%", "config.toml")
                .replace("%%ENV_PREFIX%%", "APP")
        );
        assert!(
            tree.read("config-contract.yaml")
                .contains("contract: contracts/app.json")
        );
        let vendored: Json =
            serde_json::from_str(&tree.read("contracts/app.json")).expect("vendored JSON");
        assert_eq!(vendored["source"]["image"], json!("docker.io/org/app"));
        assert_eq!(vendored["source"]["digest"], json!("sha256:0123"));
    }

    #[test]
    fn a_second_run_owes_nothing_even_though_the_fetch_time_moved() {
        let tree = Tree::new();
        tree.copied("templates/deployment.yaml", DEPLOYMENT);
        let fetched = fetched();
        run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-01-01T00:00:00Z",
            },
        )
        .expect("it plans")
        .write()
        .expect("it writes");

        let again = run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-02-02T00:00:00Z",
            },
        )
        .expect("it plans again");
        assert!(!again.owed(), "{}", again.report());
        assert!(
            again
                .changes
                .iter()
                .all(|change| change.action == Action::Unchanged)
        );
        assert!(
            again.report().contains("nothing to write"),
            "{}",
            again.report()
        );
        assert_eq!(again.diff(), "");
    }

    #[test]
    fn a_file_that_says_something_else_is_refused_and_nothing_is_written() {
        let tree = Tree::new();
        tree.copied("values.yaml", "image: {}\n")
            .copied("config-contract.yaml", "documents: []\n")
            .copied("templates/deployment.yaml", DEPLOYMENT);
        let fetched = fetched();
        let refused = run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-01-01T00:00:00Z",
            },
        )
        .expect_err("a configured chart is refused")
        .to_string();
        for named in ["app/values.yaml", "app/config-contract.yaml", "adopt"] {
            assert!(refused.contains(named), "{refused}");
        }
        assert_eq!(tree.read("values.yaml"), "image: {}\n");
        assert!(!tree.chart().join("contracts").exists());
    }

    #[test]
    fn a_placeholder_the_copy_left_is_refused() {
        let tree = Tree::new();
        tree.copied("Chart.yaml", "name: %%CHART%%\n");
        let fetched = fetched();
        let refused = run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-01-01T00:00:00Z",
            },
        )
        .expect_err("a leftover placeholder is refused")
        .to_string();
        assert!(refused.contains("%%CHART%%"), "{refused}");
        assert!(refused.contains("app/Chart.yaml"), "{refused}");
    }

    #[test]
    fn dependencies_and_vendored_contracts_are_not_template_text() {
        // A packaged dependency or a contract's own prose can hold anything; neither is filled.
        let tree = Tree::new();
        tree.copied("charts/common/templates/_x.tpl", "%%PREFIX%% %%OTHER%%\n")
            .copied("contracts/other.json", "{\"docs\": \"%%PREFIX%%\"}\n")
            .copied("docs/%%PREFIX%%.md", "%%PREFIX%%\n");
        let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
        let filled: Vec<String> = actions(&init)
            .into_iter()
            .filter(|(_, action)| matches!(action, Action::Fill(_)))
            .map(|(path, _)| path)
            .collect();
        assert_eq!(filled, ["docs/%%PREFIX%%.md"]);
    }

    #[test]
    fn a_value_that_would_break_a_line_is_refused() {
        let tree = Tree::new();
        tree.copied("templates/configmap.yaml", "key: %%DOCUMENT_KEY%%\n");
        let pin = Pin::parse(IMAGE).expect("a pin");
        let refused = plan(
            &tree.0,
            &Request {
                chart: "app",
                pin: &pin,
                document: "app",
                document_key: "config\n.toml",
                chassis: "",
            },
            &Source::Plain { opt_out: None },
        );
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn a_chart_name_helm_would_refuse_is_refused_before_anything_is_read() {
        let tree = Tree::new();
        let pin = Pin::parse(IMAGE).expect("a pin");
        let refused = plan(
            &tree.0,
            &Request {
                chart: "App",
                pin: &pin,
                document: "app",
                document_key: "config.toml",
                chassis: "",
            },
            &Source::Plain { opt_out: None },
        );
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn the_diff_creates_new_files_against_dev_null_and_fills_in_place() {
        let tree = Tree::new();
        tree.copied("templates/deployment.yaml", DEPLOYMENT);
        let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
        let diff = init.diff();
        let values = format!("{}/values.yaml", super::posix(&tree.chart()));
        assert!(
            diff.contains(&format!("--- /dev/null\n+++ {values}\n@@ -0,0 +1,")),
            "{diff}"
        );
        let deployment = format!("{}/templates/deployment.yaml", super::posix(&tree.chart()));
        assert!(
            diff.contains(&format!(
                "--- {deployment}\n+++ {deployment}\n@@ -1,4 +1,4 @@\n env:\n\
                 -  - name: %%PREFIX%%CONFIG\n"
            )),
            "{diff}"
        );
        assert!(diff.contains("+  - name: APP_CONFIG\n"), "{diff}");
    }

    #[test]
    fn the_report_names_every_file_and_what_happens_to_it() {
        let tree = Tree::new();
        tree.copied("templates/deployment.yaml", DEPLOYMENT);
        let report = run(&tree, &Source::Plain { opt_out: None })
            .expect("it plans")
            .report();
        let chart = super::posix(&tree.chart());
        assert!(
            report.starts_with(&format!("==> {chart}: the plain form\n")),
            "{report}"
        );
        assert!(
            report.contains(&format!("    create     {chart}/values.yaml\n")),
            "{report}"
        );
        assert!(
            report.contains(&format!(
                "    fill       {chart}/templates/deployment.yaml (%%DOCUMENT_KEY%%, \
                 %%ENV_PREFIX%%, %%PREFIX%%)\n"
            )),
            "{report}"
        );
    }

    // --------------------------------------------------------------------------------------
    // The plain form
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_third_party_image_gets_no_declaration_and_no_configuration_files() {
        let tree = Tree::new();
        tree.copied("templates/deployment.yaml", DEPLOYMENT);
        let init = run(&tree, &Source::Plain { opt_out: None }).expect("it plans");
        let mut expected = created(&[
            "values.yaml",
            "templates/_helpers.tpl",
            "ci/test-values.yaml",
        ]);
        expected.push((
            "templates/deployment.yaml".to_owned(),
            Action::Fill(vec!["DOCUMENT_KEY", "ENV_PREFIX", "PREFIX"]),
        ));
        assert_eq!(actions(&init), expected);
        assert!(init.undescribed.is_empty() && init.invented.is_empty());
        init.write().expect("it writes");
        // No dialect to read, so the chart name stands in.
        assert!(
            tree.read("templates/deployment.yaml")
                .contains("name: APP_CONFIG")
        );
        assert!(
            init.closing().contains("No configuration surface"),
            "{}",
            init.closing()
        );
    }

    #[test]
    fn a_first_party_image_gets_the_opt_out_with_its_reason() {
        let tree = Tree::new();
        let init = run(
            &tree,
            &Source::Plain {
                opt_out: Some("The image is configured by flags alone."),
            },
        )
        .expect("it plans");
        assert_eq!(
            actions(&init),
            created(&[
                "values.yaml",
                "templates/_helpers.tpl",
                "config-contract.yaml",
                "ci/test-values.yaml",
            ])
        );
        let declaration = &init.changes[2].after;
        assert!(declaration.contains("documents: []"), "{declaration}");
        assert!(
            declaration.contains("  The image is configured by flags alone."),
            "{declaration}"
        );
        let parsed = crate::helm::declaration::parse_declaration(declaration, Path::new("app"))
            .expect("the loader accepts the opt-out");
        assert!(parsed.documents.is_empty());
        assert!(parsed.reason.is_some());
    }

    #[test]
    fn the_closing_lists_what_the_contract_left_for_a_person() {
        let tree = Tree::new();
        let fetched = fetched();
        let closing = run(
            &tree,
            &Source::Contract {
                fetched: &fetched,
                at: "2026-01-01T00:00:00Z",
            },
        )
        .expect("it plans")
        .closing();
        for expected in [
            "2 setting(s) surfaced as chart values, 1 credential(s) delivered as files",
            "1. Add whatever the workload needs",
            "grouping block",
            "a.port",
            "`unbound` reasons",
        ] {
            assert!(closing.contains(expected), "{expected}: {closing}");
        }
    }
}
