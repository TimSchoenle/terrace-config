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
use terrace_contract::helm::Selection;
use terrace_contract::render::{
    self, Column, Docs, Format, JsonSchema, Options, TomlExample, image,
};
use terrace_contract::repo::{self, Settings, Source};
use terrace_contract::report::Report;
use terrace_contract::{Contract, DEFAULT_PATH, Error, Tier, conform, helm, validate};

/// The release, then the document versions it reads, so a pin can be checked against a producer.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (reads terrace_contract 1, schema_version 1 to 4)"
);

/// Read, render and check configuration contracts.
#[derive(Parser)]
#[command(
    name = "terrace-contract",
    version = VERSION,
    about = "Read, render and check configuration contracts, whatever produced them.",
    long_about = "Everything the configuration contract needs after a document exists.\n\n\
                  Producing a contract needs the service's own types and is per-language. \
                  Everything downstream of the document needs the document and nothing else, \
                  which is this. See spec/v1/FORMAT.md for what a document is."
)]
struct Cli {
    #[command(flatten)]
    repo: Repo,

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
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART")]
        only: Vec<String>,

        /// The directory a render wrote into.
        ///
        /// Read rather than produced: the manifests checked here must be byte-identical to the ones
        /// every other gate sees, and a second renderer with its own flags is a second answer to
        /// what the chart produces.
        #[arg(long, value_name = "DIR", default_value = "rendered")]
        manifests: PathBuf,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

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
    ///
    /// It also reads the templates, crudely: every key a marker binds must appear in
    /// `templates/*.tpl` as `leaf:` or `"leaf"`, so a value that is bound and documented but
    /// projected by nothing fails here rather than in a round-trip suite nobody ran.
    Bindings {
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART")]
        only: Vec<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// Write the chart values an enrolled chart's unbound contract keys are owed, from the contract.
    ///
    /// The repair `bindings` asks for, done for the case that needs no judgement: a key no chart
    /// value surfaces gets a value typed, defaulted and documented from the contract, with the
    /// marker that binds it; a credential gets a value delivered as a file and its write-off; a
    /// reserved key gets its write-off. The template lines that project a new value are appended
    /// where the define does not already carry the key's root, and printed for a person where it
    /// does.
    ///
    /// It never touches a value that exists. A key whose chart value is already there is refused,
    /// with the marker to write by hand: on an existing value, which key it feeds is a claim only a
    /// person can make.
    ///
    /// Without a mode it plans and prints, and exits 1 only when a key was refused or a chart is
    /// blocked by a broken marker. `--check` prints the same plan and exits 1 when any enrolled
    /// chart owes anything at all, 2 when it could not answer. `--diff` prints the unified diff
    /// `--write` would apply. `--write` applies it.
    Adopt(Adopt),

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
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART")]
        only: Vec<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

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

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

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

    /// Name the chart lines that still use something a contract removed since a revision.
    ///
    /// `diff` says a key or an element property is gone; this says where the chart still uses it,
    /// under `templates/`, the hand-written `tests/*.yaml` and `ci/*.yaml`. No writer edits those
    /// files, so a removal is the one contract change a refresh cannot follow, and without this it
    /// surfaces as schema refusals in a test run with nothing naming the cause. The chart's markers
    /// are read at the revision too, because the value a removed key was bound to has usually been
    /// deleted by then.
    ///
    /// A report, never a writer. Exits 0 when nothing references a removal, 2 when something does,
    /// and 1 when it could not answer — the split `diff --exit-code` uses, so a caller can tell work
    /// to do from a broken run.
    Migrate {
        /// Only this chart.
        #[arg(value_name = "CHART")]
        chart: Option<String>,

        /// The revision to compare against: where the branch started, not `HEAD`, once the refresh
        /// is committed.
        #[arg(long, default_value = "origin/main")]
        since: String,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Emit the findings as JSON, for something that is not a person.
        #[arg(long)]
        json: bool,
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
        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// A file listing the repositories this organisation builds, one pattern per line.
        ///
        /// Default: `helm.firstParty` in terrace.yaml, else
        /// `.github/configs/first-party-images.txt`.
        #[arg(long, value_name = "FILE")]
        first_party: Option<PathBuf>,

        /// How to write the findings.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// List every credential the vendored contracts declare, or reconcile it against a render.
    ///
    /// Two questions read at two removes — what the images declare secret, and whether anything
    /// actually supplies it. The inventory needs no render and is cheap enough to consult while
    /// writing a values file; `--reconcile` needs one and answers the harder half.
    ///
    /// The reconciliation exits 0 whatever it finds. Its three questions are new, and the first
    /// pass over an established repository is where a report earns its triage: a finding that turns
    /// out to be a considered design decision is a line in a document, not a red pipeline on a
    /// pull request that changed nothing. Promoting it is `--exit-code`, once they are triaged.
    Secrets {
        /// Reconcile against the manifests in this directory instead of listing the inventory.
        #[arg(long, value_name = "RENDERED")]
        reconcile: Option<PathBuf>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Emit the whole answer as JSON, for something that is not a person.
        #[arg(long)]
        json: bool,

        /// Fail on what the reconciliation found, rather than reporting it.
        #[arg(long)]
        exit_code: bool,
    },

    /// Write the credential reference each chart's README template carries, from the contract.
    ///
    /// Which credentials the image needs, what to name the keys of the Secret that carries them,
    /// and which environment spelling addresses the same value are all published by the image, and
    /// all three used to be transcribed by hand into prose nothing downstream reads. The one column
    /// no contract can fill — *when* this chart needs the credential — stays hand-written, in the
    /// declaration beside the keys it describes.
    Readme {
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART")]
        only: Vec<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Fail when a reference is behind its contract, rather than writing it.
        #[arg(long)]
        check: bool,
    },

    /// Write the restart sets each chart rolls its pods by, from the contracts it vendors.
    ///
    /// An image that applies a change without restarting publishes which keys a rebuild applies
    /// and which only a restart does. A chart that hashes all of its configuration into a pod
    /// annotation throws the reload away; one that hashes none of it leaves every restart-only key
    /// changed on disk and never applied. This writes `templates/_config-reload.tpl`, naming the
    /// restart-only part of each document, for the chart's checksum to hash and nothing else.
    Reload {
        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Fail when a chart's restart sets are behind its contracts, rather than writing them.
        #[arg(long)]
        check: bool,
    },

    /// Generate the round-trip suites a chart's contracts imply, or report the ones that drifted.
    ///
    /// The document gate proves a rendered document satisfies the contract, and a document missing
    /// a setting entirely satisfies it perfectly. These cases prove the other half: that a setting
    /// written into the chart's values arrives in the application's document, at the path the image
    /// reads it from, carrying the value that was asked for.
    ///
    /// A chart is generated for when its declaration sets `tests: true` (or, for a chart not yet
    /// migrated, when it carries `contract-tests.yaml`), so the rollout is a property of the tree
    /// rather than of a list somebody edits.
    Tests {
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART")]
        only: Vec<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Report drifted suites and exit non-zero instead of writing them.
        #[arg(long)]
        check: bool,
    },

    /// Say what a chart's pinned images actually read, from their vendored contracts.
    ///
    /// Every other reader of those files is a gate: it takes a rendered manifest, holds it against
    /// the contract and reports the difference. This reads the contract for its own sake, which is
    /// what an operator asking "what can I set here, and why is my value being ignored?" needs.
    ///
    /// Offline and read-only, through the same declaration and the same staleness interlock the
    /// gates use — so it cannot report facts a gate would refuse to trust.
    ///
    /// The exit status is a decision rather than an accident: 0 printed, 1 a pattern matched
    /// nothing, 2 no such chart, 3 the interlock refused. `1` is grep's convention and the right
    /// one here — the question is "does this image read this setting?", and an answer
    /// indistinguishable from a typo in the pattern is not an answer.
    Explain {
        /// The chart to explain. Without one, the charts that carry a contract.
        #[arg(value_name = "CHART")]
        chart: Option<String>,

        /// A substring, or a glob where it carries one of `*?[`.
        #[arg(value_name = "PATTERN")]
        pattern: Option<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Print full entries even without a pattern.
        #[arg(long)]
        full: bool,

        /// Emit the same selection as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print the charts the commits since a revision touched, for a pipeline to narrow its gates.
    ///
    /// One line of chart names separated by spaces, or an empty line when every chart has to be
    /// checked. The line is empty whenever the change reaches outside the application charts' own
    /// directories (the pinned CLI, `terrace.yaml`, a workflow, a library chart), and whenever the
    /// change cannot be read, as in a shallow clone. It can therefore widen a gate and never narrow
    /// one by mistake. Why it widened goes to stderr.
    Scope {
        /// The revision the change is measured from, through its merge base with `HEAD`.
        #[arg(long, value_name = "REF", default_value = "origin/main")]
        since: String,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// Print one JSON object instead of the line.
        #[arg(long)]
        json: bool,
    },

    /// Bring every file derived from the vendored contracts up to date, or check them all.
    ///
    /// Without `--check` it writes: `pull` refreshes the vendored contracts from the images the
    /// charts pin, then `shapes` and `readme` rewrite what derives from them. The round-trip suites
    /// are not written: a generated test that lands without its author reading it is a test nobody
    /// has read, so `tests` stays a command a person runs.
    ///
    /// With `--check` it writes nothing and needs no network: `bindings`, `shapes --check`,
    /// `readme --check`, `tests --check` and `coverage`, plus `check` when `--manifests` names a
    /// render. Every step runs even when an earlier one failed, so one run reports everything.
    /// Exits 0 when every step passed, 1 when any step found something, 2 when any step could not
    /// run.
    Sync {
        /// Only these charts, by directory name; every chart when none is named. A name the
        /// tree does not hold is refused, so a typo cannot select nothing and pass.
        #[arg(value_name = "CHART", conflicts_with = "since")]
        only: Vec<String>,

        /// Only the charts the commits since this revision touched, as `scope` prints them.
        #[arg(long, value_name = "REF")]
        since: Option<String>,

        /// Run the gates instead of the writers.
        #[arg(long)]
        check: bool,

        /// A directory a render wrote into, so that `--check` also runs `check` on it.
        #[arg(long, value_name = "DIR", requires = "check")]
        manifests: Option<PathBuf>,

        /// Skip `pull`, and write only from the contracts already vendored.
        #[arg(long, conflicts_with = "check")]
        no_pull: bool,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// The signing identity `pull` verifies against, as a regular expression.
        #[arg(long, value_name = "IDENTITY", env = "CONTRACT_SIGNER")]
        signer: Option<String>,

        /// How to write the findings: `text` or `github`.
        #[arg(long, default_value = "text")]
        format: Output,
    },

    /// Refresh every vendored contract from the image its chart pins.
    ///
    /// The one subcommand that talks to a container registry. Everything else reads the committed
    /// file, so a registry outage cannot fail a pull request that changes no image, a re-run on an
    /// old commit validates against what was true then, and the contract diff lands in the same
    /// pull request as the digest bump — which is the whole point.
    ///
    /// There is deliberately no path to fetching one unverified. A contract that cannot be proven
    /// to belong to the pinned digest is worse than none, because every gate downstream trusts it.
    Pull {
        /// One chart, or every chart that declares a contract.
        #[arg(value_name = "CHART")]
        chart: Option<String>,

        /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
        #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
        charts: Option<PathBuf>,

        /// The signing identity a contract must carry, as a regular expression.
        ///
        /// Required, from this flag, `CONTRACT_SIGNER` or `helm.signer` in terrace.yaml, and never
        /// defaulted: without it the verification would accept any signature at all, which is the
        /// same as accepting none.
        #[arg(long, value_name = "IDENTITY", env = "CONTRACT_SIGNER")]
        signer: Option<String>,

        /// Report what would be written, and write nothing.
        ///
        /// Still fetches, still verifies: the question it answers is whether the committed copy is
        /// the one the pinned digest publishes, and only the registry knows that.
        #[arg(long)]
        check: bool,
    },

    /// Commands that only make sense in a Helm chart repository.
    Helm {
        #[command(subcommand)]
        command: HelmCommand,
    },

    /// Check a built image against the document it claims to carry.
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },

    /// Read or write the repository's terrace.yaml.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Check that this repository and this machine can run every gate, and say what is missing.
    ///
    /// Reads terrace.yaml strictly: a key this release does not read is a failure here, because a
    /// repository relying on a setting its CLI ignores is exactly what this is for finding. Exits 1
    /// on any failure; a warning is something only some commands need.
    Doctor,
}

#[derive(Subcommand)]
enum HelmCommand {
    /// Write the configuration half of a new chart from the contract its image publishes.
    ///
    /// The repository copies its own chart template first — `Chart.yaml`, the workload templates,
    /// the README source — and this writes every file the contract decides: the vendored contract,
    /// `values.yaml` (the image pin, a block per setting with the marker that binds it, then the
    /// chassis), `templates/_helpers.tpl`, `config-contract.yaml` (enrolled in the marker check
    /// and the round-trip suites), `ci/test-values.yaml` and `tests/configmap_test.yaml`. It fills
    /// `%%PREFIX%%`, `%%ENV_PREFIX%%`, `%%SEPARATOR%%` and `%%DOCUMENT_KEY%%` in the copied
    /// templates from the contract's dialect, and refuses any other `%%NAME%%` still there.
    ///
    /// The contract is fetched and verified exactly as `pull` does it; there is no way to give it
    /// one. A file already there that says something else is refused, never overwritten: `adopt`
    /// is what writes into a chart that is configured.
    ///
    /// Without a mode it plans and prints. `--check` exits 1 when anything would be written, 0 when
    /// the chart already holds exactly this, 2 when it could not answer. `--diff` prints a
    /// `patch -p0` diff of every file `--write` would write. Any other failure exits 1.
    Init(Init),
}

/// What `helm init` needs to know about the new chart.
#[derive(Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap flags: `no_contract` picks the form and the other three the mode, which clap \
              already holds mutually exclusive; an enum would re-state that rule by hand"
)]
struct Init {
    /// The chart's name, and its directory under the chart tree.
    #[arg(value_name = "CHART")]
    chart: String,

