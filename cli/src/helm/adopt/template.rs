//! The template edits: projecting the new values into the rendered configuration document through
//! the chart's `define` blocks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::Error;

use crate::helm::regions::split_lines;
use crate::helm::scaffold::{
    Placement, Plan, Run, credential_entries_for, derived_for, secrets_file_name,
};

use super::{FileEdit, read_text};

/// The Go-template actions that open a nesting level. `end` closes one; `else` is neither.
pub(super) const OPENERS: [&str; 5] = ["if", "with", "range", "define", "block"];

/// One `define` in a chart's templates, as the lines a caller may append to.
///
/// `end` is the line of the define's own closing action, so a new top-level entry is inserted after
/// `end - 1` and lands inside the define. `roots` are the mapping keys already written at column
/// zero (the ones a second block of the same name would shadow), whether or not an
/// `if`/`with`/`range` wraps them, since a chart's helpers never let one of those add YAML
/// indentation of its own: every nested value reaches its indent through an explicit `nindent`.
/// `wraps_context` is [`wraps_context`] read over the define's own lines, never the whole file, so a
/// neighbouring define cannot vouch for this one's calling convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Define {
    end: usize,
    roots: BTreeSet<String>,
    wraps_context: bool,
}

/// Finds one define and the top-level keys it already writes; [`None`] when it cannot be read.
///
/// A helper is hand-written Go template, and this reads exactly as much of it as an append needs:
/// where the define ends, and which keys already occupy its rendered top level. Depth is tracked
/// only to find the closing `end`: a projection is always appended at the define's own depth,
/// outside every `if`/`with`/`range`, so it is never silently written under somebody else's
/// condition.
///
/// An occupied root, however, is detected by column alone, at any depth: a key written from two or
/// three actions deep (a `scheduler:` guarded by `{{- if eq $service ... }}` around a
/// `{{- with include ... }}`) still lands at column zero in the rendered document. Gating this on
/// depth 1 would miss exactly that, and append a new key under the root as a second, colliding
/// block that `fromYaml` resolves by dropping the first in silence.
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
                    return alone.then(|| Define {
                        end: number + 1,
                        roots,
                        wraps_context: lines[start..=number].iter().any(|line| wraps_context(line)),
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
/// this knows wraps the root context and the service together, unwrapping it again with the
/// define's own `{{- $ctx := .ctx -}}`. Every reference written has to agree with that: `.Values`
/// inside such a define is a nil pointer. Read from the text rather than assumed from the document
/// count: a multi-document chart shaped some other way would make `$ctx.Values` exactly as wrong a
/// guess as `.Values` is, and there is no third convention to fall back on, so such a chart is
/// treated the same as one with no define at all.
pub(super) fn wraps_context(text: &str) -> bool {
    template_patterns().context_assignment.is_match(text)
}

/// The template file that opens define `name`, and what [`read_define`] found there.
///
/// Every `templates/*.tpl` is a candidate, not only `_helpers.tpl`: a chart whose services split
/// their configuration across several partials keeps `derivedConfig` in `_config.tpl`, and reading
/// one file only made every projection of such a chart print as still owed, silently. A fixed order
/// makes two files that both open a define resolve the same way every run.
pub(super) fn find_define(
    chart_dir: &Path,
    name: &str,
) -> Result<Option<(PathBuf, Define)>, Error> {
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
            return Ok(Some((path, define)));
        }
    }
    Ok(None)
}

