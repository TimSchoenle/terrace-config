//! Generating the round-trip test suites a chart's contracts imply, and keeping them in step.
//!
//! The walk over the charts, the file IO and what it means for a suite to be stale. The model —
//! choosing a probe, refusing to invent one, and spelling the assertion — lives in
//! [`super::testgen`], so every rule in it is testable by calling it.
//!
//! # Enrolment is a declared switch, not a list
//!
//! A chart is generated for when its declaration says `tests: true`, exactly as `bindings: true`
//! enrols it in the marker gate. That is what makes a phased rollout a property of the tree rather
//! than of a constant somebody has to remember to edit. Each document's `tests` block is where a
//! chart says which values every generated case has to carry before it will render at all, and —
//! where the operator-facing configuration tree is not the layer that wins — which values path a
//! probe is to be written to.
//!
//! The older form, a separate `contract-tests.yaml` whose presence is the enrolment, is still read
//! for a chart whose declaration does not enrol it. A chart enrolled both ways is refused.
//!
//! # A baseline and a render prerequisite are two different fields because they are two different
//! things
//!
//! A `baseline` states configuration — flat dotted paths under the document's probe root — and is
//! dropped from the one case probing the key it sets, because a baseline supplying the probed value
//! would make that case pass whether or not the chart delivered anything. A `prerequisites` block
//! states the chart's own first-class values, the ones a render guard refuses to render without, and
//! is carried by every case including the identity one, because a case that does not render proves
//! nothing either. Being undroppable is exactly why it may not name a path inside that same probe
//! root: that is refused here with the file's name on it, and refused again by [`super::testgen`],
//! so the guarantee does not depend on this loader having been the caller.
//!
//! # How a document is told from its siblings is derived, not declared
//!
//! A suite finds its document by the key the declaration names; a chart rendering that key into
//! several documents needs the labels as well, and the declaration already carries them per
//! document. Asking the enrolment to repeat them would let the two disagree, and the gate reading
//! the first would then be validating an object the suite never selects.
//!
//! # The vendored contracts are read directly rather than through the binding
//!
//! The binding exists to refuse validating a document against a contract for some other digest, and
//! it is right for a gate. It is wrong here: a suite is a function of the contract's *keys* and of
//! nothing else, so tying generation to the digest interlock would block a regeneration during the
//! window between a digest bump and the contract refresh — and would report the same staleness the
//! document gate already reports, with a worse message.
//!
//! # Line endings are the platform's, deliberately
//!
//! A repository developed from a Windows checkout with `core.autocrlf` on holds CRLF in the working
//! tree and LF in the index; a generator writing `\n` unconditionally would produce a file differing
//! from its own committed copy on every line, and a staleness gate that can never pass. Writing and
//! comparing through the platform's own convention makes both ends agnostic.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use crate::error::Error;
use crate::union::union_contracts;

use super::bindings::has_path;
use super::declaration::{
    Declaration, Document, Selection, load_declaration, read_yaml, reject_unknown, vendored_for,
};
use super::markers::{Block, Class, Marker};
use super::testgen::{
    Carrier, Plan, Route, Target, VALUES_ROOT, plan, prerequisite_conflict, render_suite,
    unsupplied_entries,
};
use super::{dig, shapes};

/// The file that enrols a chart.
pub const ENROLMENT: &str = "contract-tests.yaml";

/// The keys the enrolment may carry at each level.
///
/// Anything else is a typo that would otherwise be ignored in silence — the same rule the
/// declaration is read under, and for the same reason.
const ENROLMENT_KEYS: [&str; 1] = ["documents"];
const DOCUMENT_KEYS: [&str; 6] = [
    "name",
    "baseline",
    "probe",
    "reason",
    "prerequisites",
    "unrouted",
];
/// What a document's `tests` block in the declaration may carry: an entry of the file, less its
/// `name`, which the document already has.
pub(crate) const ENTRY_KEYS: [&str; 5] =
    ["baseline", "probe", "reason", "prerequisites", "unrouted"];
pub(crate) const UNROUTED_KEYS: [&str; 2] = ["keys", "reason"];
pub(crate) const PREREQUISITE_KEYS: [&str; 2] = ["values", "reason"];

/// Where a generated suite goes, and the name that identifies one.
///
/// The prefix is what lets the generator own the removal of a suite whose document is gone: any file
/// matching it under a generated chart's tests directory was written by this and by nothing else.
const SUITES: &str = "tests";
const SUITE_PREFIX: &str = "contract_roundtrip_";
const SUITE_SUFFIX: &str = "_test.yaml";

/// Values every case for one document carries, and why it has to.
///
/// A chart is free to refuse a values combination its image would accept, and a probe that walked
/// into such a pair would fail for a reason that has nothing to do with the round trip. The chart
/// states the way out of its own guard, because the guard is the chart's and no contract knows about
/// it.
///
/// A baseline is a hole in what the generated suite proves, so it carries a reason for the same
/// cause the declaration demands one for an exemption: an unexplained hole is indistinguishable from
/// an oversight.
///
/// Deliberately confined to paths under the document's probe root, and read under the same root the
/// probes are: the collision check that drops a baseline entry compares two paths as strings, so a
/// baseline one layer off would never compare equal to a probe and would quietly supply the value
/// the case exists to prove the chart delivered.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    /// The values, sorted.
    pub values: Vec<(String, Json)>,
    /// Why.
    pub reason: Option<String>,
}

