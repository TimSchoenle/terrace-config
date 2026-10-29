//! Wrapping and clipping prose to the report's width.

use super::WIDTH;

/// Re-flow a documentation block to this width, keeping its paragraph breaks.
#[must_use]
pub fn prose(text: &str, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (index, paragraph) in text.split("\n\n").enumerate() {
        let collapsed = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if index > 0 && !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(wrap(&collapsed, WIDTH - indent.chars().count()));
    }
    lines
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect()
}

/// Greedy wrap on whitespace, never breaking a word and never re-spacing one that fits.
///
/// A spelling, a pattern or an image reference is a single token, and a token split across two
/// lines is one nobody can copy, so a value wider than the width overflows rather than being
/// broken.
///
/// Internal whitespace is carried rather than collapsed: `PathBuf  (read as text)` puts two spaces
/// between the type and its gloss on purpose, and a wrapper that normalised them would quietly
/// re-format every line it did not need to touch. Only whitespace a line break lands on is dropped.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let limit = width.max(16);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut pending = String::new();

    for chunk in chunks(text) {
        if chunk.starts_with(char::is_whitespace) {
            chunk.clone_into(&mut pending);
            continue;
        }
        let width_of = |held: &str| held.chars().count();
        if !current.is_empty() && width_of(&current) + width_of(&pending) + width_of(chunk) > limit
        {
            lines.push(std::mem::take(&mut current));
            pending.clear();
        }
        current.push_str(&std::mem::take(&mut pending));
        current.push_str(chunk);
    }
    if lines.is_empty() || !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// One text as alternating runs of whitespace and of everything else.
pub(super) fn chunks(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut at = 0;
    for (index, held) in text.char_indices() {
        let boundary =
            index > at && text[at..].starts_with(char::is_whitespace) != held.is_whitespace();
        if boundary {
            found.push(&text[at..index]);
            at = index;
        }
    }
    if at < text.len() {
        found.push(&text[at..]);
    }
    found
}

/// One value clipped to a column, with an ellipsis where it was cut.
#[must_use]
pub fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    format!("{}...", text.chars().take(width - 3).collect::<String>())
}
