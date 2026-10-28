//! The chart gates that read a chart tree and report findings: `check`, `bindings`, `shapes`,
//! `coverage`, `readme`, `reload` and `tests`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use terrace_contract::helm::Selection;
use terrace_contract::report::Report;
use terrace_contract::{Error, helm};

use crate::args::Output;
use crate::output::write_report;
use crate::settings::Repo;

/// Hold every selected chart's `@config` markers against the contracts they name.
pub(crate) fn bindings_command(
    charts: &Path,
    selection: &Selection,
    format: Output,
) -> Result<ExitCode, Error> {
    let found = helm::check_bindings(charts, selection)?;
    if format == Output::Text || format == Output::Github {
        for (chart, keys, external) in &found.enrolled {
            println!(
                "bound: {chart} ({keys} contract key(s), {external} declared external variable(s))"
            );
        }
        if found.enrolled.is_empty() {
            println!("==> no chart carries a `@config` marker; nothing to check");
        }
    }
    Ok(write_report(
        &found.report,
        format,
        "Configuration bindings",
        "Every contract key of every enrolled chart is bound by a value, or written off with a \
         reason, and every bound key is rendered by a template.",
    ))
}

/// Write or check every generated `@schema` block.
///
/// One walk, two endings. The writer replaces a block that does not match; the checker reports it
/// and adds the one assertion the writer deliberately does not make — that a hand transcription is
/// still current — because a writer that refused over it would turn the job that repairs the tree
/// into a gate over the one thing in it no repair reaches.
pub(crate) fn shapes_command(
    charts: &Path,
    selection: &Selection,
    check: bool,
    format: Output,
) -> Result<ExitCode, Error> {
    let mut report = Report::new();
    let mut written = 0;
    let mut derived = 0;
    let mut transcriptions = 0;

    for chart_dir in selection.dirs(charts)? {
        if !chart_dir.join("values.yaml").is_file() {
            continue;
        }
        let mut chart = helm::shapes::Chart::read(&chart_dir)?;

        derived += chart
            .shapes
            .iter()
            .filter(|shape| !shape.handwritten)
            .count();
        transcriptions += chart
            .shapes
            .iter()
            .filter(|shape| shape.handwritten)
            .count();

        if check {
            helm::shapes::check(&mut chart);
            let transcribed: Vec<_> = chart
                .shapes
                .iter()
                .filter(|shape| shape.handwritten)
                .cloned()
                .collect();
            for shape in &transcribed {
                chart.check_handwritten(shape);
            }
        } else {
            let (text, count) = helm::shapes::rewrite(&mut chart);
            if count > 0 {
                std::fs::write(chart_dir.join("values.yaml"), text)
                    .map_err(|e| Error::io(chart_dir.join("values.yaml").display(), e))?;
                written += count;
                if format != Output::Json {
                    println!("wrote: {} ({count} block(s))", chart.name);
                }
            }
        }

        for problem in &chart.problems {
            report.fail(format!("{}: values.yaml", chart.name), problem.clone());
        }
    }

    if format == Output::Text || format == Output::Github {
        if check && !report.failed() && derived + transcriptions == 0 {
            // A run that examined nothing must not read like a clean one.
            println!("==> no selected chart carries a config shape; nothing to check");
        } else if check && !report.failed() {
            println!(
                "==> {derived} generated and {transcriptions} hand-transcribed config shape(s), all current"
            );
        } else if !check && written == 0 {
            println!("==> every generated `@schema` block already matches its contract");
        }
    }

    Ok(write_report(
        &report,
        format,
        "Derived value schemas",
        "Every bound value's `@schema` block is the one its contract describes.",
    ))
}