/// Placements of one or more roots, partitioned into the `$service` runs that render them.
///
/// What a multi-document chart's `derivedConfig` has to say about a root is decided by which
/// documents declare each key under it, and that is a fact in the contracts, not a guess:
///
/// - **Every key under the root is declared by every document**: one unguarded run. It renders
///   the same way for all of them, which is also every root of a one-document chart.
/// - **Otherwise**: one run per distinct set of keys a service declares under the root, gated on
///   the services that declare exactly that set. A service sees exactly the keys its own contract
///   declares, and since every service lands in exactly one run per root, no two runs naming the
///   root ever render for the same service: nothing is shadowed by `fromYaml`.
///
/// A document whose image path does not name a `$service` value (see
/// [`service_key_for`](crate::helm::scaffold::service_key_for)) cannot be gated on, so its share of
/// the root comes back in [`Scoped::unresolved`] instead, with the documents that declare it, and
/// the rest of the root is written regardless. Its keys reach the services that *can* be named
/// either way.
pub(super) fn scope(
    placements: &[Placement],
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> Scoped {
    let mut by_root: BTreeMap<&str, Vec<&Placement>> = BTreeMap::new();
    for item in placements {
        by_root.entry(root_of(item)).or_default().push(item);
    }

    let mut scoped = Scoped::default();
    for items in by_root.into_values() {
        let declared_by = |item: &Placement, document: &str| {
            item.documents
                .as_ref()
                .is_none_or(|named| named.iter().any(|held| held == document))
        };
        let everywhere = items.iter().all(|item| {
            all_documents
                .iter()
                .all(|document| declared_by(item, document))
        });
        if all_documents.len() <= 1 || everywhere {
            scoped
                .runs
                .push(Run::everywhere(items.into_iter().cloned().collect()));
            continue;
        }

        // Grouped by the exact keys a document declares under this root, in document order, so
        // the runs and their gates come out the same way every time.
        let mut runs: Vec<(Vec<&str>, Run)> = Vec::new();
        let mut unresolved: Vec<(Vec<&str>, Unresolved)> = Vec::new();
        for document in all_documents {
            let mine: Vec<&Placement> = items
                .iter()
                .copied()
                .filter(|item| declared_by(item, document))
                .collect();
            if mine.is_empty() {
                continue;
            }
            let paths: Vec<&str> = mine.iter().map(|item| item.path()).collect();
            if let Some(service) = service_key_of.get(document) {
                match runs.iter_mut().find(|(held, _)| *held == paths) {
                    Some((_, run)) => run.services.push(service.clone()),
                    None => runs.push((
                        paths,
                        Run {
                            services: vec![service.clone()],
                            placements: mine.into_iter().cloned().collect(),
                        },
                    )),
                }
            } else {
                match unresolved.iter_mut().find(|(held, _)| *held == paths) {
                    Some((_, held)) => held.documents.push(document.clone()),
                    None => unresolved.push((
                        paths,
                        Unresolved {
                            documents: vec![document.clone()],
                            placements: mine.into_iter().cloned().collect(),
                        },
                    )),
                }
            }
        }
        scoped.runs.extend(runs.into_iter().map(|(_, run)| run));
        scoped
            .unresolved
            .extend(unresolved.into_iter().map(|(_, held)| held));
    }
    scoped
}

/// What [`scope`] makes of some placements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Scoped {
    /// The runs that can be rendered, by root and then in document order.
    pub(super) runs: Vec<Run>,
    /// The shares of a root declared by documents no `$service` value names.
    pub(super) unresolved: Vec<Unresolved>,
}

/// The keys under one root that some documents declare, where none of those documents names a
/// `$service` value to gate them on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unresolved {
    /// The documents.
    pub(super) documents: Vec<String>,
    /// The keys they declare under the root.
    pub(super) placements: Vec<Placement>,
}

/// A placement's top-level contract segment, which is the root it is projected under.
pub(super) fn root_of(placement: &Placement) -> &str {
    placement.path().split('.').next().unwrap_or_default()
}