/// Chart values every case for one document carries before the chart will render, and why.
///
/// A chart that refuses its own default render fails its template before a document exists to assert
/// anything about. What unblocks it is the chart's own first-class values, which is a different kind
/// of thing from a [`Baseline`] — that one writes into the configuration document the cases assert
/// against, and this one writes into the chart's values.
///
/// Held as flat dotted paths with whatever shape the leaf has. A `set` mapping *is* flat dotted
/// paths, so what the enrolment states is what the generated file carries and there is no
/// translation between the two to go wrong in silence; the refusal that keeps a prerequisite out of
/// the tree the cases probe is then a prefix test on one string, checkable by eye. The leaves are
/// free to be structures, because a map keyed by a path with a `/` in it reads worse split up than
/// it does whole.
#[derive(Debug, Clone, Default)]
pub struct Prerequisites {
    /// The values, sorted.
    pub values: Vec<(String, Json)>,
    /// Why.
    pub reason: Option<String>,
}

/// What one chart's enrolment says about one of its documents.
///
/// `probe` is the values path every case for this document writes into, defaulting to the
/// operator-facing configuration tree. It exists because that tree is not always the layer that
/// wins, and it is declared rather than derived: nothing in a declaration spells a chart's
/// per-service values key from a document's name. It sits here rather than on the baseline because
/// it governs all three fields at once — the probes are written under it, the baseline is read under
/// it, and the prerequisites are refused from it.
///
/// `unrouted` is the one field that takes something away. A probe is written into the chart's own
/// value for the key wherever a `projection` marker names one, because that is what proves the
/// chart's mapping rather than only its escape hatch — and a chart may have a guard that a
/// synthesised value cannot satisfy at that position. Such a key is named here, with the reason, and
/// its probe goes into the raw tree instead: the case still proves the merge, and the file says
/// which proof it is making.
#[derive(Debug, Clone)]
pub struct Enrolment {
    /// The baseline.
    pub baseline: Baseline,
    /// The render prerequisites.
    pub prerequisites: Prerequisites,
    /// The values path a probe is written under.
    pub probe: String,
    /// Keys probed through the raw tree, with the reason.
    pub unrouted: Vec<(String, String)>,
}

impl Default for Enrolment {
    fn default() -> Self {
        Self {
            baseline: Baseline::default(),
            prerequisites: Prerequisites::default(),
            probe: VALUES_ROOT.to_owned(),
            unrouted: Vec::new(),
        }
    }
}

/// One chart's enrolment: how each of its documents' suites is generated, and where that was said.
///
/// Read from the declaration's `tests` when it enrols the chart, and from the older
/// `contract-tests.yaml` otherwise. Both is refused: two files enrolling one chart can disagree,
/// and whichever won would be decided by this reader rather than by anybody who wrote them.
#[derive(Debug, Clone)]
pub struct Enrolments {
    /// The file the enrolment was read from, which every refusal about it names.
    pub path: PathBuf,
    /// Each document's entry, by document name. A document with none takes the defaults.
    pub documents: BTreeMap<String, Enrolment>,
}

/// Read one chart's enrolment; [`None`] when the chart is not enrolled.
///
/// # Errors
/// [`Error::Invalid`] naming whichever entry is not one, or when both the declaration and
/// `contract-tests.yaml` enrol the chart; [`Error::Io`] when the file cannot be read.
pub fn load_enrolment(
    chart_dir: &Path,
    declaration: Option<&Declaration>,
) -> Result<Option<Enrolments>, Error> {
    let file = chart_dir.join(ENROLMENT);
    let declared = declaration.filter(|held| held.tests);

    if let Some(declaration) = declared {
        if file.is_file() {
            return Err(Error::Invalid(format!(
                "{}: enrols this chart a second time; `tests: true` in {} already does. Move its \
                 entries into the matching documents' `tests` blocks and delete this file",
                file.display(),
                declaration.path.display()
            )));
        }
        return Ok(Some(Enrolments {
            path: declaration.path.clone(),
            documents: declaration
                .documents
                .iter()
                .filter_map(|document| {
                    document
                        .tests
                        .clone()
                        .map(|tests| (document.name.clone(), tests))
                })
                .collect(),
        }));
    }

    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file).map_err(|e| Error::io(file.display(), e))?;
    Ok(Some(Enrolments {
        documents: parse_enrolment(&text, chart_dir)?,
        path: file,
    }))
}

