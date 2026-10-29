//! Where a new block goes in values.yaml: the parent it belongs under, and the line it is inserted
//! after.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value as Json;

use crate::error::Error;
use crate::text::quoted;

use crate::helm::bindings::has_path;
use crate::helm::markers::{MARKER, Marker};
use crate::helm::regions::Region;
use crate::helm::scaffold::{
    Destination, Placement, Plan, Tree, branch_prefix, branches, render_tree, tree_at,
};

use super::{Insertion, Refusal};

/// The values path one level up, or the empty path for a top-level value.
pub(super) fn parent_of(values_path: &str) -> &str {
    values_path
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent)
}

/// The values paths whose subtree binds at least one contract key.
///
/// A block bears a binding if it or anything under it carries a marker, which is what makes
/// `telemetry` part of the configuration surface when the markers are three levels down.
pub(super) fn bearing(regions: &[Region], markers: &[Marker]) -> BTreeSet<String> {
    regions
        .iter()
        .filter(|region| {
            let nested = format!("{}.", region.values_path);
            markers.iter().any(|marker| {
                marker.values_path == region.values_path || marker.values_path.starts_with(&nested)
            })
        })
        .map(|region| region.values_path.clone())
        .collect()
}

/// The line a new child of `ancestor` goes after.
///
/// After the last child that binds something, so the new value lands at the end of the
/// configuration surface rather than at the end of the file; after the ancestor's own last line
/// when none of its children binds anything; and at the end of the file for a top-level block in a
/// chart none of whose top-level blocks binds anything.
pub(super) fn insertion_point(
    regions: &[Region],
    bound_paths: &BTreeSet<String>,
    ancestor: &str,
    lines: usize,
) -> usize {
    let binding = regions
        .iter()
        .filter(|region| {
            parent_of(&region.values_path) == ancestor && bound_paths.contains(&region.values_path)
        })
        .map(|region| region.end)
        .max();
    if let Some(end) = binding {
        return end;
    }
    if ancestor.is_empty() {
        return lines;
    }
    regions
        .iter()
        .find(|region| region.values_path == ancestor)
        .map_or(lines, |region| region.end)
}

