//! The text an adoption run prints: what was written, what is still owed, and what to run next.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use crate::error::Error;

use crate::helm::scaffold::{
    credential_entries_for, default_for, derived_for, invented, values_scalar,
};

use super::{Adoption, posix};

/// One chart's result: what was inserted, what was refused, and what is still owed.
///
/// # Errors
/// As [`secrets_file_name`](crate::helm::scaffold::secrets_file_name), for a credential still owed its `secretData` line.
pub fn report(adoption: &Adoption, written: bool) -> Result<String, Error> {
    let rule = "=".repeat(92);
    let mut out = format!("\n{rule}\n{}\n{rule}\n", adoption.chart);

    if !adoption.blocked.is_empty() {
        for message in &adoption.blocked {
            let _ = writeln!(out, "  blocked: {message}");
        }
        out.push_str(if adoption.by_hand {
            "\n  Nothing was planned. Once the declaration is fixed by hand, this plans the rest.\n"
        } else {
            "\n  Nothing was planned. `just check-config-bindings` reports the same thing.\n"
        });
        return Ok(out);
    }

    let values = posix(&adoption.values);
    for insertion in &adoption.insertions {
        let state = if written {
            "written after"
        } else {
            "goes after"
        };
        let _ = writeln!(out, "\n  {state} {values}:{}\n", insertion.after);
        indented(&mut out, &insertion.lines);
    }

    for refusal in &adoption.refusals {
        let _ = writeln!(out, "\n  refused: {} {}", refusal.path, refusal.reason);
    }

    still_owed(&mut out, adoption, written)?;
    Ok(out)
}

/// Lines under a report heading, four spaces in, a blank line staying blank.
pub(super) fn indented(out: &mut String, lines: &[String]) {
    for line in lines {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "    {line}");
        }
    }
}

/// What is left for a person, and the edits that are not.
///
/// Printed on a dry run as well, because the values block on its own is the half that reads as
/// done: a value that is typed, documented and bound but that no template projects is a setting an
/// operator can set and nothing honours, which is the state this output exists to make loud.
pub(super) fn still_owed(
    out: &mut String,
    adoption: &Adoption,
    written: bool,
) -> Result<(), Error> {
    let plan = adoption.placed();
    let chart = &adoption.chart;

    if !adoption.owed_projected.is_empty() {
        let _ = writeln!(
            out,
            "\n  still owed — {chart}/templates/_helpers.tpl, in `derivedConfig`:\n"
        );
        for line in derived_for(&adoption.owed_projected, "  ", None, ".Values") {
            let _ = writeln!(out, "  {line}");
        }
        out.push_str(
            "\n    The define already writes this key's root, and a second one would shadow\n    \
             the first — `fromYaml` keeps the last. Place these in the block that exists.\n",
        );
    }

    if !adoption.owed_credentials.is_empty() {
        let _ = writeln!(
            out,
            "\n  still owed — {chart}/templates/_helpers.tpl, in `secretData`:\n"
        );
        for line in credential_entries_for(&adoption.owed_credentials)? {
            let _ = writeln!(out, "    {line}");
        }
    }

    let chosen = invented(&plan);
    if !chosen.is_empty() {
        out.push_str("\n  still owed — a chosen default, in place of the one invented for:\n\n");
        for item in chosen {
            let _ = writeln!(
                out,
                "    {}: {}",
                item.values_path,
                values_scalar(&default_for(&item.key))
            );
        }
        out.push_str(
            "\n    Each is a key its image requires and publishes no default for, so the value\n    \
             above is a legal one drawn from the constraint rather than a chosen one.\n",
        );
    }

    let described: Vec<&String> = adoption
        .insertions
        .iter()
        .flat_map(|insertion| &insertion.branches)
        .collect();
    if !described.is_empty() {
        out.push_str(
            "\n  worth a sentence — these blocks were described from their key prefix:\n\n",
        );
        for path in described {
            let _ = writeln!(out, "    {path}");
        }
    }

    for edit in &adoption.edits {
        let state = if written { "written into" } else { "goes into" };
        let _ = writeln!(
            out,
            "\n  {state} {}:{}, under `{}`:\n",
            posix(&edit.path),
            edit.after,
            edit.what
        );
        indented(out, &edit.lines);
    }
    Ok(())
}

