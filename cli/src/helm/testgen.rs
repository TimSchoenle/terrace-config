//! Turning a configuration contract into round-trip test cases, and the suite they are written to.
//!
//! The document gate validates the configuration a chart *renders*. What no gate can see is the
//! round trip: that a setting an operator writes into the chart's values arrives in the
//! application's document, at the path the image reads it from, carrying the value that was asked
//! for. A typo in a template helper produces a document that satisfies the contract perfectly:
//! every key it does contain is legal, and the setting is simply absent. The pod starts, reports
//! healthy, and runs on a compiled default nobody chose, which is the same failure the contract
//! pipeline exists to remove, one layer further up.
//!
//! Proving the round trip needs a *different* value on each side of it, so this module's whole job
//! is choosing one: a probe the chart cannot have produced by accident, synthesised from what the
//! contract says the key will accept. Everything here is a pure function of a contract key and a
//! plan, so every rule is testable by calling it.
//!
//! Seven rules are normative rather than convenient. The first four are about the probe, and the
//! last three about the shape of the chart it is written into.
//!
//! **A probe must differ from the key's default.** A case that sets a key to the value it already
//! has passes whether or not the chart delivers anything, which is worse than no case at all: it
//! reports a proof it has not earned. Where no value other than the default is admissible (a
//! `const`, a single-entry `values` list), the key is skipped and says so.
//!
//! **Three forms carry no probe, and the reason differs in each.** `secret: true` is refused because
//! the probe is committed to a repository, and a plausible-looking credential in a test file is a
//! credential. `structured` is refused because the keys beneath it are the operator's own names, so
//! the contract describes the container and not its contents and nothing about a leaf inside it can
//! be synthesised. `unknown` is refused because the producer publishes no constraint at all, which
//! is a gap stated explicitly rather than an answer. These are three different situations and the
//! generated file names which one applies, so a reader is never left to guess whether a missing case
//! is a decision or an oversight.
//!
//! **The assertion is scoped to the table the key belongs to, not merely to its leaf name.** In
//! TOML a key belongs to the most recent `[table]` header, so a pattern matching `ttl_secs = 4242`
//! anywhere in the document would accept a chart that wrote the value under the wrong table, which
//! is precisely the defect a round-trip test exists to catch. The generated pattern anchors on the
//! header, walks only lines that do not open a new table, and then matches the leaf; RE2 has no
//! lookahead, so "a line that is not a header" is spelled as an alternation.
//!
//! **Regex metacharacters are escaped against the intersection of two engines.** The patterns are
//! written here and evaluated by Go, and a general escaper escapes punctuation (`-`, `&`, `~`, `#`,
//! the space) that Go's parser is not obliged to accept behind a backslash. So only the characters
//! that are metacharacters in both are escaped, and everything else is passed through as the
//! literal it already is.
//!
//! **A document is selected by the key it carries, and by its labels only where the key is not
//! enough.** The document gate reads a document out of the key its declaration names, so selecting
//! on that same fact is what ties the two to one object, where a label selector on its own matches
//! the Deployment and the Service as well. A chart that renders the same key into several documents
//! breaks that, and there the labels the declaration already selects on are added to the key. The
//! two facts go into one `JSONPath` filter rather than into two matchers, because the test
//! framework's document selector carries exactly one path and one value.
//!
//! **Which values path a probe is written to is the chart's decision, not this module's.** A chart
//! that merges its operator-facing configuration tree *over* whatever it derives can be probed
//! through that tree. One that merges the derived wiring over it cannot: a probe written into the
//! root tree is overwritten before it reaches the file, and the case fails for a reason that is not
//! a defect. The root is therefore per document and stated in the chart's enrolment rather than
//! guessed at, and a document's baseline is read under the same root, because the check that stops a
//! baseline from supplying the very value a case is probing is a comparison of two paths as strings.
//!
//! **A render prerequisite may never be written into the tree the probes are written to.** A chart
//! that refuses its own default render needs values supplied before any of its cases renders at
//! all, the case that only checks the document's identity included. Those are the chart's own
//! first-class values, and a prerequisite is therefore carried by every case without exception
//! rather than dropped on a collision the way a baseline is. That is exactly why the collision has
//! to be made unreachable instead: a prerequisite writing under the probe root would sit in the
//! same tree every case probes, free to supply the very value a case exists to prove the chart
//! delivered. The rule lives here, in the model, and not only in the loader that reads the
//! enrolment: a caller reaching past the loader would otherwise reach past the rule with it.
//!
//! **A map the contract requires to hold entries is supplied by the enrolment, or nothing is
//! generated.** A `structured` key still carries no probe. But a map a producer refined
//! (`required: true`, with the entries its image refuses to start without inside its `constraint`)
//! is not a key a suite may leave out: every case renders the chart, and a render lacking those
//! entries is a document the image rejects, so each case would be proving a round trip through a
//! configuration that cannot boot. Generation therefore fails, naming the path and the entries,
//! unless the chart's own values or the enrolment's `baseline` supply every one of them under the
//! probe root. A placeholder was the alternative and is refused on purpose: written into a file
//! nobody may edit, it would either fail the document gate on every case or, shaped to pass it, be
//! a fabricated value presented as a fixture. The baseline already carries a mandatory reason,
//! which is where an operator states what those entries hold and why.
//!
//! # Two limits, stated rather than papered over
//!
//! The probe is chosen to differ from the *contract's* default, which is the image's compiled-in
//! value; a chart is free to render some other value when the key is unset, and if that value
//! happens to equal the probe the case passes without proving anything. For text the probe is a
//! token nothing else would produce, so the overlap is unreachable in practice; for a boolean there
//! are only two values and the overlap is real. Closing it would mean rendering the chart, and the
//! staleness gate would stop being a pure comparison of committed bytes.
//!
//! Refusing the probe root makes a prerequisite unable to reach a probed key *directly*; a
//! first-class chart value the chart derives a probed key from is a longer path to the same place,
//! and no mapping from one to the other exists in the contract to consult. What closes it is what
//! the probe root is chosen to be (the layer that wins), so the probe outranks whatever a
//! prerequisite set. That is a property of the chart rather than of the generator, and the
//! enrolment's mandatory reason is where a chart says so.

