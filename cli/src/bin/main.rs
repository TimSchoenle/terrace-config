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
use terrace_contract::helm::{CHARTS_DIR, FIRST_PARTY};
use terrace_contract::render::{
    self, Column, Docs, Format, JsonSchema, Options, TomlExample, image,
};
use terrace_contract::report::Report;
use terrace_contract::{Contract, DEFAULT_PATH, Error, Tier, conform, helm, validate};

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

        /// The JSON Schema's `title`.
        ///
        /// Per-service, and the one thing about that rendering a document cannot supply: a title
        /// is what an editor shows, and "configuration" is not it.
        #[arg(long)]
        title: Option<String>,

        /// The JSON Schema's `$id`.
        ///
        /// No default, deliberately. An `$id` is a URL under the service's own repository, and a
        /// wrong one is worse than none because an editor will try to resolve it.
        #[arg(long)]
        id: Option<String>,

        /// Emit a schema that permits keys it does not declare.
        ///
        /// Off by default: an unknown key is the defect this whole scheme exists to catch, and an
        /// open schema catches none of them.
        #[arg(long)]
        open: bool,

        /// Leave the preamble off the TOML rendering.
        ///
        /// For a file embedded in an image, or one whose page already says what it is.
        #[arg(long)]
        no_header: bool,

        /// Carry the whole doc comment above each key rather than its first paragraph.
        ///
        /// For a `config.example.toml` that is the only documentation an operator gets: it is read
        /// once while being filled in, and the paragraphs below the summary are what explain the
        /// shape.
        #[arg(long)]
        full_docs: bool,
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

    /// Hold a document to the specification, at a tier.
    ///
    /// This is the whole of a new implementation's obligation. `spec/v1/` says what a contract is;
    /// this says it back in a form a build can run, so a producer in any language answers "is what
    /// I emitted a contract?" by invoking this rather than by re-reading prose.
    Conform {
        #[command(flatten)]
        input: Input,

        /// Which tier to hold it to: 1 (document), 2 (dialect) or 3 (byte).
        ///
        /// Claim the one you meet. An implementation claiming a tier it does not meet is worse
        /// than one claiming none, because the point of the document is that a consumer can act on
        /// it without reading the producer's source.
        #[arg(long, default_value = "1")]
        tier: Tier,

        /// Skip the meta-schema check and run only the rules a schema cannot express.
        #[arg(long)]
        no_schema: bool,
    },

    /// Check a document against the published meta-schema, and nothing else.
    Validate {
        #[command(flatten)]
        input: Input,
    },

    /// Hold a rendered Kubernetes tree to the contracts of the images its charts pin.
    ///
    /// The one thing no other gate can see into. A schema for a chart's *values* describes the
    /// values; a Kubernetes validator describes Kubernetes objects, and a `ConfigMap` holding a stale
    /// configuration document is a perfectly valid `ConfigMap`. So on the day an application renames a
    /// key, every other gate passes, the pod starts, reports healthy, and runs on a compiled default
    /// nobody chose.
    Check {
        /// The directory a render wrote into.
        ///
        /// Read rather than produced: the manifests checked here must be byte-identical to the ones
        /// every other gate sees, and a second renderer with its own flags is a second answer to
        /// what the chart produces.
        #[arg(long, value_name = "DIR", default_value = "rendered")]
        manifests: PathBuf,

        /// The chart tree.
        #[arg(long, value_name = "DIR", default_value = CHARTS_DIR)]
        charts: PathBuf,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// Hold every `@config` marker in a chart's values against the contract it names.
    ///
    /// Coverage is chart-level: it answers whether a chart has a contract at all. This is the
    /// key-level half, and it catches the failure a chart repository actually keeps hitting — an
    /// image release adds a setting, an automated bump repins the digest and omits everything else,
    /// and no gate notices that the new key is reached by no chart value. `check` cannot see it
    /// either: a key nothing renders is not a key rendered wrongly.
    Bindings {
        /// The chart tree.
        #[arg(long, value_name = "DIR", default_value = CHARTS_DIR)]
        charts: PathBuf,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// Write every bound value's `@schema` block from the contract that describes it.
    ///
    /// A contract states a key's type as JSON Schema, and since `schema_version: 2` it states what
    /// one *element* of a container holds. This is what turns that into the block a chart ships, so
    /// an operator's editor catches a misspelt field before the service does.
    ///
    /// The writer and the checker are one code path, so the two cannot disagree about what "already
    /// correct" means. `--check` additionally holds every hand transcription against the release it
    /// was read at, which is the one thing the writer cannot repair: re-reading a struct somebody
    /// else owns is a person's job.
    Shapes {
        /// The chart tree.
        #[arg(long, value_name = "DIR", default_value = CHARTS_DIR)]
        charts: PathBuf,

        /// Report what would change instead of writing it.
        #[arg(long)]
        check: bool,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// Report how a chart's vendored contracts changed against another revision, and what it costs.
    ///
    /// A refresh runs inside the pull request that repins the digest, so the new document and the
    /// bump arrive together — which is the design's whole point, and also why the reviewer is
    /// looking at a large reordered JSON diff instead of a sentence. This is the sentence.
    ///
    /// It writes no chart version. The severity table is a defensible default, and the reviewer is
    /// the one who knows whether the chart writes the key that moved; a tool that edited the
    /// version would be asserting it knows that, and would be wrong the first time a removed key
    /// was one no template ever emitted.
    Diff {
        /// Only this chart.
        #[arg(value_name = "CHART")]
        chart: Option<String>,

        /// The revision to compare against.
        #[arg(long, default_value = "origin/main")]
        since: String,

        /// The chart tree.
        #[arg(long, value_name = "DIR", default_value = CHARTS_DIR)]
        charts: PathBuf,

        /// Emit the whole comparison as JSON, for something that is not a person.
        #[arg(long)]
        json: bool,

        /// Report differences as an exit status: 0 identical, 1 could not answer, 2 differences.
        ///
        /// Three rather than two, because a status that meant both "differences" and "something
        /// broke" would leave a caller unable to tell a changed contract from an unreadable one.
        /// Off by default: differences are the normal outcome, so they exit 0 and only a failure
        /// to answer exits 1.
        #[arg(long)]
        exit_code: bool,
    },

    /// Report which charts a configuration contract covers, and which it does not.
    ///
    /// Adopting one is opt-in: a chart that carries a declaration is covered, and one that does not
    /// is reported rather than failed. Images adopt the format on their own release schedules, so a
    /// gate that failed every chart whose image had not caught up would be red for reasons nobody in
    /// the consuming repository can fix — and would end up disabled, which is worse than absent.
    ///
    /// What still fails is a chart contradicting itself: one that declares documents and leaves one
    /// of its own first-party images unaccounted for.
    Coverage {
        /// The chart tree.
        #[arg(long, value_name = "DIR", default_value = CHARTS_DIR)]
        charts: PathBuf,

        /// The list of repositories this organisation builds.
        #[arg(long, value_name = "FILE", default_value = FIRST_PARTY)]
        first_party: PathBuf,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
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

/// How a run of findings is written.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Output {
    /// Warnings to stdout, failures to stderr, one line each.
    Text,
    /// Every finding as one JSON object, for a tool rather than a person.
    Json,
    /// The text rendering, plus a table appended to `$GITHUB_STEP_SUMMARY`.
    Github,
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
        Contract::from_json(&self.read_text()?)
    }

    /// The bytes, unparsed.
    ///
    /// `conform` and `validate` need these rather than a `Contract`: a document that fails the
    /// meta-schema may not deserialise at all, and reporting "not a valid document" when the real
    /// answer is "the `dialect` object is missing its `prefix`" is the difference between a
    /// message a producer's author can act on and one they cannot.
    fn read_text(&self) -> Result<String, Error> {
        if self.contract == "-" {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| Error::io("<stdin>", e))?;
            Ok(buffer)
        } else {
            read(Path::new(&self.contract))
        }
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
    // One arm per subcommand and nothing else: each of the three below owns its own arguments, so
    // adding a fourth is a function rather than another branch in a growing match.
    match cli.command {
        Command::Render { .. } => render_command(cli.command),
        Command::Stamp { .. } => stamp_command(cli.command),
        Command::Conform {
            input,
            tier,
            no_schema,
        } => conform_command(&input, tier, no_schema),

        Command::Validate { input } => {
            let errors = validate::validate(&input.read_text()?)?;
            Ok(report_lines(
                &errors,
                "the document does not satisfy `spec/v1/contract.schema.json`:",
            ))
        }

        Command::Check {
            manifests,
            charts,
            format,
        } => {
            if !manifests.is_dir() {
                return Err(Error::Invalid(format!(
                    "{}: no rendered manifests; render the charts into it first",
                    manifests.display()
                )));
            }
            let checked = helm::check(&charts, &manifests)?;
            if checked.charts == 0 {
                // Worth saying out loud: a run that validated nothing looks exactly like a clean one
                // from the outside.
                println!("==> no chart declares a configuration contract; nothing to validate");
            }
            Ok(write_report(
                &checked.report,
                format,
                "Configuration contracts",
                "Every rendered document, container environment and secret mount matches the contract of the image its chart pins.",
            ))
        }

        Command::Bindings { charts, format } => {
            let found = helm::check_bindings(&charts)?;
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
                "Every contract key of every enrolled chart is bound by a value, or written off with a reason.",
            ))
        }

        Command::Shapes {
            charts,
            check,
            format,
        } => shapes_command(&charts, check, format),

        Command::Diff {
            chart,
            since,
            charts,
            json,
            exit_code,
        } => diff_command(&charts, &since, chart.as_deref(), json, exit_code),

        Command::Coverage {
            charts,
            first_party,
            format,
        } => {
            let found = helm::coverage(&charts, &first_party)?;
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

        Command::Image { command } => {
            let ImageCommand::Verify {
                input,
                labels,
                dockerfile,
                path,
            } = command;
            verify_command(&input, labels.as_deref(), dockerfile.as_deref(), &path)
        }
    }
}

/// Write a report the way the caller asked for, and decide the exit code.
///
/// The rules returned findings and decided none of this. Which is the whole split: a build script, a
/// test or a plugin calls the same code and never goes through a process.
fn write_report(report: &Report, format: Output, headline: &str, clean: &str) -> ExitCode {
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

/// Write or check every generated `@schema` block.
///
/// One walk, two endings. The writer replaces a block that does not match; the checker reports it
/// and adds the one assertion the writer deliberately does not make — that a hand transcription is
/// still current — because a writer that refused over it would turn the job that repairs the tree
/// into a gate over the one thing in it no repair reaches.
fn shapes_command(charts: &Path, check: bool, format: Output) -> Result<ExitCode, Error> {
    let mut report = Report::new();
    let mut written = 0;
    let mut derived = 0;
    let mut transcriptions = 0;

    for chart_dir in helm::declaration::chart_dirs(charts)? {
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
                println!("wrote: {} ({count} block(s))", chart.name);
            }
        }

        for problem in &chart.problems {
            report.fail(format!("{}: values.yaml", chart.name), problem.clone());
        }
    }

    if format == Output::Text || format == Output::Github {
        if check && !report.failed() {
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

/// Compare every vendored contract against another revision.
fn diff_command(
    charts: &Path,
    since: &str,
    only: Option<&str>,
    as_json: bool,
    exit_code: bool,
) -> Result<ExitCode, Error> {
    let revision = helm::Committed::resolve(since)?;
    let found = helm::collect(charts, &revision, only)?;

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "tool": "terrace-contract diff",
                "ref": since,
                "commit": revision.commit(),
                "changed": found.changed(),
                "impact": found.impact().label(),
                "charts": found.charts.iter().map(helm::ChartDiff::as_json).collect::<Vec<_>>(),
                "problems": found.report.entries().iter().map(|entry| serde_json::json!({
                    "where": entry.at,
                    "level": entry.finding.level.label(),
                    "message": entry.finding.message,
                })).collect::<Vec<_>>(),
            }))
            .expect("a diff of owned values serialises")
        );
    } else {
        print!("{}", render_diff(&found, since));
    }

    let (_, problems) = found.report.text();
    eprint!("{problems}");

    if found.report.failed() {
        // Something could not be answered, which is a different outcome from a difference.
        return Ok(ExitCode::from(1));
    }
    if exit_code && found.changed() {
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

/// Prose and prose alone are collapsed: they are real changes and they are shown, but one line per
/// key would bury the findings a reviewer is here for.
const PROSE_FIELDS: [&str; 2] = ["docs", "note"];

/// The human report: one block per chart, and the suggestion with its reasons under it.
fn render_diff(found: &helm::Diffed, reference: &str) -> String {
    use std::fmt::Write as _;
    use terrace_contract::diff::{Severity, Status};

    let changed: Vec<&helm::ChartDiff> = found
        .charts
        .iter()
        .filter(|chart| chart.impact() != Severity::None)
        .collect();
    if changed.is_empty() {
        return format!("==> no vendored contract differs from {reference}\n");
    }

    let mut out = String::new();
    for chart in &changed {
        let _ = writeln!(out, "==> {}", chart.chart);
        for contract in &chart.contracts {
            if contract.status == Status::Unchanged {
                continue;
            }
            let _ = writeln!(out, "  {}  ({})", contract.path, contract.status.label());

            // Severity order, stable within a level. The reviewer's first question is whether
            // anything here is major, and a document-order listing buries the answer under the
            // digest line that every single refresh produces.
            let mut ordered: Vec<&terrace_contract::diff::Change> =
                contract.changes.iter().collect();
            ordered.sort_by_key(|change| std::cmp::Reverse(change.severity));

            let mut prose: Vec<&str> = Vec::new();
            for change in ordered {
                let is_prose = change
                    .field
                    .as_deref()
                    .is_some_and(|field| PROSE_FIELDS.contains(&field));
                if is_prose && change.severity == Severity::Patch {
                    prose.push(&change.subject);
                    continue;
                }
                let _ = writeln!(
                    out,
                    "    {:<5}  {:<8}  {}",
                    change.severity.label(),
                    change.area.label(),
                    change.message
                );
            }
            if !prose.is_empty() {
                prose.sort_unstable();
                prose.dedup();
                let _ = writeln!(
                    out,
                    "    {:<5}  {:<8}  {} documentation-only change(s): {}",
                    Severity::Patch.label(),
                    "docs",
                    prose.len(),
                    prose.join(", ")
                );
            }
        }

        let _ = writeln!(out, "  impact: {}", chart.impact().label());
        // Named rather than repeated in full: every driver has already been printed above, and the
        // question this block answers is which of those lines set the impact.
        for change in chart.drivers() {
            let _ = writeln!(
                out,
                "    because  {} {}  {}",
                change.area.label(),
                change.kind.label(),
                change.subject
            );
        }
        let _ = writeln!(out, "  {}", version_line(chart, reference));
        let _ = writeln!(out);
    }

    let _ = writeln!(
        out,
        "==> {} of {} chart(s) changed against {reference}; suggested impact {}",
        changed.len(),
        found.charts.len(),
        terrace_contract::diff::worst(changed.iter().map(|chart| chart.impact())).label()
    );
    out
}

/// What the chart's version is, and whether the bump already in the branch is large enough.
fn version_line(chart: &helm::ChartDiff, reference: &str) -> String {
    use terrace_contract::diff::suggest_version;

    let where_ = format!(
        "version: {} at {reference}, {} in the working tree",
        chart.old_version.as_deref().unwrap_or("unset"),
        chart.new_version.as_deref().unwrap_or("unset")
    );
    let Some(suggested) = suggest_version(chart.new_version.as_deref(), chart.impact()) else {
        return format!("{where_}; suggested impact is {}", chart.impact().label());
    };
    match chart.satisfied() {
        None => format!(
            "{where_}; a {} change suggests {suggested}",
            chart.impact().label()
        ),
        Some(true) => format!(
            "{where_}; the bump in this branch already covers a {} change",
            chart.impact().label()
        ),
        Some(false) => format!(
            "{where_}; a {} change suggests {suggested}, which this branch does not carry",
            chart.impact().label()
        ),
    }
}

/// Print a list of findings, or say nothing and succeed.
fn report_lines(findings: &[String], headline: &str) -> ExitCode {
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

fn conform_command(input: &Input, tier: Tier, no_schema: bool) -> Result<ExitCode, Error> {
    let text = input.read_text()?;

    // The meta-schema first, and its failures are not merged with the rule failures below. A
    // document that is not the right *shape* produces rule violations that are artefacts of the
    // misreading, and a producer's author chasing those is chasing the wrong bug.
    if !no_schema {
        let errors = validate::validate(&text)?;
        if !errors.is_empty() {
            return Ok(report_lines(
                &errors,
                "the document does not satisfy `spec/v1/contract.schema.json`, so the rules \
                 below it were not run:",
            ));
        }
    }

    let contract = Contract::from_json(&text)?;
    if let Some(ahead) = contract.schema_version_ahead() {
        eprintln!(
            "note: the document declares `schema_version` {ahead}, and this build \n             understands {}. Every field this build knows still means what it \n             meant; there may be more here than was checked.",
            terrace_contract::SCHEMA_VERSION
        );
    }

    let violations: Vec<String> = conform::conform(&contract, tier)
        .into_iter()
        .map(|violation| violation.to_string())
        .collect();

    Ok(report_lines(
        &violations,
        &format!("the document does not conform at tier {tier}:"),
    ))
}

fn render_command(command: Command) -> Result<ExitCode, Error> {
    let Command::Render {
        input,
        format,
        path,
        columns,
        title,
        id,
        open,
        no_header,
        full_docs,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };

    let contract = input.read()?;
    let columns = if columns.is_empty() {
        Column::DEFAULT.to_vec()
    } else {
        columns
    };
    let options = Options {
        path: &path,
        columns: &columns,
        json_schema: JsonSchema {
            title,
            id,
            closed: !open,
            ..JsonSchema::default()
        },
        toml_example: TomlExample {
            header: !no_header,
            docs: if full_docs { Docs::Full } else { Docs::Summary },
            ..TomlExample::default()
        },
    };

    // Exactly one trailing newline, whatever the rendering ended with, because a build redirects
    // this into a committed file and a trailing blank line is invisible on a terminal and a diff
    // in the file.
    print!(
        "{}",
        render::one_newline(render::render(&contract, format, &options)?)
    );
    Ok(ExitCode::SUCCESS)
}

fn stamp_command(command: Command) -> Result<ExitCode, Error> {
    let Command::Stamp {
        input,
        app_version,
        revision,
        created,
        source,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };

    let mut contract = input.read()?;
    // Each field is set only when given, so stamping twice with different arguments accumulates
    // rather than clearing what the first run wrote.
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

fn verify_command(
    input: &Input,
    labels: Option<&Path>,
    dockerfile: Option<&Path>,
    path: &str,
) -> Result<ExitCode, Error> {
    if labels.is_none() && dockerfile.is_none() {
        return Err(Error::Invalid(
            "nothing to verify against: pass --labels, --dockerfile, or both. A verify with neither would report success without comparing anything, which is the one outcome this check cannot afford."
                .to_owned(),
        ));
    }

    let contract = input.read()?;
    let mut failed = false;

    if let Some(path_to_labels) = labels {
        let found = image::labels_from_json(&read(path_to_labels)?)?;
        if let Some(report) = image::report(&image::check_labels(&contract, path, &found)) {
            eprintln!("{report}");
            failed = true;
        }
    }

    if let Some(path_to_dockerfile) = dockerfile {
        let text = read(path_to_dockerfile)?;
        // A Dockerfile checked out with CRLF would otherwise never match a block rendered
        // with LF, and the diff would be invisible in every terminal that shows it.
        let committed = image::committed_block(&text)?.replace("\r\n", "\n");
        let expected = image::dockerfile_labels(&contract, path);
        if committed.trim_end() != expected.trim_end() {
            eprintln!(
                "{}: the committed label block is not the one this document renders.\n\
                 --- committed\n{committed}\n\
                 --- expected\n{}",
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