    /// The image the chart pins, as values.yaml spells it: `REPOSITORY:TAG`, ideally followed by
    /// `@sha256:...`. A Docker Hub image is written without a registry.
    #[arg(long, value_name = "REPOSITORY:TAG[@DIGEST]")]
    image: String,

    /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
    #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
    charts: Option<PathBuf>,

    /// The signing identity the contract must carry, as a regular expression. Default:
    /// `CONTRACT_SIGNER`, else `helm.signer` in terrace.yaml; required unless `--no-contract`.
    #[arg(long, value_name = "IDENTITY", env = "CONTRACT_SIGNER")]
    signer: Option<String>,

    /// The configuration document's name in config-contract.yaml. Default: the chart's name.
    #[arg(long, value_name = "NAME")]
    document: Option<String>,

    /// The `ConfigMap` key the document is rendered under.
    #[arg(long, value_name = "KEY", default_value = helm::init::DEFAULT_DOCUMENT_KEY)]
    document_key: String,

    /// The repository's shared values, appended to values.yaml after the configuration surface.
    #[arg(long, value_name = "FILE")]
    chassis: Option<PathBuf>,

    /// Write the plain form, for an image that publishes no configuration contract.
    ///
    /// Not a way to skip the network for an image that does publish one: the chart gets no
    /// configuration surface at all, which is strictly less than what could have been generated.
    #[arg(long)]
    no_contract: bool,

