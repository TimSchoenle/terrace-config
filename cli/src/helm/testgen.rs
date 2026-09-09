//! Turning a configuration contract into round-trip test cases, and the suite they are written to.
//!
//! The document gate validates the configuration a chart *renders*. What no gate can see is the
//! round trip: that a setting an operator writes into the chart's values arrives in the
//! application's document, at the path the image reads it from, carrying the value that was asked
//! for. A typo in a template helper produces a document that satisfies the contract perfectly —
//! every key it does contain is legal — and the setting is simply absent. The pod starts, reports
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
//! reports a proof it has not earned. Where no value other than the default is admissible — a
//! `const`, a single-entry `values` list — the key is skipped and says so.
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
//! **The assertion is scoped to the table the key belongs to, not merely to its leaf name.** In TOML
//! a key belongs to the most recent `[table]` header, so a pattern matching `ttl_secs = 4242`
//! anywhere in the document would accept a chart that wrote the value under the wrong table — which
//! is precisely the defect a round-trip test exists to catch. The generated pattern anchors on the
//! header, walks only lines that do not open a new table, and then matches the leaf; RE2 has no
//! lookahead, so "a line that is not a header" is spelled as an alternation.
//!
//! **Regex metacharacters are escaped against the intersection of two engines.** The patterns are
//! written here and evaluated by Go, and a general escaper escapes punctuation — `-`, `&`, `~`, `#`,
//! the space — that Go's parser is not obliged to accept behind a backslash. So only the characters
//! that are metacharacters in both are escaped, and everything else is passed through as the literal
//! it already is.
//!
//! **A document is selected by the key it carries, and by its labels only where the key is not
//! enough.** The document gate reads a document out of the key its declaration names, so selecting
//! on that same fact is what ties the two to one object — where a label selector on its own matches
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
//! that refuses its own default render needs values supplied before any of its cases renders at all,
//! the case that only checks the document's identity included. Those are the chart's own first-class
//! values, and a prerequisite is therefore carried by every case without exception rather than
//! dropped on a collision the way a baseline is. That is exactly why the collision has to be made
//! unreachable instead: a prerequisite writing under the probe root would sit in the same tree every
//! case probes, free to supply the very value a case exists to prove the chart delivered. The rule
//! lives here, in the model, and not only in the loader that reads the enrolment — a caller reaching
//! past the loader would otherwise reach past the rule with it.
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
//! the probe root is chosen to be — the layer that wins — so the probe outranks whatever a
//! prerequisite set. That is a property of the chart rather than of the generator, and the
//! enrolment's mandatory reason is where a chart says so.

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::{Map, Value as Json};

use crate::document::TextForm;
use crate::error::Error;
use crate::union::Merged;
use crate::value::{assert_value, beyond_scalar_vocabulary};

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

/// Text probes are built from this stem so a value appearing in a rendered document is unmistakably
/// a test fixture and never a plausible setting somebody meant.
const PROBE_STEM: &str = "contract-probe";

/// The first integer probe tried.
///
/// A distinctive number rather than "the default plus one": a passing assertion should not be
/// explicable as a coincidence, and `1` is a value half of any chart repository renders somewhere by
/// accident. Anything the key's bounds refuse falls back to a search from the lower bound.
const DISTINCTIVE_INTEGER: i64 = 4242;

/// How far the fallback search walks before giving up and skipping the key.
///
/// A bound this narrow is reached only by a constraint no probe fits at all, which is a skip either
/// way.
const SEARCH_WIDTH: i64 = 16;

/// Characters that are metacharacters in both this engine and Go's RE2.
const METACHARACTERS: &str = r"\.+*?()|[]{}^$";

/// One or more whole lines that do not open a new table: either a line whose first character is not
/// `[`, or an empty one.
///
/// This is what separates a table header from the key underneath it without running past the next
/// header, and it is an alternation because RE2 has no lookahead.
const GAP: &str = r"(?:[^\[\n][^\n]*\n|\n)*";

/// Why a `structured` key carries no probe.
const STRUCTURED_REASON: &str = "`structured`: the keys beneath it are the operator's own names, so \
                                 the contract describes the container and not its contents and no \
                                 probe addresses a leaf";

/// Why an `unknown` key carries no probe.
const UNKNOWN_REASON: &str = "`unknown`: the producer publishes no constraint for this key, so no \
                              probe can be known-valid";

/// Why a credential carries no probe.
const SECRET_REASON: &str = "`secret: true`: the probe would be committed to this repository, and a \
                             credential-shaped value in a test file is a credential";

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

// ------------------------------------------------------------------------------------------------
// Choosing a probe
// ------------------------------------------------------------------------------------------------

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
/// Exactly one of the two is ever set, and the reason is written into the generated file verbatim —
/// an unexplained absence is indistinguishable from an oversight, which for a file whose whole job
/// is to be exhaustive is the worst possible failure mode.
///
/// `also` is a second schema the candidate has to satisfy, and it is what makes a probe written into
/// the chart's *own* value safe. Such a value carries a `@schema` block, so the templating engine
/// validates what is set there — and that block may be stricter than the contract, deliberately: a
/// chart that types a port `minimum: 1` where the producer's `u16` allows 0 would refuse a probe the
/// contract accepts, and the failure would read as a broken chart rather than as a probe nothing can
/// satisfy. Passing the chart's own block here makes the two agree by construction; the caller falls
/// back to the untyped escape hatch when nothing satisfies both.
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
/// published default — otherwise the case proves the chart delivered a value it would have got
/// anyway — and it has to survive the environment spelling as well. Neither applies to a caller that
/// simply needs a legal value: scaffolding writes one into a new chart's values for a required key
/// whose image publishes no default, where "differs from the default" is vacuous and the environment
/// layer is not involved at all.
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
fn candidates(form: TextForm, path: &str, key: &Merged, constraint: &Json) -> Vec<Json> {
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
fn integer_candidates(constraint: &Json) -> Vec<Json> {
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
fn text_candidates(path: &str, constraint: &Json) -> Vec<Json> {
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
fn environment_text(value: &Json) -> String {
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
/// shape and reporting the same defect twice makes the first line harder to find. That difference is
/// unreachable from here — every candidate is of the key's own `text_form`, and the one cross-shape
/// check is a string against a `text_constraint`, which describes text.
fn satisfies(constraint: &Json, candidate: &Json) -> bool {
    matches!(assert_value(constraint, candidate, ""), Ok(None))
}

/// Whether two values are the same value *and* the same shape.
///
/// `1` and `true` are equal in some languages and are two different probes here, so the comparison
/// is on the JSON value, which distinguishes them.
fn same_value(candidate: &Json, default: &Json) -> bool {
    candidate == default
}

/// One key's schema field, as an object, empty when it carries none.
fn schema_of(key: &Merged, name: &str) -> Json {
    match key.fields.get(name) {
        Some(Json::Object(fields)) => Json::Object(fields.clone()),
        _ => Json::Object(Map::new()),
    }
}

/// Whether a schema asserts nothing.
fn is_empty_schema(schema: &Json) -> bool {
    schema.as_object().is_none_or(Map::is_empty)
}

/// One value as JSON, spaced the way the generated files are already written.
///
/// `{"a": 1, "b": 2}` rather than `{"a":1,"b":2}`. A cosmetic difference everywhere except here: the
/// suites are committed, and a staleness gate is a comparison of bytes, so the spacing is part of
/// the format rather than a preference. Keys come out sorted because the document model holds an
/// ordered map — which is the same property that makes a published contract a byte-for-byte round
/// trip, applied to a file a person reads.
fn json_text(value: &Json) -> String {
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

/// Runs of anything that is not a letter or a digit, for the probe stem.
fn non_alphanumeric() -> &'static Regex {
    static HELD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    HELD.get_or_init(|| Regex::new("[^A-Za-z0-9]+").expect("a literal pattern compiles"))
}

// ------------------------------------------------------------------------------------------------
// Spelling the assertion
// ------------------------------------------------------------------------------------------------

/// Escape only what is a metacharacter to both this engine and Go's RE2.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for held in text.chars() {
        if METACHARACTERS.contains(held) {
            out.push('\\');
        }
        out.push(held);
    }
    out
}

/// One TOML key as a chart's own helper renders it: bare where TOML allows, quoted where not.
#[must_use]
pub fn toml_key(name: &str) -> String {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|held| held.is_ascii_alphanumeric() || held == '_' || held == '-');
    if bare {
        name.to_owned()
    } else {
        Json::String(name.to_owned()).to_string()
    }
}

/// One TOML scalar as a chart's own helper renders it, which is a raw JSON conversion.
///
/// A TOML basic string, integer, float and boolean are spelled exactly as their JSON equivalents,
/// which is why a library chart emits every leaf through one conversion. Non-ASCII is not escaped,
/// because Go's marshaller does not escape it either and a `\uXXXX` in the pattern would match a
/// document that does not contain one.
#[must_use]
pub fn toml_scalar(value: &Json) -> String {
    value.to_string()
}

/// A regex matching the leaf under the table its contract path names, and nothing else.
///
/// # Errors
/// [`Error::Invalid`] when a contract path has an empty segment, which is a path no table structure
/// corresponds to.
pub fn document_pattern(path: &str, text: &str) -> Result<String, Error> {
    let segments: Vec<&str> = path.split('.').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(Error::Invalid(format!(
            "the contract path '{path}' has an empty segment"
        )));
    }

    // `split` always yields at least one segment, and the emptiness of every one of them was just
    // refused, so the leaf is there.
    let leaf = escape(&format!(
        "{} = {text}",
        toml_key(segments.last().copied().unwrap_or_default())
    ));
    if segments.len() == 1 {
        // A key with no table sits above every header, so the walk starts at the document.
        return Ok(format!(r"(?m)\A{GAP}^{leaf}$"));
    }

    let header = escape(&format!(
        "[{}]",
        segments[..segments.len() - 1]
            .iter()
            .map(|segment| toml_key(segment))
            .collect::<Vec<_>>()
            .join(".")
    ));
    Ok(format!(r"(?m)^{header}$\n{GAP}^{leaf}$"))
}

