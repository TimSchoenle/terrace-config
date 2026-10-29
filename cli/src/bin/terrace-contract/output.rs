//! Writing a run's findings to the streams, and the exit code that follows from them.

use std::process::ExitCode;

use terrace_contract::report::Report;

use crate::args::Output;

/// Writes a report the way the caller asked for, and decides the exit code.
///
/// The rules returned findings and decided none of this. Which is the whole split: a build script, a
/// test or a plugin calls the same code and never goes through a process.
pub(crate) fn write_report(
    report: &Report,
    format: Output,
    headline: &str,
    clean: &str,
) -> ExitCode {
    match format {
        Output::Json => println!("{}", report.json()),
        Output::Text | Output::Github => {
            let (out, errors) = report.text();
            print!("{out}");
            eprint!("{errors}");
        }
    }

    if format == Output::Github
        && let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY")
    {
        // A summary that could not be appended is not worth failing a gate over: the findings
        // themselves already went to the streams above.
        if let Ok(mut handle) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            use std::io::Write as _;
            let _ = handle.write_all(report.step_summary(headline, clean).as_bytes());
        }
    }

    let failures = report.error_count();
    if failures == 0 {
        return ExitCode::SUCCESS;
    }
    eprintln!(
        "
{failures} {}",
        if failures == 1 {
            "configuration contract violation"
        } else {
            "configuration contract violations"
        }
    );
    // 1, not 2: the tool worked and the tree is wrong.
    ExitCode::FAILURE
}

/// Prints a list of findings, or says nothing and succeeds.
pub(crate) fn report_lines(findings: &[String], headline: &str) -> ExitCode {
    if findings.is_empty() {
        return ExitCode::SUCCESS;
    }
    eprintln!("{headline}");
    for finding in findings {
        eprintln!("  {finding}");
    }
    // 1, not 2: the tool worked and the document is wrong.
    ExitCode::FAILURE
}
