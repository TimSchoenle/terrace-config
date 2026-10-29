//! Choosing a probe: a value for one key that satisfies its constraint, differs from its default
//! and can be found again in the rendered document.

use serde_json::{Map, Value as Json};

use crate::document::TextForm;
use crate::error::Error;
use crate::union::Merged;
use crate::value::{assert_value, beyond_scalar_vocabulary};

use super::pattern::{non_alphanumeric, toml_scalar};

/// Text probes are built from this stem so a value appearing in a rendered document is unmistakably
/// a test fixture and never a plausible setting somebody meant.
pub(super) const PROBE_STEM: &str = "contract-probe";

/// The first integer probe tried.
///
/// A distinctive number rather than "the default plus one": a passing assertion should not be
/// explicable as a coincidence, and `1` is a value half of any chart repository renders somewhere by
/// accident. Anything the key's bounds refuse falls back to a search from the lower bound.
pub(super) const DISTINCTIVE_INTEGER: i64 = 4242;

/// How far the fallback search walks before giving up and skipping the key.
///
/// A bound this narrow is reached only by a constraint no probe fits at all, which is a skip either
/// way.
pub(super) const SEARCH_WIDTH: i64 = 16;

/// Why a `structured` key carries no probe.
pub(super) const STRUCTURED_REASON: &str = "`structured`: the keys beneath it are the operator's own names, so \
                                 the contract describes the container and not its contents and no \
                                 probe addresses a leaf";

/// Why an `unknown` key carries no probe.
pub(super) const UNKNOWN_REASON: &str = "`unknown`: the producer publishes no constraint for this key, so no \
                              probe can be known-valid";

/// Why a credential carries no probe.
pub(super) const SECRET_REASON: &str = "`secret: true`: the probe would be committed to this repository, and a \
                             credential-shaped value in a test file is a credential";

/// One value to write into a chart's values, and what it should render as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The value, as it is written into the `set` block.
    pub value: Json,
    /// The same value as the rendered document spells it.
    pub text: String,
}

/// The probe for one contract key, or the reason it carries none.
///
/// Exactly one of the two is ever set, and the reason is written into the generated file verbatim:
/// an unexplained absence is indistinguishable from an oversight, which for a file whose whole job
/// is to be exhaustive is the worst possible failure mode.
///
/// `also` is a second schema the candidate has to satisfy, and it is what makes a probe written
/// into the chart's *own* value safe. Such a value carries a `@schema` block, so the templating
/// engine validates what is set there, and that block may be stricter than the contract,
/// deliberately: a chart that types a port `minimum: 1` where the producer's `u16` allows 0 would
/// refuse a probe the contract accepts, and the failure would read as a broken chart rather than as
/// a probe nothing can satisfy. Passing the chart's own block here makes the two agree by
/// construction; the caller falls back to the untyped escape hatch when nothing satisfies both.
///
/// # Errors
/// [`Error::Invalid`] when a key has no `path` at all, which is a contract this build cannot read
/// rather than a key it cannot probe.
pub fn probe_for(key: &Merged, also: Option<&Json>) -> Result<Result<Probe, String>, Error> {
    use std::fmt::Write as _;

    let path = key.text("path").filter(|path| !path.is_empty());
    let Some(path) = path else {
        return Err(Error::Invalid("a contract key has no `path`".to_owned()));
    };

    if key.fields.get("secret").and_then(Json::as_bool) == Some(true) {
        return Ok(Err(SECRET_REASON.to_owned()));
    }

    let form = match key.entry().text_form() {
        Ok(TextForm::Structured) => return Ok(Err(STRUCTURED_REASON.to_owned())),
        Ok(TextForm::Unknown) => return Ok(Err(UNKNOWN_REASON.to_owned())),
        Ok(form) => form,
        Err(failure) => return Err(failure),
    };

    let constraint = schema_of(key, "constraint");
    let text_constraint = schema_of(key, "text_constraint");
    for (name, schema) in [
        ("constraint", &constraint),
        ("text_constraint", &text_constraint),
    ] {
        let outside = beyond_scalar_vocabulary(schema);
        if !outside.is_empty() {
            return Ok(Err(format!(
                "`{name}` carries {}, which is outside the vocabulary this generator can satisfy",
                outside.join(", ")
            )));
        }
    }

    let default = key.fields.get("default_value").unwrap_or(&Json::Null);
    for candidate in candidates(form, path, key, &constraint) {
        if same_value(&candidate, default) {
            continue;
        }
        if !satisfies(&constraint, &candidate) {
            continue;
        }
        if also.is_some_and(|also| !satisfies(also, &candidate)) {
            continue;
        }
        // The environment spelling of the same setting is what `text_constraint` governs, and a
        // probe the chart could deliver through the file but not through the environment would be a
        // value the deployment cannot actually carry both ways. Checked against the bare text rather
        // than the TOML literal: a variable holds `4242`, not `"4242"`.
        if !satisfies(
            &text_constraint,
            &Json::String(environment_text(&candidate)),
        ) {
            continue;
        }
        let text = toml_scalar(&candidate);
        return Ok(Ok(Probe {
            value: candidate,
            text,
        }));
    }

    let mut refused = format!("`constraint` {}", json_text(&constraint));
    if !is_empty_schema(&text_constraint) {
        let _ = write!(
            refused,
            " and `text_constraint` {}",
            json_text(&text_constraint)
        );
    }
    if let Some(also) = also.filter(|also| !is_empty_schema(also)) {
        let _ = write!(refused, " and the chart's own schema {}", json_text(also));
    }
    Ok(Err(format!(
        "no value this generator can synthesise satisfies {refused} while also differing from the \
         default {default}"
    )))
}

