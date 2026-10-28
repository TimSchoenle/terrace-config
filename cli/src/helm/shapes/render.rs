//! Writing a schema back out as the YAML lines of a shape block.

use serde_json::{Map, Value as Json};

use super::ORDER;
use super::divergences::Added;
use super::expected::scalar_name;

/// One `@schema` mapping as the comment lines a values file carries it as.
pub fn render(schema: &Json, indent: usize, added: &Added) -> Vec<String> {
    let padding = " ".repeat(indent);
    lines_of(schema, "", added)
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                format!("{padding}#")
            } else {
                format!("{padding}# {line}")
            }
        })
        .collect()
}

/// The schema as YAML, in [`ORDER`], block form throughout.
///
/// Block form rather than flow, because that is what a hand-written block uses and these lines are
/// read by people first. Written here rather than handed to a YAML emitter, which would sort keys
/// alphabetically, wrap mid-value and quote strings on rules of its own: three things that would
/// each show up as churn in a generated file.
pub(super) fn lines_of(schema: &Json, at: &str, added: &Added) -> Vec<String> {
    let Some(fields) = schema.as_object() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for name in ordered(fields) {
        let below = if at.is_empty() {
            name.clone()
        } else {
            format!("{at}.{name}")
        };
        lines.extend(keyword_lines(&name, &fields[&name], &below, at, added));
    }
    lines
}

/// The keywords of one level, canonical ones first and the rest alphabetically after.
pub(super) fn ordered(fields: &Map<String, Json>) -> Vec<String> {
    let mut known: Vec<String> = ORDER
        .iter()
        .filter(|name| fields.contains_key(**name))
        .map(|name| (*name).to_owned())
        .collect();
    let mut rest: Vec<String> = fields
        .keys()
        .filter(|name| !ORDER.contains(&name.as_str()))
        .cloned()
        .collect();
    rest.sort();
    known.extend(rest);
    known
}

/// One JSON Schema keyword as the YAML lines an `@schema` comment carries.
pub(super) fn keyword_lines(
    name: &str,
    value: &Json,
    below: &str,
    at: &str,
    added: &Added,
) -> Vec<String> {
    if name == "type" {
        return vec![type_line(value)];
    }
    if name == "properties"
        && let Some(fields) = value.as_object().filter(|fields| !fields.is_empty())
    {
        // Field names, not keywords. Kept in the order the producer declared them, because that is
        // the order the type is written in and the order its documentation reads in, and sorting
        // them would make a twenty-field element unrecognisable against the source it came from.
        let mut lines = vec!["properties:".to_owned()];
        let mut names: Vec<&String> = fields.keys().collect();
        // The producer's fields in the order the document gives them back, then whatever a
        // narrowing added, in the order the markers were written.
        names.sort_by_key(|field| usize::from(added.holds(at, field)));
        for field in names {
            let subschema = &fields[field];
            // Rendered here rather than through this function, which reads its first argument
            // as a schema keyword: a struct with a field called `type` or `items` would
            // otherwise be spelt as the keyword of that name and come out as something else.
            match subschema.as_object().filter(|held| !held.is_empty()) {
                Some(_) => {
                    lines.push(format!("  {}:", field_name(field)));
                    lines.extend(
                        lines_of(subschema, &format!("{below}.{field}"), added)
                            .into_iter()
                            .map(|line| format!("    {line}")),
                    );
                }
                None => lines.push(format!("  {}: {{}}", field_name(field))),
            }
        }
        return lines;
    }
    match value {
        Json::Object(fields) if fields.is_empty() => vec![format!("{name}: {{}}")],
        Json::Object(_) => {
            let mut lines = vec![format!("{name}:")];
            lines.extend(
                lines_of(value, below, added)
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
            lines
        }
        Json::Array(items) if items.iter().any(|item| item.is_object() || item.is_array()) => {
            let mut lines = vec![format!("{name}:")];
            for item in items {
                if item.is_object() {
                    let rendered = lines_of(item, below, added);
                    let mut held = rendered.into_iter();
                    if let Some(first) = held.next() {
                        lines.push(format!("  - {first}"));
                        lines.extend(held.map(|line| format!("    {line}")));
                    }
                } else {
                    lines.push(format!("  - {}", scalar(item)));
                }
            }
            lines
        }
        other => vec![format!("{name}: {}", scalar(other))],
    }
}

/// Field names YAML 1.1 reads as something other than a string.
///
/// `yes`, `no`, `on` and `off` are the ones that bite: a struct field called `on` is an ordinary
/// field name in every language a producer is written in, and unquoted it is the boolean true.
pub(super) const YAML_WORDS: [&str; 11] = [
    "y", "n", "yes", "no", "on", "off", "true", "false", "null", "none", "~",
];

/// One struct field name, as the mapping key an `@schema` block carries it as.
pub(super) fn field_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|held| held.is_ascii_alphabetic() || held == '_')
        && name
            .chars()
            .all(|held| held.is_ascii_alphanumeric() || matches!(held, '_' | '.' | '-'));
    if plain && !YAML_WORDS.contains(&name.to_lowercase().as_str()) {
        name.to_owned()
    } else {
        quoted(name)
    }
}

