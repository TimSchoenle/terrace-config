//! The template edits: projecting the new values into the rendered configuration document through
//! the chart's `define` blocks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::Error;

use crate::helm::regions::split_lines;
use crate::helm::scaffold::{
    Placement, Plan, credential_entries_for, derived_for, secrets_file_name,
};

use super::{FileEdit, read_text};

/// The Go-template actions that open a nesting level. `end` closes one; `else` is neither.
pub(super) const OPENERS: [&str; 5] = ["if", "with", "range", "define", "block"];

/// One `define` in a chart's templates, as the lines a caller may append to.
///
/// `end` is the line of the define's own closing action, so a new top-level entry is inserted after
/// `end - 1` and lands inside the define. `roots` are the mapping keys already written at column
/// zero — the ones a second block of the same name would shadow — whether or not an
/// `if`/`with`/`range` wraps them, since a chart's helpers never let one of those add YAML
/// indentation of its own: every nested value reaches its indent through an explicit `nindent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Define {
    end: usize,
    roots: BTreeSet<String>,
}

/// Find one define and the top-level keys it already writes; [`None`] when it cannot be read.
///
/// A helper is hand-written Go template, and this reads exactly as much of it as an append needs:
/// where the define ends, and which keys already occupy its rendered top level. Depth is tracked
/// only to find the closing `end` — a projection is always appended at the define's own depth,
/// outside every `if`/`with`/`range`, so it is never silently written under somebody else's
/// condition.
///
/// An occupied root, however, is detected by column alone, at any depth: a key written from two or
/// three actions deep — a `scheduler:` guarded by `{{- if eq $service ... }}` around a
/// `{{- with include ... }}` — still lands at column zero in the rendered document. Gating this on
/// depth 1 once missed exactly that, and a new key under the root was appended as a second,
/// colliding block that `fromYaml` resolved by dropping the first in silence.
///
/// [`None`] when the define is absent, or when its actions do not balance: a template shape this
/// cannot read is one it must not edit, and the caller prints the lines instead.
pub(super) fn read_define(text: &str, name: &str) -> Option<Define> {
    let lines = split_lines(text, false);
    let opening = format!("define \"{name}\"");
    let start = lines.iter().position(|line| line.contains(&opening))?;

    let mut depth: i64 = 0;
    let mut roots = BTreeSet::new();
    for (number, line) in lines.iter().enumerate().skip(start) {
        if depth >= 1
            && let Some(key) = template_patterns().top_level_key.captures(line)
        {
            roots.insert(key["name"].to_owned());
        }
        let actions: Vec<&str> = template_patterns()
            .action
            .captures_iter(line)
            .filter_map(|found| found.get(1))
            .map(|found| found.as_str())
            .collect();
        for action in &actions {
            if OPENERS.contains(action) {
                depth += 1;
            } else if *action == "end" {
                depth -= 1;
                if depth == 0 {
                    // Lines are inserted *above* the closing `end`, so it has to own its line.
                    // Sharing it with the define's opening would put them above the define, and
                    // sharing it with an inner `{{- end }}` would put them inside that inner
                    // block, under a condition nobody wrote for them. Either way the shape is not
                    // one an append can be trusted with, and the lines are printed instead.
                    let alone = number != start && actions.len() == 1;
                    return alone.then_some(Define {
                        end: number + 1,
                        roots,
                    });
                }
            }
        }
    }
    None
}

/// The patterns the template reader uses, compiled once.
pub(super) struct TemplatePatterns {
    /// An action's opening word.
    action: Regex,
    /// A mapping key at column zero.
    top_level_key: Regex,
    /// The line that unwraps a `(dict "ctx" ... "service" ...)` argument.
    context_assignment: Regex,
}

