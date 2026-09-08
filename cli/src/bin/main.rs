//! `terrace-contract` — the argument parsing and the exit codes, and nothing else.
//!
//! Every rule lives in the library, returns a value, and decides nothing about where it is printed
//! or whether the run fails. That split is what lets a Gradle plugin, a build script or a test call
//! the same code without going through a process — and it is what keeps the rules testable by
//! calling them rather than by capturing stdout.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use terrace_contract::render::{self, Column, Format, Options, image};
use terrace_contract::{Contract, DEFAULT_PATH, Error};

/// Read, render and check configuration contracts.
#[derive(Parser)]
#[command(
    name = "terrace-contract",
    version,
    about = "Read, render and check configuration contracts, whatever produced them.",
    long_about = "Everything the configuration contract needs after a document exists.\n\n\
                  Producing a contract needs the service's own types and is per-language. \
                  Everything downstream of the document needs the document and nothing else, \
                  which is this. See spec/v1/FORMAT.md for what a document is."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a document as a table, a file, or the labels an image carries.
    Render {
        #[command(flatten)]
        input: Input,

        /// Which rendering to emit.
        #[arg(short, long, default_value = "json")]
        format: Format,

        /// Where the document lives inside the image, for the label formats.
        #[arg(long, default_value = DEFAULT_PATH)]
        path: String,

        /// Which columns the key table carries, comma-separated.
        #[arg(long, value_delimiter = ',')]
        columns: Vec<Column>,
    },

    /// Write a build's identity onto a document, changing nothing else.
    Stamp {
        #[command(flatten)]
        input: Input,

        /// The version the image tag carries, e.g. `v2.5.0`.
        #[arg(long)]
        app_version: Option<String>,

        /// The commit the build came from.
        #[arg(long)]
        revision: Option<String>,

        /// When the build happened, as RFC 3339.
        #[arg(long)]
        created: Option<String>,

        /// Where the source lives.
        #[arg(long)]
        source: Option<String>,
    },

    /// Check a built image against the document it claims to carry.
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },
}

#[derive(Subcommand)]
enum ImageCommand {
    /// Hold an image's labels, or a Dockerfile's committed block, against the document.
    ///
    /// The two checks answer different questions and neither replaces the other. The Dockerfile
    /// check is cheap and runs in the pull request that renamed a key, in a diff a reviewer reads.
    /// The label check costs an image and catches what no source diff can: a build argument that
    /// failed to interpolate, a label a base image overrode, a `LABEL` line deleted on a branch
    /// nobody diffed.
    Verify {
        #[command(flatten)]
        input: Input,

        /// What `docker inspect` or `crane config` printed.
        #[arg(long, value_name = "FILE")]
        labels: Option<PathBuf>,

        /// A Dockerfile whose committed label block should match this document.
        #[arg(long, value_name = "FILE")]
        dockerfile: Option<PathBuf>,

        /// Where the document lives inside the image.
        #[arg(long, default_value = DEFAULT_PATH)]
        path: String,
    },
}

/// The document every subcommand starts from.
///
/// A path, or `-` for stdin. Stdin is not an afterthought: the producing step and the rendering
/// step are two stages of one pipeline, and a build that has to land a temporary file between them
/// has to clean it up and has one more thing to get wrong.
#[derive(Args)]
struct Input {
    /// The contract document, or `-` to read stdin.
    #[arg(value_name = "CONTRACT", default_value = "-")]
    contract: String,
}

impl Input {
    fn read(&self) -> Result<Contract, Error> {
        let text = if self.contract == "-" {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| Error::io("<stdin>", e))?;
            buffer
        } else {
            read(Path::new(&self.contract))?
        };
        Contract::from_json(&text)
    }
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))
}

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

fn run(cli: Cli) -> Result<ExitCode, Error> {
    match cli.command {
        Command::Render {
            input,
            format,
            path,
            columns,
        } => {
            let contract = input.read()?;
            let columns = if columns.is_empty() {
                Column::DEFAULT.to_vec()
            } else {
                columns
            };
            let options = Options {
                path: &path,
                columns: &columns,
            };
            // Exactly one trailing newline, whatever the rendering ended with, because a build
            // redirects this into a committed file and a trailing blank line is invisible on a
            // terminal and a diff in the file.
            print!(
                "{}",
                render::one_newline(render::render(&contract, format, &options)?)
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Stamp {
            input,
            app_version,
            revision,
            created,
            source,
        } => {
            let mut contract = input.read()?;
            // Each field is set only when given, so stamping twice with different arguments
            // accumulates rather than clearing what the first run wrote.
            if app_version.is_some() {
                contract.app.version = app_version;
            }
            if revision.is_some() {
                contract.app.revision = revision;
            }
            if created.is_some() {
                contract.app.created = created;
            }
            if source.is_some() {
                contract.app.source = source;
            }
            print!(
                "{}",
                render::one_newline(render::render(
                    &contract,
                    Format::Contract,
                    &Options::default()
                )?)
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Image {
            command:
                ImageCommand::Verify {
                    input,
                    labels,
                    dockerfile,
                    path,
                },
        } => {
            if labels.is_none() && dockerfile.is_none() {
                return Err(Error::Invalid(
                    "nothing to verify against: pass --labels, --dockerfile, or both. A verify \
                     with neither would report success without comparing anything, which is the \
                     one outcome this check cannot afford."
                        .to_owned(),
                ));
            }

            let contract = input.read()?;
            let mut failed = false;

            if let Some(path_to_labels) = &labels {
                let found = image::labels_from_json(&read(path_to_labels)?)?;
                if let Some(report) = image::report(&image::check_labels(&contract, &path, &found))
                {
                    eprintln!("{report}");
                    failed = true;
                }
            }

            if let Some(path_to_dockerfile) = &dockerfile {
                let text = read(path_to_dockerfile)?;
                let committed = image::committed_block(&text)?.replace("\r\n", "\n");
                let expected = image::dockerfile_labels(&contract, &path);
                if committed.trim_end() != expected.trim_end() {
                    eprintln!(
                        "{}: the committed label block is not the one this document renders.\n\
                         --- committed\n{committed}\n--- expected\n{}",
                        path_to_dockerfile.display(),
                        expected.trim_end()
                    );
                    failed = true;
                }
            }

            // 1, not 2: the tool worked and the image is wrong.
            Ok(if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}
