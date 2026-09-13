//! Generating the round-trip test suites a chart's contracts imply, and keeping them in step.
//!
//! The walk over the charts, the file IO and what it means for a suite to be stale. The model —
//! choosing a probe, refusing to invent one, and spelling the assertion — lives in
//! [`super::testgen`], so every rule in it is testable by calling it.
//!
//! # Enrolment is a file, not a list
//!
//! A chart is generated for when it carries `contract-tests.yaml`, exactly as `config-contract.yaml`
//! is what enrols it in the document gate. That is what makes a phased rollout a property of the
//! tree rather than of a constant somebody has to remember to edit, and it is also where a chart
//! says which values every generated case has to carry before it will render at all, and — where the
//! operator-facing configuration tree is not the layer that wins — which values path a probe is to
//! be written to.
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
    Declaration, Document, chart_dirs, load_declaration, read_yaml, reject_unknown, vendored_for,
};
use super::markers::Class;
use super::testgen::{Plan, Route, Target, VALUES_ROOT, plan, prerequisite_conflict, render_suite};
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
const UNROUTED_KEYS: [&str; 2] = ["keys", "reason"];
const PREREQUISITE_KEYS: [&str; 2] = ["values", "reason"];

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

/// Read one chart's enrolment; [`None`] when the chart is not enrolled.
///
/// # Errors
/// [`Error::Invalid`] naming whichever entry is not one, [`Error::Io`] when the file cannot be read.
pub fn load_enrolment(chart_dir: &Path) -> Result<Option<BTreeMap<String, Enrolment>>, Error> {
    let path = chart_dir.join(ENROLMENT);
    if !path.is_file() {
        return Ok(None);
    }
    let at = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&at, e))?;
    let document = read_yaml(&text, &path)?;
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

        let probe = load_probe(&at, name, entry.get("probe"))?;
        let reason = entry
            .get("reason")
            .and_then(Json::as_str)
            .map(str::to_owned);
        let baseline = Baseline {
            values: load_baseline(&at, name, entry.get("baseline"), &probe)?,
            reason,
        };
        if (!baseline.values.is_empty() || probe != VALUES_ROOT) && baseline.reason.is_none() {
            return Err(Error::Invalid(format!(
                "{at}: the entry for `{name}` sets a baseline or a probe path and has no \
                 `reason`; either narrows what every generated case proves, and an unexplained \
                 hole in a gate is indistinguishable from an oversight"
            )));
        }

        enrolments.insert(
            name.to_owned(),
            Enrolment {
                prerequisites: load_prerequisites(&at, name, entry.get("prerequisites"), &probe)?,
                unrouted: load_unrouted(&at, name, entry.get("unrouted"))?,
                baseline,
                probe,
            },
        );
    }
    Ok(Some(enrolments))
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
    chart_dir: &Path,
    declaration: &Declaration,
    document: &Document,
    values: &Json,
    root: &str,
    unrouted: &[(String, String)],
) -> Result<BTreeMap<String, Route>, Error> {
    if !declaration.bindings {
        return Ok(BTreeMap::new());
    }

    let path = chart_dir.join("values.yaml");
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
    let chart = chart_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    let (markers, blocks) = super::markers::read(&text, chart)?;

    let by_path: BTreeMap<&str, &super::markers::Block> = blocks
        .iter()
        .map(|block| (block.values_path.as_str(), block))
        .collect();

    let mut by_key: BTreeMap<&str, Vec<&super::markers::Marker>> = BTreeMap::new();
    for marker in &markers {
        if marker.class != Class::Projection {
            continue;
        }
        if marker
            .documents
            .as_ref()
            .is_some_and(|scope| !scope.contains(&document.name))
        {
            continue;
        }
        by_key.entry(&marker.target).or_default().push(marker);
    }

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
            chart_dir.join(ENROLMENT).display(),
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

/// Every suite one chart owns, as a path to text mapping.
///
/// # Errors
/// [`Error::Invalid`] when the enrolment or the declaration cannot be reconciled, [`Error::Io`] when
/// the chart cannot be read.
pub fn build(
    chart_dir: &Path,
    declaration: &Declaration,
    enrolments: &BTreeMap<String, Enrolment>,
) -> Result<BTreeMap<PathBuf, String>, Error> {
    let declared: BTreeSet<&str> = declaration
        .documents
        .iter()
        .map(|document| document.name.as_str())
        .collect();
    let unknown: Vec<&str> = enrolments
        .keys()
        .map(String::as_str)
        .filter(|name| !declared.contains(name))
        .collect();
    if !unknown.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: names document(s) {}, which {} does not declare",
            chart_dir.join(ENROLMENT).display(),
            unknown.join(", "),
            declaration.path.display()
        )));
    }

    let path = chart_dir.join("values.yaml");
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.display(), e))?;
    let values = read_yaml(&text, &path)?;

    let default = Enrolment::default();
    let mut suites = BTreeMap::new();
    for document in &declaration.documents {
        let enrolment = enrolments.get(&document.name).unwrap_or(&default);
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
            contracts: document
                .images
                .iter()
                .map(|reference| repository_path(chart_dir, &reference.contract))
                .collect(),
            discriminator: discriminator_for(declaration, document)?,
            root: enrolment.probe.clone(),
        };

        let routes = routes_for(
            chart_dir,
            declaration,
            document,
            &values,
            &enrolment.probe,
            &enrolment.unrouted,
        )?;
        let keys: Vec<&crate::union::Merged> = union.keys.iter().collect();
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
pub fn collect(charts: &Path, only: Option<&str>) -> Result<Generated, Error> {
    let mut generated = Generated::default();
    for chart_dir in chart_dirs(charts)? {
        let name = chart_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned();
        if only.is_some_and(|wanted| wanted != name) {
            continue;
        }

        let Some(enrolments) = load_enrolment(&chart_dir)? else {
            generated.unenrolled.push(name);
            generated
                .stale
                .extend(orphans(&chart_dir, &BTreeMap::new()));
            continue;
        };

        let declaration = load_declaration(&chart_dir)?;
        let declaration = declaration.filter(|held| !held.documents.is_empty());
        let Some(declaration) = declaration else {
            return Err(Error::Invalid(format!(
                "{}: enrols this chart, but it declares no configuration document to generate a \
                 suite for",
                chart_dir.join(ENROLMENT).display()
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
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use serde_json::{Value as Json, json};

    use super::{ENROLMENT, Enrolment, discriminator_for, load_enrolment, orphans, probe_tree};
    use crate::gate::document::{DocumentFormat, DocumentSource};
    use crate::helm::declaration::{Declaration, Document, ImageRef};
    use crate::helm::testgen::VALUES_ROOT;

    /// A chart directory holding the files a test wants, removed when it goes out of scope.
    ///
    /// Hand-rolled rather than a dependency: several of these need a directory on disk, and the
    /// crate that would supply one would be carried by every consumer of this library's test build.
    struct Chart(PathBuf);

    impl Chart {
        fn of(files: &[(&str, &str)]) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir().join(format!(
                "terrace-suites-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(&at).expect("a directory");
            for (name, body) in files {
                let path = at.join(name);
                std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
                std::fs::write(path, body).expect("a file is written");
            }
            Self(at)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Chart {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// One enrolment file, read.
    fn enrolment(body: &str) -> Result<BTreeMap<String, Enrolment>, crate::error::Error> {
        let chart = Chart::of(&[(ENROLMENT, body)]);
        Ok(load_enrolment(chart.path())?.expect("the file is there"))
    }

    fn refused(body: &str) -> String {
        enrolment(body)
            .expect_err("the enrolment is refused")
            .to_string()
    }

    /// One declared document, as a name, a source key and the labels it is selected on.
    type Declared<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    /// One chart's declaration, from those triples.
    fn declaration(documents: &[Declared<'_>]) -> Declaration {
        Declaration {
            chart: "chart".to_owned(),
            path: PathBuf::from("charts/chart/config-contract.yaml"),
            documents: documents
                .iter()
                .map(|(name, key, selector)| Document {
                    name: (*name).to_owned(),
                    source: DocumentSource {
                        kind: "ConfigMap".to_owned(),
                        selector: selector
                            .iter()
                            .map(|(label, value)| ((*label).to_owned(), (*value).to_owned()))
                            .collect(),
                        key: (*key).to_owned(),
                        format: DocumentFormat::Toml,
                    },
                    images: vec![ImageRef {
                        values: "image".to_owned(),
                        contract: "contracts/one.json".to_owned(),
                    }],
                    consumers: Vec::new(),
                    exempt: Vec::new(),
                })
                .collect(),
            reason: None,
            unconfigured: Vec::new(),
            bindings: false,
            unbound: Vec::new(),
            credentials: BTreeMap::new(),
        }
    }

    // -- the enrolment file ------------------------------------------------------------------

    #[test]
    fn a_chart_without_the_file_is_not_enrolled() {
        let chart = Chart::of(&[]);
        assert!(
            load_enrolment(chart.path())
                .expect("an absent file reads")
                .is_none()
        );
        assert!(
            enrolment("documents: []")
                .expect("an empty list is legal")
                .is_empty()
        );
    }

    #[test]
    fn a_baseline_is_read_as_sorted_pairs() {
        let enrolled = enrolment(
            "documents:\n  - name: server\n    baseline:\n      config.b: 1\n      \
             config.a: false\n    reason: because\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["server"].baseline.values,
            [
                ("config.a".to_owned(), json!(false)),
                ("config.b".to_owned(), json!(1)),
            ]
        );
    }

    #[test]
    fn a_baseline_without_a_reason_is_refused() {
        // An unexplained hole in a gate is indistinguishable from an oversight.
        let failure =
            refused("documents:\n  - name: server\n    baseline:\n      config.a: false\n");
        assert!(failure.contains("reason"), "{failure}");
    }

    #[test]
    fn a_baseline_that_is_not_configuration_at_the_probed_layer_is_refused() {
        // Outside the tree it would never compare equal to a probe, so the collision it exists to
        // be dropped for would go unnoticed. Nested, the comparison could not be made at all.
        assert!(
            refused(
                "documents:\n  - name: server\n    baseline:\n      image.tag: v1\n    \
                 reason: because\n"
            )
            .contains("under `config`")
        );
        assert!(
            refused(
                "documents:\n  - name: server\n    baseline:\n      config.csp: {a: 1}\n    \
                 reason: because\n"
            )
            .contains("not a scalar")
        );
    }

    #[test]
    fn a_document_with_no_probe_is_probed_through_the_configuration_tree() {
        let enrolled = enrolment("documents:\n  - name: server\n").expect("it reads");
        assert_eq!(enrolled["server"].probe, VALUES_ROOT);
    }

    #[test]
    fn a_probe_path_is_read_as_the_root_every_case_writes_under() {
        let enrolled = enrolment(
            "documents:\n  - name: api\n    probe: services.api.config\n    reason: because\n",
        )
        .expect("it reads");
        assert_eq!(enrolled["api"].probe, "services.api.config");
    }

    #[test]
    fn a_probe_that_is_not_a_dotted_values_path_is_refused() {
        // It is used both as a `set` prefix and as a walk into the chart's values, and neither
        // reads anything else.
        for offender in [
            "'services[\"api\"].config'",
            "'services..config'",
            "'.config'",
            "''",
            "7",
        ] {
            let body =
                format!("documents:\n  - name: api\n    probe: {offender}\n    reason: because\n");
            assert!(enrolment(&body).is_err(), "{offender}");
        }
    }

    #[test]
    fn a_probe_without_a_reason_is_refused() {
        // It narrows the suite to one layer, which is the same hole a baseline opens.
        let failure = refused("documents:\n  - name: api\n    probe: services.api.config\n");
        assert!(failure.contains("reason"), "{failure}");
    }

    #[test]
    fn a_baseline_is_read_under_the_probe_root_rather_than_under_the_default() {
        let enrolled = enrolment(
            "documents:\n  - name: api\n    probe: services.api.config\n    baseline:\n      \
             services.api.config.a: false\n    reason: because\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["api"].baseline.values,
            [("services.api.config.a".to_owned(), json!(false))]
        );

        assert!(
            enrolment(
                "documents:\n  - name: api\n    probe: services.api.config\n    baseline:\n      \
                 config.a: false\n    reason: because\n",
            )
            .is_err()
        );
    }

    #[test]
    fn a_typo_is_refused_rather_than_ignored() {
        // The same rule the declaration is read under: a key nobody reads is a key somebody meant
        // to spell differently.
        assert!(enrolment("documents: []\nreason: stray").is_err());
        assert!(enrolment("documents:\n  - name: server\n  - name: server\n").is_err());
    }

    // -- render prerequisites ----------------------------------------------------------------

    fn prerequisites(values: &str) -> Result<BTreeMap<String, Enrolment>, crate::error::Error> {
        enrolment(&format!(
            "documents:\n  - name: server\n    prerequisites:\n      values:\n{values}      \
             reason: because the guard insists\n"
        ))
    }

    #[test]
    fn prerequisites_are_read_as_sorted_pairs_with_their_shapes_intact() {
        // A map keyed by a request path reads worse split up than it does whole, and a flattened
        // one would set the wrong thing.
        let enrolled = prerequisites(
            "        webhook.targetBase: https://example.invalid\n        bucket.entries:\n          \
             link: {bucket: b, object: o}\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["server"].prerequisites.values,
            [
                (
                    "bucket.entries".to_owned(),
                    json!({"link": {"bucket": "b", "object": "o"}})
                ),
                (
                    "webhook.targetBase".to_owned(),
                    json!("https://example.invalid")
                ),
            ]
        );
        assert_eq!(
            enrolled["server"].prerequisites.reason.as_deref(),
            Some("because the guard insists")
        );
    }

    #[test]
    fn a_prerequisite_inside_the_probed_tree_is_refused_with_the_file_named() {
        // The guarantee, at the loader; the model refuses it again for a caller that skips this.
        let failure = prerequisites("        config.isr.ttl_secs: 99\n")
            .expect_err("it is refused")
            .to_string();
        assert!(failure.contains(ENROLMENT), "{failure}");
        assert!(failure.contains(VALUES_ROOT), "{failure}");
        assert!(prerequisites("        config: {isr: {ttl_secs: 99}}\n").is_err());
    }

    #[test]
    fn a_prerequisite_block_that_explains_or_sets_nothing_is_refused() {
        // A value nobody explained is one nobody can tell from a workaround somebody stopped
        // needing, and a block that sets nothing is a field somebody meant to fill in.
        assert!(
            enrolment(
                "documents:\n  - name: server\n    prerequisites:\n      values:\n        \
                 webhook.targetBase: https://example.invalid\n",
            )
            .expect_err("it is refused")
            .to_string()
            .contains("reason")
        );
        assert!(
            enrolment("documents:\n  - name: server\n    prerequisites:\n      reason: because\n")
                .is_err()
        );
        assert!(
            enrolment(
                "documents:\n  - name: server\n    prerequisites:\n      value: {a: 1}\n      \
                 reason: because\n",
            )
            .is_err()
        );
    }

    #[test]
    fn overlapping_prerequisites_are_refused() {
        // A `set` mapping has no order, so which of the two survives is stated nowhere.
        let failure = prerequisites(
            "        bucket: {}\n        bucket.entries:\n          link: {bucket: b}\n",
        )
        .expect_err("it is refused")
        .to_string();
        assert!(failure.contains("overlap"), "{failure}");
    }

    #[test]
    fn a_moved_probe_root_moves_which_prerequisite_the_loader_refuses() {
        // The loader reads the root first, so its refusal is the model's under a moved root.
        let enrolled = enrolment(
            "documents:\n  - name: api\n    probe: services.api.config\n    reason: the derived \
             wiring outranks it\n    prerequisites:\n      values:\n        config.profile: \
             development\n      reason: because the guard insists\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["api"].prerequisites.values,
            [("config.profile".to_owned(), json!("development"))]
        );

        let failure = enrolment(
            "documents:\n  - name: api\n    probe: services.api.config\n    reason: the derived \
             wiring outranks it\n    prerequisites:\n      values:\n        \
             services.api.config.profile: development\n      reason: because the guard insists\n",
        )
        .expect_err("it is refused")
        .to_string();
        assert!(failure.contains("services.api.config"), "{failure}");
    }

    #[test]
    fn a_document_may_carry_a_baseline_and_prerequisites_at_once() {
        let enrolled = enrolment(
            "documents:\n  - name: server\n    baseline:\n      config.a: false\n    reason: \
             the chart's own guard\n    prerequisites:\n      values:\n        webhook.targetBase: \
             https://example.invalid\n      reason: the other guard\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["server"].baseline.values,
            [("config.a".to_owned(), json!(false))]
        );
        assert_eq!(
            enrolled["server"].prerequisites.values,
            [(
                "webhook.targetBase".to_owned(),
                json!("https://example.invalid")
            )]
        );
    }

    // -- unrouted keys -----------------------------------------------------------------------

    #[test]
    fn an_unrouted_entry_states_its_keys_and_one_reason_for_them() {
        let enrolled = enrolment(
            "documents:\n  - name: server\n    unrouted:\n      - keys: [b.two, a.one]\n        \
             reason: the chart guards the position\n",
        )
        .expect("it reads");
        assert_eq!(
            enrolled["server"].unrouted,
            [
                (
                    "a.one".to_owned(),
                    "the chart guards the position".to_owned()
                ),
                (
                    "b.two".to_owned(),
                    "the chart guards the position".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn an_unrouted_entry_with_no_reason_or_a_key_named_twice_is_refused() {
        // A probe moved off the chart's own value without a reason is indistinguishable from an
        // oversight, and two reasons for one key means one of them is stale.
        assert!(
            enrolment("documents:\n  - name: server\n    unrouted:\n      - keys: [a.one]\n")
                .is_err()
        );
        assert!(
            enrolment(
                "documents:\n  - name: server\n    unrouted:\n      - keys: [a.one]\n        \
                 reason: one\n      - keys: [a.one]\n        reason: two\n",
            )
            .expect_err("it is refused")
            .to_string()
            .contains("stale")
        );
    }

    // -- what a chart has to expose ----------------------------------------------------------

    #[test]
    fn a_probe_path_the_chart_does_not_expose_is_refused() {
        // The likelier mistake by far: an enrolment naming a path this chart does not have, which
        // would otherwise generate a whole suite of cases setting values nothing reads.
        let values: Json = json!({"config": {}, "services": {"api": {"config": {}}}});
        let chart = Chart::of(&[]);
        probe_tree(chart.path(), &values, "api", "services.api.config").expect("it is there");
        for missing in ["services.worker.config", "nothing"] {
            assert!(
                probe_tree(chart.path(), &values, "api", missing).is_err(),
                "{missing}"
            );
        }
    }

    // -- telling one document from its siblings ----------------------------------------------

    #[test]
    fn the_key_is_derived_to_be_enough_when_no_sibling_shares_it() {
        let held = declaration(&[
            (
                "server",
                "config.toml",
                &[("app.kubernetes.io/instance", "portfolio")],
            ),
            (
                "agent",
                "agent.toml",
                &[("app.kubernetes.io/instance", "portfolio")],
            ),
        ]);
        for document in &held.documents {
            assert!(
                discriminator_for(&held, document)
                    .expect("it is derivable")
                    .is_empty(),
                "{}",
                document.name
            );
        }
    }

    #[test]
    fn a_shared_key_derives_the_labels_from_the_declaration() {
        // A second copy in the enrolment could disagree with the first, and the gate reading the
        // first would then be validating an object the suite never selects.
        let held = declaration(&[
            (
                "api",
                "config.toml",
                &[("app.kubernetes.io/component", "api")],
            ),
            (
                "worker",
                "config.toml",
                &[("app.kubernetes.io/component", "worker")],
            ),
        ]);
        assert_eq!(
            discriminator_for(&held, &held.documents[0]).expect("it is derivable"),
            [("app.kubernetes.io/component".to_owned(), "api".to_owned())]
        );
    }

    #[test]
    fn a_shared_key_nothing_tells_apart_is_refused() {
        // Deriving them anyway would emit a selector that matches both and fails at run time.
        let held = declaration(&[("api", "config.toml", &[]), ("worker", "config.toml", &[])]);
        let failure = discriminator_for(&held, &held.documents[0])
            .expect_err("it is refused")
            .to_string();
        assert!(failure.contains("tells the documents apart"), "{failure}");

        let shared: &[(&str, &str)] = &[("app.kubernetes.io/instance", "chart")];
        let held = declaration(&[
            ("api", "config.toml", shared),
            ("worker", "config.toml", shared),
        ]);
        assert!(discriminator_for(&held, &held.documents[0]).is_err());
    }

    // -- suites left behind ------------------------------------------------------------------

    fn sweep(wanted: &[&str], present: &[&str]) -> Vec<String> {
        let files: Vec<(String, &str)> = present
            .iter()
            .map(|name| (format!("tests/{name}"), "suite: x\n"))
            .collect();
        let held: Vec<(&str, &str)> = files
            .iter()
            .map(|(name, body)| (name.as_str(), *body))
            .collect();
        let chart = Chart::of(&held);
        let wanted: BTreeMap<PathBuf, String> = wanted
            .iter()
            .map(|name| (chart.path().join("tests").join(name), String::new()))
            .collect();
        orphans(chart.path(), &wanted)
            .into_iter()
            .map(|path| {
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_suite_whose_document_is_gone_is_reported() {
        // It keeps asserting keys nothing reads any more, which is the drift to catch.
        assert_eq!(
            sweep(
                &["contract_roundtrip_api_test.yaml"],
                &[
                    "contract_roundtrip_api_test.yaml",
                    "contract_roundtrip_worker_test.yaml"
                ]
            ),
            ["contract_roundtrip_worker_test.yaml"]
        );
    }

    #[test]
    fn un_enrolling_a_chart_orphans_every_suite_it_had_and_touches_nothing_else() {
        assert_eq!(
            sweep(&[], &["contract_roundtrip_api_test.yaml"]),
            ["contract_roundtrip_api_test.yaml"]
        );
        assert!(sweep(&[], &["configmap_test.yaml"]).is_empty());
    }
}
