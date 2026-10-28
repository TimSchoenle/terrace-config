//! Spelling the assertion: the regular expression that finds a probe in the rendered document, and
//! the `JSONPath` that selects the document.

use regex::Regex;
use serde_json::Value as Json;

use crate::error::Error;

/// Characters that are metacharacters in both this engine and Go's RE2.
pub(super) const METACHARACTERS: &str = r"\.+*?()|[]{}^$";

/// One or more whole lines that do not open a new table: either a line whose first character is not
/// `[`, or an empty one.
///
/// This is what separates a table header from the key underneath it without running past the next
/// header, and it is an alternation because RE2 has no lookahead.
pub(super) const GAP: &str = r"(?:[^\[\n][^\n]*\n|\n)*";

/// Runs of anything that is not a letter or a digit, for the probe stem.
pub(super) fn non_alphanumeric() -> &'static Regex {
    static HELD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    HELD.get_or_init(|| Regex::new("[^A-Za-z0-9]+").expect("a literal pattern compiles"))
}

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