// ------------------------------------------------------------------------------------------------
// Spelling the document selector
// ------------------------------------------------------------------------------------------------

/// One string literal inside a `JSONPath` expression, double-quoted.
///
/// Double quotes rather than single ones so the whole expression survives the single-quoted YAML
/// scalar it is written into without every quote in it being doubled. A label or a key carrying a
/// double quote or a backslash is refused instead of escaped: the selector engine's lexer is not
/// this build's to reason about, and a guess here would produce a selector that silently matches
/// nothing.
///
/// # Errors
/// [`Error::Invalid`] naming the text that cannot be written into one.
pub fn jsonpath_string(text: &str) -> Result<String, Error> {
    if text.contains('"') || text.contains('\\') {
        return Err(Error::Invalid(format!(
            "{} cannot be written into a document selector: a JSONPath string literal here \
             carries neither a double quote nor a backslash",
            crate::gate::quoted(text)
        )));
    }
    Ok(format!("\"{text}\""))
}

/// The document selector path for one document: its key, narrowed by labels where needed.
///
/// The framework evaluates this against each rendered manifest and selects the manifest when it
/// resolves to anything at all, so putting the labels into a filter over the document root and the
/// key into the step after it is what makes one selector out of two facts. Its selector has room for
/// exactly one path and one value, which is why they are not spelled as two matchers.
///
/// # Errors
/// [`Error::Invalid`] when a key or a label cannot be written as a `JSONPath` string.
pub fn selector_path(key: &str, discriminator: &[(String, String)]) -> Result<String, Error> {
    if discriminator.is_empty() {
        return Ok(format!("data[{}]", jsonpath_string(key)?));
    }
    let mut predicates = Vec::new();
    for (label, value) in discriminator {
        predicates.push(format!(
            "@.metadata.labels[{}]=={}",
            jsonpath_string(label)?,
            jsonpath_string(value)?
        ));
    }
    Ok(format!(
        "$[?({})].data[{}]",
        predicates.join(" && "),
        jsonpath_string(key)?
    ))
}

// ------------------------------------------------------------------------------------------------
// The plan
// ------------------------------------------------------------------------------------------------

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
/// tree every chart exposes, and what it proves is that the tree is merged into the document — true,
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

/// Turn a union's keys into the cases and the skips of one suite.
///
/// `baseline` is carried by every case except the one probing the key it sets. A chart may refuse to
/// render a values combination the contract considers perfectly legal, and a probe that walked into
/// such a pair would fail for a reason that has nothing to do with the round trip. Dropping the
/// entry that collides with the probe is what stops the baseline from supplying the very value the
/// case is meant to prove the chart delivered.
///
/// `prerequisites` is carried by every case with no exception at all, because it is what makes the
/// chart render in the first place. So the collision the baseline is protected from by being dropped
/// is instead made unreachable — a prerequisite path inside the probes' own tree is refused here
/// rather than accommodated. The two fields are different things and this is the line between them.
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
        // same key twice, which the framework refuses outright — and the prerequisite is the one
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

// ------------------------------------------------------------------------------------------------
// Rendering the suite
// ------------------------------------------------------------------------------------------------

/// Written by hand rather than through a YAML emitter, and the reason is the header.
///
/// A generated file nobody may edit has to say so on its first line and say why, and an emitter that
/// cannot carry a comment would put the explanation nowhere a contributor reads it. Everything
/// emitted below is a scalar or a two-level mapping, so what this owes YAML is small and the quoting
/// rules are stated once, in [`yaml_scalar`].
const BANNER: &str = "Generated by `just contract-tests`. Do not edit by hand.";

/// Where a wrapped comment line stops.
const COMMENT_WIDTH: usize = 96;

/// The document a suite is written for, as the declaration and the enrolment describe it.
///
/// The last two fields carry defaults because they are what almost every chart looks like: a
/// document whose key identifies it on its own, probed through the configuration tree the chart
/// merges over its derived wiring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The chart's directory name.
    pub chart: String,
    /// The declared document.
    pub name: String,
    /// The rendered object's kind.
    pub kind: String,
    /// The labels the declaration selects it on.
    pub selector: Vec<(String, String)>,
    /// The key inside `data` holding the document.
    pub key: String,
    /// The declaration, as a path from the repository root.
    pub declaration: String,
    /// The vendored contracts, likewise.
    pub contracts: Vec<String>,
    /// Labels that tell this document from its siblings, empty when the key already does.
    pub discriminator: Vec<(String, String)>,
    /// The values path a probe is written under.
    pub root: String,
}