/// The template lines an adoption appends, and the ones it prints instead.
pub(super) struct Projections {
    pub(super) edits: Vec<FileEdit>,
    pub(super) owed_projected: Vec<Withheld>,
    pub(super) owed_credentials: Vec<Placement>,
    /// The file that opens `derivedConfig`, when one does.
    pub(super) derived_define: Option<PathBuf>,
    /// The file that opens `secretData`, when one does.
    pub(super) secret_define: Option<PathBuf>,
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
/// Three more things send lines to the printed pile, each recorded as its own [`Withholding`] so
/// the report can say which one it was: there is no define to write into, a multi-document chart's
/// define does not provably unwrap the `(dict "ctx" ... "service" ...)` convention, or a document
/// declaring the key names no `$service` value to gate it on (see [`scope`]).
///
/// Printed lines are rendered exactly as an appended block would be, in the define's own dialect
/// and with the same `$service` gates, so they can be pasted as they stand. Where the define could
/// not be read, they assume `.` is the root context and carry no gate, and the report says so.
pub(super) fn projection_edits(
    chart_dir: &Path,
    chart: &str,
    plan: &Plan,
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> Result<Projections, Error> {
    let mut edits = Vec::new();
    let mut owed_projected = Vec::new();
    let multi_document = all_documents.len() > 1;

    let found = find_define(chart_dir, &format!("{chart}.derivedConfig"))?;
    let derived_define = found.as_ref().map(|(path, _)| path.clone());
    match found {
        _ if plan.projected.is_empty() => {}
        None => owed_projected.push(Withheld::unscoped(Withholding::NoDefine, &plan.projected)),
        Some((_, derived)) if multi_document && !derived.wraps_context => owed_projected.push(
            Withheld::unscoped(Withholding::UnknownConvention, &plan.projected),
        ),
        Some((path, derived)) => {
            let values_prefix = if multi_document {
                "$ctx.Values"
            } else {
                ".Values"
            };
            let (occupied, free): (Vec<Placement>, Vec<Placement>) = plan
                .projected
                .iter()
                .cloned()
                .partition(|item| derived.roots.contains(root_of(item)));

            let writable = scope(&free, all_documents, service_key_of);
            if !writable.runs.is_empty() {
                edits.push(FileEdit {
                    path,
                    after: derived.end - 1,
                    lines: derived_for(&writable.runs, "", values_prefix),
                    what: "derivedConfig".to_owned(),
                });
            }

            let placed = scope(&occupied, all_documents, service_key_of);
            if !placed.runs.is_empty() {
                let roots: BTreeSet<String> = occupied
                    .iter()
                    .map(|item| root_of(item).to_owned())
                    .collect();
                owed_projected.push(Withheld {
                    lines: derived_for(&placed.runs, "", values_prefix),
                    reason: Withholding::RootWritten(roots.into_iter().collect()),
                    runs: placed.runs,
                });
            }
            for share in writable.unresolved.into_iter().chain(placed.unresolved) {
                let run = Run::everywhere(share.placements);
                owed_projected.push(Withheld {
                    lines: derived_for(std::slice::from_ref(&run), "", values_prefix),
                    reason: Withholding::NoService(share.documents),
                    runs: vec![run],
                });
            }
        }
    }

    let mut owed_credentials = plan.credentials.clone();
    let found = find_define(chart_dir, &format!("{chart}.secretData"))?;
    let secret_define = found.as_ref().map(|(path, _)| path.clone());
    if let Some((path, secrets)) = found {
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
        derived_define,
        secret_define,
    })
}

/// Why some `derivedConfig` lines are printed for a person rather than appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withholding {
    /// No file under `templates/` opens the define, so there is nowhere to append.
    NoDefine,
    /// The chart renders several documents, and its define does not unwrap the
    /// `(dict "ctx" ... "service" ...)` argument this knows how to address.
    UnknownConvention,
    /// The define already writes these roots at its top level, and a second block of the same
    /// name would shadow the first.
    RootWritten(Vec<String>),
    /// These documents declare the keys, and none of them names a `$service` value to gate on.
    NoService(Vec<String>),
}

/// `derivedConfig` lines withheld from the template, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withheld {
    /// Why.
    pub reason: Withholding,
    /// What the lines render, by `$service` gate.
    pub runs: Vec<Run>,
    /// The lines, rendered as an appended block would be.
    pub lines: Vec<String>,
}

impl Withheld {
    /// Lines for a define whose calling convention is unknown: `.` as the root context, no gate.
    fn unscoped(reason: Withholding, placements: &[Placement]) -> Self {
        let run = Run::everywhere(placements.to_vec());
        Self {
            lines: derived_for(std::slice::from_ref(&run), "", ".Values"),
            reason,
            runs: vec![run],
        }
    }

    /// Every placement withheld, in the order the lines render them.
    pub fn placements(&self) -> impl Iterator<Item = &Placement> {
        self.runs.iter().flat_map(|run| &run.placements)
    }
}