/// Read one chart's `contract-tests.yaml` from its text.
///
/// # Errors
/// As [`load_enrolment`].
fn parse_enrolment(text: &str, chart_dir: &Path) -> Result<BTreeMap<String, Enrolment>, Error> {
    let path = chart_dir.join(ENROLMENT);
    let at = path.display().to_string();
    let document = read_yaml(text, &path)?;
    if !document.is_object() {
        return Err(Error::Invalid(format!(
            "{at}: expected a mapping at the top level"
        )));
    }
    reject_unknown(&at, "", &document, &ENROLMENT_KEYS)?;

    let entries = document.get("documents").unwrap_or(&Json::Null);
    if !entries.is_null() && !entries.is_array() {
        return Err(Error::Invalid(format!("{at}: `documents` is not a list")));
    }

    let mut enrolments: BTreeMap<String, Enrolment> = BTreeMap::new();
    for entry in entries.as_array().into_iter().flatten() {
        if !entry.is_object() {
            return Err(Error::Invalid(format!(
                "{at}: every entry of `documents` must be a mapping"
            )));
        }
        reject_unknown(&at, "documents[]", entry, &DOCUMENT_KEYS)?;

        let name = entry
            .get("name")
            .and_then(Json::as_str)
            .filter(|name| !name.is_empty());
        let Some(name) = name else {
            return Err(Error::Invalid(format!(
                "{at}: an entry of `documents` has no `name`"
            )));
        };
        if enrolments.contains_key(name) {
            return Err(Error::Invalid(format!("{at}: `{name}` is declared twice")));
        }
        enrolments.insert(name.to_owned(), entry_fields(&at, name, entry)?);
    }
    Ok(enrolments)
}

/// One document's `tests` block in the declaration, read under the rules an entry of
/// `contract-tests.yaml` is read under.
///
/// # Errors
/// [`Error::Invalid`] naming the field that is wrong.
pub fn parse_entry(at: &str, name: &str, where_: &str, entry: &Json) -> Result<Enrolment, Error> {
    reject_unknown(at, where_, entry, &ENTRY_KEYS)?;
    entry_fields(at, name, entry)
}

/// The fields of one entry, whichever file it was written in; its keys are already checked.
fn entry_fields(at: &str, name: &str, entry: &Json) -> Result<Enrolment, Error> {
    let probe = load_probe(at, name, entry.get("probe"))?;
    let reason = entry
        .get("reason")
        .and_then(Json::as_str)
        .map(str::to_owned);
    let baseline = Baseline {
        values: load_baseline(at, name, entry.get("baseline"), &probe)?,
        reason,
    };
    if (!baseline.values.is_empty() || probe != VALUES_ROOT) && baseline.reason.is_none() {
        return Err(Error::Invalid(format!(
            "{at}: the entry for `{name}` sets a baseline or a probe path and has no \
             `reason`; either narrows what every generated case proves, and an unexplained \
             hole in a gate is indistinguishable from an oversight"
        )));
    }

    Ok(Enrolment {
        prerequisites: load_prerequisites(at, name, entry.get("prerequisites"), &probe)?,
        unrouted: load_unrouted(at, name, entry.get("unrouted"))?,
        baseline,
        probe,
    })
}

/// The values path this document's cases write into, defaulting to the configuration tree.
fn load_probe(at: &str, name: &str, probe: Option<&Json>) -> Result<String, Error> {
    let Some(probe) = probe.filter(|held| !held.is_null()) else {
        return Ok(VALUES_ROOT.to_owned());
    };
    let spelling = probe.as_str().filter(|held| is_values_path(held));
    let Some(spelling) = spelling else {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `probe` {} is not a dotted values path; it is used both as a `set` \
             prefix and as a walk into the chart's values, and neither reads anything else",
            super::shown(Some(probe))
        )));
    };
    Ok(spelling.to_owned())
}

/// A values path a probe may be written under: dotted, and nothing else.
///
/// The path is used twice — as a `set` prefix in the generated suite and as a walk into the chart's
/// values — and the two agree on dotted segments and on nothing beyond them.
fn is_values_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|segment| {
            let mut held = segment.chars();
            held.next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                && held
                    .all(|character| character.is_ascii_alphanumeric() || "_-".contains(character))
        })
}

/// One baseline, as sorted `set` path / value pairs.
fn load_baseline(
    at: &str,
    name: &str,
    baseline: Option<&Json>,
    probe: &str,
) -> Result<Vec<(String, Json)>, Error> {
    let Some(baseline) = baseline.filter(|held| !held.is_null()) else {
        return Ok(Vec::new());
    };
    let Some(fields) = baseline.as_object() else {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `baseline` must be a mapping"
        )));
    };

    let mut values = Vec::new();
    for (key, value) in fields {
        if !key.starts_with(&format!("{probe}.")) {
            return Err(Error::Invalid(format!(
                "{at}: {name}: the baseline entry {} is not a path under `{probe}`; a baseline \
                 states configuration at the layer the probes are written to, not chart values",
                crate::gate::quoted(key)
            )));
        }
        if value.is_object() || value.is_array() {
            return Err(Error::Invalid(format!(
                "{at}: {name}: the baseline entry {} is not a scalar; state each leaf as its own \
                 dotted path so a collision with a probe is visible",
                crate::gate::quoted(key)
            )));
        }
        values.push((key.clone(), value.clone()));
    }
    Ok(values)
}

