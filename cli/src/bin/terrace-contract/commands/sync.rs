//! `scope`, `sync` and `pull`: choosing the charts a change touches, and running every step over
//! them.

use std::path::PathBuf;
use std::process::ExitCode;

use terrace_contract::helm::Selection;
use terrace_contract::{Error, helm, oci};

use crate::args::{Command, Output};
use crate::commands::gates::{
    bindings_command, check_command, coverage_command, readme_command, shapes_command,
    tests_command,
};
use crate::settings::{Repo, required_signer};

/// Prints the charts a change touched, or an empty line for every chart.
pub(crate) fn scope_command(repo: &Repo, command: Command) -> Result<ExitCode, Error> {
    let Command::Scope {
        since,
        charts,
        json: as_json,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };
    let charts = repo.charts(charts)?;
    let since = since.as_str();
    let scope = helm::scope::since(&charts, since);
    if as_json {
        let (every, reason) = match &scope {
            helm::Scope::Every(why) => (true, Some(why.to_string())),
            helm::Scope::Charts(_) => (false, None),
        };
        println!(
            "{:#}",
            serde_json::json!({
                "since": since,
                "every": every,
                "charts": scope.names(),
                "reason": reason,
            })
        );
    } else {
        if let helm::Scope::Every(why) = &scope {
            eprintln!("==> every chart: {why}");
        }
        println!("{}", scope.names().join(" "));
    }
    Ok(ExitCode::SUCCESS)
}

/// The steps of one `sync` run and how each ended.
pub(crate) struct Steps {
    format: Output,
    failed: Vec<&'static str>,
    broken: Vec<&'static str>,
    ran: usize,
}

impl Steps {
    pub(crate) const fn new(format: Output) -> Self {
        Self {
            format,
            failed: Vec::new(),
            broken: Vec::new(),
            ran: 0,
        }
    }

    /// Runs one step under a heading, and records how it ended without stopping the run.
    pub(crate) fn run(
        &mut self,
        name: &'static str,
        step: impl FnOnce() -> Result<ExitCode, Error>,
    ) {
        self.ran += 1;
        if self.format == Output::Github {
            println!("::group::sync: {name}");
        } else {
            println!("==> sync: {name}");
        }
        match step() {
            Ok(code) if code == ExitCode::SUCCESS => {}
            Ok(_) => self.failed.push(name),
            Err(error) => {
                eprintln!("{error}");
                self.broken.push(name);
            }
        }
        if self.format == Output::Github {
            println!("::endgroup::");
        }
    }

    /// The exit code: 2 when a step could not run, else 1 when a step found something, else 0.
    pub(crate) fn finish(self, mode: &str) -> ExitCode {
        let unclean = self.failed.len() + self.broken.len();
        if unclean == 0 {
            println!("==> {mode}: {} step(s) passed", self.ran);
            return ExitCode::SUCCESS;
        }
        let mut names: Vec<&str> = self.broken.clone();
        names.extend(&self.failed);
        eprintln!(
            "==> {mode}: {unclean} of {} step(s) did not pass: {}",
            self.ran,
            names.join(", ")
        );
        if self.broken.is_empty() {
            ExitCode::FAILURE
        } else {
            ExitCode::from(2)
        }
    }
}

