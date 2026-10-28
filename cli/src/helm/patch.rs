//! Writing a planned edit as a unified diff.
//!
//! Two writers here show what they would change before changing it: `adopt`, which inserts runs of
//! lines into files that exist, and `helm init`, which creates files and fills placeholders in
//! others. Neither diffs two texts to find out what changed: each knows its edit exactly, so each
//! lays it out as rows itself and this turns the rows into hunks. A general text diff could only
//! rediscover the alignment the writer already had or, where a new blank line matches an old one,
//! choose a different but equally valid one, and print a diff that is not the edit being made.
//!
//! The output is what `patch -p0` and `git apply` read: three lines of context, changes closer than
//! twice that sharing a hunk, a count left off when it is one, and the missing-newline marker on a
//! last line that had none.

use std::fmt::Write as _;

/// The lines of context around each change.
const CONTEXT: usize = 3;

/// One line of a diff, holding its text as the file has it, terminator included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    /// A line both sides have.
    Context(String),
    /// A line only the old side has.
    Removed(String),
    /// A line only the new side has.
    Added(String),
}

impl Row {
    const fn in_old(&self) -> bool {
        matches!(self, Self::Context(_) | Self::Removed(_))
    }

    const fn in_new(&self) -> bool {
        matches!(self, Self::Context(_) | Self::Added(_))
    }

    fn write(&self, out: &mut String) {
        let (marker, text) = match self {
            Self::Context(text) => (' ', text),
            Self::Removed(text) => ('-', text),
            Self::Added(text) => ('+', text),
        };
        out.push(marker);
        // Only the `\n` goes: a CRLF file's `\r` is part of the line as that file has it, and a
        // patch that dropped it would not apply to the file it was made from.
        out.push_str(text.strip_suffix('\n').unwrap_or(text));
        out.push('\n');
        if !text.ends_with('\n') {
            out.push_str("\\ No newline at end of file\n");
        }
    }
}

/// The rows as a unified diff from `old` to `new`, or the empty string when nothing changes.
///
/// `old` and `new` are the names on the two header lines: the file's own path on both sides for an
/// edit, `/dev/null` on the old side for a file that does not exist yet.
pub(crate) fn unified(old: &str, new: &str, rows: &[Row]) -> String {
    // Changes closer than twice the context share a hunk, as every unified diff joins them.
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if matches!(row, Row::Context(_)) {
            continue;
        }
        match hunks.last_mut() {
            Some(last) if index - last.1 <= 2 * CONTEXT + 1 => last.1 = index,
            _ => hunks.push((index, index)),
        }
    }
    if hunks.is_empty() {
        return String::new();
    }

    let mut out = format!("--- {old}\n+++ {new}\n");
    for (first, last) in hunks {
        let low = first.saturating_sub(CONTEXT);
        let high = (last + CONTEXT + 1).min(rows.len());
        let before = &rows[..low];
        let old_start = before.iter().filter(|row| row.in_old()).count();
        let new_start = before.iter().filter(|row| row.in_new()).count();
        let hunk = &rows[low..high];
        let old_len = hunk.iter().filter(|row| row.in_old()).count();
        let new_len = hunk.iter().filter(|row| row.in_new()).count();
        let _ = writeln!(
            out,
            "@@ -{} +{} @@",
            range(old_start, old_len),
            range(new_start, new_len)
        );
        for row in hunk {
            row.write(&mut out);
        }
    }
    out
}

/// The rows of a file that does not exist yet: every line added.
#[cfg(feature = "oci")]
pub(crate) fn created(text: &str) -> Vec<Row> {
    text.split_inclusive('\n')
        .map(|line| Row::Added(line.to_owned()))
        .collect()
}

/// The rows of an edit that replaced text within lines and moved none.
///
/// `before` and `after` must have the same lines in the same places (the caller's edit changes what
/// is on a line, never how many there are), so the alignment is the line number. A run of changed
/// lines is written as every removed line and then every added one, the way a reader of any other
/// diff expects to see a block that changed.
///
/// A caller handing two texts of different lengths gets the surplus as added or removed rows, which
/// is a wrong-looking diff rather than a lost one.
#[cfg(feature = "oci")]
pub(crate) fn replaced(before: &str, after: &str) -> Vec<Row> {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let mut rows = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |rows: &mut Vec<Row>, removed: &mut Vec<Row>, added: &mut Vec<Row>| {
        rows.append(removed);
        rows.append(added);
    };
    for index in 0..old.len().max(new.len()) {
        match (old.get(index), new.get(index)) {
            (Some(was), Some(now)) if was == now => {
                flush(&mut rows, &mut removed, &mut added);
                rows.push(Row::Context((*was).to_owned()));
            }
            (was, now) => {
                removed.extend(was.map(|held| Row::Removed((*held).to_owned())));
                added.extend(now.map(|held| Row::Added((*held).to_owned())));
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

/// One side of a hunk header: the first line and the count, the count left off when it is one and
/// the start being the line before when it is zero, as every unified diff writes it.
fn range(before: usize, length: usize) -> String {
    match length {
        0 => format!("{before},0"),
        1 => format!("{}", before + 1),
        _ => format!("{},{length}", before + 1),
    }
}

#[cfg(all(test, feature = "oci"))]
mod tests {
    use super::{Row, created, replaced, unified};

    #[test]
    fn a_new_file_is_one_hunk_against_nothing() {
        let diff = unified("/dev/null", "a/b.yaml", &created("one\ntwo\n"));
        assert_eq!(
            diff,
            "--- /dev/null\n+++ a/b.yaml\n@@ -0,0 +1,2 @@\n+one\n+two\n"
        );
    }

    #[test]
    fn a_run_of_changed_lines_is_removed_and_then_added() {
        let rows = replaced("a\nX\nY\nb\n", "a\nx\ny\nb\n");
        assert_eq!(
            rows,
            [
                Row::Context("a\n".to_owned()),
                Row::Removed("X\n".to_owned()),
                Row::Removed("Y\n".to_owned()),
                Row::Added("x\n".to_owned()),
                Row::Added("y\n".to_owned()),
                Row::Context("b\n".to_owned()),
            ]
        );
        assert_eq!(
            unified("f", "f", &rows),
            "--- f\n+++ f\n@@ -1,4 +1,4 @@\n a\n-X\n-Y\n+x\n+y\n b\n"
        );
    }

    #[test]
    fn distant_changes_are_separate_hunks_and_nothing_changed_is_no_diff() {
        let mut lines: Vec<String> = (0..20).map(|line| line.to_string()).collect();
        let before = lines.join("\n") + "\n";
        lines[2] = "two".to_owned();
        lines[17] = "seventeen".to_owned();
        let after = lines.join("\n") + "\n";
        let diff = unified("f", "f", &replaced(&before, &after));
        assert_eq!(diff.matches("@@ -").count(), 2, "{diff}");
        assert_eq!(unified("f", "f", &replaced(&before, &before)), "");
    }

    #[test]
    fn a_last_line_without_a_terminator_is_marked() {
        let diff = unified("/dev/null", "f", &created("only"));
        assert!(
            diff.ends_with("+only\n\\ No newline at end of file\n"),
            "{diff}"
        );
    }
}