/// One document's render prerequisites, as sorted `set` path / value pairs and their reason.
///
/// Every refusal here is loud rather than lenient, and the first of them is the one the whole field
/// turns on: a prerequisite inside the tree the cases probe is undroppable *and* able to supply what
/// they assert, which is the one combination that lets a generated suite pass without having proven
/// anything. [`prerequisite_conflict`] owns the rule so the message is the one the model raises;
/// what this adds is the name of the file to fix.
fn load_prerequisites(
    at: &str,
    name: &str,
    block: Option<&Json>,
    probe: &str,
) -> Result<Prerequisites, Error> {
    let Some(block) = block.filter(|held| !held.is_null()) else {
        return Ok(Prerequisites::default());
    };
    if !block.is_object() {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `prerequisites` must be a mapping"
        )));
    }
    reject_unknown(
        at,
        &format!("documents[{name}].prerequisites"),
        block,
        &PREREQUISITE_KEYS,
    )?;

    let declared = block.get("values").unwrap_or(&Json::Null);
    if !declared.is_null() && !declared.is_object() {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `prerequisites.values` must be a mapping of chart values path to the \
             value to set"
        )));
    }

    let mut values: Vec<(String, Json)> = Vec::new();
    for (key, value) in declared.as_object().into_iter().flatten() {
        if let Some(conflict) = prerequisite_conflict(key, probe) {
            return Err(Error::Invalid(format!(
                "{at}: {name}: the render prerequisite {} {conflict}",
                crate::gate::quoted(key)
            )));
        }
        values.push((key.clone(), value.clone()));
    }

    // Compared pairwise rather than only against a neighbour: sorted, `a`, `a.b` and `ab` fall in
    // that order, so a path precedes both of its own extensions without being adjacent to the later
    // one. Overlapping entries are refused because a `set` mapping has no order, so which of the two
    // survives is stated nowhere.
    for (index, (outer, _)) in values.iter().enumerate() {
        for (inner, _) in &values[index + 1..] {
            if inner.starts_with(&format!("{outer}.")) {
                return Err(Error::Invalid(format!(
                    "{at}: {name}: the render prerequisites {} and {} overlap; a `set` mapping \
                     has no order, so which of the two survives is stated nowhere — write the one \
                     subtree that carries both",
                    crate::gate::quoted(outer),
                    crate::gate::quoted(inner)
                )));
            }
        }
    }

    if values.is_empty() {
        return Err(Error::Invalid(format!(
            "{at}: {name}: `prerequisites` declares no `values`; a block that sets nothing is a \
             field somebody meant to fill in"
        )));
    }
    let reason = block
        .get("reason")
        .and_then(Json::as_str)
        .filter(|reason| !reason.is_empty());
    let Some(reason) = reason else {
        return Err(Error::Invalid(format!(
            "{at}: {name}: the render prerequisites have no `reason`; they are carried by every \
             generated case, and a value nobody explained is one nobody can tell from a \
             workaround somebody stopped needing"
        )));
    };
    Ok(Prerequisites {
        values,
        reason: Some(reason.to_owned()),
    })
}