    /// Why the image publishes no contract. Required with `--no-contract` when the image is
    /// first-party (`helm.firstParty` in terrace.yaml), and written as the declaration's opt-out.
    #[arg(long, value_name = "TEXT", requires = "no_contract")]
    reason: Option<String>,

    /// Write nothing, and exit 1 when anything would be written.
    #[arg(long, conflicts_with_all = ["diff", "write"])]
    check: bool,

    /// Write nothing, and print a unified diff of every file `--write` would write.
    #[arg(long, conflicts_with = "write")]
    diff: bool,

    /// Write the files rather than printing the plan.
    #[arg(long)]
    write: bool,
}

#[derive(Clone, Subcommand)]
enum ConfigCommand {
    /// Print every setting, its value, and where the value came from.
    ///
    /// With `--chart`, print that chart's declaration instead, with every default written out and
    /// the fields the defaults supplied named below it: what the gates actually hold the chart to.
    /// The printed declaration is itself a valid `config-contract.yaml`.
    Show {
        /// Print one JSON object instead of the text form.
        #[arg(long)]
        json: bool,

        /// The chart whose effective declaration to print, by directory name.
        #[arg(long, value_name = "NAME")]
        chart: Option<String>,

        /// The chart tree, overriding `helm.charts` in terrace.yaml.
        #[arg(long, requires = "chart", value_name = "DIR")]
        charts: Option<PathBuf>,
    },