/// The blocks to insert, and the keys no block can be written for.
///
/// Placements are grouped by the existing block they land in, so two keys arriving under one new
/// `telemetry.sentry` produce one `sentry:` heading rather than two.
pub(super) fn insertions_for(
    values: &Json,
    regions: &[Region],
    markers: &[Marker],
    plan: &Plan,
    lines: usize,
) -> Result<(Vec<Insertion>, Vec<Refusal>), Error> {
    let placed: BTreeSet<&str> = regions
        .iter()
        .map(|region| region.values_path.as_str())
        .collect();
    // A key written twice is read as its last occurrence, which is what YAML makes of it.
    let by_path: HashMap<&str, &Region> = regions
        .iter()
        .map(|region| (region.values_path.as_str(), region))
        .collect();
    let bound_paths = bearing(regions, markers);

    let mut refusals = Vec::new();
    let mut groups: Vec<(String, Vec<Placement>)> = Vec::new();

    for placement in plan.surfaced() {
        let values_path = placement.values_path.as_str();
        if placed.contains(values_path) || has_path(values, values_path) {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: occupied_refusal(placement),
                occupied: true,
            });
            continue;
        }

        let segments: Vec<&str> = values_path.split('.').collect();
        let ancestor = (1..segments.len())
            .rev()
            .map(|cut| segments[..cut].join("."))
            .find(|candidate| placed.contains(candidate.as_str()))
            .unwrap_or_default();

        // A mapping key written with nothing under it — `feed:` whose only child was removed — is
        // a parent to insert into rather than a leaf to refuse: `null` there is the absence of
        // children, and every other scalar is a value that cannot also be a mapping.
        let occupant = crate::helm::dig(values, &ancestor).filter(|held| !held.is_null());
        if !ancestor.is_empty() && occupant.is_some_and(|held| !held.is_object()) {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: format!(
                    "needs the chart value {}, and {} is a leaf value on the way there. The two \
                     cannot both exist, so this mapping has to be written by hand",
                    quoted(values_path),
                    quoted(&ancestor)
                ),
                occupied: false,
            });
            continue;
        }

        // `auth: {}` and `telemetry: null` are mappings to YAML, but written on one line: a block
        // inserted under either is a second value for the same key, and the file no longer parses.
        // Turning one into a bare `auth:` is a rewrite of a line somebody wrote, which an
        // insertion-only writer does not do.
        if let Some(region) = by_path.get(ancestor.as_str())
            && !ancestor.is_empty()
            && !region.bare
        {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: format!(
                    "needs the chart value {}, and {} is written with its value on the key's own \
                     line, so nothing can be inserted under it. Rewrite it as a bare `{}:` with \
                     its children indented below, then run this again",
                    quoted(values_path),
                    quoted(&ancestor),
                    ancestor.rsplit('.').next().unwrap_or_default()
                ),
                occupied: false,
            });
            continue;
        }

        match groups.iter_mut().find(|(held, _)| *held == ancestor) {
            Some((_, members)) => members.push(placement.clone()),
            None => groups.push((ancestor, vec![placement.clone()])),
        }
    }

    let mut insertions = Vec::new();
    for (ancestor, placements) in groups {
        let (depth, indent, ancestor_line) = match by_path.get(ancestor.as_str()) {
            // Read off the block being inserted into rather than computed from the depth, so a
            // file indented by something other than two spaces is extended in its own style.
            Some(region) if !ancestor.is_empty() => (
                ancestor.split('.').count(),
                " ".repeat(region.indent + 2),
                Some(region.line),
            ),
            _ => (0, String::new(), None),
        };
        let tree = tree_at(&placements, depth)?;
        let after = insertion_point(regions, &bound_paths, &ancestor, lines);
        let mut paths: Vec<String> = placements
            .iter()
            .map(|item| item.path().to_owned())
            .collect();
        paths.sort();
        insertions.push(Insertion {
            after,
            lines: render_tree(&tree, &indent, depth, &branch_description)?,
            paths,
            branches: branches(&tree, &ancestor),
            spaced: ancestor_line.is_none_or(|line| line != after),
            depth,
        });
    }

    // Listed by the line each goes after and otherwise in the order planned, which is the order
    // the report has always printed them in. Where two of them meet at one line is the writer's
    // concern, not the listing's: see [`splices`].
    insertions.sort_by_key(|item| item.after);
    Ok((insertions, refusals))
}

/// Why a key whose chart value already exists is refused, and what to write instead.
///
/// The one refusal that is a rule rather than an obstacle — see this module's documentation. A
/// credential gets a different sentence because it never carries a marker: what an existing value
/// is owed there is the write-off and the Secret line, not a binding.
pub(super) fn occupied_refusal(placement: &Placement) -> String {
    let values_path = quoted(&placement.values_path);
    if placement.destination == Destination::SecretFile {
        return format!(
            "needs the chart value {values_path}, which this chart already has. A credential \
             carries no marker, so what is owed is an `unbound` entry naming this key and a \
             `secretData` line delivering that value"
        );
    }
    let optional = if placement.optional() {
        " optional"
    } else {
        ""
    };
    format!(
        "needs the chart value {values_path}, which this chart already has. Whether that value \
         feeds this key is a claim only a person can make, so write `# # {MARKER} {} {}{optional}` \
         into its `@schema` block by hand",
        placement.marker_class().label(),
        placement.path()
    )
}

/// The `# --` line a grouping block created here is given, and why it is not a `TODO`.
///
/// A contract says nothing about the blocks a chart groups its keys into, so the sentence somebody
/// would write cannot be derived. A scaffold's answer is a `TODO` an author reads while writing the
/// chart; here that answer was measured, twice, on one pull request: the placeholder was deleted
/// rather than filled in, the values-documentation gate went red on a block with no description at
/// all, and in between a `TODO` was published into the README table and into the values schema.
///
/// So this writes the plainest true sentence instead — the same fallback a key whose producer wrote
/// no prose gets. Nothing is left to delete, the README row is dull rather than wrong, and the
/// closing output still names every block worth a better sentence.
pub(super) fn branch_description(node: &Tree, depth: usize) -> Result<String, Error> {
    Ok(format!("The `{}` settings.", branch_prefix(node, depth)?))
}
