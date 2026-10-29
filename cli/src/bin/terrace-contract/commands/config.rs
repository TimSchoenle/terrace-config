//! The `config` subcommands and `doctor`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use terrace_contract::repo::{self, Settings, Source};
use terrace_contract::{Error, helm, oci};

use crate::args::ConfigCommand;
use crate::settings::{Repo, environment_charts, environment_signer, unknown_key_lines, variable};

/// Dispatch the `config` subcommands: `show` (the settings, or one chart's declaration) and `init`.
pub(crate) fn config_command(command: ConfigCommand, repo: &Repo) -> Result<ExitCode, Error> {
    match command {
        ConfigCommand::Show {
            json,
            chart: Some(chart),
            charts,
        } => {
            // `--charts` cannot carry `env` here: it requires `--chart`, so a set TERRACE_CHARTS
            // would refuse a plain `config show`. The variable is read in its place instead.
            let charts = charts.or_else(|| variable("TERRACE_CHARTS").map(PathBuf::from));
            config_show_chart_command(&repo.charts(charts)?, &chart, json)
        }
        ConfigCommand::Show { json, .. } => config_show_command(&repo.settings()?, json),
        ConfigCommand::Init { force } => config_init_command(repo.named.as_deref(), force),
    }
}

/// Print every setting with its value and the layer it came from.
pub(crate) fn config_show_command(settings: &Settings, json: bool) -> Result<ExitCode, Error> {
    let file = settings
        .file
        .as_ref()
        .map(|file| file.path.display().to_string());
    let cli = settings
        .file
        .as_ref()
        .and_then(|file| file.cli)
        .map(|pin| pin.to_string());
    let charts = environment_charts(settings);
    let charts = (charts.0.display().to_string(), charts.1);
    // Shown as the patterns themselves rather than the file holding them: the question is which
    // repositories count, and a missing default file is a finding here, not a reason to fail.
    let first_party = settings.first_party(None).map_or_else(
        |error| (Err(error.to_string()), Source::Default),
        |(patterns, source)| (Ok(patterns), source),
    );
    let signer = environment_signer(settings);

    if json {
        let first_party_value = match &first_party.0 {
            Ok(patterns) => serde_json::json!(patterns),
            Err(_) => serde_json::Value::Null,
        };
        let shown = serde_json::json!({
            "file": file,
            "cli": cli,
            "running": repo::running()?.to_string(),
            "charts": { "value": charts.0, "source": charts.1.name() },
            "firstParty": { "value": first_party_value, "source": first_party.1.name() },
            "signer": signer.as_ref().map(|(value, source)| {
                serde_json::json!({ "value": value, "source": source.name() })
            }),
            "unknown": settings.file.as_ref().map(|file| file.unknown.clone()).unwrap_or_default(),
        });
        println!("{shown:#}");
        return Ok(ExitCode::SUCCESS);
    }

    println!("file          {}", file.as_deref().unwrap_or("(none)"));
    println!(
        "cli           {} (running {})",
        cli.as_deref().unwrap_or("(not pinned)"),
        repo::running()?
    );
    println!("helm.charts   {}  [{}]", charts.0, charts.1.name());
    match &first_party.0 {
        Ok(patterns) if patterns.is_empty() => {
            println!("firstParty    (none)  [{}]", first_party.1.name());
        }
        Ok(patterns) => {
            println!(
                "firstParty    {}  [{}]",
                patterns.join(", "),
                first_party.1.name()
            );
        }
        Err(error) => println!("firstParty    (unavailable: {error})"),
    }
    match &signer {
        Some((value, source)) => println!("helm.signer   {value}  [{}]", source.name()),
        None => println!("helm.signer   (none; `pull` refuses to run without one)"),
    }
    Ok(ExitCode::SUCCESS)
}

/// Print one chart's declaration with every default written out.
pub(crate) fn config_show_chart_command(
    charts: &Path,
    chart: &str,
    json: bool,
) -> Result<ExitCode, Error> {
    let chart_dir = charts.join(chart);
    if !chart_dir.is_dir() {
        return Err(Error::Invalid(format!(
            "{}: no such chart directory",
            chart_dir.display()
        )));
    }
    let Some(declaration) = helm::load_declaration(&chart_dir)? else {
        return Err(Error::Invalid(format!(
            "{}: no such file; `{chart}` declares no contracts",
            chart_dir.join(helm::DECLARATION).display()
        )));
    };
    let enrolments = helm::suites::load_enrolment(&chart_dir, Some(&declaration))?;
    let shown = helm::effective::effective(&declaration, enrolments.as_ref());
    if json {
        println!("{:#}", shown.to_json());
    } else {
        print!("{}", shown.to_yaml());
    }
    Ok(ExitCode::SUCCESS)
}

