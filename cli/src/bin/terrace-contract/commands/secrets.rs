//! `secrets`: the credential inventory of a chart tree, and its reconciliation against a cluster.

use std::path::Path;
use std::process::ExitCode;

use terrace_contract::report::Report;
use terrace_contract::{Error, helm};

/// The credential inventory, or the report reconciling it against what the charts deliver.
pub(crate) fn secrets_command(
    charts: &Path,
    rendered: Option<&Path>,
    as_json: bool,
    exit_code: bool,
) -> Result<ExitCode, Error> {
    let Some(rendered) = rendered else {
        let rows = helm::secrets::credentials(&helm::secrets::inventory(charts)?);
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "credentials": rows.iter().map(credential_json).collect::<Vec<_>>(),
                }))
                .expect("an inventory of owned values serialises")
            );
        } else {
            print_inventory(charts, &rows)?;
        }
        return Ok(ExitCode::SUCCESS);
    };

    if !rendered.is_dir() {
        return Err(Error::Invalid(format!(
            "{}: no rendered manifests; render the charts there first",
            rendered.display()
        )));
    }
    let surface = helm::secrets::reconcile(charts, rendered)?;
    let report = helm::secrets::report_of(&surface);

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&surface_json(&surface))
                .expect("a surface of owned values serialises")
        );
    } else {
        print_reconciliation(&surface, &report);
    }

    // Report only unless asked otherwise, for the reason the subcommand's own documentation gives.
    if exit_code && report.failed() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

/// One block per contracted chart.
pub(crate) fn print_inventory(
    charts: &Path,
    rows: &[helm::secrets::Credential],
) -> Result<(), Error> {
    let charts_seen = helm::secrets::contracted_charts(charts)?;
    for (_, declaration) in &charts_seen {
        let mine: Vec<&helm::secrets::Credential> = rows
            .iter()
            .filter(|row| row.chart == declaration.chart)
            .collect();
        println!("==> {}", declaration.chart);
        if mine.is_empty() {
            println!(
                "    no key of the {} document(s) this chart declares is marked secret",
                declaration.documents.len()
            );
            println!();
            continue;
        }
        print_chart(declaration, &mine);
        println!();
    }

    if rows.is_empty() {
        println!("no chart declares a configuration contract with a secret key");
    } else {
        println!(
            "{} credential(s) across {} chart(s)",
            rows.len(),
            charts_seen.len()
        );
    }
    Ok(())
}

/// One chart's credentials as a table, with the derivation stated once above it.
pub(crate) fn print_chart(declaration: &helm::Declaration, rows: &[&helm::secrets::Credential]) {
    let images: std::collections::BTreeSet<&str> = rows
        .iter()
        .flat_map(|row| row.images.iter().map(String::as_str))
        .collect();
    let vendored: usize = declaration
        .documents
        .iter()
        .map(|document| document.images.len())
        .sum();
    println!(
        "    {} credential(s), read by {} of the {vendored} image(s) this chart vendors a \
contract for",
        rows.len(),
        images.len()
    );

    // The environment spellings are derived from the config path by the dialect, and stating the
    // derivation once is both shorter and more useful than a column of values every one of which
    // restates it. Checked rather than assumed: a credential whose contract spells it otherwise
    // gets its literal spellings printed under its row, so the rule can never quietly become a lie
    // the table tells.
    let prefix = helm::secrets::common_prefix(rows.iter().map(|row| row.env.as_str()));
    if !prefix.is_empty() {
        println!(
            "    environment: {prefix}<PATH>, with dots as `__` and upper-cased; the same \
spelling with `_FILE` appended names a file whose contents supply it"
        );
    }

    let multi = images.len() > 1;
    let path_width = rows.iter().map(|row| row.path.len()).max().unwrap_or(0);
    let file_width = rows
        .iter()
        .map(|row| row.secrets_file.len())
        .max()
        .unwrap_or(0);
    let header = format!(
        "    {:<path_width$}  {:<file_width$}  required",
        "config path", "secrets file"
    );
    println!();
    println!("{header}");
    println!("    {}", "-".repeat(header.len() - 4));
    for row in rows {
        println!(
            "    {:<path_width$}  {:<file_width$}  {}",
            row.path,
            row.secrets_file,
            if row.required { "yes" } else { "no" }
        );
        if multi {
            println!("        read by  {}", row.images.join(", "));
        }
        if row.env_file != format!("{}_FILE", row.env) || prefix.is_empty() {
            println!("        env      {}", row.env);
            println!("        _FILE    {}", row.env_file);
        }
    }
}