    /// Write a terrace.yaml at the root of the Git repository.
    ///
    /// Pins `cli` to this release's minor, and carries the patterns of an existing
    /// `.github/configs/first-party-images.txt` into `helm.firstParty`, after which that file can
    /// be deleted.
    Init {
        /// Replace an existing file.
        #[arg(long)]
        force: bool,
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

        Command::Image { command } => {
            let ImageCommand::Verify {
                input,
                labels,
                dockerfile,
                path,
            } = command;
            verify_command(&input, labels.as_deref(), dockerfile.as_deref(), &path)
        }

        Command::Config { command } => config_command(command, &repo),

        Command::Doctor => doctor_command(&repo),
    }
}

/// Hold a document to the published meta-schema.
fn validate_command(input: &Input) -> Result<ExitCode, Error> {
    let errors = validate::validate(&input.read_text()?)?;
    Ok(report_lines(
        &errors,
        "the document does not satisfy `spec/v1/contract.schema.json`:",
    ))
}

fn config_command(command: ConfigCommand, repo: &Repo) -> Result<ExitCode, Error> {
    match command {
        ConfigCommand::Show {
            json,
            chart: Some(chart),
            charts,
        } => config_show_chart_command(&repo.charts(charts)?, &chart, json),
        ConfigCommand::Show { json, .. } => config_show_command(&repo.settings()?, json),
        ConfigCommand::Init { force } => config_init_command(repo.named.as_deref(), force),
    }
}

