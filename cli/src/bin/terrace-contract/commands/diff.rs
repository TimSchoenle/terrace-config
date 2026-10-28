//! `diff` and `migrate`: what changed between two contracts, and the chart edits that follow.

use std::path::Path;
use std::process::ExitCode;

use terrace_contract::helm::Selection;
use terrace_contract::{Error, helm};

use crate::args::Command;
use crate::settings::Repo;

/// Compare every vendored contract against another revision.
pub(crate) fn diff_command(
    charts: &Path,
    since: &str,
    only: Option<&str>,
    as_json: bool,
    exit_code: bool,
) -> Result<ExitCode, Error> {
    let revision = helm::Committed::resolve_for(charts, since)?;
    let found = helm::collect(charts, &revision, only)?;

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "tool": "terrace-contract diff",
                "ref": since,
                "commit": revision.commit(),
                "changed": found.changed(),
                "impact": found.impact().label(),
                "charts": found.charts.iter().map(helm::ChartDiff::as_json).collect::<Vec<_>>(),
                "problems": found.report.entries().iter().map(|entry| serde_json::json!({
                    "where": entry.at,
                    "level": entry.finding.level.label(),
                    "message": entry.finding.message,
                })).collect::<Vec<_>>(),
            }))
            .expect("a diff of owned values serialises")
        );
    } else {
        print!("{}", render_diff(&found, since));
    }

    let (_, problems) = found.report.text();
    eprint!("{problems}");

    if found.report.failed() {
        // Something could not be answered, which is a different outcome from a difference.
        return Ok(ExitCode::from(1));
    }
    if exit_code && found.changed() {
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

/// Name every chart line that still uses something a contract removed since `since`.
///
/// Returns its exit code rather than an error, because an error reaching `main` exits 2 — which is
/// this command's "a chart still references something removed". Every way of failing to answer is
/// 1 instead, so the three outcomes stay distinguishable to a caller.
///
/// A vendored contract that cannot be read on either side is a failure to answer, and nothing is
/// reported past it: the comparison would read the unreadable side as absent and name every key of
/// that contract as removed.
pub(crate) fn migrate_command(repo: &Repo, command: Command) -> ExitCode {
    let Command::Migrate {
        chart: only,
        since,
        charts,
        json: as_json,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };
    let answer = || -> Result<ExitCode, Error> {
        let charts = repo.charts(charts)?;
        if let Some(name) = &only {
            // Refused by name, so a misspelt chart cannot compare nothing and pass.
            Selection::of([name]).dirs(&charts)?;
        }
        let revision = helm::Committed::resolve_for(&charts, &since)?;
        let diffed = helm::collect(&charts, &revision, only.as_deref())?;
        let (_, problems) = diffed.report.text();
        eprint!("{problems}");
        if diffed.report.failed() {
            return Err(Error::Invalid(format!(
                "a vendored contract could not be read at {since} or in the working tree, so what \
                 it removed is unknown"
            )));
        }

        let found = helm::migrate::migrations(&charts, &diffed.charts, &revision)?;
        let root = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        if as_json {
            let mut document = helm::migrate::as_json(&found, &root);
            document["tool"] = "terrace-contract migrate".into();
            document["ref"] = since.as_str().into();
            document["commit"] = revision.commit().into();
            println!(
                "{}",
                serde_json::to_string_pretty(&document).expect("owned values serialise")
            );
        } else {
            print!("{}", helm::migrate::render(&found, &root));
        }

        let open = found
            .iter()
            .any(|migration| !migration.references.is_empty());
        Ok(if open {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        })
    };
    answer().unwrap_or_else(|error| {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    })
}

/// Prose and prose alone are collapsed: they are real changes and they are shown, but one line per
/// key would bury the findings a reviewer is here for.
pub(crate) const PROSE_FIELDS: [&str; 2] = ["docs", "note"];

/// The human report: one block per chart, and the suggestion with its reasons under it.
pub(crate) fn render_diff(found: &helm::Diffed, reference: &str) -> String {
    use std::fmt::Write as _;
    use terrace_contract::diff::{Severity, Status};

    let changed: Vec<&helm::ChartDiff> = found
        .charts
        .iter()
        .filter(|chart| chart.impact() != Severity::None)
        .collect();
    if changed.is_empty() {
        return format!("==> no vendored contract differs from {reference}\n");
    }

    let mut out = String::new();
    for chart in &changed {
        let _ = writeln!(out, "==> {}", chart.chart);
        for contract in &chart.contracts {
            if contract.status == Status::Unchanged {
                continue;
            }
            let _ = writeln!(out, "  {}  ({})", contract.path, contract.status.label());

            // Severity order, stable within a level. The reviewer's first question is whether
            // anything here is major, and a document-order listing buries the answer under the
            // digest line that every single refresh produces.
            let mut ordered: Vec<&terrace_contract::diff::Change> =
                contract.changes.iter().collect();
            ordered.sort_by_key(|change| std::cmp::Reverse(change.severity));

            let mut prose: Vec<&str> = Vec::new();
            for change in ordered {
                let is_prose = change
                    .field
                    .as_deref()
                    .is_some_and(|field| PROSE_FIELDS.contains(&field));
                if is_prose && change.severity == Severity::Patch {
                    prose.push(&change.subject);
                    continue;
                }
                let _ = writeln!(
                    out,
                    "    {:<5}  {:<8}  {}",
                    change.severity.label(),
                    change.area.label(),
                    change.message
                );
            }
            if !prose.is_empty() {
                prose.sort_unstable();
                prose.dedup();
                let _ = writeln!(
                    out,
                    "    {:<5}  {:<8}  {} documentation-only change(s): {}",
                    Severity::Patch.label(),
                    "docs",
                    prose.len(),
                    prose.join(", ")
                );
            }
        }

        let _ = writeln!(out, "  impact: {}", chart.impact().label());
        // Named rather than repeated in full: every driver has already been printed above, and the
        // question this block answers is which of those lines set the impact.
        for change in chart.drivers() {
            let _ = writeln!(
                out,
                "    because  {} {}  {}",
                change.area.label(),
                change.kind.label(),
                change.subject
            );
        }
        let _ = writeln!(out, "  {}", version_line(chart, reference));
        let _ = writeln!(out);
    }

    let _ = writeln!(
        out,
        "==> {} of {} chart(s) changed against {reference}; suggested impact {}",
        changed.len(),
        found.charts.len(),
        terrace_contract::diff::worst(changed.iter().map(|chart| chart.impact())).label()
    );
    out
}

/// What the chart's version is, and whether the bump already in the branch is large enough.
pub(crate) fn version_line(chart: &helm::ChartDiff, reference: &str) -> String {
    use terrace_contract::diff::suggest_version;

    let where_ = format!(
        "version: {} at {reference}, {} in the working tree",
        chart.old_version.as_deref().unwrap_or("unset"),
        chart.new_version.as_deref().unwrap_or("unset")
    );
    let Some(suggested) = suggest_version(chart.new_version.as_deref(), chart.impact()) else {
        return format!("{where_}; suggested impact is {}", chart.impact().label());
    };
    match chart.satisfied() {
        None => format!(
            "{where_}; a {} change suggests {suggested}",
            chart.impact().label()
        ),
        Some(true) => format!(
            "{where_}; the bump in this branch already covers a {} change",
            chart.impact().label()
        ),
        Some(false) => format!(
            "{where_}; a {} change suggests {suggested}, which this branch does not carry",
            chart.impact().label()
        ),
    }
}