pub(super) fn template_patterns() -> &'static TemplatePatterns {
    static HELD: std::sync::OnceLock<TemplatePatterns> = std::sync::OnceLock::new();
    HELD.get_or_init(|| TemplatePatterns {
        action: Regex::new(r"\{\{-?\s*(\w+)").expect("a literal pattern compiles"),
        top_level_key: Regex::new(r"^(?P<name>[A-Za-z0-9_][A-Za-z0-9_.\-]*):")
            .expect("a literal pattern compiles"),
        context_assignment: Regex::new(r"\$ctx\s*:=\s*\.ctx\b")
            .expect("a literal pattern compiles"),
    })
}

/// Whether a `derivedConfig` define is included with `.` set to `(dict "ctx" ... "service" ...)`
/// rather than to the root context.
///
/// A multi-document chart's `derivedConfig` needs to know which service is rendering, and the shape
/// this knows wraps the root context and the service together, unwrapping it again with the define's
/// own `{{- $ctx := .ctx -}}`. Every reference written has to agree with that: `.Values` inside such
/// a define is a nil pointer. Read from the text rather than assumed from the document count — a
/// multi-document chart shaped some other way would make `$ctx.Values` exactly as wrong a guess as
/// `.Values` is, and there is no third convention to fall back on, so such a chart is treated the
/// same as one with no define at all.
pub(super) fn wraps_context(text: &str) -> bool {
    template_patterns().context_assignment.is_match(text)
}

/// The template file that opens define `name`, its text, and what [`read_define`] found there.
///
/// Every `templates/*.tpl` is a candidate, not only `_helpers.tpl`: a chart whose services split
/// their configuration across several partials keeps `derivedConfig` in `_config.tpl`, and reading
/// one file only made every projection of such a chart print as still owed, silently. A fixed order
/// makes two files that both open a define resolve the same way every run.
pub(super) fn find_define(
    chart_dir: &Path,
    name: &str,
) -> Result<Option<(PathBuf, String, Define)>, Error> {
    let templates = chart_dir.join("templates");
    let Ok(entries) = std::fs::read_dir(&templates) else {
        return Ok(None);
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Error::io(templates.display(), e))?.path();
        let named = path.extension().is_some_and(|extension| extension == "tpl");
        if named && path.is_file() {
            candidates.push(path);
        }
    }
    candidates.sort();

    for path in candidates {
        let text = read_text(&path)?;
        if let Some(define) = read_define(&text, name) {
            return Ok(Some((path, text, define)));
        }
    }
    Ok(None)
}