/// The complete suite for one document, as the text to write.
///
/// Deterministic in every part: cases sorted by contract key path, skips with them, selector labels
/// sorted, and nothing that churns on its own — a digest, a timestamp, an application version —
/// written into the file at all. That is what lets the staleness gate be a comparison of bytes, and
/// it is what keeps a digest bump that changes no key from producing a diff here.
///
/// `reason` is the enrolment's explanation for whatever it made non-default about this document — a
/// baseline, a probe root, or both — and is rendered once beside them. The prerequisites carry their
/// own, because they are a different field explaining a different thing.
///
/// # Errors
/// [`Error::Invalid`] when the document's key or a selector label cannot be written into a selector.
pub fn render_suite(
    target: &Target,
    plan: &Plan,
    baseline: &[(String, Json)],
    reason: Option<&str>,
    prerequisites: &[(String, Json)],
    prerequisite_reason: Option<&str>,
    unrouted: &[(String, String)],
) -> Result<String, Error> {
    let mut lines = comment(&preamble(target, plan), "");
    lines.push(String::new());
    lines.push(format!(
        "suite: {}",
        yaml_scalar(&Json::String(format!(
            "contract round trip ({})",
            target.name
        )))
    ));
    lines.push(String::new());
    lines.extend(comment(&release_note(), ""));
    lines.push("release:".to_owned());
    lines.push(format!(
        "  name: {}",
        yaml_scalar(&Json::String(target.chart.clone()))
    ));
    lines.push(String::new());
    if !prerequisites.is_empty() {
        lines.extend(comment(
            &prerequisite_note(&target.root, prerequisite_reason),
            "",
        ));
    }
    lines.push("tests:".to_owned());
    lines.extend(identity_case(target, prerequisites)?);

    let notes = case_notes(target, baseline, reason);
    if !notes.is_empty() {
        lines.push(String::new());
        lines.extend(comment(&notes, "  "));
    }

    if !unrouted.is_empty() {
        lines.push(String::new());
        lines.extend(comment(&unrouted_note(unrouted, &target.root), "  "));
    }

    for case in &plan.cases {
        lines.push(String::new());
        lines.extend(probe_case(case, target)?);
    }

    if !plan.skipped.is_empty() {
        lines.push(String::new());
        lines.extend(comment(&skipped_note(&plan.skipped), "  "));
    }

    lines.push(String::new());
    Ok(lines.join("\n"))
}

/// One scalar, quoted whenever bare would be ambiguous or would change its type.
fn yaml_scalar(value: &Json) -> String {
    match value {
        Json::Bool(true) => return "true".to_owned(),
        Json::Bool(false) => return "false".to_owned(),
        held if held.is_i64() || held.is_u64() => return held.to_string(),
        _ => {}
    }
    let text = match value {
        Json::String(text) => text.clone(),
        Json::Null => "None".to_owned(),
        other => other.to_string(),
    };
    if plain_scalar(&text) {
        text
    } else {
        yaml_quoted(&text)
    }
}

/// YAML scalars safe to emit bare.
///
/// Deliberately narrow: anything outside it — a leading digit, a colon, a word YAML would read as a
/// boolean — is single-quoted instead, which is always correct and never changes the type the test
/// framework sees.
fn plain_scalar(text: &str) -> bool {
    let mut held = text.chars();
    held.next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && held.all(|character| character.is_ascii_alphanumeric() || "_./-".contains(character))
}

/// A selector path YAML reads back unchanged as a plain scalar.
///
/// `data["config.toml"]` is one by YAML's block-context rules; the filter form opens with `$[?(` and
/// carries `&&`, and quoting that is cheaper than making every reader confirm none of it is an
/// indicator.
fn plain_selector(text: &str) -> bool {
    let mut held = text.chars();
    held.next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && held.all(|character| character.is_ascii_alphanumeric() || "_.\"[]/-".contains(character))
}

/// One value on the right of a `set` entry, whatever its shape, on a single line.
///
/// A render prerequisite may be a whole subtree, and a JSON conversion produces a YAML flow
/// collection as readily as a JSON one — so the conversion that already spells a TOML scalar spells a
/// structure here too. Kept to one line and sorted, for the two reasons everything here is: a `set`
/// block that stays a flat list of paths is one a reader scans, and a gate comparing bytes cannot
/// survive a mapping whose order is whatever a hash table happened to produce.
fn yaml_value(value: &Json) -> String {
    match value {
        Json::Object(_) | Json::Array(_) => json_text(value),
        scalar => yaml_scalar(scalar),
    }
}

/// A single-quoted YAML scalar, which is the one form that keeps a backslash literal.
///
/// Every pattern this module produces is dense with backslashes, and a double-quoted scalar would
/// consume them as YAML escapes before the regex engine ever saw them.
fn yaml_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Prefix each line with a comment marker, leaving a blank line as a bare one.
fn comment(lines: &[String], indent: &str) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            if line.is_empty() {
                format!("{indent}#")
            } else {
                format!("{indent}# {line}")
            }
        })
        .collect()
}

fn preamble(target: &Target, plan: &Plan) -> Vec<String> {
    let covered = plan.cases.len();
    let total = covered + plan.skipped.len();
    let routed = plan
        .cases
        .iter()
        .filter(|case| case.through.is_some())
        .count();

    let mut lines = vec![BANNER.to_owned(), String::new()];
    lines.extend(wrap(
        &format!(
            "Every case below writes one setting into the chart's own value for it — or into \
             `{}` where no value is bound to that key — and asserts that it arrives in `{}`, \
             under the table its contract path names. That is the round trip no other gate can \
             see: `just check-config` proves the rendered document satisfies the contract, and a \
             document missing a setting entirely satisfies it perfectly.",
            target.root, target.key
        ),
        COMMENT_WIDTH - 2,
    ));
    lines.push(String::new());
    lines.push(format!("Chart:       {}", target.chart));
    lines.push(format!(
        "Declaration: {} (document `{}`)",
        target.declaration, target.name
    ));
    lines.extend(
        target
            .contracts
            .iter()
            .map(|path| format!("Contract:    {path}")),
    );
    lines.push(format!(
        "Coverage:    {covered} of {total} contract keys carry a probe, {routed} through the \
         chart's own value"
    ));
    lines.push(String::new());
    lines.extend(wrap(
        "Regenerate with `just contract-tests`. `just check-contract-tests` fails a pull request \
         whose committed copy has drifted from the contract it was generated against.",
        COMMENT_WIDTH - 2,
    ));
    lines
}

fn release_note() -> Vec<String> {
    wrap(
        "`just render` installs every chart under a release named after it, and the declaration's \
         selector is written against that render. Naming the release the same way here is what \
         makes `app.kubernetes.io/instance` carry the value the declaration selects on.",
        COMMENT_WIDTH - 2,
    )
}

fn identity_case(target: &Target, prerequisites: &[(String, Json)]) -> Result<Vec<String>, Error> {
    let mut lines = comment(&wrap(&selection_note(target), COMMENT_WIDTH - 4), "  ");
    lines.push(format!(
        "  - it: renders {} on the {} the declaration selects",
        target.key, target.kind
    ));
    lines.extend(selector(target)?);
    // This case sets nothing of its own, and still carries the prerequisites: without them the
    // chart's guard refuses the render, and a case asserting on a document that was never produced
    // fails for a reason that has nothing to do with what it checks.
    lines.extend(set_block(prerequisites));
    lines.push("    asserts:".to_owned());
    lines.push("      - isKind:".to_owned());
    lines.push(format!(
        "          of: {}",
        yaml_scalar(&Json::String(target.kind.clone()))
    ));

    let mut labels = target.selector.clone();
    labels.sort();
    for (label, value) in labels {
        lines.push("      - equal:".to_owned());
        lines.push(format!("          path: metadata.labels[\"{label}\"]"));
        lines.push(format!(
            "          value: {}",
            yaml_scalar(&Json::String(value))
        ));
    }
    Ok(lines)
}

/// The document selector, matching on the key existing rather than on its contents.
fn selector(target: &Target) -> Result<Vec<String>, Error> {
    let path = selector_path(&target.key, &target.discriminator)?;
    let scalar = if plain_selector(&path) {
        path
    } else {
        yaml_quoted(&path)
    };
    Ok(vec![
        "    documentSelector:".to_owned(),
        format!("      path: {scalar}"),
    ])
}

/// Why every case below selects the document it does, in whichever form actually applies.
fn selection_note(target: &Target) -> String {
    if target.discriminator.is_empty() {
        return format!(
            "Every case below selects its document by the key it carries, which is what \
             `check-config.py` reads it from — a label would match the Deployment and the Service \
             too. This case is what ties that selection back to the declaration: the object \
             holding `{}` has to be the {} the declaration names, carrying the labels the \
             declaration selects on.",
            target.key, target.kind
        );
    }

    let labels: Vec<String> = target
        .discriminator
        .iter()
        .map(|(label, value)| format!("`{label}: {value}`"))
        .collect();
    format!(
        "Every case below selects its document by the key it carries and by {}, because this \
         chart renders `{}` into more than one document and the key on its own identifies all of \
         them at once. The labels on their own would not do it either — they are on the workload \
         and its Service as well — so the two are spelled as one JSONPath filter, which is what \
         helm-unittest's single-matcher selector has room for. This case is what ties that \
         selection back to the declaration: the object holding `{}` has to be the {} the \
         declaration names, carrying the labels the declaration selects on.",
        labels.join(", "),
        target.key,
        target.key,
        target.kind
    )
}

