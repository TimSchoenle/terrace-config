//! Resolving one value against the contracts that bind it, and deciding which generated block it
//! gets.

use std::collections::BTreeMap;

use crate::union::Merged;

use crate::helm::declaration::Bound;
use crate::helm::markers::{Block, MARKER, Marker};

use super::expected::{describes_element, describing};
use super::{Publisher, Resolved, Shape, derives, ordinal};

/// The one binding a derived block can come from, or [`None`] with the refusal reported.
///
/// A value feeding several keys has no single constraint to be derived from, and a value feeding
/// none has nothing to derive from at all — so both are refusals rather than a choice.
pub(super) fn one_binding<'a>(
    shape: &Shape,
    markers: &'a [Marker],
    refuse: &mut impl FnMut(String),
) -> Option<&'a Marker> {
    let derived: Vec<&Marker> = markers
        .iter()
        .filter(|marker| derives(marker.class))
        .collect();
    if derived.is_empty() {
        refuse(format!(
            "{}: '{}' carries no `{MARKER}` marker naming the contract key it feeds, so there is              nothing to derive its schema from",
            shape.at(),
            shape.values_path
        ));
        return None;
    }
    if derived.len() > 1 {
        refuse(format!(
            "{}: '{}' binds {}, and a value feeding several keys has no single constraint to be              derived from",
            shape.at(),
            shape.values_path,
            derived
                .iter()
                .map(|marker| crate::gate::quoted(&marker.target))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return None;
    }
    Some(derived[0])
}

/// The contract key one value feeds, resolved against every document that carries it.
///
/// `Ok(None)` when the value cannot be resolved and the caller asked not to be told — a hand
/// transcription in a chart with no contract at all is the ordinary state of one, not a defect.
pub fn resolve(
    bound: Option<&Bound>,
    shape: &Shape,
    markers: &[Marker],
    problems: &mut Vec<String>,
    report: bool,
) -> Option<Resolved> {
    let mut refuse = |message: String| {
        if report {
            problems.push(message);
        }
    };

    let marker = one_binding(shape, markers, &mut refuse)?;
    let Some(bound) = bound else {
        refuse(format!(
            "{}: '{}' binds '{}', and this chart has no {} naming the document that declares it",
            shape.at(),
            shape.values_path,
            marker.target,
            crate::helm::DECLARATION
        ));
        return None;
    };

    let candidates: Vec<String> = match &marker.documents {
        Some(scope) => scope.clone(),
        None => bound.documents.keys().cloned().collect(),
    };
    // Kept paired with the document that carried it rather than flattened: which images publish the
    // key is what decides the release a hand transcription is held against, and for a multi-service
    // chart that is a strict subset of the documents it declares.
    let carried: Vec<(&String, &Merged)> = candidates
        .iter()
        .filter_map(|name| {
            bound
                .namespace(name, marker.class.targets_a_key())
                .and_then(|namespace| namespace.get(&marker.target))
                .map(|entry| (name, entry))
        })
        .collect();
    if carried.is_empty() {
        // The bindings gate reports this one properly, with the suggestion and the scope
        // diagnostic. Repeating that here would print the same defect twice.
        refuse(format!(
            "{}: '{}' binds '{}', which no contract this chart declares carries — see the \
             bindings gate",
            shape.at(),
            shape.values_path,
            marker.target
        ));
        return None;
    }

    let first = carried[0].1.fields.get("constraint");
    if carried
        .iter()
        .any(|(_, entry)| entry.fields.get("constraint") != first)
    {
        refuse(format!(
            "{}: the documents this chart declares describe '{}' differently, so no one schema is \
             derivable from them",
            shape.at(),
            marker.target
        ));
        return None;
    }

    let structured = carried
        .iter()
        .all(|(_, entry)| entry.text("text_form") == Some("structured"));
    let names: std::collections::BTreeSet<&str> = carried
        .iter()
        .map(|(_, entry)| entry.text("ty").unwrap_or(""))
        .collect();
    let publishers: Vec<Publisher> = carried
        .iter()
        .flat_map(|(name, _)| {
            bound
                .releases
                .get(*name)
                .into_iter()
                .flatten()
                .map(move |version| Publisher {
                    document: (*name).clone(),
                    version: version.clone(),
                })
        })
        .collect();

    Some(Resolved {
        constraint: first.filter(|held| !held.is_null()).cloned(),
        optional: marker.optional,
        structured,
        ty: if names.len() == 1 {
            names.into_iter().next().unwrap_or("").to_owned()
        } else {
            String::new()
        },
        publishers,
    })
}

/// Whether a block read at one release still describes an image published at another.
///
/// True when the two are the same release, and when the first is the later: a copy read at 8.9.1
/// covers a service still at 8.9.0, at worst by narrowing it, and demanding a re-read there would
/// fail a chart the moment its images stopped moving in lockstep.
///
/// Ordering is consulted only when both parse as a release. A tag nobody writes degrades to nothing,
/// and two of those would compare equal and pass; such a pair is held to equality instead, which
/// errs towards asking for a re-read rather than towards a stale copy that looks maintained.
pub fn covers(read_at: &str, published: &str) -> bool {
    if crate::helm::release(read_at) == crate::helm::release(published) {
        return true;
    }
    let (mine, theirs) = (ordinal(read_at), ordinal(published));
    !mine.is_empty() && !theirs.is_empty() && mine >= theirs
}

/// Whether a hand-transcribed shape is one the contract now publishes itself.
///
/// Two ways for that to become true, and they are the two reasons a transcription is allowed in the
/// first place: a container whose element the producer had not described, and a key it published no
/// constraint for at all. Either one arriving retires the hand copy.
pub fn superseded(resolved: &Resolved) -> bool {
    if resolved.structured {
        describes_element(resolved.constraint.as_ref())
    } else {
        describing(resolved.constraint.as_ref())
    }
}

/// Every generated shape a chart has, from its markers and its blocks.
///
/// Which is every bound value that keeps no transcription marker. Enrolment used to be per value
/// and opt-in, and the default is now the other way round, so a key an image retypes moves the chart
/// on the next run whether or not anybody remembered to enrol the value.
pub fn generated(
    chart: &str,
    markers: &BTreeMap<String, Vec<Marker>>,
    blocks: &BTreeMap<String, Block>,
    transcribed: &[Shape],
) -> Vec<Shape> {
    let declared: Vec<&str> = transcribed
        .iter()
        .map(|shape| shape.values_path.as_str())
        .collect();

    let mut shapes: Vec<Shape> = markers
        .iter()
        .filter(|(values_path, held)| {
            !declared.contains(&values_path.as_str())
                && held.iter().any(|marker| derives(marker.class))
        })
        .map(|(values_path, held)| Shape {
            chart: chart.to_owned(),
            line: blocks
                .get(values_path)
                .map_or_else(|| held[0].line, |block| block.start),
            values_path: values_path.clone(),
            handwritten: false,
            version: None,
            source: None,
            declared: false,
        })
        .collect();
    shapes.sort_by_key(|shape| shape.line);
    shapes
}