mod pattern;
mod probe;
mod render;

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::error::Error;
use crate::union::Merged;

pub use self::pattern::{
    document_pattern, escape, jsonpath_string, selector_path, toml_key, toml_scalar,
};
pub use self::probe::{Probe, probe_for, satisfying};
pub use self::render::{Target, render_suite};

#[cfg(test)]
use self::probe::DISTINCTIVE_INTEGER;

/// The values key a chart exposes for the operator's own configuration tree, merged over whatever
/// the chart derives from its first-class values.
///
/// It is what makes a chart-agnostic generator possible: every contract key is reachable as
/// `config.<path>` whether or not the chart also spells it as a camelCase value of its own. A chart
/// without one cannot be probed this way and is reported rather than guessed at.
///
/// The default rather than the rule: a chart whose derived wiring outranks this tree names a
/// higher-precedence one per document in its enrolment.
pub const VALUES_ROOT: &str = "config";

/// A render prerequisite naming nothing.
pub const PREREQUISITE_EMPTY: &str = "is empty, so it names no value at all";

/// A render prerequisite inside the tree every case probes.
#[must_use]
pub fn prerequisite_inside_probes(root: &str) -> String {
    format!(
        "writes under `{root}`, which is the tree every case probes: a prerequisite there could \
         supply the value a case exists to prove the chart delivered. A render prerequisite states \
         the chart's own values, and nothing under `{root}` is one"
    )
}

/// A render prerequisite enclosing the tree every case probes.
#[must_use]
pub fn prerequisite_around_probes(root: &str) -> String {
    format!(
        "encloses `{root}`, the tree every case probes: the prerequisite and the probe would be \
         two entries of one `set` mapping with the second nested inside the first, and a `set` \
         mapping has no order, so which of them survives is stated nowhere"
    )
}