/// One case's `set` mapping, or nothing at all when it has no values to write.
fn set_block(values: &[(String, Json)]) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["    set:".to_owned()];
    for (name, value) in values {
        lines.push(format!(
            "      {}: {}",
            yaml_scalar(&Json::String(name.clone())),
            yaml_value(value)
        ));
    }
    lines
}

const BASELINE_NOTE: &str = "Every case below also carries the chart's baseline values, minus \
                             whichever of them the case is itself probing — a baseline that \
                             supplied the probed value would make the assertion pass whether or \
                             not the chart delivered anything. The baseline exists because a chart \
                             may refuse to render a combination the contract considers legal, and \
                             a probe that walked into one would fail for a reason that has nothing \
                             to do with the round trip.";

fn root_note(root: &str) -> String {
    format!(
        "Every case below writes its probe into `{root}` rather than into `{VALUES_ROOT}`, which \
         the chart's enrolment states because this chart merges its derived wiring *over* \
         `{VALUES_ROOT}` rather than under it. A probe written into `{VALUES_ROOT}` for a key the \
         chart derives would be overwritten before it reached the document, and the case would \
         fail on a chart that is behaving correctly. What that costs is real: `{root}` is the \
         layer these cases prove, and nothing here proves `{VALUES_ROOT}` still reaches a key the \
         chart does not derive."
    )
}

/// Whatever the enrolment made non-default about these cases, and the reason it gives.
///
/// One block carrying one reason rather than a note per field: the document entry states a single
/// reason for its baseline and its probe root alike, and a reader who sees the same paragraph twice
/// stops reading it. The render prerequisites are not here — they are a separate field with a
/// separate reason, and their note sits above `tests:` because the identity case carries them too.
fn case_notes(target: &Target, baseline: &[(String, Json)], reason: Option<&str>) -> Vec<String> {
    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    if !baseline.is_empty() {
        paragraphs.push(wrap(BASELINE_NOTE, COMMENT_WIDTH - 4));
    }
    if target.root != VALUES_ROOT {
        paragraphs.push(wrap(&root_note(&target.root), COMMENT_WIDTH - 4));
    }
    if paragraphs.is_empty() {
        return Vec::new();
    }
    if let Some(reason) = reason {
        paragraphs.push(wrap(reason.trim(), COMMENT_WIDTH - 4));
    }

    let mut lines: Vec<String> = Vec::new();
    for paragraph in paragraphs {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(paragraph);
    }
    lines
}

fn prerequisite_note(root: &str, reason: Option<&str>) -> Vec<String> {
    let mut lines = wrap(
        &format!(
            "Every case below carries this chart's render prerequisites, the identity case \
             included: chart values its own `validateValues` insists on before it will render \
             anything at all. They are first-class chart values rather than configuration, and \
             none of them may be written under `{root}` — a prerequisite there would sit in the \
             tree every case probes, free to supply the value the case exists to prove the chart \
             delivered. Unlike a baseline, a prerequisite is never dropped for the case it would \
             collide with, because a case without it does not render at all; the refusal is what \
             takes that dropping's place."
        ),
        COMMENT_WIDTH - 2,
    );
    if let Some(reason) = reason {
        lines.push(String::new());
        lines.extend(wrap(reason.trim(), COMMENT_WIDTH - 2));
    }
    lines
}

fn probe_case(case: &Case, target: &Target) -> Result<Vec<String>, Error> {
    // The case says which route it took, because the two prove different things and a reader
    // scanning the suite has no other way to tell them apart: `from <value>` exercises the chart's
    // own mapping onto the key, and its absence means the probe went into the raw tree and the
    // mapping is untested.
    let through = case
        .through
        .as_ref()
        .map_or_else(String::new, |value| format!(" from {value}"));
    let mut lines = vec![format!(
        "  - it: delivers {} into {}{through}",
        case.path, target.key
    )];
    lines.extend(selector(target)?);
    lines.extend(set_block(&case.set_values));
    lines.push("    asserts:".to_owned());
    lines.push("      - matchRegex:".to_owned());
    lines.push(format!("          path: data[\"{}\"]", target.key));
    lines.push(format!("          pattern: {}", yaml_quoted(&case.pattern)));
    Ok(lines)
}

