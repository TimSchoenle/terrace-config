//! A chart's YAML files as the lines each mapping value occupies, for a writer rather than a reader.
//!
//! [`super::markers`] answers what a value *says*: its dotted path and the markers in its `@schema`
//! block. A writer inserting a new sibling needs the other fact, where the value *ends*, so that
//! the new block lands after it and nowhere else. That is what a [`Region`] is.
//!
//! # Why this is a line walk and not the event parser
//!
//! The marker reader takes a value's path from `saphyr`'s events, which is right for a reader: a
//! `port: 1` inside a block scalar is one scalar event and never a key. A writer has a different
//! obligation. Every line number it reports is a line it will splice into, so it has to agree, byte
//! for byte, with the indentation walk the chart files are maintained against about *which* lines
//! belong to a value. The walk here is that one, with the same three patterns and the same three
//! blind spots: a sequence item's interior, a block scalar's body and a key the key pattern cannot
//! spell. None of the three holds a values path a marker may be written against, so none is
//! reported; all three are counted as content, so the value they belong to ends after them rather
//! than before.
//!
//! One departure from that walk: inside a block scalar's body or a sequence item's interior,
//! *every* line is content, a line that looks like a comment or like a `# @schema` delimiter
//! included. A walk that checks for those two before it checks whether it is inside such a body
//! ends a `|` scalar with a `# ...` line in its text early (and lets one with a `# @schema` line
//! swallow every line after it), and a writer then inserts into the middle of the text.
//!
//! # Lines are counted the way the files were always counted
//!
//! [`split_lines`] is a port of Python's `str.splitlines`, not of [`str::lines`]. The two disagree
//! on a lone `\r` and on a handful of separators nobody types on purpose, and a writer whose line
//! numbers drifted by one from the planner that chose them would insert a block inside the value
//! above it.

use regex::Regex;

use super::markers::split_comment;

/// One mapping value in a YAML file, as the lines it occupies.
///
/// `end` is the last line of content under the key, and deliberately not the last line before the
/// next sibling: the comment run between two values belongs to the second one, and an insertion
/// that landed inside it would separate a value from its `@schema` block, the one placement the
/// marker reader refuses to read. A leaf's `end` is its own line.
///
/// Lines are 1-based, as they are in every message this crate prints, so a line `n` is the
/// `n - 1`th entry of [`split_lines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// The dotted path of the value.
    pub values_path: String,
    /// The line its key is on.
    pub line: usize,
    /// The key's indentation, in spaces.
    pub indent: usize,
    /// The last line of content belonging to it.
    pub end: usize,
    /// Whether the key stands alone on its line, comment aside: `auth:` rather than `auth: {}`,
    /// `auth: null` or `auth: |`. Only such a key can take a block child: a line under `auth: {}`
    /// is a second value for one key, which no YAML parser accepts.
    pub bare: bool,
}

/// Every mapping value in one YAML file's text, with the lines it spans, in file order.
///
/// A key written twice is reported twice, which is what the file says; a caller keying regions by
/// path decides which of the two it means.
pub fn regions(text: &str) -> Vec<Region> {
    // Every line that is neither blank nor comment-only, as `(number, indent)`. A value ends at the
    // last of these below it and more indented than it.
    let mut content: Vec<(usize, usize)> = Vec::new();
    // `(line, indent, values_path, bare)` for each mapping key, closed in the second pass.
    let mut keys: Vec<(usize, usize, String, bool)> = Vec::new();

    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut skip_deeper_than: Option<usize> = None;
    let mut in_schema = false;

    for (offset, raw) in split_lines(text, false).into_iter().enumerate() {
        let number = offset + 1;
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();

        // Inside a block scalar's body or a sequence item's interior every line is content,
        // whatever it looks like: `# not a comment` in a `|` scalar is text, and so is a line that
        // reads `# @schema`. Counting it before the comment and delimiter checks is what keeps the
        // value above from ending early, and a value that ended early would take a new sibling in
        // the middle of its own text, truncating it.
        if skip_deeper_than.is_some_and(|limit| indent > limit) {
            content.push((number, indent));
            continue;
        }

        if patterns().delimiter.is_match(line) {
            in_schema = !in_schema;
            continue;
        }
        if in_schema {
            continue;
        }

        let (code, _) = split_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        content.push((number, indent));

        if let Some(limit) = skip_deeper_than {
            if indent > limit {
                continue;
            }
            skip_deeper_than = None;
        }

        if patterns().sequence_item.is_match(&code) {
            skip_deeper_than = Some(indent);
            continue;
        }

        let Some(key) = patterns().mapping_key.captures(code.trim_end()) else {
            continue;
        };
        while stack.last().is_some_and(|(held, _)| *held >= indent) {
            stack.pop();
        }
        stack.push((indent, key["name"].to_owned()));

        if patterns().block_scalar.is_match(key["rest"].trim()) {
            skip_deeper_than = Some(indent);
        }

        let dotted = stack
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(".");
        keys.push((number, indent, dotted, key["rest"].trim().is_empty()));
    }

    keys.into_iter()
        .map(|(line, indent, values_path, bare)| {
            let first_below = content.partition_point(|(number, _)| *number <= line);
            let end = content[first_below..]
                .iter()
                .take_while(|(_, other)| *other > indent)
                .last()
                .map_or(line, |(number, _)| *number);
            Region {
                values_path,
                line,
                indent,
                end,
                bare,
            }
        })
        .collect()
}