/// The signing identity `pull` verifies against, from whichever layer sets one.
fn required_signer(settings: &Settings, given: Option<String>) -> Result<String, Error> {
    const MISSING: &str = "no signing identity: pass --signer, set CONTRACT_SIGNER, or set \
                           helm.signer in terrace.yaml. There is no default, because one would \
                           accept a signature nobody chose to trust.";
    settings
        .signer(given)
        .map(|(signer, _)| signer)
        .ok_or_else(|| Error::Invalid(MISSING.to_owned()))
}

/// How this run finds terrace.yaml, before it has been read.
///
/// Only the commands that take a setting from the file read it, so a document-only command
/// (`render`, `conform`, ...) never fails over a repository file it does not use.
#[derive(Args)]
struct Repo {
    /// The terrace.yaml to read, instead of the one at the root of the Git repository.
    #[arg(
        id = "config",
        long = "config",
        global = true,
        value_name = "FILE",
        env = "TERRACE_CONFIG"
    )]
    named: Option<PathBuf>,

    /// Fail, rather than warn, on a key in terrace.yaml this release does not read.
    ///
    /// `TERRACE_STRICT` takes 1/0, true/false, yes/no or on/off.
    #[arg(
        long,
        global = true,
        env = "TERRACE_STRICT",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    strict: bool,
}

impl Repo {
    /// The file this run reads: the one named, else the one at the Git root, else none.
    fn path(&self) -> Result<Option<PathBuf>, Error> {
        if let Some(named) = &self.named {
            // Named and missing is an error, where absent from the repository is not: a name is a
            // statement that the file exists, and silently running on defaults would hide a typo.
            if !named.is_file() {
                return Err(Error::Invalid(format!(
                    "{}: no such file, named by --config or TERRACE_CONFIG",
                    named.display()
                )));
            }
            return Ok(Some(named.clone()));
        }
        let here = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        Ok(repo::discover(&here))
    }

    /// The file, read without judging its unknown keys.
    fn load(&self) -> Result<Settings, Error> {
        Ok(Settings {
            file: self.path()?.as_deref().map(repo::load).transpose()?,
        })
    }

    /// The file, read, with every unknown key warned about or, under `--strict`, refused.
    fn settings(&self) -> Result<Settings, Error> {
        let settings = self.load()?;
        if let Some(file) = &settings.file
            && !file.unknown.is_empty()
        {
            let lines = unknown_key_lines(file)?;
            if self.strict {
                return Err(Error::Invalid(format!(
                    "{} (refused under --strict):\n  {}",
                    file.path.display(),
                    lines.join("\n  ")
                )));
            }
            for line in &lines {
                eprintln!("warning: {}: ignored: {line}", file.path.display());
            }
        }
        Ok(settings)
    }

    fn charts(&self, given: Option<PathBuf>) -> Result<PathBuf, Error> {
        Ok(self.settings()?.charts(given).0)
    }
}