/// The keys whose probe the enrolment moved back into the raw tree, and the reason given.
///
/// Written into the suite rather than left in the enrolment file, because the difference is visible
/// in every affected case — `delivers x into config.toml` where its neighbours say `from y` — and a
/// reader who notices that deserves the answer in the same file.
fn unrouted_note(unrouted: &[(String, String)], root: &str) -> Vec<String> {
    let mut lines = wrap(
        &format!(
            "The keys below are probed through `{root}` rather than through the chart's own value \
             for them, which their neighbours use. Each one is declared in the chart's \
             `contract-tests.yaml` with the reason repeated here: such a case proves the raw tree \
             reaches the document and says nothing about the chart's mapping onto the key."
        ),
        COMMENT_WIDTH - 4,
    );
    for (key, reason) in unrouted {
        lines.push(String::new());
        lines.push(format!("{key}:"));
        lines.extend(
            wrap(reason, COMMENT_WIDTH - 6)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
    }
    lines
}

fn skipped_note(skipped: &[Skipped]) -> Vec<String> {
    let mut lines = wrap(
        "Contract keys carrying no case, and why. An unexplained absence is indistinguishable from \
         an oversight, so every one of them is named here rather than simply left out.",
        COMMENT_WIDTH - 4,
    );
    lines.push(String::new());
    for entry in skipped {
        let indent = " ".repeat(entry.path.len() + 2);
        let wrapped = wrap(&entry.reason, COMMENT_WIDTH - 4 - indent.len());
        lines.push(format!("{}: {}", entry.path, wrapped[0]));
        lines.extend(wrapped[1..].iter().map(|line| format!("{indent}{line}")));
    }
    lines
}

/// Greedy wrap on spaces alone, so a reason reads as prose rather than as one long line.
///
/// Hand-rolled because these reasons quote JSON and constraint spellings that a general wrapper
/// would break on punctuation this keeps together.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let limit = width.max(32);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > limit {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    lines.push(current);
    lines
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value as Json, json};

    use super::{
        Case, DISTINCTIVE_INTEGER, Plan, Probe, Route, Target, VALUES_ROOT, document_pattern,
        escape, plan, prerequisite_conflict, probe_for, render_suite, selector_path, toml_key,
        toml_scalar, values_path,
    };
    use crate::union::{Merged, Ordered, Union, union_contracts};

    /// One contract key, with the fields every entry carries and no others.
    fn key(path: &str, overrides: &Json) -> Merged {
        let mut fields = json!({
            "path": path,
            "env": format!("APP_{}", path.to_uppercase().replace('.', "__")),
            "text_form": "text",
            "constraint": {"type": "string"},
            "default_value": null,
            "required": false,
            "secret": false,
            "reserved": false,
            "values": [],
        });
        for (name, value) in overrides.as_object().expect("an object") {
            fields[name] = value.clone();
        }
        Merged {
            name: path.to_owned(),
            fields: fields.as_object().expect("an object").clone(),
            sources: vec!["contracts/one.json".to_owned()],
        }
    }

    /// The three keys most of these plan over: one probeable integer, one text, one credential.
    fn keys() -> Vec<Merged> {
        vec![
            key(
                "isr.ttl_secs",
                &json!({
                    "text_form": "integer",
                    "constraint": {"type": "integer", "minimum": 0},
                    "default_value": 0,
                }),
            ),
            key("assets.dist_dir", &json!({"default_value": "public"})),
            key("database.password", &json!({"secret": true})),
        ]
    }

    fn planned(
        keys: &[Merged],
        baseline: &[(String, Json)],
        prerequisites: &[(String, Json)],
        root: &str,
        routes: &BTreeMap<String, Route>,
    ) -> Plan {
        let held: Vec<&Merged> = keys.iter().collect();
        plan(&held, baseline, prerequisites, root, routes).expect("the plan is buildable")
    }

    fn simple(keys: &[Merged]) -> Plan {
        planned(keys, &[], &[], VALUES_ROOT, &BTreeMap::new())
    }

    fn case_for<'a>(plan: &'a Plan, path: &str) -> &'a Case {
        plan.cases
            .iter()
            .find(|case| case.path == path)
            .expect("a case for that key")
    }

    fn probe(key: &Merged) -> Probe {
        probe_for(key, None)
            .expect("the key is readable")
            .expect("the key carries a probe")
    }

    fn refusal(key: &Merged) -> String {
        probe_for(key, None)
            .expect("the key is readable")
            .expect_err("the key carries no probe")
    }

    // -- choosing a probe --------------------------------------------------------------------

    #[test]
    fn a_boolean_probe_is_the_opposite_of_the_default() {
        let found = probe(&key(
            "csp.hashes",
            &json!({"text_form": "boolean", "constraint": {"type": "boolean"},
                    "default_value": true}),
        ));
        assert_eq!(found.value, json!(false));
        assert_eq!(found.text, "false");
    }

    #[test]
    fn a_boolean_with_no_default_still_gets_a_probe() {
        let found = probe(&key(
            "csp.hashes",
            &json!({"text_form": "boolean", "constraint": {"type": "boolean"}}),
        ));
        assert_eq!(found.value, json!(true));
    }

    #[test]
    fn an_integer_probe_is_distinctive_rather_than_adjacent_to_the_default() {
        // A passing assertion should not be explicable as a coincidence, and `1` is a value half
        // the charts in a repository render somewhere by accident.
        let found = probe(&key(
            "isr.ttl",
            &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 0},
                    "default_value": 0}),
        ));
        assert_eq!(found.value, json!(DISTINCTIVE_INTEGER));
    }

    #[test]
    fn an_integer_probe_respects_the_bounds_the_contract_publishes() {
        let found = probe(&key(
            "server.port",
            &json!({"text_form": "integer",
                    "constraint": {"type": "integer", "minimum": 1, "maximum": 99},
                    "default_value": 8080}),
        ));
        let held = found.value.as_i64().expect("an integer probe");
        assert!((1..=99).contains(&held), "{held}");

        let found = probe(&key(
            "tuning.step",
            &json!({"text_form": "integer",
                    "constraint": {"type": "integer", "minimum": 0, "multipleOf": 5},
                    "default_value": 0}),
        ));
        let held = found.value.as_i64().expect("an integer probe");
        assert_eq!(held % 5, 0, "{held}");
        assert_ne!(held, 0);
    }

    #[test]
    fn a_text_probe_names_the_key_it_probes() {
        // The rendered document shows which setting went missing without a reader opening the
        // contract.
        let found = probe(&key("assets.dist_dir", &json!({"default_value": "public"})));
        assert_eq!(found.value, json!("contract-probe-assets-dist-dir"));
        assert_eq!(found.text, "\"contract-probe-assets-dist-dir\"");
    }

    #[test]
    fn a_text_probe_is_shortened_to_fit_a_maximum_length() {
        let found = probe(&key(
            "assets.dist_dir",
            &json!({"constraint": {"type": "string", "maxLength": 12}}),
        ));
        assert_eq!(found.value.as_str().expect("a text probe").len(), 12);
    }

    #[test]
    fn the_environment_spelling_steers_the_choice() {
        // A probe the file could carry and a variable could not is not deliverable both ways.
        let found = probe(&key(
            "isr.ttl",
            &json!({"text_form": "integer", "constraint": {"type": "integer", "minimum": 0},
                    "text_constraint": {"type": "string", "pattern": r"^\s*[0-9]{2}\s*$"}}),
        ));
        assert_ne!(found.value, json!(DISTINCTIVE_INTEGER));
        let held = found.value.as_i64().expect("an integer probe");
        assert!((10..=99).contains(&held), "{held}");
    }

    #[test]
    fn a_choice_probe_is_a_declared_value_other_than_the_default() {
        let found = probe(&key(
            "rate_limit.backend",
            &json!({"text_form": "choice", "values": ["memory", "redis"],
                    "constraint": {"type": "string", "enum": ["memory", "redis"]},
                    "default_value": "memory"}),
        ));
        assert_eq!(found.value, json!("redis"));
    }

    // -- probes that must not be invented ----------------------------------------------------

    #[test]
    fn a_credential_is_refused_because_a_committed_probe_is_a_credential() {
        assert!(
            refusal(&key("database.password", &json!({"secret": true}))).contains("secret"),
            "{}",
            refusal(&key("database.password", &json!({"secret": true})))
        );
    }

    #[test]
    fn the_two_unprobeable_forms_are_refused_for_their_own_reasons() {
        // Three different situations, and a reader is never left to guess which applies.
        let structured = refusal(&key(
            "internal.peers",
            &json!({"text_form": "structured", "constraint": {"type": "object"}}),
        ));
        assert!(structured.contains("structured"), "{structured}");
        assert!(structured.contains("operator's own names"), "{structured}");

        let unknown = refusal(&key(
            "tuning.ratio",
            &json!({"text_form": "unknown", "constraint": {}}),
        ));
        assert!(unknown.contains("no constraint"), "{unknown}");
    }

    #[test]
    fn a_key_no_value_but_its_default_satisfies_carries_no_probe() {
        // A case that sets a key to the value it already has reports a proof it has not earned.
        for (path, overrides) in [
            (
                "app.mode",
                json!({"constraint": {"type": "string", "const": "server"},
                       "default_value": "server"}),
            ),
            (
                "app.mode",
                json!({"text_form": "choice", "values": ["server"],
                       "constraint": {"type": "string", "enum": ["server"]},
                       "default_value": "server"}),
            ),
        ] {
            let reason = refusal(&key(path, &overrides));
            assert!(reason.contains("differing from the default"), "{reason}");
        }
    }

    #[test]
    fn a_text_key_constrained_by_a_pattern_carries_no_probe() {
        let reason = refusal(&key(
            "app.slug",
            &json!({"constraint": {"type": "string", "pattern": "^[0-9]{4}$"}}),
        ));
        assert!(reason.contains("pattern"), "{reason}");
    }

    #[test]
    fn a_constraint_outside_the_vocabulary_carries_no_probe() {
        let reason = refusal(&key(
            "app.slug",
            &json!({"constraint": {"type": "string", "allOf": [{"minLength": 1}]}}),
        ));
        assert!(reason.contains("allOf"), "{reason}");
    }

    #[test]
    fn a_probe_no_environment_spelling_can_carry_is_refused() {
        // `text_constraint` governs the variable, and a key has to be deliverable both ways.
        let reason = refusal(&key(
            "isr.ttl",
            &json!({"text_form": "integer", "constraint": {"type": "integer"},
                    "text_constraint": {"type": "string", "pattern": r"^\s*[0-9]{40}\s*$"}}),
        ));
        assert!(reason.contains("differing from the default"), "{reason}");
    }

    // -- spelling the assertion --------------------------------------------------------------

    const DOCUMENT: &str = "[assets]\ndist_dir = \"public\"\n\n[csp]\nhash_inline_scripts = true\n\
                            \n[csp.cloudflare]\nscript_nonce = true\nturnstile = false\n\
                            web_analytics = false\n\n[isr]\ncache_dir = \"/tmp/isr\"\n\
                            ttl_secs = 0\n";

    fn matches(path: &str, text: &str) -> bool {
        let pattern = document_pattern(path, text).expect("a spellable path");
        regex::Regex::new(&pattern)
            .expect("the generated pattern compiles")
            .is_match(DOCUMENT)
    }

    #[test]
    fn a_leaf_under_its_own_table_matches() {
        assert!(matches("csp.cloudflare.turnstile", "false"));
        assert!(matches("csp.hash_inline_scripts", "true"));
        assert!(matches("isr.ttl_secs", "0"));
        assert!(matches("csp.cloudflare.script_nonce", "true"));
    }

    #[test]
    fn a_leaf_under_the_wrong_table_does_not_match() {
        // The defect a pattern keyed on the leaf name alone would wave through.
        assert!(!matches("isr.turnstile", "false"));
        assert!(!matches("csp.turnstile", "false"));
    }

    #[test]
    fn a_wrong_value_or_a_suffix_of_another_leaf_does_not_match() {
        assert!(!matches("csp.cloudflare.turnstile", "true"));
        assert!(!matches("isr.secs", "0"));
    }

    #[test]
    fn a_quoted_value_is_matched_as_the_renderer_writes_it() {
        assert!(matches("isr.cache_dir", &toml_scalar(&json!("/tmp/isr"))));
    }

    #[test]
    fn only_shared_metacharacters_are_escaped() {
        // A general escaper also escapes punctuation Go's parser is not obliged to accept.
        assert_eq!(escape("a-b~c#d e"), "a-b~c#d e");
        assert_eq!(escape("[a].b"), r"\[a\]\.b");
    }

    #[test]
    fn a_key_outside_toml_s_bare_alphabet_is_quoted() {
        assert_eq!(toml_key("docs/handbook"), "\"docs/handbook\"");
        assert_eq!(toml_key("dist_dir"), "dist_dir");
    }

    #[test]
    fn a_top_level_key_is_anchored_at_the_document() {
        let pattern = document_pattern("level", "\"info\"").expect("a spellable path");
        let compiled = regex::Regex::new(&pattern).expect("it compiles");
        assert!(compiled.is_match("level = \"info\"\n\n[isr]\nttl_secs = 0\n"));
        assert!(!compiled.is_match("[isr]\nlevel = \"info\"\n"));
    }

    #[test]
    fn a_path_with_an_empty_segment_is_refused_rather_than_spelled() {
        assert!(document_pattern("a..b", "1").is_err());
    }

    // -- spelling the document selector ------------------------------------------------------

    fn label(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn a_key_that_identifies_its_document_selects_on_the_key_alone() {
        assert_eq!(
            selector_path("config.toml", &[]).expect("a spellable key"),
            "data[\"config.toml\"]"
        );
    }

    #[test]
    fn a_shared_key_is_narrowed_by_the_labels_the_declaration_selects_on() {
        assert_eq!(
            selector_path(
                "config.toml",
                &[label("app.kubernetes.io/component", "api")]
            )
            .expect("a spellable key"),
            "$[?(@.metadata.labels[\"app.kubernetes.io/component\"]==\"api\")].data\
             [\"config.toml\"]"
        );
    }

    #[test]
    fn several_labels_become_one_filter() {
        // The framework's document selector carries one path and one value, and no more.
        assert_eq!(
            selector_path("config.toml", &[label("a", "1"), label("b", "2")])
                .expect("a spellable key"),
            "$[?(@.metadata.labels[\"a\"]==\"1\" && @.metadata.labels[\"b\"]==\"2\")].data\
             [\"config.toml\"]"
        );
    }

    #[test]
    fn a_label_no_string_literal_can_carry_is_refused_rather_than_escaped() {
        // A guess at another engine's lexer produces a selector that silently matches nothing.
        for offender in ["quo\"te", "back\\slash"] {
            assert!(
                selector_path(
                    "config.toml",
                    &[label("app.kubernetes.io/component", offender)]
                )
                .is_err(),
                "{offender}"
            );
        }
    }

    // -- planning ----------------------------------------------------------------------------

    #[test]
    fn cases_and_skips_are_sorted_by_contract_path() {
        let plan = simple(&keys());
        assert_eq!(
            plan.cases.iter().map(|case| &case.path).collect::<Vec<_>>(),
            ["assets.dist_dir", "isr.ttl_secs"]
        );
        assert_eq!(
            plan.skipped
                .iter()
                .map(|held| &held.path)
                .collect::<Vec<_>>(),
            ["database.password"]
        );
    }

    #[test]
    fn a_case_writes_its_key_under_the_raw_configuration_tree() {
        let plan = simple(&keys());
        let case = case_for(&plan, "isr.ttl_secs");
        assert_eq!(
            case.set_values.last(),
            Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
        );
    }

    #[test]
    fn the_baseline_is_carried_by_every_case() {
        let baseline = vec![(
            "config.csp.cloudflare.script_nonce".to_owned(),
            json!(false),
        )];
        for case in planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new()).cases {
            assert!(case.set_values.contains(&baseline[0]), "{case:?}");
        }
    }

    #[test]
    fn the_baseline_never_supplies_the_value_the_case_is_probing() {
        // Otherwise the case passes whether or not the chart delivered anything.
        let probed = values_path("isr.ttl_secs", VALUES_ROOT);
        let baseline = vec![(probed.clone(), json!(99))];
        let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new());
        let case = case_for(&plan, "isr.ttl_secs");
        assert_eq!(
            case.set_values
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [probed.as_str()]
        );
        assert_eq!(case.set_values[0].1, json!(DISTINCTIVE_INTEGER));
    }

    // -- routed probes -----------------------------------------------------------------------

    fn route(values_path: &str, condition: Option<&str>, schema: Option<Json>) -> Route {
        Route {
            values_path: values_path.to_owned(),
            condition: condition.map(str::to_owned),
            schema,
        }
    }

    fn routes() -> BTreeMap<String, Route> {
        BTreeMap::from([
            (
                "assets.dist_dir".to_owned(),
                route("assets.distDir", None, None),
            ),
            ("isr.ttl_secs".to_owned(), route("isr.ttlSecs", None, None)),
        ])
    }

    #[test]
    fn the_probe_lands_on_the_chart_value_when_a_marker_names_one() {
        // The difference between proving the escape hatch is merged and proving the chart's own
        // mapping: a typo in the helper between the two spellings passes every unrouted case.
        let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes());
        let case = case_for(&plan, "isr.ttl_secs");
        assert_eq!(
            case.set_values.last(),
            Some(&("isr.ttlSecs".to_owned(), json!(DISTINCTIVE_INTEGER)))
        );
        assert_eq!(case.through.as_deref(), Some("isr.ttlSecs"));
    }

    #[test]
    fn a_key_with_no_route_still_goes_through_the_raw_tree() {
        let plan = simple(&keys());
        let case = case_for(&plan, "isr.ttl_secs");
        assert_eq!(
            case.set_values.last(),
            Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
        );
        assert!(case.through.is_none());
    }

    #[test]
    fn a_gate_the_marker_names_is_switched_on() {
        // A `when` clause is a values path the chart tests before writing the key at all, so a
        // case that left it alone would assert against a document the probe never reached.
        let routes = BTreeMap::from([(
            "isr.ttl_secs".to_owned(),
            route("isr.ttlSecs", Some("isr.enabled"), None),
        )]);
        let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
        assert!(
            case_for(&plan, "isr.ttl_secs")
                .set_values
                .contains(&("isr.enabled".to_owned(), json!(true)))
        );
    }

    #[test]
    fn the_baseline_is_still_dropped_by_its_escape_hatch_path() {
        // The raw tree is merged over what the chart derives, so a baseline entry naming the key a
        // case probes would override the chart value the probe was just written into.
        let baseline = vec![(values_path("isr.ttl_secs", VALUES_ROOT), json!(99))];
        let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &routes());
        assert_eq!(
            case_for(&plan, "isr.ttl_secs")
                .set_values
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["isr.ttlSecs"]
        );
    }

    #[test]
    fn a_chart_schema_nothing_satisfies_falls_back_to_the_raw_tree() {
        // A chart value may be typed more tightly than the contract, deliberately. A probe that
        // satisfies the contract and not the chart would fail the templating engine's own values
        // validation, so the case goes through the untyped escape hatch rather than disappearing.
        let routes = BTreeMap::from([(
            "isr.ttl_secs".to_owned(),
            route("isr.ttlSecs", None, Some(json!({"maximum": -1}))),
        )]);
        let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
        let case = case_for(&plan, "isr.ttl_secs");
        assert_eq!(
            case.set_values.last(),
            Some(&("config.isr.ttl_secs".to_owned(), json!(DISTINCTIVE_INTEGER)))
        );
        assert!(case.through.is_none());

        let routes = BTreeMap::from([(
            "isr.ttl_secs".to_owned(),
            route(
                "isr.ttlSecs",
                None,
                Some(json!({"type": "integer", "minimum": 1})),
            ),
        )]);
        let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes);
        assert_eq!(
            case_for(&plan, "isr.ttl_secs").through.as_deref(),
            Some("isr.ttlSecs")
        );
    }

    // -- where a probe is written ------------------------------------------------------------

    const MOVED: &str = "services.api.config";

    #[test]
    fn a_case_writes_its_key_under_the_root_the_enrolment_names() {
        let plan = planned(&keys(), &[], &[], MOVED, &BTreeMap::new());
        assert_eq!(
            case_for(&plan, "isr.ttl_secs").set_values.last(),
            Some(&(format!("{MOVED}.isr.ttl_secs"), json!(DISTINCTIVE_INTEGER)))
        );
        assert_eq!(
            values_path("isr.ttl_secs", VALUES_ROOT),
            "config.isr.ttl_secs"
        );
    }

    #[test]
    fn the_baseline_collision_is_still_caught_under_a_moved_root() {
        // A baseline one layer off the probes would silently supply the value under test.
        let probed = values_path("isr.ttl_secs", MOVED);
        let plan = planned(
            &keys(),
            &[(probed.clone(), json!(99))],
            &[],
            MOVED,
            &BTreeMap::new(),
        );
        assert_eq!(
            case_for(&plan, "isr.ttl_secs")
                .set_values
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [probed.as_str()]
        );
    }

    // -- render prerequisites ----------------------------------------------------------------

    fn prerequisites() -> Vec<(String, Json)> {
        vec![(
            "bucket.entries".to_owned(),
            json!({"link": {"bucket": "b", "object": "o"}}),
        )]
    }

    #[test]
    fn every_case_carries_the_prerequisites_and_no_case_drops_them() {
        // Unlike a baseline: a case missing what the chart's guard insists on does not render, and
        // a case that does not render proves nothing either.
        let held = prerequisites();
        for case in planned(&keys(), &[], &held, VALUES_ROOT, &BTreeMap::new()).cases {
            assert_eq!(case.set_values.first(), Some(&held[0]), "{}", case.path);
        }

        let collides = vec![(values_path("isr.ttl_secs", VALUES_ROOT), json!(99))];
        let held = vec![("server.replicas".to_owned(), json!(2))];
        for case in planned(&keys(), &collides, &held, VALUES_ROOT, &BTreeMap::new()).cases {
            assert!(case.set_values.contains(&held[0]), "{}", case.path);
        }
    }

    #[test]
    fn a_prerequisite_inside_the_probed_tree_is_refused_by_the_model() {
        // The guarantee, asserted where a caller reaching past the loader cannot reach past it.
        let all = keys();
        let held: Vec<&Merged> = all.iter().collect();
        for path in [
            VALUES_ROOT.to_owned(),
            format!("{VALUES_ROOT}.isr.ttl_secs"),
        ] {
            let failure = plan(
                &held,
                &[],
                &[(path.clone(), json!(1))],
                VALUES_ROOT,
                &BTreeMap::new(),
            )
            .expect_err("a prerequisite inside the probes is refused");
            assert!(failure.to_string().contains(VALUES_ROOT), "{failure}");
        }
    }

    #[test]
    fn the_refusal_follows_the_probe_root_rather_than_the_default_tree() {
        // The one place the two enrolment fields actually meet. On a chart probing a per-service
        // layer, `config` is the *lowest*-precedence one and a prerequisite there cannot reach a
        // probed key. A rule spelled against the default would refuse the harmless path and admit
        // the dangerous one, which is exactly backwards.
        assert!(prerequisite_conflict(VALUES_ROOT, MOVED).is_none());
        assert!(prerequisite_conflict(&format!("{VALUES_ROOT}.isr.ttl_secs"), MOVED).is_none());
        assert!(prerequisite_conflict(MOVED, MOVED).is_some());
        assert!(prerequisite_conflict(&format!("{MOVED}.isr.ttl_secs"), MOVED).is_some());

        // The refusal is on the tree, not on the six letters: `configMount` is another value.
        assert!(prerequisite_conflict("configMount.rolloutOnChange", VALUES_ROOT).is_none());
        assert!(prerequisite_conflict("", VALUES_ROOT).is_some());
    }

    #[test]
    fn a_prerequisite_the_probe_root_nests_inside_is_refused() {
        // Two entries of one `set` mapping, one enclosing the other, and no order between them.
        let conflict = prerequisite_conflict("services.api", MOVED).expect("a conflict");
        assert!(conflict.contains("no order"), "{conflict}");
        assert!(prerequisite_conflict("services.worker.replicaCount", MOVED).is_none());
    }

    // -- rendering ---------------------------------------------------------------------------

    fn target() -> Target {
        Target {
            chart: "portfolio".to_owned(),
            name: "server".to_owned(),
            kind: "ConfigMap".to_owned(),
            selector: vec![label("app.kubernetes.io/instance", "portfolio")],
            key: "config.toml".to_owned(),
            declaration: "charts/portfolio/config-contract.yaml".to_owned(),
            contracts: vec!["charts/portfolio/contracts/server.json".to_owned()],
            discriminator: Vec::new(),
            root: VALUES_ROOT.to_owned(),
        }
    }

    /// The other shape a chart can have: documents sharing one key, probed through the layer that
    /// outranks the configuration tree rather than through the tree itself.
    fn shared_target() -> Target {
        Target {
            chart: "tankovault".to_owned(),
            name: "api".to_owned(),
            kind: "ConfigMap".to_owned(),
            selector: vec![label("app.kubernetes.io/component", "api")],
            key: "config.toml".to_owned(),
            declaration: "charts/tankovault/config-contract.yaml".to_owned(),
            contracts: vec!["charts/tankovault/contracts/api.json".to_owned()],
            discriminator: vec![label("app.kubernetes.io/component", "api")],
            root: MOVED.to_owned(),
        }
    }

    fn render(
        target: &Target,
        plan: &Plan,
        baseline: &[(String, Json)],
        reason: Option<&str>,
    ) -> String {
        render_suite(target, plan, baseline, reason, &[], None, &[])
            .expect("the suite is renderable")
    }

    fn parsed(text: &str) -> Json {
        let held: serde_norway::Value =
            serde_norway::from_str(text).expect("the suite is valid YAML");
        serde_json::to_value(held).expect("YAML converts to JSON")
    }

    #[test]
    fn the_suite_is_valid_yaml_shaped_like_a_test_suite() {
        let suite = parsed(&render(&target(), &simple(&keys()), &[], None));
        assert_eq!(suite["release"]["name"], json!("portfolio"));
        let tests = suite["tests"].as_array().expect("a list of cases");
        assert_eq!(tests.len(), 3);
        assert!(
            tests[0]["it"]
                .as_str()
                .expect("a title")
                .starts_with("renders"),
            "{}",
            tests[0]["it"]
        );
    }

    #[test]
    fn the_pattern_survives_yaml_with_its_backslashes_intact() {
        // A double-quoted scalar would consume them as YAML escapes before the regex engine saw
        // them, and every pattern here is dense with backslashes.
        let suite = parsed(&render(&target(), &simple(&keys()), &[], None));
        let held = suite["tests"]
            .as_array()
            .expect("a list")
            .iter()
            .find(|case| {
                case["it"]
                    .as_str()
                    .is_some_and(|it| it.contains("isr.ttl_secs"))
            })
            .expect("a case for the integer key");
        assert_eq!(
            held["asserts"][0]["matchRegex"]["pattern"],
            json!(
                document_pattern("isr.ttl_secs", &DISTINCTIVE_INTEGER.to_string())
                    .expect("a spellable path")
            )
        );
    }

    #[test]
    fn the_header_says_the_file_is_generated_and_names_its_sources() {
        let rendered = render(&target(), &simple(&keys()), &[], None);
        assert!(rendered.starts_with("# Generated by"), "{rendered}");
        assert!(rendered.contains("charts/portfolio/contracts/server.json"));
        assert!(rendered.contains("charts/portfolio/config-contract.yaml"));
    }

    #[test]
    fn every_skipped_key_is_named_in_the_file_with_its_reason() {
        // An unexplained absence is indistinguishable from an oversight, which for a file whose
        // whole job is to be exhaustive is the worst possible failure mode.
        let rendered = render(&target(), &simple(&keys()), &[], None);
        assert!(rendered.contains("database.password:"), "{rendered}");
        assert!(rendered.contains("credential"), "{rendered}");
    }

    #[test]
    fn nothing_that_churns_on_its_own_is_written_into_the_file() {
        // A digest, a timestamp or an application version would put a diff here on every bump, and
        // the staleness gate is a comparison of bytes.
        let baseline = vec![("config.a.b".to_owned(), json!(false))];
        let plan = planned(&keys(), &baseline, &[], VALUES_ROOT, &BTreeMap::new());
        let first = render(&target(), &plan, &baseline, Some("because"));
        assert_eq!(first, render(&target(), &plan, &baseline, Some("because")));
        assert!(!first.contains("sha256"), "{first}");
        assert!(
            !regex::Regex::new(r"\d{4}-\d{2}-\d{2}T")
                .expect("it compiles")
                .is_match(&first),
            "{first}"
        );
    }

    #[test]
    fn every_case_of_a_shared_key_selects_through_the_same_filter() {
        // A case reading a sibling's document would assert a key against the wrong file.
        let target = shared_target();
        let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
        let suite = parsed(&render(&target, &plan, &[], Some("because")));
        let expected = selector_path(&target.key, &target.discriminator).expect("a spellable key");
        for case in suite["tests"].as_array().expect("a list") {
            assert_eq!(
                case["documentSelector"],
                json!({"path": expected}),
                "{case}"
            );
        }
    }

    #[test]
    fn a_shared_key_still_asserts_its_document_against_the_declaration() {
        let target = shared_target();
        let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
        let suite = parsed(&render(&target, &plan, &[], Some("because")));
        assert_eq!(
            suite["tests"][0]["asserts"],
            json!([
                {"isKind": {"of": "ConfigMap"}},
                {"equal": {
                    "path": "metadata.labels[\"app.kubernetes.io/component\"]",
                    "value": "api",
                }},
            ])
        );
    }

    #[test]
    fn a_moved_probe_root_reaches_the_header_the_set_paths_and_the_note() {
        let target = shared_target();
        let plan = planned(&keys(), &[], &[], &target.root, &BTreeMap::new());
        let rendered = render(&target, &plan, &[], Some("the derived wiring outranks it"));
        assert!(
            rendered.contains("`services.api.config` where no value is bound"),
            "{rendered}"
        );
        assert!(
            rendered.contains("services.api.config.isr.ttl_secs:"),
            "{rendered}"
        );
        assert!(rendered.contains("rather than into `config`"), "{rendered}");
        assert!(
            rendered.contains("the derived wiring outranks it"),
            "{rendered}"
        );
    }

    #[test]
    fn a_document_with_neither_a_baseline_nor_a_moved_root_carries_no_note() {
        // The reason belongs to what the enrolment changed; with nothing changed it is noise.
        let rendered = render(&target(), &simple(&keys()), &[], None);
        assert!(
            !rendered.contains("Every case below also carries"),
            "{rendered}"
        );
        assert!(!rendered.contains("rather than into"), "{rendered}");
    }

    #[test]
    fn the_case_title_names_the_value_the_probe_went_through() {
        // The two kinds of case prove different things and a reader scanning the suite has no
        // other way to tell them apart.
        let plan = planned(&keys(), &[], &[], VALUES_ROOT, &routes());
        let rendered = render(&target(), &plan, &[], None);
        assert!(
            rendered.contains("delivers isr.ttl_secs into config.toml from isr.ttlSecs"),
            "{rendered}"
        );
        assert!(
            rendered
                .contains("2 of 3 contract keys carry a probe, 2 through the chart's own value"),
            "{rendered}"
        );
    }

    #[test]
    fn an_unrouted_key_is_explained_in_the_suite() {
        let unrouted = vec![(
            "isr.ttl_secs".to_owned(),
            "the chart refuses a value this probe could take".to_owned(),
        )];
        let rendered = render_suite(&target(), &simple(&keys()), &[], None, &[], None, &unrouted)
            .expect("the suite is renderable");
        assert!(rendered.contains("isr.ttl_secs:"), "{rendered}");
        assert!(
            rendered.contains("the chart refuses a value this probe could take"),
            "{rendered}"
        );
    }

    #[test]
    fn a_structured_prerequisite_survives_the_suite_as_the_value_it_was() {
        // A map of maps keyed by a request path, and a flattened one would set the wrong thing.
        let held = prerequisites();
        let plan = planned(&keys(), &[], &held, VALUES_ROOT, &BTreeMap::new());
        let rendered = render_suite(
            &target(),
            &plan,
            &[],
            None,
            &held,
            Some("because the guard insists"),
            &[],
        )
        .expect("the suite is renderable");
        let suite = parsed(&rendered);
        for case in suite["tests"].as_array().expect("a list") {
            assert_eq!(case["set"]["bucket.entries"], held[0].1, "{case}");
        }
        assert!(rendered.contains("because the guard insists"), "{rendered}");
        assert!(rendered.contains("render prerequisites"), "{rendered}");
    }

    #[test]
    fn the_identity_case_carries_the_prerequisites_too() {
        // It asserts on a document the guard would otherwise have refused to produce.
        let held = prerequisites();
        let plan = planned(&[], &[], &held, VALUES_ROOT, &BTreeMap::new());
        let suite = parsed(
            &render_suite(
                &target(),
                &plan,
                &[],
                None,
                &held,
                Some("because the guard insists"),
                &[],
            )
            .expect("the suite is renderable"),
        );
        let tests = suite["tests"].as_array().expect("a list");
        assert_eq!(tests.len(), 1);
        assert!(!tests[0]["set"]["bucket.entries"].is_null(), "{}", tests[0]);
    }

    #[test]
    fn a_key_a_real_contract_declares_is_probed_the_same_way_a_hand_built_one_is() {
        // The hand-built keys above carry exactly the fields a test needs. This runs the same walk
        // over a merged contract, so a field the merge adds or renames cannot go unnoticed.
        let contract: Json = serde_json::from_str(include_str!("../../tests/fixtures/worker.json"))
            .expect("a fixture contract");
        let union: Union =
            union_contracts(&[("worker".to_owned(), contract)]).expect("it merges with itself");
        let held: &Ordered = &union.keys;
        let probed = held
            .iter()
            .filter(|key| probe_for(key, None).is_ok_and(|held| held.is_ok()))
            .count();
        assert!(probed > 0, "no key of a real contract carries a probe");
    }
}