/// Brand-new roots split into what can be written and what must be printed, with each written
/// root's `$service` guard.
///
/// A single-document chart's roots always come back writable and unguarded. A multi-document
/// chart's `derivedConfig` takes a `$service` argument: a root every document declares renders the
/// same way for all of them, and a root only some declare needs the matching guard.
///
/// That guard is written only when it cannot be wrong. Every key under the root has to agree which
/// documents declare it — a root with keys split across scopes is two settings sharing a table
/// name — and every one of those documents has to resolve to a real `$service` value. Either failure
/// sends the whole root to the printed-for-a-person pile, the same one an occupied root lands in.
pub(super) fn scope_new_roots(
    candidates: Vec<Placement>,
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> (Vec<Placement>, BTreeMap<String, Vec<String>>) {
    let mut by_root: Vec<(String, Vec<Placement>)> = Vec::new();
    for item in candidates {
        let root = root_of(&item).to_owned();
        match by_root.iter_mut().find(|(held, _)| *held == root) {
            Some((_, members)) => members.push(item),
            None => by_root.push((root, vec![item])),
        }
    }

    let mut writable = Vec::new();
    let mut scoped = BTreeMap::new();
    for (root, items) in by_root {
        let declaring: BTreeSet<Option<&Vec<String>>> =
            items.iter().map(|item| item.documents.as_ref()).collect();
        let [documents] = declaring.into_iter().collect::<Vec<_>>()[..] else {
            continue;
        };
        let everywhere = documents
            .is_none_or(|named| named.iter().cloned().collect::<BTreeSet<_>>() == *all_documents);
        if everywhere {
            writable.extend(items);
            continue;
        }

        let named = documents.map_or(&[][..], Vec::as_slice);
        let service_keys: Vec<String> = named
            .iter()
            .filter_map(|name| service_key_of.get(name).cloned())
            .collect();
        if service_keys.len() != named.len() {
            continue;
        }
        writable.extend(items);
        scoped.insert(root, service_keys);
    }
    (writable, scoped)
}

/// A placement's top-level contract segment, which is the root it is projected under.
pub(super) fn root_of(placement: &Placement) -> &str {
    placement.path().split('.').next().unwrap_or_default()
}

/// The template lines an adoption appends, and the ones it prints instead.
pub(super) struct Projections {
    pub(super) edits: Vec<FileEdit>,
    pub(super) owed_projected: Vec<Placement>,
    pub(super) owed_credentials: Vec<Placement>,
}

/// The `derivedConfig` and `secretData` lines to append, and the ones that must be printed.
///
/// **One rule decides whether a line can be written: does the define already have this key's
/// root?** A projection is appended at the define's own depth, so it cannot land inside somebody's
/// `if`; what it *can* do is write a second `telemetry:` beside an existing one, and the document is
/// built by `fromYaml`, which keeps the last of two mapping keys and drops everything under the
/// first. The values file parses, the chart renders, and a setting has vanished. So a key whose
/// root is already in the define is printed for a person to place, and only a new root is written.
///
/// On a multi-document chart, [`scope_new_roots`] draws a second such line, and anything it leaves
/// uncertain joins the same printed pile — never a guess.
pub(super) fn projection_edits(
    chart_dir: &Path,
    chart: &str,
    plan: &Plan,
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> Result<Projections, Error> {
    let mut edits = Vec::new();

    let mut projected: Vec<Placement> = Vec::new();
    if let Some((path, text, derived)) = find_define(chart_dir, &format!("{chart}.derivedConfig"))?
    {
        let multi_document = all_documents.len() > 1;
        let mut values_prefix = ".Values";
        let mut scoped = BTreeMap::new();
        // A multi-document chart's define is unusable here unless it is provably shaped the one
        // way this knows how to address; refusing is the same posture as finding no define.
        if !multi_document || wraps_context(&text) {
            projected = plan
                .projected
                .iter()
                .filter(|item| !derived.roots.contains(root_of(item)))
                .cloned()
                .collect();
            if multi_document {
                values_prefix = "$ctx.Values";
                (projected, scoped) = scope_new_roots(projected, all_documents, service_key_of);
            }
        }
        if !projected.is_empty() {
            edits.push(FileEdit {
                path,
                after: derived.end - 1,
                lines: derived_for(&projected, "", Some(&scoped), values_prefix),
                what: "derivedConfig".to_owned(),
            });
        }
    }
    let written: BTreeSet<&str> = projected.iter().map(Placement::path).collect();
    let owed_projected = plan
        .projected
        .iter()
        .filter(|item| !written.contains(item.path()))
        .cloned()
        .collect();

    let mut owed_credentials = plan.credentials.clone();
    if let Some((path, _, secrets)) = find_define(chart_dir, &format!("{chart}.secretData"))? {
        let mut delivered = Vec::new();
        owed_credentials.clear();
        for item in &plan.credentials {
            if secrets.roots.contains(&secrets_file_name(item)?) {
                owed_credentials.push(item.clone());
            } else {
                delivered.push(item.clone());
            }
        }
        if !delivered.is_empty() {
            edits.push(FileEdit {
                path,
                after: secrets.end - 1,
                lines: credential_entries_for(&delivered)?,
                what: "secretData".to_owned(),
            });
        }
    }

    Ok(Projections {
        edits,
        owed_projected,
        owed_credentials,
    })
}
