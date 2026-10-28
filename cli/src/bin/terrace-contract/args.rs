//! The command line: every subcommand, flag and argument clap parses, and the help text it prints.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::VERSION;

use clap::{Args, Parser, Subcommand};
use terrace_contract::render::{Column, Format};
use terrace_contract::{Contract, DEFAULT_PATH, Error, Tier, helm};

use crate::settings::Repo;

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
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) repo: Repo,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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

        /// Rewrite every marked label region of this Dockerfile in place, instead of printing.
        ///
        /// With `--format dockerfile` only. Everything outside the regions is left byte for byte,
        /// and an empty region is filled, so a block is placed by writing the two marker lines
        /// where the labels belong and running this once. A Dockerfile with no region is refused.
        #[arg(long, value_name = "DOCKERFILE", conflicts_with = "check")]
        write: Option<PathBuf>,

        /// Exit 1 when any marked label region of this Dockerfile is not the block this renders.
        ///
        /// With `--format dockerfile` only. Writes nothing, and prints each stale region with what
        /// `--write` would put there.
        #[arg(long, value_name = "DOCKERFILE")]
        check: Option<PathBuf>,
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
pub(crate) enum HelmCommand {
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
pub(crate) struct Init {
    /// The chart's name, and its directory under the chart tree.
    #[arg(value_name = "CHART")]
    pub(crate) chart: String,

    /// The image the chart pins, as values.yaml spells it: `REPOSITORY:TAG`, ideally followed by
    /// `@sha256:...`. A Docker Hub image is written without a registry.
    #[arg(long, value_name = "REPOSITORY:TAG[@DIGEST]")]
    pub(crate) image: String,

    /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
    #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
    pub(crate) charts: Option<PathBuf>,

    /// The signing identity the contract must carry, as a regular expression. Default:
    /// `CONTRACT_SIGNER`, else `helm.signer` in terrace.yaml; required unless `--no-contract`.
    #[arg(long, value_name = "IDENTITY", env = "CONTRACT_SIGNER")]
    pub(crate) signer: Option<String>,

    /// The configuration document's name in config-contract.yaml. Default: the chart's name.
    #[arg(long, value_name = "NAME")]
    pub(crate) document: Option<String>,

    /// The `ConfigMap` key the document is rendered under.
    #[arg(long, value_name = "KEY", default_value = helm::init::DEFAULT_DOCUMENT_KEY)]
    pub(crate) document_key: String,

    /// The repository's shared values, appended to values.yaml after the configuration surface.
    #[arg(long, value_name = "FILE")]
    pub(crate) chassis: Option<PathBuf>,

    /// Write the plain form, for an image that publishes no configuration contract.
    ///
    /// Not a way to skip the network for an image that does publish one: the chart gets no
    /// configuration surface at all, which is strictly less than what could have been generated.
    #[arg(long)]
    pub(crate) no_contract: bool,

    /// Why the image publishes no contract. Required with `--no-contract` when the image is
    /// first-party (`helm.firstParty` in terrace.yaml), and written as the declaration's opt-out.
    #[arg(long, value_name = "TEXT", requires = "no_contract")]
    pub(crate) reason: Option<String>,

    /// Write nothing, and exit 1 when anything would be written.
    #[arg(long, conflicts_with_all = ["diff", "write"])]
    pub(crate) check: bool,

    /// Write nothing, and print a unified diff of every file `--write` would write.
    #[arg(long, conflicts_with = "write")]
    pub(crate) diff: bool,