/// The reconciliation, with the scope it was run at stated before the findings.
pub(crate) fn print_reconciliation(surface: &helm::secrets::Surface, report: &Report) {
    println!(
        "==> report only: this exits 0 whatever it finds unless `--exit-code` is given, so that \
its first pass over an established repository can be triaged rather than merged as a red pipeline"
    );
    println!(
        "    reconciled {} chart(s) against every `ci/` values file each one ships: {}",
        surface.charts.len(),
        if surface.charts.is_empty() {
            "none".to_owned()
        } else {
            surface.charts.join(", ")
        }
    );
    println!(
        "    a credential delivered under any one of them is delivered; over-projection and \
unclaimed names are per values file and name it"
    );
    println!();

    // The findings are split across the two streams every other report here splits them across, so
    // promoting this to a gate changes the exit status and nothing about the output.
    let (clean, problems) = report.text();
    print!("{clean}");
    eprint!("{problems}");

    let undeliverable = surface
        .undeliverable
        .iter()
        .filter(|entry| !entry.named_by_chart)
        .count();
    println!(
        "\n{undeliverable} undeliverable, {} over-projected container(s), {} container(s) with \
unclaimed file(s), {} unsupplied, {} elevated",
        helm::secrets::by_container(&surface.over_projected).len(),
        helm::secrets::by_container(&surface.unclaimed).len(),
        surface.undeliverable.len() - undeliverable,
        surface.elevated.len()
    );
}

/// One credential, for a reader that is not a person.
pub(crate) fn credential_json(row: &helm::secrets::Credential) -> serde_json::Value {
    serde_json::json!({
        "chart": row.chart,
        "path": row.path,
        "secrets_file": row.secrets_file,
        "env": row.env,
        "env_file": row.env_file,
        "required": row.required,
        "summary": row.summary,
        "documents": row.documents,
        "images": row.images,
        "contracts": row.contracts,
    })
}

/// One mount, for the same reader.
pub(crate) fn mount_json(mount: &helm::secrets::Mount) -> serde_json::Value {
    serde_json::json!({
        "chart": mount.chart,
        "values_file": mount.values_file,
        "workload": mount.workload,
        "container": mount.container,
        "image": mount.image,
        "mount_path": mount.mount_path,
        "file_name": mount.file_name,
        "judged_by_gate_three": mount.judged_by_gate_three,
    })
}

/// The whole scan.
pub(crate) fn surface_json(surface: &helm::secrets::Surface) -> serde_json::Value {
    serde_json::json!({
        "charts": surface.charts,
        "values_files": surface.values_files,
        "undeliverable": surface.undeliverable.iter().map(|entry| serde_json::json!({
            "credential": credential_json(&entry.credential),
            "values_files": entry.values_files,
        })).collect::<Vec<_>>(),
        "unclaimed": surface.unclaimed.iter().map(mount_json).collect::<Vec<_>>(),
        "over_projected": surface.over_projected.iter().map(mount_json).collect::<Vec<_>>(),
        "elevated": surface.elevated.iter().map(|entry| serde_json::json!({
            "chart": entry.chart,
            "path": entry.path,
            "file_name": entry.file_name,
            "text_form": entry.text_form,
            "containers": entry.containers,
        })).collect::<Vec<_>>(),
        "notes": surface.notes,
    })
}