/// The keys this document probes through the raw tree rather than the chart's own value.
///
/// Grouped as `keys` against one `reason`, the shape the declaration's `unbound` already uses, so a
/// chart with several such keys states the reason once. A key named twice is refused: two reasons
/// for one key means one of them is stale, and picking either is picking a winner in silence.
fn load_unrouted(
    at: &str,
    name: &str,
    entries: Option<&Json>,
) -> Result<Vec<(String, String)>, Error> {
    let Some(entries) = entries.filter(|held| !held.is_null()) else {
        return Ok(Vec::new());
    };
    let Some(entries) = entries.as_array() else {
        return Err(Error::Invalid(format!(
            "{at}: document '{name}': `unrouted` has to be a list"
        )));
    };

    let mut unrouted: BTreeMap<String, String> = BTreeMap::new();
    for entry in entries {
        if !entry.is_object() {
            return Err(Error::Invalid(format!(
                "{at}: document '{name}': every `unrouted` entry has to be a mapping of `keys` \
                 and `reason`"
            )));
        }
        reject_unknown(
            at,
            &format!("document '{name}': unrouted"),
            entry,
            &UNROUTED_KEYS,
        )?;

        let keys: Vec<&Json> = entry
            .get("keys")
            .and_then(Json::as_array)
            .map(|held| held.iter().collect())
            .unwrap_or_default();
        if keys.is_empty() {
            return Err(Error::Invalid(format!(
                "{at}: document '{name}': an `unrouted` entry names no key"
            )));
        }
        let reason = entry
            .get("reason")
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|reason| !reason.is_empty());
        let Some(reason) = reason else {
            return Err(Error::Invalid(format!(
                "{at}: document '{name}': the `unrouted` entry for {} carries no reason, and a \
                 probe moved off the chart's own value without one is indistinguishable from an \
                 oversight",
                keys.iter()
                    .map(|key| super::shown(Some(key)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        };

        for key in keys {
            let spelling = key.as_str().filter(|held| !held.is_empty());
            let Some(spelling) = spelling else {
                return Err(Error::Invalid(format!(
                    "{at}: document '{name}': an `unrouted` key has to be a contract path"
                )));
            };
            if unrouted.contains_key(spelling) {
                return Err(Error::Invalid(format!(
                    "{at}: document '{name}': {} is named by two `unrouted` entries, so one of \
                     the two reasons is stale",
                    crate::gate::quoted(spelling)
                )));
            }
            unrouted.insert(spelling.to_owned(), reason.to_owned());
        }
    }
    Ok(unrouted.into_iter().collect())
}

// ------------------------------------------------------------------------------------------------
// Building one chart's suites
// ------------------------------------------------------------------------------------------------

/// Refuse a document whose probe path is not a configuration tree in the chart's values.
///
/// Every contract key is reachable as `<probe>.<path>` only because a chart exposes an
/// operator-supplied tree that some layer of its render merges. A chart without one can still be
/// probed — through whichever value happens to spell each key — but not by a generator that knows
/// nothing about it, and guessing would produce a suite that silently asserts nothing. The same
/// check catches the likelier mistake by far: an enrolment naming a `probe` path this chart does not
/// have, which would otherwise generate a whole suite of cases setting values nothing reads.
fn probe_tree(chart_dir: &Path, values: &Json, name: &str, probe: &str) -> Result<(), Error> {
    if dig(values, probe).is_none_or(|held| !held.is_object()) {
        return Err(Error::Invalid(format!(
            "{}: {name}: has no `{probe}` mapping, so a contract key cannot be written into this \
             chart's values by its contract path alone",
            chart_dir.join("values.yaml").display()
        )));
    }
    Ok(())
}

/// The labels a document's selector needs beyond its key, empty where the key suffices.
///
/// Derived here rather than declared in the enrolment because the declaration already states it: a
/// second copy could disagree with the first, and the gate reading the first would then be
/// validating an object the suite never selects.
fn discriminator_for(
    declaration: &Declaration,
    document: &Document,
) -> Result<Vec<(String, String)>, Error> {
    let shared: Vec<&Document> = declaration
        .documents
        .iter()
        .filter(|other| other.name != document.name && other.source.key == document.source.key)
        .collect();
    if shared.is_empty() {
        return Ok(Vec::new());
    }

    if document.source.selector.is_empty() {
        let mut names: Vec<&str> = shared.iter().map(|other| other.name.as_str()).collect();
        names.sort_unstable();
        return Err(Error::Invalid(format!(
            "{}: `{}` shares the key `{}` with {} and carries no `source.selector`, so nothing in \
             this declaration tells the documents apart",
            declaration.path.display(),
            document.name,
            document.source.key,
            names.join(", ")
        )));
    }

    let mut twins: Vec<&str> = shared
        .iter()
        .filter(|other| other.source.selector == document.source.selector)
        .map(|other| other.name.as_str())
        .collect();
    twins.sort_unstable();
    if !twins.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: `{}` shares both the key `{}` and its `source.selector` with {}, so nothing in \
             this declaration tells the documents apart",
            declaration.path.display(),
            document.name,
            document.source.key,
            twins.join(", ")
        )));
    }

    let mut labels = document.source.selector.clone();
    labels.sort();
    Ok(labels)
}

/// Where an enrolment was written, as the phrase a generated comment names it by.
///
/// The older file keeps the wording every suite generated from it already carries, so moving to
/// this reader changes no committed suite.
fn enrolled_in(path: &Path) -> String {
    let file = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    if file == ENROLMENT {
        format!("the chart's `{file}`")
    } else {
        format!("the document's `tests` block in the chart's `{file}`")
    }
}

/// Where a generated suite for one document goes.
fn suite_path(chart_dir: &Path, document: &Document) -> PathBuf {
    chart_dir
        .join(SUITES)
        .join(format!("{SUITE_PREFIX}{}{SUITE_SUFFIX}", document.name))
}

/// One path as it is spelled from the repository root, whatever the charts directory was given as.
///
/// The generated header names the files it was generated from, and a path built from the filesystem
/// would be absolute whenever the caller passed an absolute charts directory — which would put the
/// checkout directory into a committed file and make the staleness gate fail on any machine but the
/// one that last ran the generator.
fn repository_path(chart_dir: &Path, tail: &str) -> String {
    fn named(path: Option<&Path>) -> &str {
        path.and_then(Path::file_name)
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
    }
    format!(
        "{}/{}/{tail}",
        named(chart_dir.parent()),
        named(Some(chart_dir))
    )
}

