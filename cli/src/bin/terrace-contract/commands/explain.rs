//! `explain`: every setting a chart exposes, with where its value comes from.

use std::path::Path;
use std::process::ExitCode;

use terrace_contract::report::Report;
use terrace_contract::{Error, helm};

/// Reads a chart's vendored contracts back out.
pub(crate) fn explain_command(
    charts: &Path,
    chart: Option<&str>,
    pattern: Option<&str>,
    show_full: bool,
    as_json: bool,
) -> Result<ExitCode, Error> {
    let pattern = pattern.filter(|held| !held.is_empty());
    let Some(chart) = chart.filter(|held| !held.is_empty()) else {
        print!("{}", list_charts(charts)?);
        return Ok(ExitCode::SUCCESS);
    };

    let chart_dir = charts.join(chart);
    let declaration = if chart_dir.is_dir() {
        helm::load_declaration(&chart_dir)?
    } else {
        None
    };
    let Some(declaration) = declaration else {
        eprintln!("error: '{chart}' is not a chart with a configuration contract");
        eprint!("{}", list_charts(charts)?);
        return Ok(ExitCode::from(2));
    };
    if declaration.documents.is_empty() {
        print!("{}", opted_out(&declaration));
        return Ok(ExitCode::SUCCESS);
    }

    let mut report = Report::new();
    let Some(surface) = helm::explain::collect(&chart_dir, &declaration, &mut report)? else {
        // The interlock, and the reason it is worth honouring in a command that changes nothing: a
        // contract that is not for the digest the chart pins describes some other build of the
        // image, and printing its settings as this chart's would be a confident wrong answer to the
        // only question anyone runs this to ask.
        let (_, problems) = report.text();
        eprint!("{problems}");
        eprintln!(
            "\nerror: the vendored contracts are not for the images this chart pins, so nothing \
             here can be shown to describe what is deployed; refresh them"
        );
        return Ok(ExitCode::from(3));
    };
    helm::explain::report_divergences(&surface, pattern, &mut report);

    let selected = helm::explain::select(&surface.keys, pattern).len()
        + helm::explain::select(&surface.loader, pattern).len()
        + helm::explain::select(&surface.external, pattern).len();

    if as_json {
        // Warnings go to stderr here rather than stdout, so a piped run reads JSON and nothing
        // else. No step summary is written: this is an explanation, not a gate.
        let (_, problems) = report.text();
        eprint!("{problems}");
        println!(
            "{}",
            serde_json::to_string_pretty(&explained_json(&surface, pattern))
                .expect("a surface of owned values serialises")
        );
    } else {
        print!("{}", helm::explain::render(&surface, pattern, show_full));
        let (clean, problems) = report.text();
        print!("{clean}");
        eprint!("{problems}");
    }

    if pattern.is_some() && selected == 0 {
        if !as_json {
            eprintln!(
                "    nothing this chart's images read matches '{}'",
                pattern.unwrap_or_default()
            );
        }
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

/// Every chart carrying a declaration, whether or not it declares any document.
///
/// Not documents-only: a chart that opted out explicitly carries a reason, and printing that reason
/// is one of the two things this command exists to do.
pub(crate) fn list_charts(charts: &Path) -> Result<String, Error> {
    use std::fmt::Write as _;

    let covered = helm::declared(charts, false)?;
    if covered.is_empty() {
        return Ok(
            "==> no chart in this repository declares a configuration contract\n".to_owned(),
        );
    }
    let mut out = String::from("==> charts with a configuration contract\n\n");
    for (_, declaration) in &covered {
        if declaration.documents.is_empty() {
            let _ = writeln!(out, "    {:<38}opted out", declaration.chart);
            continue;
        }
        let images: usize = declaration
            .documents
            .iter()
            .map(|document| document.images.len())
            .sum();
        let _ = writeln!(
            out,
            "    {:<38}{} document(s), {images} image(s)",
            declaration.chart,
            declaration.documents.len()
        );
    }
    let _ = write!(out, "\n    terrace-contract explain <chart> [pattern]\n");
    Ok(out)
}

/// A chart that declared no document, and the reason it gave.
pub(crate) fn opted_out(declaration: &helm::Declaration) -> String {
    use std::fmt::Write as _;

    let mut out = format!(
        "==> {} has explicitly opted out of configuration contracts\n\n",
        declaration.chart
    );
    let reason = declaration.reason.as_deref().unwrap_or("");
    for line in helm::explain::prose(reason, "    ") {
        let _ = writeln!(out, "{line}");
    }
    if !declaration.unconfigured.is_empty() {
        // Values paths, not repository names: the field is unioned with every declared image's
        // values path and compared against the paths a chart pins.
        let _ = writeln!(
            out,
            "\n    values paths pinning an image that carries no contract: {}",
            declaration.unconfigured.join(", ")
        );
    }
    out
}

/// The same selection, for something other than a person.
///
/// Every entry is emitted as the contract published it, with the readers and the two derived
/// readings added. Deriving those here rather than leaving them to the consumer is the point of the
/// format: they are the rules a reimplementation gets wrong.
pub(crate) fn explained_json(
    surface: &helm::explain::Surface,
    pattern: Option<&str>,
) -> serde_json::Value {
    use helm::explain::select;

    let mut readers: Vec<&helm::explain::Reader> = surface.readers.iter().collect();
    readers.sort_by(|left, right| left.name.cmp(&right.name));
    let images: Vec<serde_json::Value> = readers
        .iter()
        .map(|reader| {
            serde_json::json!({
                "name": reader.name,
                "contract": reader.contract,
                "image": reader.image,
                "digest": reader.digest,
                "app": reader.app,
                "version": reader.version,
                "documents": reader.documents,
                "reload": reader.reload,
            })
        })
        .collect();

    serde_json::json!({
        "chart": surface.chart,
        "dialect": surface.dialect,
        "unknown": surface.unknown,
        "ignore": surface.ignore,
        "pattern": pattern,
        "images": images,
        "keys": select(&surface.keys, pattern)
            .iter()
            .map(|setting| setting_json(setting, true))
            .collect::<Vec<_>>(),
        "loader": select(&surface.loader, pattern)
            .iter()
            .map(|setting| setting_json(setting, false))
            .collect::<Vec<_>>(),
        "external": select(&surface.external, pattern)
            .iter()
            .map(|setting| setting_json(setting, true))
            .collect::<Vec<_>>(),
    })
}

/// One setting as the contract published it, plus what this build derives from it.
pub(crate) fn setting_json(setting: &helm::explain::Setting, derive: bool) -> serde_json::Value {
    let entry = setting.representative();
    let mut held = entry.clone();
    held.insert("readers".to_owned(), serde_json::json!(setting.readers()));
    if derive {
        let window = terrace_contract::value::Entry(&entry);
        held.insert(
            "text_form".to_owned(),
            serde_json::json!(
                window
                    .text_form()
                    .map_or("unknown", terrace_contract::document::TextForm::label)
            ),
        );
        held.insert(
            "file_supplyable".to_owned(),
            serde_json::json!(window.file_supplyable().unwrap_or(false)),
        );
    }
    let divergent = setting.divergent();
    if !divergent.is_empty() {
        held.insert(
            "divergent".to_owned(),
            divergent
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        serde_json::json!(
                            setting
                                .variants(name)
                                .into_iter()
                                .map(|(value, readers)| serde_json::json!({
                                    "value": value,
                                    "readers": readers,
                                }))
                                .collect::<Vec<_>>()
                        ),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
                .into(),
        );
    }
    serde_json::Value::Object(held)
}