/// Any value one key's `constraint` accepts, or [`None`] when nothing here can synthesise one.
///
/// The candidate walk without [`probe_for`]'s two extra demands. A probe has to *differ* from the
/// published default (otherwise the case proves the chart delivered a value it would have got
/// anyway), and it has to survive the environment spelling as well. Neither applies to a caller
/// that simply needs a legal value: scaffolding writes one into a new chart's values for a required
/// key whose image publishes no default, where "differs from the default" is vacuous and the
/// environment layer is not involved at all.
///
/// Public rather than left to a caller reaching for the candidate walk: the walk knows about
/// `multipleOf`, exclusive bounds and the choice vocabulary, and a second implementation of it would
/// be wrong in exactly the places this one was fixed.
#[must_use]
pub fn satisfying(key: &Merged) -> Option<Json> {
    let constraint = schema_of(key, "constraint");
    if !beyond_scalar_vocabulary(&constraint).is_empty() {
        return None;
    }
    let form = match key.entry().text_form() {
        Ok(TextForm::Structured | TextForm::Unknown) | Err(_) => return None,
        Ok(form) => form,
    };
    candidates(form, key.text("path").unwrap_or(""), key, &constraint)
        .into_iter()
        .find(|candidate| satisfies(&constraint, candidate))
}

/// Every value worth trying for one key, in the order they are preferred.
pub(super) fn candidates(form: TextForm, path: &str, key: &Merged, constraint: &Json) -> Vec<Json> {
    match form {
        TextForm::Boolean => match key.fields.get("default_value").and_then(Json::as_bool) {
            Some(held) => vec![Json::Bool(!held)],
            None => vec![Json::Bool(true), Json::Bool(false)],
        },
        TextForm::Choice => key
            .fields
            .get("values")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default(),
        TextForm::Integer => integer_candidates(constraint),
        _ => text_candidates(path, constraint),
    }
}

/// The distinctive probe first, then a walk up from the constraint's lower bound.
pub(super) fn integer_candidates(constraint: &Json) -> Vec<Json> {
    let low = constraint
        .get("minimum")
        .and_then(Json::as_f64)
        .or_else(|| {
            constraint
                .get("exclusiveMinimum")
                .and_then(Json::as_f64)
                .map(|bound| bound + 1.0)
        })
        .unwrap_or(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a bound outside i64 is a constraint no probe fits, which is a skip either way"
    )]
    let start = low as i64;

    let mut found = vec![Json::from(DISTINCTIVE_INTEGER)];
    found.extend((0..SEARCH_WIDTH).map(|step| Json::from(start.saturating_add(step))));

    // A `multipleOf` refuses almost every value a linear walk produces, so the multiples are offered
    // as well rather than relying on the walk happening to land on one.
    if let Some(step) = constraint
        .get("multipleOf")
        .and_then(Json::as_i64)
        .filter(|step| *step > 0)
    {
        found.extend((1..=SEARCH_WIDTH).map(|factor| Json::from(step.saturating_mul(factor))));
    }
    found
}