/// Where each of this document's keys is better probed: the chart's own value for it.
///
/// A probe written into the raw configuration tree proves the tree reaches the document. It says
/// nothing about the mapping every chart also has, and a typo in the helper that spells one name
/// from the other passes every case in the suite. The `projection` markers are what close that: each
/// one names the contract key its value feeds, so a probe can be written where an operator would
/// actually write it.
///
/// Four things disqualify a marker, and each one is a case where the route would prove something
/// other than what the case claims. A class other than `projection` transforms the value on the way,
/// or holds names the operator chose, so neither is a value a probe can be compared against. A
/// marker scoped to other documents does not bind the key here. A key several values feed has no
/// single value to write the probe into. And a chart value under the probe root is the escape hatch
/// already.
///
/// A `when` clause travels with the route: the chart tests that path before writing the key at all,
/// so a case that did not switch it on would assert against a document the probe never reached. Both
/// the value and the gate are required to exist, which is what stops a renamed value from turning
/// into a probe the templating engine silently creates and nothing renders.
fn routes_for(
    enrolment_path: &Path,
    markers: &[Marker],
    blocks: &[Block],
    document: &Document,
    values: &Json,
    root: &str,
    unrouted: &[(String, String)],
) -> Result<BTreeMap<String, Route>, Error> {
    let by_path: BTreeMap<&str, &Block> = blocks
        .iter()
        .map(|block| (block.values_path.as_str(), block))
        .collect();

    let by_key = by_key(markers, document, &[Class::Projection]);

    let declined: BTreeSet<&str> = unrouted.iter().map(|(key, _)| key.as_str()).collect();
    let stale: Vec<&&str> = declined
        .iter()
        .filter(|key| !by_key.contains_key(*key))
        .collect();
    if !stale.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: document '{}' declares {} `unrouted`, and no `@config projection` marker binds \
             that key here — so nothing was routed away from and the entry is describing a chart \
             that no longer exists",
            enrolment_path.display(),
            document.name,
            stale
                .iter()
                .map(|key| crate::gate::quoted(key))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let mut routes = BTreeMap::new();
    for (target, markers) in by_key {
        if declined.contains(target) || markers.len() != 1 {
            continue;
        }
        let marker = markers[0];
        let path = &marker.values_path;
        if path == root || path.starts_with(&format!("{root}.")) {
            continue;
        }
        if !has_path(values, path) {
            continue;
        }
        if marker
            .condition
            .as_deref()
            .is_some_and(|gate| !has_path(values, gate))
        {
            continue;
        }

        let schema = by_path
            .get(path.as_str())
            .and_then(|block| shapes::block_schema(block).ok())
            .filter(|schema| schema.as_object().is_some_and(|held| !held.is_empty()));

        routes.insert(
            target.to_owned(),
            Route {
                values_path: path.clone(),
                condition: marker.condition.clone(),
                schema,
            },
        );
    }
    Ok(routes)
}

/// The chart value each of this document's keys is written from unchanged, where there is one.
///
/// What [`unsupplied_entries`] reads a render prerequisite through: a chart whose own schema demands
/// a required map's entries states them once, as the chart value, and the key holds them because
/// the template copies that value across. A `projection` or `structured` marker is such a copy; a
/// `composed` one is not. A key several values feed is left out, since which of them the template
/// writes is not something a marker says, and so is a chart value under the probe root, which the
/// check already reads as the tree itself.
fn carriers_for(markers: &[Marker], document: &Document, root: &str) -> BTreeMap<String, Carrier> {
    by_key(markers, document, &[Class::Projection, Class::Structured])
        .into_iter()
        .filter_map(|(target, markers)| match markers.as_slice() {
            [marker] => Some((target, *marker)),
            _ => None,
        })
        .filter(|(_, marker)| {
            let path = &marker.values_path;
            path != root && !path.starts_with(&format!("{root}."))
        })
        .map(|(target, marker)| {
            (
                target.to_owned(),
                Carrier {
                    values_path: marker.values_path.clone(),
                    condition: marker.condition.clone(),
                },
            )
        })
        .collect()
}

/// [`unsupplied_entries`] over one document, with the enrolment's file named on the refusal.
fn refuse_unsupplied(
    enrolment_path: &Path,
    document: &Document,
    enrolment: &Enrolment,
    keys: &[&crate::union::Merged],
    values: &Json,
    markers: &[Marker],
) -> Result<(), Error> {
    unsupplied_entries(
        keys,
        &enrolment.baseline.values,
        &enrolment.prerequisites.values,
        values,
        &enrolment.probe,
        &carriers_for(markers, document, &enrolment.probe),
    )
    .map_err(|gap| {
        Error::Invalid(format!(
            "{}: {}: {gap}",
            enrolment_path.display(),
            document.name
        ))
    })
}

/// The chart's `@config` markers and the values blocks they annotate, read from `values.yaml` once.
///
/// None for a chart the declaration leaves out of the bindings gate: its markers are unchecked, so
/// neither a route nor a carrier may be read from them.
fn markers_of(
    chart_dir: &Path,
    declaration: &Declaration,
    text: &str,
) -> Result<(Vec<Marker>, Vec<Block>), Error> {
    if !declaration.bindings {
        return Ok((Vec::new(), Vec::new()));
    }
    let chart = chart_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    super::markers::read(text, chart)
}