/// The patterns the walk reads a line with, compiled once.
struct Patterns {
    /// A `# @schema` delimiter, alone on its line.
    delimiter: Regex,
    /// A mapping key the walk can name. A quoted key, or one carrying a `/`, is content and not a
    /// path, exactly as the marker gate treats it.
    mapping_key: Regex,
    /// A sequence item, whose interior holds no values path.
    sequence_item: Regex,
    /// A block scalar's header, whose body holds no values path either.
    block_scalar: Regex,
}

fn patterns() -> &'static Patterns {
    static HELD: std::sync::OnceLock<Patterns> = std::sync::OnceLock::new();
    HELD.get_or_init(|| Patterns {
        delimiter: Regex::new(r"^ *# @schema *$").expect("a literal pattern compiles"),
        mapping_key: Regex::new(
            r"^(?P<indent> *)(?P<name>[A-Za-z0-9_][A-Za-z0-9_.\-]*):(?P<rest> .*|)$",
        )
        .expect("a literal pattern compiles"),
        sequence_item: Regex::new(r"^ *-( .*|)$").expect("a literal pattern compiles"),
        block_scalar: Regex::new(r"^[|>][-+0-9]*$").expect("a literal pattern compiles"),
    })
}

/// The characters Python's `str.splitlines` breaks a line at, besides `\r\n` taken as one.
const BREAKS: [char; 10] = [
    '\n', '\r', '\u{0b}', '\u{0c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// A text's lines, split where Python's `str.splitlines` splits them.
///
/// `keep_ends` keeps each line's terminator on it, which is what a writer splicing lines back
/// together needs so that a CRLF file stays CRLF; without it the terminators are dropped. A final
/// line with no terminator is still a line, and an empty text has none.
pub fn split_lines(text: &str, keep_ends: bool) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if !BREAKS.contains(&character) {
            continue;
        }
        let mut after = index + character.len_utf8();
        if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
            characters.next();
            after += 1;
        }
        lines.push(if keep_ends {
            &text[start..after]
        } else {
            &text[start..index]
        });
        start = after;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{Region, regions, split_lines};

    fn region<'a>(found: &'a [Region], path: &str) -> &'a Region {
        found
            .iter()
            .find(|region| region.values_path == path)
            .unwrap_or_else(|| panic!("no region for {path}: {found:?}"))
    }

    #[test]
    fn a_value_ends_at_its_last_line_of_content_not_at_the_next_sibling() {
        let found = regions(
            "auth:\n  # @schema\n  # type: integer\n  # @schema\n  sessionTtl: 3600\n\n\
             # @schema\n# type: object\n# @schema\n# -- Limits.\nresources: {}\n",
        );
        assert_eq!(
            *region(&found, "auth"),
            Region {
                values_path: "auth".to_owned(),
                line: 1,
                indent: 0,
                end: 5,
                bare: true,
            }
        );
        assert_eq!(region(&found, "auth.sessionTtl").end, 5);
        assert_eq!(region(&found, "auth.sessionTtl").indent, 2);
        assert_eq!(region(&found, "resources").line, 11);
        assert_eq!(region(&found, "resources").end, 11);
    }

    #[test]
    fn a_sequence_and_a_block_scalar_are_content_and_never_paths() {
        let found =
            regions("list:\n  - name: a\n    port: 1\nliteral: |\n  not: a key\nafter: 1\n");
        let paths: Vec<&str> = found.iter().map(|item| item.values_path.as_str()).collect();
        assert_eq!(paths, ["list", "literal", "after"]);
        assert_eq!(region(&found, "list").end, 3);
        assert_eq!(region(&found, "literal").end, 5);
    }

    #[test]
    fn a_key_the_pattern_cannot_spell_is_content_of_the_value_above_it() {
        let found = regions("labels:\n  app.kubernetes.io/name: x\nnext: 1\n");
        assert_eq!(region(&found, "labels").end, 2);
        assert!(found.iter().all(|item| !item.values_path.contains('/')));
    }

    #[test]
    fn a_block_scalars_body_is_content_whatever_it_looks_like() {
        // A `#` line and a `# @schema` line inside a `|` scalar are text. Read as a comment and a
        // delimiter they ended `script` early, and a new sibling went in mid-scalar.
        let found = regions(
            "outer:\n  script: |\n    echo one\n    # not a comment\n    # @schema\n    \
             echo two\n  after: 1\n",
        );
        assert_eq!(region(&found, "outer.script").end, 6);
        assert_eq!(region(&found, "outer.after").line, 7);
        assert_eq!(region(&found, "outer").end, 7);
    }

    #[test]
    fn only_a_key_alone_on_its_line_is_bare() {
        let found = regions("a:\n  x: 1\nb: {}\nc: null\nd: # a comment\ne: |\n  text\nf: []\n");
        let bare: Vec<(&str, bool)> = found
            .iter()
            .map(|item| (item.values_path.as_str(), item.bare))
            .collect();
        assert_eq!(
            bare,
            [
                ("a", true),
                ("a.x", false),
                ("b", false),
                ("c", false),
                ("d", true),
                ("e", false),
                ("f", false)
            ]
        );
    }

    #[test]
    fn a_hash_inside_a_quoted_scalar_is_not_a_comment() {
        let found = regions("colour: \"a\\\" #b\"\nnext: 1\n");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn lines_split_where_python_splits_them() {
        assert_eq!(split_lines("a\r\nb\rc\nd", false), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\r\nb\n", true), ["a\r\n", "b\n"]);
        assert_eq!(split_lines("a\u{2028}b", false), ["a", "b"]);
        assert_eq!(split_lines("", false), [""; 0]);
        assert_eq!(split_lines("\n", false), [""]);
    }
}