/// One line per key this release does not read, saying why it might be there.
fn unknown_key_lines(file: &repo::RepoConfig) -> Result<Vec<String>, Error> {
    let running = repo::running()?;
    // An older CLI cannot know which release introduced a key, but the file's own pin can say the
    // repository expects a newer one, which is the likeliest reason and the actionable one.
    let hint = file
        .cli
        .filter(|pin| pin.is_newer_than(running))
        .map(|pin| format!("; the file pins cli {pin} and this is {running}, so upgrade"))
        .unwrap_or_default();
    Ok(file
        .unknown
        .iter()
        .map(|key| format!("`{key}` is not a key this terrace-contract reads{hint}"))
        .collect())
}

/// A variable's value, treating an empty one as unset the way clap does.
fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The chart tree for a command with no `--charts` flag of its own, attributed to its layer.
fn environment_charts(settings: &Settings) -> (PathBuf, Source) {
    variable("TERRACE_CHARTS").map_or_else(
        || settings.charts(None),
        |value| (PathBuf::from(value), Source::Environment),
    )
}

/// The signing identity for a command with no `--signer` flag of its own, attributed to its layer.
fn environment_signer(settings: &Settings) -> Option<(String, Source)> {
    variable("CONTRACT_SIGNER")
        .map(|value| (value, Source::Environment))
        .or_else(|| settings.signer(None))
}