/// The `type` keyword, whose members are schema vocabulary rather than data.
///
/// Written bare for that reason. `null` is the one member that has to be quoted: unquoted it is
/// YAML's null, and the schema would then declare no type at all where it meant the null type.
pub(super) fn type_line(value: &Json) -> String {
    let names: Vec<String> = match value {
        Json::Array(items) => items.iter().map(scalar_name).collect(),
        single => vec![scalar_name(single)],
    };
    let rendered: Vec<String> = names
        .iter()
        .map(|name| {
            if name == "null" {
                "'null'".to_owned()
            } else {
                name.clone()
            }
        })
        .collect();
    if names.len() == 1 {
        format!("type: {}", rendered[0])
    } else {
        format!("type: [{}]", rendered.join(", "))
    }
}

/// One value, as the YAML a values file or an `@schema` comment carries.
///
/// `null` is written bare, which is the spelling a hand-written block already uses; the `type`
/// keyword is where it has to be quoted, and that is [`type_line`]'s business.
///
/// Shared with the scaffold rather than written twice: both spell a scalar into the same file under
/// the same rules, and two copies would be two chances to disagree about how a string is escaped.
pub(in crate::helm) fn scalar(value: &Json) -> String {
    match value {
        Json::Bool(true) => "true".to_owned(),
        Json::Bool(false) => "false".to_owned(),
        Json::Null => "null".to_owned(),
        Json::Number(number) => number.to_string(),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(scalar).collect::<Vec<_>>().join(", ")
        ),
        Json::String(text) => quoted(text),
        Json::Object(_) => "{}".to_owned(),
    }
}

/// A YAML double-quoted scalar. Used wherever a value could be read as something else.
///
/// Every character that would break the line is escaped, not only the quote and the backslash. The
/// scalar is written onto one line of a values file or of an `@schema` comment, and a raw newline
/// inside it would end that line: in a comment the rest of the default becomes a line of YAML, and
/// in a value it becomes a folded scalar that no longer says what the contract said. `\n`, `\r`
/// and `\t` keep their short escapes, YAML's own line and paragraph separators (U+0085, U+2028,
/// U+2029) and every other control character are written as `\u` escapes, which every YAML 1.2
/// reader decodes back to the same text.
///
/// This departs from the script it replaces, which escaped the quote and the backslash alone.
pub(in crate::helm) fn quoted(text: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // `is_control` covers U+0085 with the rest of C1; the two separators are format
            // characters to Unicode and line breaks to YAML, so they are named here.
            breaking if breaking.is_control() || matches!(breaking, '\u{2028}' | '\u{2029}') => {
                let _ = write!(out, "\\u{:04X}", u32::from(breaking));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