    /// Write the files rather than printing the plan.
    #[arg(long)]
    pub(crate) write: bool,
}

#[derive(Clone, Subcommand)]
pub(crate) enum ConfigCommand {
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
pub(crate) enum ImageCommand {
    /// Hold an image's labels, its carriers, or a Dockerfile's committed block, against the document.
    ///
    /// The checks answer different questions and none replaces another. The Dockerfile check is
    /// cheap and runs in the pull request that renamed a key, in a diff a reviewer reads. The image
    /// checks cost an image and catch what no source diff can: a build argument that failed to
    /// interpolate, a label a base image overrode, a `LABEL` line deleted on a branch nobody
    /// diffed, a `COPY` of the wrong file.
    ///
    /// `--image` reads the image itself, through `oras`: the labels of every platform, which must
    /// agree; the file at the labelled path, which must be this document byte for byte; and a
    /// referrer attached to the digest, which must be too. At least one of the two carriers must be
    /// there. `--labels` is the offline form of the label check alone, for what `docker inspect`
    /// printed.
    ///
    /// Exits 1 when the image or the Dockerfile is wrong, 2 when it could not be read.
    Verify {
        #[command(flatten)]
        input: Input,

        /// What `docker inspect` or `crane config` printed.
        #[arg(long, value_name = "FILE", conflicts_with = "image")]
        labels: Option<PathBuf>,

        /// A Dockerfile whose committed label blocks should match this document.
        #[arg(long, value_name = "FILE")]
        dockerfile: Option<PathBuf>,

        /// The image to read, fully qualified: `REGISTRY/REPOSITORY[:TAG][@DIGEST]`.
        ///
        /// A tag is resolved to a digest once, and every check is made against the digest.
        #[arg(long, value_name = "REF")]
        image: Option<String>,

        /// Read `--image` as an OCI image layout, `PATH[:TAG][@DIGEST]`, rather than a registry.
        ///
        /// A directory or a tarball, as `docker buildx build --output type=oci` writes one: the
        /// image is checked after it is built and before it is pushed. Carries no signature, so it
        /// cannot be combined with `--signer`.
        #[arg(long, requires = "image", conflicts_with = "signer")]
        oci_layout: bool,

        /// Also verify the image's signature, keyless, against this identity regular expression.
        ///
        /// Through `cosign`, under the GitHub Actions OIDC issuer. Nothing else is read when the
        /// signature does not verify.
        #[arg(long, value_name = "IDENTITY", requires = "image")]
        signer: Option<String>,

        /// Where the document lives inside the image.
        #[arg(long, default_value = DEFAULT_PATH)]
        path: String,
    },

    /// Attach the document to an image's digest as an OCI referrer, and print the referrer's digest.
    ///
    /// Under the artifact type `application/vnd.terrace.config-schema.v1+json`, which is what `pull`
    /// and `image verify --image` look for. A tag is resolved first, so the document is tied to the
    /// digest and never to a tag that can move. Running it again is safe: a referrer holding this
    /// document byte for byte is reported rather than attached a second time, and one holding
    /// anything else is refused, since a digest with two contracts has none a consumer can trust.
    ///
    /// Run after the push. Signing the referrer is a separate step: `cosign sign` on the printed
    /// digest.
    Attach {
        /// The contract document. A file, not stdin: the registry stores it under its file name.
        #[arg(value_name = "CONTRACT")]
        contract: PathBuf,

        /// The image to attach to, fully qualified: `REGISTRY/REPOSITORY[:TAG][@DIGEST]`.
        #[arg(long, value_name = "REF")]
        image: String,

        /// Read `--image` as an OCI image layout directory, `PATH[:TAG][@DIGEST]`, rather than a
        /// registry. A directory, not a tarball: the referrer is written into the layout.
        #[arg(long)]
        oci_layout: bool,
    },
}

/// How a run of findings is written.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Output {
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
pub(crate) struct Input {
    /// The contract document, or `-` to read stdin.
    #[arg(value_name = "CONTRACT", default_value = "-")]
    contract: String,
}

impl Input {
    pub(crate) fn read(&self) -> Result<Contract, Error> {
        Contract::from_json(&self.read_text()?)
    }

    /// The bytes, unparsed.
    ///
    /// `conform` and `validate` need these rather than a `Contract`: a document that fails the
    /// meta-schema may not deserialise at all, and reporting "not a valid document" when the real
    /// answer is "the `dialect` object is missing its `prefix`" is the difference between a
    /// message a producer's author can act on and one they cannot.
    pub(crate) fn read_text(&self) -> Result<String, Error> {
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

pub(crate) fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))
}

/// The options of `adopt`, held apart so that its dispatch stays one line.
#[derive(Args)]
pub(crate) struct Adopt {
    /// One chart, or every enrolled chart when none is named.
    #[arg(value_name = "CHART")]
    pub(crate) chart: Option<String>,

    /// Adopt only this contract key; repeatable. Keys belong to one chart, so name the chart.
    #[arg(long = "key", value_name = "PATH")]
    pub(crate) keys: Vec<String>,

    /// The chart tree. Default: `helm.charts` in terrace.yaml, else `charts`.
    #[arg(long, value_name = "DIR", env = "TERRACE_CHARTS")]
    pub(crate) charts: Option<PathBuf>,

    /// Write nothing, and exit 1 when any enrolled chart owes a value, a write-off or a fix.
    #[arg(long, conflicts_with_all = ["diff", "write"])]
    pub(crate) check: bool,

    /// Write nothing, and print a unified diff of every file `--write` would change.
    #[arg(long, conflicts_with = "write")]
    pub(crate) diff: bool,

    /// Insert the blocks rather than printing them.
    #[arg(long)]
    pub(crate) write: bool,
}