/// Print every setting with its value and the layer it came from.
fn config_show_command(settings: &Settings, json: bool) -> Result<ExitCode, Error> {
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
fn config_show_chart_command(charts: &Path, chart: &str, json: bool) -> Result<ExitCode, Error> {
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
fn config_init_command(named: Option<&Path>, force: bool) -> Result<ExitCode, Error> {
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
fn doctor_command(repo: &Repo) -> Result<ExitCode, Error> {
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

    match helm::pull::Tools::found() {
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

/// Hold every selected chart's `@config` markers against the contracts they name.
fn bindings_command(
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

/// The options of `adopt`, held apart so that its dispatch stays one line.
#[derive(Args)]
struct Adopt {
    /// One chart, or every enrolled chart when none is named.
    #[arg(value_name = "CHART")]
    chart: Option<String>,

    /// Adopt only this contract key; repeatable. Keys belong to one chart, so name the chart.
    #[arg(long = "key", value_name = "PATH")]
    keys: Vec<String>,

    /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
    #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
    charts: Option<PathBuf>,

    /// Write nothing, and exit 1 when any enrolled chart owes a value, a write-off or a fix.
    #[arg(long, conflicts_with_all = ["diff", "write"])]
    check: bool,

    /// Write nothing, and print a unified diff of every file `--write` would change.
    #[arg(long, conflicts_with = "write")]
    diff: bool,

    /// Insert the blocks rather than printing them.
    #[arg(long)]
    write: bool,
}

/// Plan, check, diff or write the configuration half of one new chart.
///
/// The exit codes follow `adopt`'s: 1 for any failure, except under `--check`, where 1 means
/// "something would be written" and a failure to answer is 2.
fn helm_init_command(repo: &Repo, options: Init) -> ExitCode {
    use helm::init::{self, Pin, Request, Source};

    let check = options.check;
    let run = || -> Result<ExitCode, Error> {
        let settings = repo.settings()?;
        let charts = settings.charts(options.charts).0;
        let pin = Pin::parse(&options.image)?;
        let chassis = options
            .chassis
            .as_deref()
            .map(read)
            .transpose()?
            .unwrap_or_default();
        let request = Request {
            chart: &options.chart,
            pin: &pin,
            document: options.document.as_deref().unwrap_or(&options.chart),
            document_key: &options.document_key,
            chassis: &chassis,
        };

        let fetched;
        let at;
        let source = if options.no_contract {
            Source::Plain {
                opt_out: no_contract_opt_out(&settings, &pin, options.reason.as_deref())?,
            }
        } else {
            let signer = required_signer(&settings, options.signer)?;
            let tools = helm::pull::Tools::found()?;
            fetched = helm::pull::fetch_pinned(&tools, &pin.block(), None, &signer).map_err(
                |failure| {
                    Error::Invalid(format!(
                        "{failure}\n       pass --no-contract (and --reason, for a first-party \
                         image) if this image publishes none"
                    ))
                },
            )?;
            at = helm::pull::now();
            Source::Contract {
                fetched: &fetched,
                at: &at,
            }
        };

        let planned = init::plan(&charts, &request, &source)?;
        if options.diff {
            print!("{}", planned.diff());
            return Ok(ExitCode::SUCCESS);
        }
        print!("{}", planned.report());
        if options.write && planned.owed() {
            planned.write()?;
            print!("{}", planned.closing());
        }
        Ok(if check && planned.owed() {
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

/// The opt-out a plain chart's declaration carries, when the coverage gate will demand one.
///
/// Decided from the same first-party list the gate reads, and refused rather than guessed when no
/// list resolves: a chart pinning a first-party image owes a declaration, and an opt-out without a
/// reason is indistinguishable from a chart nobody got round to declaring.
fn no_contract_opt_out<'a>(
    settings: &Settings,
    pin: &helm::init::Pin,
    reason: Option<&'a str>,
) -> Result<Option<&'a str>, Error> {
    let (patterns, _) = settings.first_party(None).map_err(|failure| {
        Error::Invalid(format!(
            "no first-party image list resolved, so whether this chart owes a declaration cannot \
             be decided ({failure}); set `helm.firstParty` in terrace.yaml (`terrace-contract \
             doctor` says why)"
        ))
    })?;
    if !helm::init::owes_a_declaration(&patterns, &pin.repository) {
        return Ok(None);
    }
    match reason.filter(|held| !held.trim().is_empty()) {
        Some(reason) => Ok(Some(reason)),
        None => Err(Error::Invalid(format!(
            "{} is a first-party image, so the coverage gate will demand a declaration from this \
             chart. Give --reason to write the opt-out, or drop --no-contract to vendor the \
             contract it publishes",
            pin.repository
        ))),
    }
}

/// Plan, check, diff or write the values every selected enrolled chart is owed.
///
/// At most one of `check`, `diff` and `write` is set. The exit codes are the ones the
/// command this replaced used, so the recipes that call it need no change: 2 for a selection that
/// names no chart or a `--key` with no chart, 1 for a refused key, a blocked chart or a tree that
/// could not be read, 0 otherwise. `--check` alone departs from that, to the convention `sync
/// --check` reads: 1 for anything owed, 2 for a tree it could not read.
fn adopt_command(repo: &Repo, options: Adopt) -> ExitCode {
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

/// Write or check every generated `@schema` block.
///
/// One walk, two endings. The writer replaces a block that does not match; the checker reports it
/// and adds the one assertion the writer deliberately does not make — that a hand transcription is
/// still current — because a writer that refused over it would turn the job that repairs the tree
/// into a gate over the one thing in it no repair reaches.
fn shapes_command(
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
    let revision = helm::Committed::resolve_for(charts, since)?;
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

/// Name every chart line that still uses something a contract removed since `since`.
///
/// Returns its exit code rather than an error, because an error reaching `main` exits 2 — which is
/// this command's "a chart still references something removed". Every way of failing to answer is
/// 1 instead, so the three outcomes stay distinguishable to a caller.
///
/// A vendored contract that cannot be read on either side is a failure to answer, and nothing is
/// reported past it: the comparison would read the unreadable side as absent and name every key of
/// that contract as removed.
fn migrate_command(repo: &Repo, command: Command) -> ExitCode {
    let Command::Migrate {
        chart: only,
        since,
        charts,
        json: as_json,
    } = command
    else {
        unreachable!("dispatched on the variant")
    };
    let answer = || -> Result<ExitCode, Error> {
        let charts = repo.charts(charts)?;
        if let Some(name) = &only {
            // Refused by name, so a misspelt chart cannot compare nothing and pass.
            Selection::of([name]).dirs(&charts)?;
        }
        let revision = helm::Committed::resolve_for(&charts, &since)?;
        let diffed = helm::collect(&charts, &revision, only.as_deref())?;
        let (_, problems) = diffed.report.text();
        eprint!("{problems}");
        if diffed.report.failed() {
            return Err(Error::Invalid(format!(
                "a vendored contract could not be read at {since} or in the working tree, so what \
                 it removed is unknown"
            )));
        }

        let found = helm::migrate::migrations(&charts, &diffed.charts, &revision)?;
        let root = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        if as_json {
            let mut document = helm::migrate::as_json(&found, &root);
            document["tool"] = "terrace-contract migrate".into();
            document["ref"] = since.as_str().into();
            document["commit"] = revision.commit().into();
            println!(
                "{}",
                serde_json::to_string_pretty(&document).expect("owned values serialise")
            );
        } else {
            print!("{}", helm::migrate::render(&found, &root));
        }

        let open = found
            .iter()
            .any(|migration| !migration.references.is_empty());
        Ok(if open {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        })
    };
    answer().unwrap_or_else(|error| {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    })
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

/// Hold every rendered document, container environment and secret mount against its contract.
fn check_command(
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
    if checked.charts == 0 {
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
fn coverage_command(
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

/// The credential inventory, or the report reconciling it against what the charts deliver.
fn secrets_command(
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
fn print_inventory(charts: &Path, rows: &[helm::secrets::Credential]) -> Result<(), Error> {
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
fn print_chart(declaration: &helm::Declaration, rows: &[&helm::secrets::Credential]) {
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
fn print_reconciliation(surface: &helm::secrets::Surface, report: &Report) {
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
fn credential_json(row: &helm::secrets::Credential) -> serde_json::Value {
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
fn mount_json(mount: &helm::secrets::Mount) -> serde_json::Value {
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
fn surface_json(surface: &helm::secrets::Surface) -> serde_json::Value {
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

/// Write, or compare, every chart's generated credential reference.
fn readme_command(charts: &Path, selection: &Selection, check: bool) -> Result<ExitCode, Error> {
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
fn reload_command(charts: &Path, check: bool) -> Result<ExitCode, Error> {
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
fn tests_command(charts: &Path, selection: &Selection, check: bool) -> Result<ExitCode, Error> {
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

/// Read a chart's vendored contracts back out.
fn explain_command(
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
fn list_charts(charts: &Path) -> Result<String, Error> {
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
fn opted_out(declaration: &helm::Declaration) -> String {
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
fn explained_json(surface: &helm::explain::Surface, pattern: Option<&str>) -> serde_json::Value {
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
fn setting_json(setting: &helm::explain::Setting, derive: bool) -> serde_json::Value {
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

/// Print the charts a change touched, or an empty line for every chart.
fn scope_command(repo: &Repo, command: Command) -> Result<ExitCode, Error> {
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
struct Steps {
    format: Output,
    failed: Vec<&'static str>,
    broken: Vec<&'static str>,
    ran: usize,
}

impl Steps {
    const fn new(format: Output) -> Self {
        Self {
            format,
            failed: Vec::new(),
            broken: Vec::new(),
            ran: 0,
        }
    }

    /// Run one step under a heading, and record how it ended without stopping the run.
    fn run(&mut self, name: &'static str, step: impl FnOnce() -> Result<ExitCode, Error>) {
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
    fn finish(self, mode: &str) -> ExitCode {
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

/// Write every contract-derived file, or run every offline gate over the selected charts.
fn sync_command(repo: &Repo, command: Command) -> Result<ExitCode, Error> {
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
fn pull_command(
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
    let tools = helm::pull::Tools::found()?;
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
        let expected = image::dockerfile_labels(&contract, path);
        // Every region is compared: a Dockerfile building several images carries one per stage,
        // and each is a place the contract is published from.
        // Named here because the parser only knows lines: a refusal that does not say which file
        // is no help in a build checking several.
        let regions = image::committed_blocks(&text).map_err(|error| match error {
            Error::Invalid(message) => {
                Error::Invalid(format!("{}: {message}", path_to_dockerfile.display()))
            }
            other => other,
        })?;
        for region in regions {
            // A Dockerfile checked out with CRLF would otherwise never match a block rendered
            // with LF, and the diff would be invisible in every terminal that shows it.
            let committed = region.block.replace("\r\n", "\n");
            if committed.is_empty() {
                eprintln!(
                    "{}:{}: the `{}` region is empty, so it publishes no labels. Paste the output \
                     of `render --format dockerfile`, markers included.",
                    path_to_dockerfile.display(),
                    region.line,
                    terrace_contract::document::MARKER_BEGIN
                );
                failed = true;
            } else if committed.trim_end() != expected.trim_end() {
                eprintln!(
                    "{}:{}: the committed label block is not the one this document renders.\n\
                     --- committed\n{committed}\n\
                     --- expected\n{}",
                    path_to_dockerfile.display(),
                    region.line,
                    expected.trim_end()
                );
                failed = true;
            }
        }
    }

    // 1, not 2: the tool worked and the image is wrong.
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
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