/// What a run added up to, and the work that has to follow it.
///
/// `rows` are the bindings gate's per-chart counts for the charts this run wrote values into, read
/// back after the write; see [`counted`].
pub fn closing(adoptions: &[Adoption], written: bool, rows: &[String]) -> String {
    let values: usize = adoptions
        .iter()
        .map(|item| {
            let placed = item.placed();
            placed.projected.len() + placed.credentials.len()
        })
        .sum();
    let charts: Vec<&str> = adoptions
        .iter()
        .filter(|item| !item.insertions.is_empty())
        .map(|item| item.chart.as_str())
        .collect();

    if !written {
        return format!(
            "\n==> {values} value(s) across {} chart(s); nothing was written.\n    Re-run with \
             --write to insert them.\n",
            charts.len()
        );
    }

    let mut out = format!(
        "\n==> wrote {values} value(s) into {} chart(s): {}\n\n",
        charts.len(),
        charts.join(", ")
    );
    out.push_str(AFTERWARDS);
    if !rows.is_empty() {
        out.push('\n');
        for row in rows {
            let _ = writeln!(out, "{row}");
        }
    }
    out
}

/// The work a written run leaves, in the order it has to be done.
///
/// Led by the two generated files the run does not touch and that are owed at once: the values
/// schema, without which every render fails on a block it has never seen, and the round-trip suite,
/// whose new cases are what prove the projection arrived.
pub(super) const AFTERWARDS: &str =
    "    Then, in order — `just sync-config --write` does 1 and 5 for you:
      1. `just schema`. values.schema.json is committed and Helm validates every
         render against it, so a new block it does not know fails every job
      2. read the `derivedConfig` and `secretData` lines that were written, and
         place by hand any the output above says could not be
      3. replace the text after each `TODO: ` — keep the `# --` line, since a
         value with no description fails `just check-values-docs`
      4. add a `note:` to each credential row saying when a release needs it, and
         choose a default for each invented value named above
      5. `just contract-tests <chart>`, then `just update-snapshots <chart>` —
         the round trip gains a case per marker, and every snapshot case that
         covers the ConfigMap fails until the committed `.snap` is rewritten
      6. paste the row(s) below into tests/test_contract_bindings.py, in
         `test_every_enrolled_chart_passes_the_gate`
      7. `just config-readme` — the `credentials` row is a column of the
         generated table — then bump the chart version and run `just docs`
      8. `just check-config-bindings`, `just check-config-wired`,
         `just check-values-docs`, `just test`
";

/// The rows a chart repository's tree-level test asserts, for the charts this run wrote into.
///
/// That test holds the per-chart marker counts as literals, so every adoption moves one of them and
/// the suite goes red until somebody edits it. The number is the bindings gate's own (markers times
/// the documents each binds), and re-deriving it here would be a second implementation of exactly
/// the arithmetic that test pins. So it is read back out of the gate after the write, and printed
/// as the line to paste rather than written: a test whose expected values a generator edits
/// silently is a test nobody reads.
///
/// # Errors
/// As [`crate::helm::bindings::check`].
pub fn counted(charts: &Path, adoptions: &[Adoption]) -> Result<Vec<String>, Error> {
    let touched: BTreeSet<&str> = adoptions
        .iter()
        .filter(|item| !item.insertions.is_empty())
        .map(|item| item.chart.as_str())
        .collect();
    let found = crate::helm::bindings::check(charts, &crate::helm::declaration::Selection::all())?;
    Ok(found
        .enrolled
        .iter()
        .filter(|(chart, _, _)| touched.contains(chart.as_str()))
        .map(|(chart, keys, external)| format!("        (\"{chart}\", {keys}, {external}),"))
        .collect())
}
