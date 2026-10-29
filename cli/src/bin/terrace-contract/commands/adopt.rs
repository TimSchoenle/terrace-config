//! `adopt`, which writes the keys a newer contract adds into a configured chart.

use std::path::PathBuf;
use std::process::ExitCode;

use terrace_contract::{Error, helm};

use crate::args::Adopt;
use crate::settings::Repo;

/// Plan, check, diff or write the values every selected enrolled chart is owed.
///
/// At most one of `check`, `diff` and `write` is set. The exit codes are the ones the
/// command this replaced used, so the recipes that call it need no change: 2 for a selection that
/// names no chart or a `--key` with no chart, 1 for a refused key, a blocked chart or a tree that
/// could not be read, 0 otherwise. `--check` alone departs from that, to the convention `sync
/// --check` reads: 1 for anything owed, 2 for a tree it could not read.
pub(crate) fn adopt_command(repo: &Repo, options: Adopt) -> ExitCode {
    use helm::adopt;

    let Adopt {
        chart,
        keys,
        charts,
        check,
        diff,
        write,
    } = options;
    // An empty name selects every chart, as the recipe that passes `''` through expects.
    let chart = chart.as_deref().filter(|name| !name.is_empty());

    // Everything that reads the tree runs in here, the chart list included, so that a tree that
    // cannot be read exits the way the contract above says rather than the way `main` would.
    let run = || -> Result<ExitCode, Error> {
        let charts = &repo.charts(charts)?;
        let chart_dirs: Vec<PathBuf> = helm::declaration::chart_dirs(charts)?
            .into_iter()
            .filter(|dir| chart.is_none_or(|name| dir.file_name() == Some(name.as_ref())))
            .collect();
        if let Some(name) = chart
            && chart_dirs.is_empty()
        {
            eprintln!("error: {} is not a chart", adopt::posix(&charts.join(name)));
            return Ok(ExitCode::from(2));
        }
        if !keys.is_empty() && chart.is_none() {
            eprintln!("error: --key selects keys of one chart, so name the chart");
            return Ok(ExitCode::from(2));
        }

        let wanted = keys.iter().cloned().collect();
        let adoptions = adopt::adopt_all(&chart_dirs, &wanted)?;
        if adoptions.is_empty() {
            if !diff {
                println!("==> every contract key of every enrolled chart is bound or written off");
            }
            return Ok(ExitCode::SUCCESS);
        }
        let refused = adoptions
            .iter()
            .any(|item| !item.blocked.is_empty() || !item.refusals.is_empty());

        if diff {
            for adoption in &adoptions {
                for splice in adopt::splices(adoption)? {
                    print!("{}", splice.diff());
                }
                for message in &adoption.blocked {
                    eprintln!("{}: blocked: {message}", adoption.chart);
                }
                for refusal in &adoption.refusals {
                    eprintln!(
                        "{}: refused: {} {}",
                        adoption.chart, refusal.path, refusal.reason
                    );
                }
            }
        } else {
            // Every file of every chart, or none: see `write_all`.
            if write {
                adopt::write_all(&adoptions)?;
            }
            for adoption in &adoptions {
                print!("{}", adopt::report(adoption, write)?);
            }
            let rows = if write {
                adopt::counted(charts, &adoptions)?
            } else {
                Vec::new()
            };
            print!("{}", adopt::closing(&adoptions, write, &rows));
        }

        Ok(if check || refused {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    };

    run().unwrap_or_else(|failure| {
        eprintln!("error: {failure}");
        ExitCode::from(if check { 2 } else { 1 })
    })
}