/// One probe, as the case that proves the chart delivers it.
///
/// `through` is the chart value the probe was written into, and [`None`] when it went into the raw
/// configuration tree instead. That is the difference between a case that proves the chart's own
/// mapping and one that proves only that the escape hatch is merged into the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The contract key it probes.
    pub path: String,
    /// The `set` mapping, in the order it is written.
    pub set_values: Vec<(String, Json)>,
    /// The assertion.
    pub pattern: String,
    /// The chart value the probe went into, when it went into one.
    pub through: Option<String>,
}

/// One contract key's own chart value: where a probe for it is better written.
///
/// Written from the `projection` marker the value carries, which is the single statement in a chart
/// repository of which key a chart value feeds. Without one a probe goes into the raw configuration
/// tree every chart exposes, and what it proves is that the tree is merged into the document: true,
/// worth asserting once, and silent about the chart's own mapping. A typo in the helper that spells
/// one name from the other passes every case in the suite.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// The chart value.
    pub values_path: String,
    /// The marker's `when` clause: a values path the chart tests before writing the key at all,
    /// which a case has to switch on or the probe is dropped before it reaches the document and the
    /// assertion fails for the wrong reason.
    pub condition: Option<String>,
    /// The chart's own `@schema` block for the value, which the probe has to satisfy as well.
    pub schema: Option<Json>,
}

/// The one chart value a template writes into a contract key unchanged.
///
/// Written from a `projection` or `structured` marker. Both hand the value to the key as it is, so
/// whatever a render holds at this path (a prerequisite, or the chart's own default) is what the
/// key holds in the document. A `composed` marker is not a carrier: the template builds the key's
/// value from several inputs, and no one of them is the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carrier {
    /// The chart value.
    pub values_path: String,
    /// The marker's `when` clause: a values path the chart tests before writing the key at all, so
    /// the value reaches the document only while the render has it switched on.
    pub condition: Option<String>,
}

/// One key carrying no case, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The contract key.
    pub path: String,
    /// The reason, written into the generated file verbatim.
    pub reason: String,
}

/// Everything a suite is rendered from, sorted by contract key path.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Plan {
    /// The cases.
    pub cases: Vec<Case>,
    /// The keys carrying none.
    pub skipped: Vec<Skipped>,
}

/// Where an operator writes one contract key, as a `set` path.
#[must_use]
pub fn values_path(path: &str, root: &str) -> String {
    format!("{root}.{path}")
}

/// Why one render prerequisite may not be written, or [`None`] when it may be.
///
/// The whole of the guarantee, in two comparisons of one string, so that a reader checks it by eye
/// rather than by reasoning about how two trees merge. A caller that reports the conflict wraps this
/// in whatever names the file it came from; [`plan`] refuses on it, because a suite built past this
/// rule is a suite that proves less than it says.
///
/// `root` is the values path the probes are written under, which the enrolment may move. Comparing
/// against the root rather than against the default is the whole point: the rule exists to keep a
/// prerequisite out of the tree the cases probe. Both directions are refused, because either nesting
/// puts two entries of one unordered `set` mapping inside each other.
#[must_use]
pub fn prerequisite_conflict(path: &str, root: &str) -> Option<String> {
    if path.is_empty() {
        return Some(PREREQUISITE_EMPTY.to_owned());
    }
    if path == root || path.starts_with(&format!("{root}.")) {
        return Some(prerequisite_inside_probes(root));
    }
    if root.starts_with(&format!("{path}.")) {
        return Some(prerequisite_around_probes(root));
    }
    None
}

/// Whether a render prerequisite already writes at, under or around one values path.
///
/// All three collide in a `set` mapping: the same path twice is a duplicate key, and either nesting
/// puts two entries of one unordered mapping inside each other.
fn occupied(values_path: &str, prerequisites: &[(String, Json)]) -> bool {
    prerequisites.iter().any(|(name, _)| {
        name == values_path
            || values_path.starts_with(&format!("{name}."))
            || name.starts_with(&format!("{values_path}."))
    })
}

