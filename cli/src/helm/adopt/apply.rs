//! Turning an adoption into byte-level splices, and writing them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::Error;

use crate::helm::files;
use crate::helm::patch::{Row, unified};
use crate::helm::regions::split_lines;

use super::{Adoption, Insertion, posix, read_text};

/// One file and the runs of lines inserted into it, before anything is written.
///
/// The one representation both `--write` and `--diff` read, so the diff a reviewer is shown is the
/// edit that would be made rather than a second rendering of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The file.
    pub path: PathBuf,
    /// Its text as it is now.
    pub original: String,
    /// Each run of lines and the 1-based line it goes after, in the order they land in the file.
    pub insertions: Vec<(usize, Vec<String>)>,
}

impl Splice {
    /// The lines of the original, each with its terminator, the last one terminated if it was not.
    ///
    /// Returns whether the last line had to be terminated: a last line with no terminator would
    /// otherwise have the first inserted line appended to it.
    pub(super) fn lines(&self) -> (Vec<String>, &'static str, bool) {
        // One CRLF anywhere makes the file a CRLF file: a rewrite that normalised it would reflow
        // every line of every file it touched.
        let ending = if self.original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut lines: Vec<String> = split_lines(&self.original, true)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut terminated = false;
        if let Some(last) = lines.last_mut()
            && !last.ends_with(['\n', '\r'])
        {
            last.push_str(ending);
            terminated = true;
        }
        (lines, ending, terminated)
    }

    /// The runs inserted before each original line, by index, with the runs past the end at
    /// `lines.len()`.
    pub(super) fn runs(&self, count: usize, ending: &str) -> BTreeMap<usize, Vec<String>> {
        let mut runs: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (after, body) in &self.insertions {
            runs.entry((*after).min(count))
                .or_default()
                .extend(body.iter().map(|line| format!("{line}{ending}")));
        }
        runs
    }

    /// The text after every insertion.
    pub fn apply(&self) -> String {
        let (lines, ending, _) = self.lines();
        let mut runs = self.runs(lines.len(), ending);
        let mut out = String::new();
        for (index, line) in lines.iter().enumerate() {
            for inserted in runs.remove(&index).unwrap_or_default() {
                out.push_str(&inserted);
            }
            out.push_str(line);
        }
        for inserted in runs.into_values().flatten() {
            out.push_str(&inserted);
        }
        out
    }

    /// The change as a unified diff with three lines of context, against the file's own path.
    ///
    /// Built from the insertions rather than by comparing two texts: every edit here is an
    /// insertion, so the alignment is known exactly, and a general diff could only rediscover it —
    /// or, where a new blank line matches an old one, choose a different but equally valid one.
    pub fn diff(&self) -> String {
        let (lines, ending, terminated) = self.lines();
        let mut runs = self.runs(lines.len(), ending);
        let mut rows: Vec<Row> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            for inserted in runs.remove(&index).unwrap_or_default() {
                rows.push(Row::Added(inserted));
            }
            if terminated && index + 1 == lines.len() {
                let original = line.strip_suffix(ending).unwrap_or(line).to_owned();
                rows.push(Row::Removed(original));
                rows.push(Row::Added(line.clone()));
            } else {
                rows.push(Row::Context(line.clone()));
            }
        }
        rows.extend(runs.into_values().flatten().map(Row::Added));

        let shown = posix(&self.path);
        unified(&shown, &shown, &rows)
    }
}

/// Every file one adoption changes, in the order they are written.
///
/// `values.yaml` first, then the other files in path order, each with its runs in the order they
/// land in the file. For the declaration and the templates that is the order they were planned,
/// so two runs meeting at one line keep the order the plan gave them; for the values it is that
/// order corrected for depth, as the comment in the body explains.
///
/// # Errors
/// [`Error::Io`] when a file cannot be read.
pub fn splices(adoption: &Adoption) -> Result<Vec<Splice>, Error> {
    let mut found = Vec::new();
    if !adoption.insertions.is_empty() {
        // Two blocks can meet at one line: a new leaf at the end of a block, and a new sibling of
        // that block after it. The deeper one belongs *inside* the block that ends there, so it is
        // written first; the other way round, the leaf would land under the new sibling and become
        // its child — or, when the sibling is a scalar, turn the file into YAML that does not
        // parse. Blocks of one depth never meet at one line (one ancestor is one block), so the
        // last key only keeps the order total, laying such ties out the way the script this
        // replaces did: the later planned above the earlier.
        let mut ordered: Vec<(usize, &Insertion)> =
            adoption.insertions.iter().enumerate().collect();
        ordered.sort_by_key(|(index, insertion)| {
            (
                insertion.after,
                std::cmp::Reverse(insertion.depth),
                std::cmp::Reverse(*index),
            )
        });
        found.push(Splice {
            path: adoption.values.clone(),
            original: read_text(&adoption.values)?,
            insertions: ordered
                .into_iter()
                .map(|(_, insertion)| {
                    let mut body = Vec::new();
                    if insertion.spaced {
                        body.push(String::new());
                    }
                    body.extend(insertion.lines.iter().cloned());
                    (insertion.after, body)
                })
                .collect(),
        });
    }

    let paths: BTreeSet<&PathBuf> = adoption.edits.iter().map(|edit| &edit.path).collect();
    for path in paths {
        let mut insertions: Vec<(usize, Vec<String>)> = adoption
            .edits
            .iter()
            .filter(|edit| &edit.path == path)
            .map(|edit| (edit.after, edit.lines.clone()))
            .collect();
        insertions.sort_by_key(|(after, _)| *after);
        found.push(Splice {
            path: path.clone(),
            original: read_text(path)?,
            insertions,
        });
    }
    Ok(found)
}

/// Apply one chart's edits: the values, the declaration and the template lines they oblige.
///
/// All three, because that set is what a green chart needs: values with no write-off fail the
/// bindings gate, and values with no projection fail the round trip the same run regenerates. A
/// command that wrote one of the three would be a command whose output is never the finished state.
///
/// # Errors
/// As [`write_all`].
pub fn write(adoption: &Adoption) -> Result<(), Error> {
    write_all(std::slice::from_ref(adoption))
}

/// Apply every chart's edits, all of them or none.
///
/// Every splice of every chart is planned first, reading each file once, and the whole set is
/// handed to [`files::write_all`], which refuses a file that resolves outside its chart and
/// replaces the rest, putting back what it replaced if a later replacement fails.
///
/// # Errors
/// [`Error::Io`] when a file cannot be read, resolved or written, and [`Error::Invalid`] when one
/// resolves outside its chart. No file has changed, unless putting one back failed too, as
/// [`files::write_all`] describes.
pub fn write_all(adoptions: &[Adoption]) -> Result<(), Error> {
    let mut planned = Vec::new();
    for adoption in adoptions.iter().filter(|item| item.writes()) {
        let chart = adoption.values.parent().unwrap_or_else(|| Path::new("."));
        for splice in splices(adoption)? {
            planned.push(files::Planned {
                chart: chart.to_path_buf(),
                text: splice.apply(),
                path: splice.path,
            });
        }
    }
    files::write_all(&planned)
}