/// This document's markers of the given classes, grouped by the contract key each one feeds.
///
/// A marker scoped to other documents does not bind its key here, so it is left out.
fn by_key<'a>(
    markers: &'a [Marker],
    document: &Document,
    classes: &[Class],
) -> BTreeMap<&'a str, Vec<&'a Marker>> {
    let mut grouped: BTreeMap<&str, Vec<&Marker>> = BTreeMap::new();
    for marker in markers {
        if !classes.contains(&marker.class) {
            continue;
        }
        if marker
            .documents
            .as_ref()
            .is_some_and(|scope| !scope.contains(&document.name))
        {
            continue;
        }
        grouped.entry(&marker.target).or_default().push(marker);
    }
    grouped
}

/// Every suite one chart owns, as a path to text mapping.
///
/// # Errors
/// [`Error::Invalid`] when the enrolment or the declaration cannot be reconciled, [`Error::Io`] when
/// the chart cannot be read.
pub fn build(
    chart_dir: &Path,
    declaration: &Declaration,
    enrolments: &Enrolments,
) -> Result<BTreeMap<PathBuf, String>, Error> {
    let declared: BTreeSet<&str> = declaration
        .documents
        .iter()
        .map(|document| document.name.as_str())
        .collect();
    let unknown: Vec<&str> = enrolments
        .documents
        .keys()
        .map(String::as_str)
        .filter(|name| !declared.contains(name))
        .collect();
    if !unknown.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: names document(s) {}, which {} does not declare",
            enrolments.path.display(),
            unknown.join(", "),
            declaration.path.display()
        )));
    }

    let path = chart_dir.join("values.yaml");
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
    let values = read_yaml(&text, &path)?;
    let (markers, blocks) = markers_of(chart_dir, declaration, &text)?;

    let default = Enrolment::default();
    let mut suites = BTreeMap::new();
    for document in &declaration.documents {
        let enrolment = enrolments.documents.get(&document.name).unwrap_or(&default);
        probe_tree(chart_dir, &values, &document.name, &enrolment.probe)?;

        let loaded = vendored_for(chart_dir, document)?;
        let contracts: Vec<(String, Json)> = loaded
            .iter()
            .map(|item| (item.label.clone(), item.vendored.contract.clone()))
            .collect();
        let union = union_contracts(&contracts)?;

        let target = Target {
            chart: chart_dir
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_owned(),
            name: document.name.clone(),
            kind: document.source.kind.clone(),
            selector: document.source.selector.clone(),
            key: document.source.key.clone(),
            declaration: repository_path(
                chart_dir,
                declaration
                    .path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default(),
            ),
            enrolment: enrolled_in(&enrolments.path),
            contracts: document
                .images
                .iter()
                .map(|reference| repository_path(chart_dir, &reference.contract))
                .collect(),
            discriminator: discriminator_for(declaration, document)?,
            root: enrolment.probe.clone(),
        };

        let routes = routes_for(
            &enrolments.path,
            &markers,
            &blocks,
            document,
            &values,
            &enrolment.probe,
            &enrolment.unrouted,
        )?;
        let keys: Vec<&crate::union::Merged> = union.keys.iter().collect();
        refuse_unsupplied(
            &enrolments.path,
            document,
            enrolment,
            &keys,
            &values,
            &markers,
        )?;
        let held: Plan = plan(
            &keys,
            &enrolment.baseline.values,
            &enrolment.prerequisites.values,
            &enrolment.probe,
            &routes,
        )?;

        suites.insert(
            suite_path(chart_dir, document),
            render_suite(
                &target,
                &held,
                &enrolment.baseline.values,
                enrolment.baseline.reason.as_deref(),
                &enrolment.prerequisites.values,
                enrolment.prerequisites.reason.as_deref(),
                &enrolment.unrouted,
            )?,
        );
    }
    Ok(suites)
}

/// Generated suites left behind by a document — or a whole chart — that no longer declares one.
///
/// Owned rather than reported: a suite for a removed document keeps asserting keys nothing reads any
/// more, and leaving it in place while claiming the tree is generated would be the drift this whole
/// mechanism exists to remove. Swept for on every chart rather than only on the enrolled ones,
/// because un-enrolling a chart is the case that leaves the most behind and the case an
/// enrolled-only sweep cannot see.
fn orphans(chart_dir: &Path, wanted: &BTreeMap<PathBuf, String>) -> Vec<PathBuf> {
    let directory = chart_dir.join(SUITES);
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.starts_with(SUITE_PREFIX) && name.ends_with(SUITE_SUFFIX))
                && !wanted.contains_key(path)
        })
        .collect();
    found.sort();
    found
}

// ------------------------------------------------------------------------------------------------
// The walk
// ------------------------------------------------------------------------------------------------

/// Every enrolled chart's suites, and every generated suite that no longer has a document.
#[derive(Debug, Default)]
pub struct Generated {
    /// The suites, by the path each is written to.
    pub suites: BTreeMap<PathBuf, String>,
    /// Generated suites whose document is gone.
    pub stale: Vec<PathBuf>,
    /// Charts skipped for carrying no enrolment.
    pub unenrolled: Vec<String>,
}