/// What a render holds at one values path, given flat `set` entries layered over the chart's values.
///
/// A `set` entry at the path, or at a map around it, wins over the chart's own values; one nested
/// beneath the path means the render holds a map there with at least that entry in it.
fn rendered(path: &str, set: &[(String, Json)], values: &Json) -> Rendered {
    for (name, value) in set {
        if name == path {
            return Rendered::from(Some(value));
        }
        if let Some(rest) = path.strip_prefix(&format!("{name}.")) {
            return Rendered::from(super::dig(value, rest));
        }
    }
    if set
        .iter()
        .any(|(name, _)| name.starts_with(&format!("{path}.")))
    {
        return Rendered::Filled;
    }
    Rendered::from(super::dig(values, path))
}

/// How much [`rendered`] found, in the terms the two callers ask about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rendered {
    /// Nothing, or an explicit null.
    Absent,
    /// A value Helm's `if` treats as off: `false`, zero, an empty string, map or list.
    Empty,
    /// Anything else.
    Filled,
}

impl From<Option<&Json>> for Rendered {
    fn from(value: Option<&Json>) -> Self {
        match value {
            None | Some(Json::Null) => Self::Absent,
            Some(Json::Bool(false)) => Self::Empty,
            Some(Json::Number(number)) if number.as_f64() == Some(0.0) => Self::Empty,
            Some(Json::String(text)) if text.is_empty() => Self::Empty,
            Some(Json::Array(items)) if items.is_empty() => Self::Empty,
            Some(Json::Object(fields)) if fields.is_empty() => Self::Empty,
            Some(_) => Self::Filled,
        }
    }
}

/// Refuses a suite in which a map the contract requires to hold entries is supplied by nothing.
///
/// An entry is supplied in either of two places, each of which every case renders:
///
/// - at `<root>.<path>.<entry>`, by the enrolment's `baseline` or the chart's own `values.yaml`;
/// - at `<carrier>.<entry>`, by the render `prerequisites` or `values.yaml`, where `carriers` names
///   the chart value the template writes into the key unchanged. A carrier behind a `when` clause
///   counts only while the same two sources switch that clause on, because otherwise the chart never
///   writes the key.
///
/// The second is what lets a chart whose own schema already demands the entries state them once, as
/// the chart value an operator writes, rather than a second time under the escape hatch.
///
/// Every unsupplied entry of every such key is named at once, so one run says the whole of what the
/// enrolment has to state. A key whose default already holds its entries is published
/// `required: false` and is skipped: the image supplies it when the chart does not.
///
/// # Errors
/// [`Error::Invalid`] naming each key and its unsupplied entries.
pub fn unsupplied_entries(
    keys: &[&Merged],
    baseline: &[(String, Json)],
    prerequisites: &[(String, Json)],
    values: &Json,
    root: &str,
    carriers: &BTreeMap<String, Carrier>,
) -> Result<(), Error> {
    let mut gaps = Vec::new();
    for key in keys {
        if key.fields.get("required").and_then(Json::as_bool) != Some(true) {
            continue;
        }
        let Some(path) = key.text("path") else {
            continue;
        };
        let at = values_path(path, root);
        let carrier = carriers.get(path).filter(|carrier| {
            carrier
                .condition
                .as_deref()
                .is_none_or(|gate| rendered(gate, prerequisites, values) == Rendered::Filled)
        });
        let missing: Vec<&str> = crate::document::required_entries(key.fields.get("constraint"))
            .into_iter()
            .filter(|entry| {
                let in_tree = rendered(&format!("{at}.{entry}"), baseline, values);
                let delivered = carrier.map_or(Rendered::Absent, |held| {
                    rendered(
                        &format!("{}.{entry}", held.values_path),
                        prerequisites,
                        values,
                    )
                });
                in_tree == Rendered::Absent && delivered == Rendered::Absent
            })
            .collect();
        if !missing.is_empty() {
            let chart_value = carriers.get(path).map_or_else(String::new, |carrier| {
                format!(
                    ", or under `{}` in its `prerequisites.values`",
                    carrier.values_path
                )
            });
            gaps.push(format!(
                "operator must supply fixture for {path}: must contain {} (unsupplied: {}). The \
                 contract marks the key required and its image refuses to start without those \
                 entries, so every case would render a document that cannot boot. State them \
                 under `{at}` in the enrolment's `baseline`, one leaf per line{chart_value}, or in \
                 the chart's values",
                crate::document::required_entries(key.fields.get("constraint")).join(", "),
                missing.join(", ")
            ));
        }
    }
    if gaps.is_empty() {
        Ok(())
    } else {
        Err(Error::Invalid(gaps.join("\n")))
    }
}