/// Hold every rendered document, container environment and secret mount against its contract.
pub(crate) fn check_command(
    charts: &Path,
    selection: &Selection,
    manifests: &Path,
    format: Output,
) -> Result<ExitCode, Error> {
    if !manifests.is_dir() {
        return Err(Error::Invalid(format!(
            "{}: no rendered manifests; render the charts into it first",
            manifests.display()
        )));
    }
    let checked = helm::check(charts, selection, manifests)?;
    if checked.charts == 0 && format != Output::Json {
        // Worth saying out loud: a run that validated nothing looks exactly like a clean one from
        // the outside.
        println!("==> no chart declares a configuration contract; nothing to validate");
    }
    Ok(write_report(
        &checked.report,
        format,
        "Configuration contracts",
        "Every rendered document, container environment and secret mount matches the contract of \
         the image its chart pins.",
    ))
}

/// Which charts a contract covers, and which pin a first-party image without one.
pub(crate) fn coverage_command(
    repo: &Repo,
    charts: Option<PathBuf>,
    first_party: Option<&Path>,
    format: Output,
) -> Result<ExitCode, Error> {
    let settings = repo.settings()?;
    let (patterns, _) = settings.first_party(first_party)?;
    let found = helm::coverage(&settings.charts(charts).0, &patterns)?;
    if format == Output::Text || format == Output::Github {
        for name in &found.covered {
            println!("covered: {name}");
        }
        if found.covered.is_empty() && found.uncovered.is_empty() {
            println!("==> no chart pins a first-party image");
        }
    }
    Ok(write_report(
        &found.report,
        format,
        "Contract coverage",
        "Every chart pinning a first-party image declares a configuration contract.",
    ))
}

/// Write, or compare, every chart's generated credential reference.
pub(crate) fn readme_command(
    charts: &Path,
    selection: &Selection,
    check: bool,
) -> Result<ExitCode, Error> {
    let written = helm::readme::walk(charts, selection, check)?;

    for problem in &written.problems {
        eprintln!("{problem}");
    }
    if !written.problems.is_empty() {
        eprintln!(
            "\nerror: {} credential reference problem(s)",
            written.problems.len()
        );
        return Ok(ExitCode::from(1));
    }

    if check {
        println!("==> every credential reference is current");
    } else if written.touched > 0 {
        println!("==> rewrote {} credential reference(s)", written.touched);
    } else {
        println!("==> every credential reference already matches its contract");
    }
    Ok(ExitCode::SUCCESS)
}

/// Write, or compare, every chart's generated restart sets.
pub(crate) fn reload_command(charts: &Path, check: bool) -> Result<ExitCode, Error> {
    let written = helm::restarts::walk(charts, check)?;

    for problem in &written.problems {
        eprintln!("{problem}");
    }
    if !written.problems.is_empty() {
        eprintln!("\nerror: {} restart set problem(s)", written.problems.len());
        return Ok(ExitCode::from(1));
    }

    if check {
        println!("==> every restart set is current");
    } else if written.touched > 0 {
        println!("==> rewrote {} restart set file(s)", written.touched);
    } else {
        println!("==> every restart set already matches its contracts");
    }
    Ok(ExitCode::SUCCESS)
}

/// Write, or compare, every enrolled chart's generated round-trip suites.
pub(crate) fn tests_command(
    charts: &Path,
    selection: &Selection,
    check: bool,
) -> Result<ExitCode, Error> {
    let generated = helm::suites::collect(charts, selection)?;
    for chart in &generated.unenrolled {
        println!("==> {chart}: not enrolled, skipping");
    }

    if check {
        let outcome = helm::suites::check(&generated);
        if outcome.drift.is_empty() {
            println!(
                "==> {} generated suite(s) are in step with their contracts",
                generated.suites.len()
            );
            return Ok(ExitCode::SUCCESS);
        }
        for entry in &outcome.drift {
            eprintln!("{entry}");
        }
        eprintln!("regenerate them to bring the tree back into step");
        return Ok(ExitCode::from(1));
    }

    let outcome = helm::suites::sync(&generated)?;
    for path in &outcome.written {
        println!("==> {}: written", path.display());
    }
    for path in &outcome.removed {
        println!(
            "==> {}: removed, its document is no longer declared",
            path.display()
        );
    }
    if outcome.written.is_empty() && outcome.removed.is_empty() {
        println!(
            "==> {} generated suite(s) were already in step with their contracts",
            generated.suites.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}
