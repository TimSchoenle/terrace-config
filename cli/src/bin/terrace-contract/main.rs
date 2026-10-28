//! The `terrace-contract` binary: argument parsing, output and exit codes.
//!
//! Every rule lives in the library, returns a value, and decides nothing about where it is printed
//! or whether the run fails. That split is what lets a Gradle plugin, a build script or a test call
//! the same code without going through a process — and it is what keeps the rules testable by
//! calling them rather than by capturing stdout.
//!
//! `args` holds the clap types and their help text, `settings` resolves terrace.yaml, `output`
//! writes a report and picks the exit code, and `commands` has one module per subcommand group.

mod args;
mod commands;
mod output;
mod settings;

use std::process::ExitCode;

use clap::Parser;
use terrace_contract::Error;
use terrace_contract::helm::Selection;

use crate::args::{Cli, Command, HelmCommand, ImageCommand};
use crate::commands::adopt::adopt_command;
use crate::commands::config::{config_command, doctor_command};
use crate::commands::diff::{diff_command, migrate_command};
use crate::commands::document::{conform_command, render_command, stamp_command, validate_command};
use crate::commands::explain::explain_command;
use crate::commands::gates::{
    bindings_command, check_command, coverage_command, readme_command, reload_command,
    shapes_command, tests_command,
};
use crate::commands::image::{Verify, attach_command, verify_command};
use crate::commands::init::helm_init_command;
use crate::commands::secrets::secrets_command;
use crate::commands::sync::{pull_command, scope_command, sync_command};

/// The release, then the document versions it reads, so a pin can be checked against a producer.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (reads terrace_contract 1, schema_version 1 to 4)"
);

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            // 2, not 1: a document this tool could not read is a different outcome from a document
            // it read and found wanting, and a pipeline that treats them the same cannot tell a
            // failing gate from a broken one.
            ExitCode::from(2)
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one arm per subcommand, each a single call into its handler; the length is the \
              number of subcommands, and splitting the match would only hide that list"
)]
fn run(cli: Cli) -> Result<ExitCode, Error> {
    let repo = cli.repo;
    match cli.command {
        Command::Render { .. } => render_command(cli.command),
        Command::Stamp { .. } => stamp_command(cli.command),
        Command::Conform {
            input,
            tier,
            no_schema,
        } => conform_command(&input, tier, no_schema),

        Command::Validate { input } => validate_command(&input),

        Command::Check {
            only,
            manifests,
            charts,
            format,
        } => check_command(
            &repo.charts(charts)?,
            &Selection::of(only),
            &manifests,
            format,
        ),

        Command::Bindings {
            only,
            charts,
            format,
        } => bindings_command(&repo.charts(charts)?, &Selection::of(only), format),

        Command::Adopt(adopt) => Ok(adopt_command(&repo, adopt)),

        Command::Shapes {
            only,
            charts,
            check,
            format,
        } => shapes_command(&repo.charts(charts)?, &Selection::of(only), check, format),

        Command::Diff {
            chart,
            since,
            charts,
            json,
            exit_code,
        } => diff_command(
            &repo.charts(charts)?,
            &since,
            chart.as_deref(),
            json,
            exit_code,
        ),

        Command::Migrate { .. } => Ok(migrate_command(&repo, cli.command)),

        Command::Secrets {
            reconcile,
            charts,
            json,
            exit_code,
        } => secrets_command(&repo.charts(charts)?, reconcile.as_deref(), json, exit_code),

        Command::Readme {
            only,
            charts,
            check,
        } => readme_command(&repo.charts(charts)?, &Selection::of(only), check),
        Command::Reload { charts, check } => reload_command(&repo.charts(charts)?, check),

        Command::Tests {
            only,
            charts,
            check,
        } => tests_command(&repo.charts(charts)?, &Selection::of(only), check),

        Command::Explain {
            chart,
            pattern,
            charts,
            full,
            json,
        } => explain_command(
            &repo.charts(charts)?,
            chart.as_deref(),
            pattern.as_deref(),
            full,
            json,
        ),

        Command::Scope { .. } => scope_command(&repo, cli.command),

        Command::Sync { .. } => sync_command(&repo, cli.command),

        Command::Pull {
            chart,
            charts,
            signer,
            check,
        } => pull_command(&repo, charts, chart.as_deref(), signer, check),
        Command::Coverage {
            charts,
            first_party,
            format,
        } => coverage_command(&repo, charts, first_party.as_deref(), format),

        Command::Helm { command } => match command {
            HelmCommand::Init(init) => Ok(helm_init_command(&repo, init)),
        },

        Command::Image { command } => match command {
            ImageCommand::Verify {
                input,
                labels,
                dockerfile,
                image,
                oci_layout,
                signer,
                path,
            } => verify_command(
                &input,
                &Verify {
                    labels: labels.as_deref(),
                    dockerfile: dockerfile.as_deref(),
                    image: image.as_deref(),
                    oci_layout,
                    signer: signer.as_deref(),
                    path: &path,
                },
            ),
            ImageCommand::Attach {
                contract,
                image,
                oci_layout,
            } => attach_command(&contract, &image, oci_layout),
        },

        Command::Config { command } => config_command(command, &repo),

        Command::Doctor => doctor_command(&repo),
    }
}

#[cfg(test)]
mod version_tests {
    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory as _;
        super::Cli::command().debug_assert();
    }

    #[test]
    fn the_version_line_states_what_the_library_reads() {
        let suffix = format!("({})", terrace_contract::document::COMPATIBILITY);
        assert!(super::VERSION.ends_with(&suffix), "{}", super::VERSION);
    }
}