/// Turns a union's keys into the cases and the skips of one suite.
///
/// `baseline` is carried by every case except the one probing the key it sets. A chart may refuse to
/// render a values combination the contract considers perfectly legal, and a probe that walked into
/// such a pair would fail for a reason that has nothing to do with the round trip. Dropping the
/// entry that collides with the probe is what stops the baseline from supplying the very value the
/// case is meant to prove the chart delivered.
///
/// `prerequisites` is carried by every case with no exception at all, because it is what makes the
/// chart render in the first place. So the collision the baseline is protected from by being
/// dropped is instead made unreachable: a prerequisite path inside the probes' own tree is refused
/// here rather than accommodated. The two fields are different things and this is the line between
/// them.
///
/// `routes` moves a probe off the raw tree and onto the chart's own value for the key. The baseline
/// is still dropped by the key's escape-hatch path rather than by the route's, and deliberately: a
/// baseline entry naming the key a case probes would override the chart value it was just written
/// into, since the raw tree is merged over what the chart derives.
///
/// # Errors
/// [`Error::Invalid`] when a render prerequisite writes into the tree the cases probe, or when a
/// contract key cannot be read at all.
pub fn plan(
    keys: &[&Merged],
    baseline: &[(String, Json)],
    prerequisites: &[(String, Json)],
    root: &str,
    routes: &BTreeMap<String, Route>,
) -> Result<Plan, Error> {
    for (name, _) in prerequisites {
        if let Some(conflict) = prerequisite_conflict(name, root) {
            return Err(Error::Invalid(format!(
                "the render prerequisite {} {conflict}",
                crate::gate::quoted(name)
            )));
        }
    }

    let mut ordered: Vec<&&Merged> = keys.iter().collect();
    ordered.sort_by_key(|key| key.text("path").unwrap_or(""));

    let mut plan = Plan::default();
    for key in ordered {
        let path = key.text("path").unwrap_or("").to_owned();
        let mut route = routes.get(&path);

        // A render prerequisite and a routed probe on one values path is one `set` mapping with the
        // same key twice, which the framework refuses outright, and the prerequisite is the one
        // that cannot move, since without it the chart does not render at all. The probe goes into
        // the raw tree instead, where it still outranks what the chart derives.
        if route.is_some_and(|route| occupied(&route.values_path, prerequisites)) {
            route = None;
        }

        // The chart's own value first, and its own schema with it. A key whose chart value is typed
        // more tightly than the contract can leave nothing that satisfies both, and the answer there
        // is the escape hatch rather than no case at all: a probe that proves only the merge is
        // worth more than a hole in the coverage.
        let mut probe = None;
        if let Some(held) = route {
            match probe_for(key, held.schema.as_ref())? {
                Ok(found) => probe = Some(found),
                Err(_) => route = None,
            }
        }
        if probe.is_none() {
            match probe_for(key, None)? {
                Ok(found) => probe = Some(found),
                Err(reason) => {
                    plan.skipped.push(Skipped { path, reason });
                    continue;
                }
            }
        }
        // Set on every path that did not `continue` above, and a `let ... else` here would
        // need a branch that cannot happen to put something in.
        let Some(probe) = probe else { continue };

        let hatch = values_path(&path, root);
        let mut set_values: Vec<(String, Json)> = prerequisites.to_vec();
        set_values.extend(baseline.iter().filter(|(name, _)| name != &hatch).cloned());
        if let Some(route) = route {
            if let Some(condition) = &route.condition {
                set_values.push((condition.clone(), Json::Bool(true)));
            }
            set_values.push((route.values_path.clone(), probe.value.clone()));
        } else {
            set_values.push((hatch, probe.value.clone()));
        }

        plan.cases.push(Case {
            pattern: document_pattern(&path, &probe.text)?,
            path,
            set_values,
            through: route.map(|route| route.values_path.clone()),
        });
    }
    Ok(plan)
}

#[cfg(test)]
mod tests;