/// A token naming the key it probes, adjusted to whatever length bounds the contract sets.
///
/// Naming the key in the value is what makes a failure readable: the rendered document shows which
/// setting went missing without a reader opening the contract.
pub(super) fn text_candidates(path: &str, constraint: &Json) -> Vec<Json> {
    let mut stem = format!("{PROBE_STEM}-{}", non_alphanumeric().replace_all(path, "-"));
    stem = stem.trim_matches('-').to_owned();

    let mut found = vec![Json::String(stem.clone())];
    if let Some(minimum) = constraint.get("minLength").and_then(Json::as_u64)
        && (stem.chars().count() as u64) < minimum
    {
        let mut padded = stem.clone();
        while (padded.chars().count() as u64) < minimum {
            padded.push('x');
        }
        found.push(Json::String(padded));
    }
    if let Some(maximum) = constraint.get("maxLength").and_then(Json::as_u64)
        && maximum > 0
        && maximum < stem.chars().count() as u64
    {
        found.push(Json::String(
            stem.chars()
                .take(usize::try_from(maximum).unwrap_or(0))
                .collect(),
        ));
    }
    found
}

/// The candidate as a variable would hold it, which is what `text_constraint` describes.
pub(super) fn environment_text(value: &Json) -> String {
    match value {
        Json::Bool(true) => "true".to_owned(),
        Json::Bool(false) => "false".to_owned(),
        Json::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Whether one candidate satisfies a flat constraint.
///
/// The shared validator rather than a second implementation of the vocabulary. It differs from a
/// generator-specific one in a single place: a numeric bound on a value that is not a number is
/// *skipped* rather than counted as a failure, because `type` is what says a value is the wrong
/// shape and reporting the same defect twice makes the first line harder to find. That difference
/// is unreachable from here: every candidate is of the key's own `text_form`, and the one
/// cross-shape check is a string against a `text_constraint`, which describes text.
pub(super) fn satisfies(constraint: &Json, candidate: &Json) -> bool {
    matches!(assert_value(constraint, candidate, ""), Ok(None))
}

/// Whether two values are the same value *and* the same shape.
///
/// `1` and `true` are equal in some languages and are two different probes here, so the comparison
/// is on the JSON value, which distinguishes them.
pub(super) fn same_value(candidate: &Json, default: &Json) -> bool {
    candidate == default
}

/// One key's schema field, as an object, empty when it carries none.
pub(super) fn schema_of(key: &Merged, name: &str) -> Json {
    match key.fields.get(name) {
        Some(Json::Object(fields)) => Json::Object(fields.clone()),
        _ => Json::Object(Map::new()),
    }
}

/// Whether a schema asserts nothing.
pub(super) fn is_empty_schema(schema: &Json) -> bool {
    schema.as_object().is_none_or(Map::is_empty)
}

/// One value as JSON, spaced the way the generated files are already written.
///
/// `{"a": 1, "b": 2}` rather than `{"a":1,"b":2}`. A cosmetic difference everywhere except here:
/// the suites are committed, and a staleness gate is a comparison of bytes, so the spacing is part
/// of the format rather than a preference. Keys come out sorted because the document model holds an
/// ordered map, which is the same property that makes a published contract a byte-for-byte round
/// trip, applied to a file a person reads.
pub(super) fn json_text(value: &Json) -> String {
    match value {
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(json_text).collect::<Vec<_>>().join(", ")
        ),
        Json::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(name, held)| format!("{}: {}", Json::String(name.clone()), json_text(held)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        scalar => scalar.to_string(),
    }
}