/// Write a terrace.yaml, carrying over an existing first-party list.
pub(crate) fn config_init_command(named: Option<&Path>, force: bool) -> Result<ExitCode, Error> {
    let target = if let Some(path) = named {
        path.to_path_buf()
    } else {
        let here = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        repo::git_root(&here)
            .ok_or_else(|| {
                Error::Invalid(
                    "not inside a Git repository: terrace.yaml belongs at its root. Run this from \
                     inside one, or name the file with --config."
                        .to_owned(),
                )
            })?
            .join(repo::FILE_NAME)
    };
    if target.exists() && !force {
        return Err(Error::Invalid(format!(
            "{}: already exists; pass --force to replace it",
            target.display()
        )));
    }

    let root = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let list = root.join(helm::FIRST_PARTY);
    let patterns = if list.is_file() {
        helm::coverage::first_party_patterns(&list)?
    } else {
        Vec::new()
    };

    let text = repo::template(repo::running()?, &patterns);
    std::fs::write(&target, text).map_err(|e| Error::io(target.display(), e))?;
    println!("==> wrote {}", target.display());
    if !patterns.is_empty() {
        println!(
            "==> carried {} pattern(s) over from {}; that file is no longer read and can be deleted",
            patterns.len(),
            list.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Check the repository's settings and this machine's tools, one line each.
pub(crate) fn doctor_command(repo: &Repo) -> Result<ExitCode, Error> {
    let mut failures = 0_usize;
    let mut say = |level: &str, line: String| {
        if level == "fail" {
            failures += 1;
        }
        println!("{level:<4}  {line}");
    };

    // A file that cannot be read is the first thing to report, not a reason to stop: the checks
    // below still say what the defaults would do.
    let settings = repo.load().unwrap_or_else(|error| {
        say("fail", error.to_string());
        Settings::none()
    });
    let running = repo::running()?;
    match &settings.file {
        None => say(
            "warn",
            format!(
                "no {}: every setting comes from flags, the environment or defaults \
                 (`terrace-contract config init` writes one)",
                repo::FILE_NAME
            ),
        ),
        Some(file) => {
            say(
                "ok",
                format!("{}: version {}", file.path.display(), repo::VERSION),
            );
            for line in unknown_key_lines(file)? {
                say("fail", line);
            }
            match file.cli {
                None => say("warn", format!("no `cli` pin; running {running}")),
                Some(pin) if pin.accepts(running) => {
                    say("ok", format!("cli {pin} pinned, {running} running"));
                }
                Some(pin) => say(
                    "fail",
                    format!("cli {pin} pinned, but {running} is running: install {pin}"),
                ),
            }
        }
    }

    let (charts, source) = environment_charts(&settings);
    match helm::declaration::chart_dirs(&charts) {
        Ok(found) => say(
            "ok",
            format!(
                "charts: {} ({} chart(s), {})",
                charts.display(),
                found.len(),
                source.name()
            ),
        ),
        Err(error) => say("fail", format!("charts: {error}")),
    }

    match settings.first_party(None) {
        Ok((patterns, source)) => say(
            "ok",
            format!(
                "firstParty: {} pattern(s) ({})",
                patterns.len(),
                source.name()
            ),
        ),
        Err(error) => say(
            "warn",
            format!("firstParty: {error}; only `coverage` needs it"),
        ),
    }

    match environment_signer(&settings) {
        Some((_, source)) => say("ok", format!("signer: set ({})", source.name())),
        None => say("warn", "signer: none; only `pull` needs it".to_owned()),
    }

    match oci::Tools::found() {
        Ok(_) => say("ok", "oras and cosign: found".to_owned()),
        Err(error) => say("warn", format!("{error}")),
    }

    if failures == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!(
            "\n{failures} {}",
            if failures == 1 { "failure" } else { "failures" }
        );
        Ok(ExitCode::FAILURE)
    }
}
