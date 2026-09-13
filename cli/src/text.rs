//! Quoting a value for a message, in the two spellings the reports use.
//!
//! Small, and here rather than beside one caller because three do it. The spellings are not
//! interchangeable and neither is arbitrary: every rule that names a key path, a file name or a
//! variable uses [`quoted`], and every rule that shows a *selector* or a piece of a document uses
//! [`json_text`], because one is prose about a name and the other is a fragment somebody will
//! compare against a file.

use serde_json::Value as Json;

/// One string, quoted the way the reports name a thing.
///
/// Deliberately not [`std::fmt::Debug`]'s spelling. The corpus of expected output these messages
/// were written against uses single quotes, so keeping the spelling is what makes a difference in a
/// report a difference in a rule.
pub fn quoted(text: &str) -> String {
    // A control character is escaped rather than written through, and that is not cosmetic:
    // every finding is one line, and a key's documentation embedded verbatim in a message would
    // break that line in the middle of a report somebody is reading top to bottom.
    let escaped: String = text
        .chars()
        .map(|held| match held {
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            other => other.to_string(),
        })
        .collect();
    if escaped.contains('\'') && !escaped.contains('"') {
        format!("\"{escaped}\"")
    } else {
        format!("'{}'", escaped.replace('\'', "\\'"))
    }
}

/// One value, quoted the way JSON quotes it.
pub fn json_text(text: &str) -> String {
    Json::String(text.to_owned()).to_string()
}

/// One JSON value on one line, short enough to sit inside a sentence.
///
/// Truncated rather than wrapped: these appear mid-sentence in a report a person skims, and a
/// hundred-line constraint printed in full would bury the twelve findings around it. The full value
/// is on the change itself, for a consumer that wants to render a before and after.
pub fn short(value: Option<&Json>) -> String {
    const LIMIT: usize = 60;
    match value {
        None | Some(Json::Null) => "unset".to_owned(),
        Some(Json::String(text)) => {
            if text.chars().count() <= LIMIT {
                quoted(text)
            } else {
                let held: String = text.chars().take(LIMIT - 3).collect();
                quoted(&format!("{held}..."))
            }
        }
        Some(other) => {
            let text = compact(other);
            if text.chars().count() <= LIMIT {
                text
            } else {
                let held: String = text.chars().take(LIMIT - 3).collect();
                format!("{held}...")
            }
        }
    }
}

/// One JSON value with no whitespace, which is what a message inside a sentence wants.
fn compact(value: &Json) -> String {
    match value {
        Json::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(name, held)| format!("{}:{}", json_text(name), compact(held)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(compact).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{json_text, quoted, short};

    #[test]
    fn a_control_character_is_escaped_so_a_finding_stays_one_line() {
        assert_eq!(quoted("a\nb"), r"'a\nb'");
        assert_eq!(quoted("a\tb"), r"'a\tb'");
        assert_eq!(quoted(r"a\b"), r"'a\\b'");
    }

    #[test]
    fn a_name_is_quoted_the_way_the_reports_quote_one() {
        assert_eq!(quoted("server.port"), "'server.port'");
        assert_eq!(quoted("it's"), "\"it's\"");
        assert_eq!(json_text("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn an_absent_value_says_so_rather_than_printing_null() {
        assert_eq!(short(None), "unset");
        assert_eq!(short(Some(&json!(null))), "unset");
    }

    #[test]
    fn a_long_value_is_truncated_rather_than_wrapped() {
        let held = json!({"pattern": "x".repeat(200)});
        let rendered = short(Some(&held));
        assert!(rendered.ends_with("..."), "{rendered}");
        assert!(rendered.chars().count() <= 60, "{rendered}");
    }

    #[test]
    fn a_short_value_is_printed_whole_and_without_whitespace() {
        assert_eq!(
            short(Some(&json!({"type": "integer"}))),
            r#"{"type":"integer"}"#
        );
        assert_eq!(short(Some(&json!([1, 2]))), "[1,2]");
        assert_eq!(short(Some(&json!("x"))), "'x'");
    }
}