/// Writes every contract-derived file, or runs every offline gate over the selected charts.
pub(crate) fn sync_command(repo: &Repo, command: Command) -> Result<ExitCode, Error> {
    let Command::Sync {
        only,
        since,
        check,
        manifests,
        no_pull,
        charts,
        signer,
        format,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };
    if format == Output::Json {
        return Err(Error::Invalid(
            "sync writes several reports, which one JSON object cannot hold; pass --format text \
             or --format github, or run the gates one at a time for JSON"
                .to_owned(),
        ));
    }
    let charts = repo.charts(charts)?;
    let selection = match &since {
        Some(base) => {
            let scope = helm::scope::since(&charts, base);
            match &scope {
                helm::Scope::Every(why) => println!("==> sync: every chart, because {why}"),
                helm::Scope::Charts(names) => println!("==> sync: {}", names.join(" ")),
            }
            Selection::of(scope.names().to_vec())
        }
        None => Selection::of(only),
    };
    // Resolved before any step runs, so a name the tree does not hold is refused up front rather
    // than after a write step already ran for the other charts.
    let names: Vec<String> = selection
        .dirs(&charts)?
        .iter()
        .filter_map(|dir| {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();

    let mut steps = Steps::new(format);

    if check {
        steps.run("bindings", || bindings_command(&charts, &selection, format));
        steps.run("shapes", || {
            shapes_command(&charts, &selection, true, format)
        });
        steps.run("readme", || readme_command(&charts, &selection, true));
        steps.run("tests", || tests_command(&charts, &selection, true));
        if let Some(manifests) = &manifests {
            steps.run("check", || {
                check_command(&charts, &selection, manifests, format)
            });
        }
        // Estate-wide whatever the selection: whether any chart pins a first-party image without
        // a contract is a question about the tree, not about the charts a change touched. Only
        // when the repository names its first-party images: without that list the question has
        // no answer, and a repository that never configured one is not misconfigured.
        let settings = repo.load()?;
        let configured = settings
            .file
            .as_ref()
            .is_some_and(|file| file.helm.first_party.is_some())
            || settings.default_first_party().is_file();
        if configured {
            steps.run("coverage", || {
                coverage_command(repo, Some(charts.clone()), None, format)
            });
        } else {
            println!("==> sync: coverage skipped, because terrace.yaml sets no helm.firstParty");
        }
        return Ok(steps.finish("sync --check"));
    }

    if !no_pull {
        steps.run("pull", || {
            if selection.is_all() {
                return pull_command(repo, Some(charts.clone()), None, signer.clone(), false);
            }
            // One chart per call, which is what `pull` takes. Every chart is attempted; the step
            // then reports the first chart that could not run, else whether any found a problem.
            let mut worst = ExitCode::SUCCESS;
            let mut first_error = None;
            for name in &names {
                match pull_command(
                    repo,
                    Some(charts.clone()),
                    Some(name),
                    signer.clone(),
                    false,
                ) {
                    Ok(code) if code == ExitCode::SUCCESS => {}
                    Ok(code) => worst = code,
                    // The first is returned for the step to report; any later one is reported
                    // here, so no chart's failure goes unsaid.
                    Err(error) if first_error.is_none() => first_error = Some(error),
                    Err(error) => eprintln!("{error}"),
                }
            }
            first_error.map_or(Ok(worst), Err)
        });
    }
    // After `pull` and in this order, because each reads what the one before it wrote: the
    // schemas derive from the refreshed contracts, and the README reference from the same.
    steps.run("shapes", || {
        shapes_command(&charts, &selection, false, format)
    });
    steps.run("readme", || readme_command(&charts, &selection, false));
    Ok(steps.finish("sync"))
}

/// Refresh the vendored contracts, or say which of them are behind their image.
pub(crate) fn pull_command(
    repo: &Repo,
    charts: Option<PathBuf>,
    chart: Option<&str>,
    signer: Option<String>,
    check: bool,
) -> Result<ExitCode, Error> {
    let settings = repo.settings()?;
    let signer = required_signer(&settings, signer)?;
    let signer = signer.as_str();
    let charts = settings.charts(charts).0;
    let charts = charts.as_path();
    let tools = oci::Tools::found()?;
    let refreshed = if check {
        // Written into a scratch tree rather than over the chart's own: `--check` answers whether
        // the committed bytes are current, and answering it by writing them would make the answer
        // true by construction.
        let scratch = helm::pull::Scratch::of(charts)?;
        helm::pull::refresh(scratch.path(), chart, &tools, signer, &helm::pull::now())?
    } else {
        helm::pull::refresh(charts, chart, &tools, signer, &helm::pull::now())?
    };

    for path in &refreshed.changed {
        if check {
            println!("{}: is behind the image its chart pins", path.display());
        } else {
            println!("==> updated {}", path.display());
        }
    }
    for path in &refreshed.unchanged {
        if !check {
            println!("==> unchanged {}", path.display());
        }
    }
    for problem in &refreshed.problems {
        eprintln!("{problem}");
    }

    if !refreshed.problems.is_empty() {
        eprintln!(
            "\nerror: {} chart(s) could not be refreshed",
            refreshed.problems.len()
        );
        return Ok(ExitCode::from(1));
    }
    if check {
        if refreshed.changed.is_empty() {
            println!(
                "==> {} vendored contract(s) are the ones their images publish",
                refreshed.unchanged.len()
            );
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!(
            "\nerror: {} vendored contract(s) are behind; refresh them",
            refreshed.changed.len()
        );
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}
