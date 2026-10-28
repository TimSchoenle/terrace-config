//! Applying the divergences a chart declares with `@config-shape-except` and `@config-shape-
//! narrow`.

use serde_json::Value as Json;

use super::{Departure, Divergence, EXCEPT_MARKER, NARROW_MARKER};

/// The generated schema with each declared position taken from the chart's own block.
///
/// Returns the result and the problems found, rather than stopping at the first: a value with two
/// stale departures should report both.
///
/// A sub-path always has to resolve in the chart's own block, since that is where the kept text
/// comes from. Whether it resolves in the *generated* schema is what the two markers disagree about,
/// and holding each to its own answer is what makes a departure age visibly:
///
/// | Marker | In the generated shape | What it says |
/// |---|---|---|
/// | `@config-shape-except` | present | the contract describes this, and the chart overrides it |
/// | `@config-shape-narrow` | absent | the contract describes nothing here, and the chart adds a bound |
///
/// Written the wrong way round, each is refused with the other named. That is not pedantry: a
/// narrowing whose position the contract *starts* describing is exactly the moment somebody has to
/// decide whether the producer's own bound supersedes the chart's, and an override whose position
/// the contract *stops* describing is a keyword nothing upstream stands behind any more. Both are
/// silent under a single marker that accepts either.
pub fn apply_divergences(
    generated: &Json,
    present: &Json,
    divergences: &[&Divergence],
) -> (Json, Added, Vec<String>) {
    let mut result = generated.clone();
    let mut added = Added::default();
    let mut problems = Vec::new();

    for divergence in divergences {
        let parts: Vec<&str> = divergence.sub_path.split('.').collect();
        let Some(kept) = dig(present, &parts).cloned() else {
            problems.push(format!(
                "{}: keeps '{}' of '{}', which the block below does not declare — there is \
                 nothing to keep",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        };

        let described = dig(generated, &parts).is_some();
        if divergence.kind == Departure::Override && !described {
            problems.push(format!(
                "{}: overrides '{}' of '{}', which the generated schema does not contain. Either \
                 the contract no longer describes it — in which case this is a narrowing and the \
                 marker is `{NARROW_MARKER}` — or the sub-path is a typo",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }
        if divergence.kind == Departure::Narrowing && described {
            problems.push(format!(
                "{}: narrows '{}' of '{}', which the contract now describes itself. Decide \
                 between them: `{EXCEPT_MARKER}` keeps the chart's, and deleting the marker takes \
                 the image's",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }
        if !described && dig(&result, &parts[..parts.len() - 1]).is_none() {
            problems.push(format!(
                "{}: narrows '{}' of '{}', whose enclosing position the generated schema does not \
                 contain either, so there is nowhere to put it",
                divergence.at(),
                divergence.sub_path,
                divergence.values_path
            ));
            continue;
        }

        if !described {
            added.record(&parts);
        }
        put(&mut result, &parts, kept);
    }

    (settle(result), added, problems)
}

/// The struct fields a narrowing put into the generated schema, by the position they sit at.
///
/// A document's member order is a fact the reader here cannot keep (every object is read through a
/// sorted map, which is what makes a re-rendered document byte-identical to the one a producer
/// wrote), so a generated block spells the producer's fields in the order the document gives them
/// back. A field the *chart* adds has no such order to recover, and appending it is the only answer
/// that is stable: sorting it in would move it the first time somebody renamed it, and every value
/// in the chart would come out with a different diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    positions: Vec<String>,
}

impl Added {
    /// Remember one position, if it names a struct field rather than a keyword.
    pub(super) fn record(&mut self, parts: &[&str]) {
        if parts.len() >= 2 && parts[parts.len() - 2] == "properties" {
            self.positions.push(parts.join("."));
        }
    }

    /// Whether the field at this position was added rather than published.
    pub(super) fn holds(&self, at: &str, field: &str) -> bool {
        let full = if at.is_empty() {
            format!("properties.{field}")
        } else {
            format!("{at}.properties.{field}")
        };
        self.positions.contains(&full)
    }
}

/// One assembled block, with the pair a schema generator refuses resolved in the enum's favour.
///
/// [`expected`] never emits `enum` beside `type` at the top level, because a generator exits
/// fatally on it and leaves every chart without a schema. A narrowing can reintroduce the pair (a
/// chart that enumerates the members of a key the contract only types as a string is the ordinary
/// case), so the same rule is applied after the departures rather than only before. The enum wins
/// because it is the stricter of the two and because it is the half somebody wrote down
/// deliberately.
pub(super) fn settle(schema: Json) -> Json {
    let Json::Object(mut fields) = schema else {
        return schema;
    };
    if fields.contains_key("enum") && fields.contains_key("type") {
        fields.remove("type");
    }
    Json::Object(fields)
}

/// One position inside a schema, by a dotted walk of keywords.
pub(super) fn dig<'a>(schema: &'a Json, parts: &[&str]) -> Option<&'a Json> {
    let mut current = schema;
    for part in parts {
        current = current.as_object()?.get(*part)?;
    }
    Some(current)
}

/// Writes one position inside a schema, whose enclosing position the caller has established.
pub(super) fn put(schema: &mut Json, parts: &[&str], value: Json) {
    let mut current = schema;
    for part in &parts[..parts.len() - 1] {
        let Some(next) = current.as_object_mut().and_then(|held| held.get_mut(*part)) else {
            return;
        };
        current = next;
    }
    if let Some(held) = current.as_object_mut() {
        held.insert(
            (*parts.last().expect("a sub-path has a last part")).to_owned(),
            value,
        );
    }
}