/// Walk the chart tree and build every enrolled chart's suites.
///
/// # Errors
/// [`Error::Invalid`] when a chart's enrolment or declaration cannot be reconciled, [`Error::Io`]
/// when the tree cannot be walked.
pub fn collect(charts: &Path, selection: &Selection) -> Result<Generated, Error> {
    let mut generated = Generated::default();
    for chart_dir in selection.dirs(charts)? {
        let name = chart_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned();

        let declaration = load_declaration(&chart_dir)?;
        let Some(enrolments) = load_enrolment(&chart_dir, declaration.as_ref())? else {
            generated.unenrolled.push(name);
            generated
                .stale
                .extend(orphans(&chart_dir, &BTreeMap::new()));
            continue;
        };

        let declaration = declaration.filter(|held| !held.documents.is_empty());
        let Some(declaration) = declaration else {
            return Err(Error::Invalid(format!(
                "{}: enrols this chart, but it declares no configuration document to generate a \
                 suite for",
                enrolments.path.display()
            )));
        };

        let wanted = build(&chart_dir, &declaration, &enrolments)?;
        generated.stale.extend(orphans(&chart_dir, &wanted));
        generated.suites.extend(wanted);
    }
    Ok(generated)
}

/// What a run of [`sync`] or [`check`] did or would do.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Suites written, or that would be.
    pub written: Vec<PathBuf>,
    /// Suites removed, or that would be.
    pub removed: Vec<PathBuf>,
    /// The differences, for a check that reports rather than writes.
    pub drift: Vec<String>,
}

/// Write what changed and remove what is orphaned.
///
/// # Errors
/// [`Error::Io`] when a suite cannot be written or an orphan removed.
pub fn sync(generated: &Generated) -> Result<Outcome, Error> {
    let mut outcome = Outcome::default();
    for (path, text) in &generated.suites {
        let current = std::fs::read_to_string(path).ok();
        if current
            .as_deref()
            .is_some_and(|held| normalised(held) == normalised(text))
        {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
        }
        std::fs::write(path, as_written(text, current.as_deref()))
            .map_err(|e| Error::io(path.display(), e))?;
        outcome.written.push(path.clone());
    }
    for path in &generated.stale {
        std::fs::remove_file(path).map_err(|e| Error::io(path.display(), e))?;
        outcome.removed.push(path.clone());
    }
    Ok(outcome)
}

/// Report every suite that has drifted, writing nothing.
#[must_use]
pub fn check(generated: &Generated) -> Outcome {
    let mut outcome = Outcome::default();
    for (path, text) in &generated.suites {
        let current = std::fs::read_to_string(path).unwrap_or_default();
        if normalised(&current) == normalised(text) {
            continue;
        }
        outcome.written.push(path.clone());
        outcome.drift.push(format!(
            "{}: {}\n{}",
            path.display(),
            if current.is_empty() {
                "missing"
            } else {
                "drifted"
            },
            moved_lines(&current, text)
        ));
    }
    for path in &generated.stale {
        outcome.removed.push(path.clone());
        outcome.drift.push(format!(
            "{}: is a generated suite for a document no longer declared",
            path.display()
        ));
    }
    outcome
}

/// One suite's text with its line endings normalised.
///
/// The comparison a staleness gate can actually make on a tree that may be checked out with either
/// convention. A repository developed from a Windows checkout with `core.autocrlf` on holds CRLF in
/// the working tree and LF in the index, and a gate comparing raw bytes would fail on every line of
/// every file, forever.
fn normalised(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// One suite as it is written back, keeping whatever convention the committed copy already used.
///
/// Preserved rather than imposed, and preserved rather than fixed at LF. Imposing the platform's
/// convention is what the implementation this was ported from did, and it makes the *first* run on
/// a differently-checked-out tree rewrite every line of every file. Keeping the file's own means a
/// regeneration touches only what a rule actually changed, which is what makes reviewing one
/// possible. A file that does not exist yet has no convention to keep, and takes the platform's.
fn as_written(text: &str, current: Option<&str>) -> String {
    let crlf = match current {
        Some(held) => held.contains("\r\n"),
        None => cfg!(windows),
    };
    let text = normalised(text);
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// The lines that moved, as a reviewer of a failing check wants to see them.
///
/// A whole-file listing would print a generated suite, and the only lines a reader needs are the
/// ones that disagree. Deliberately not a minimal edit script: this compares two renderings of one
/// generated file, so an aligned comparison would spend a dependency on making an already short
/// answer marginally shorter.
fn moved_lines(before: &str, after: &str) -> String {
    use std::fmt::Write as _;

    let (before, after) = (normalised(before), normalised(after));
    let (before, after): (Vec<&str>, Vec<&str>) =
        (before.lines().collect(), after.lines().collect());
    let mut out = String::new();
    for line in before.iter().filter(|line| !after.contains(line)) {
        let _ = writeln!(out, "  -{line}");
    }
    for line in after.iter().filter(|line| !before.contains(line)) {
        let _ = writeln!(out, "  +{line}");
    }
    out
}

#[cfg(test)]
mod tests;
