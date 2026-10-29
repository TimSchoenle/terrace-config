//! The credential surface: what the images declare secret, and what the charts deliver.
//!
//! The gates in [`crate::gate::container`] are structurally unable to close this gap. Gate 3 asks
//! whether every *delivered* file name is known to the contract, so it rejects a name nothing
//! declares and is blind by construction to a declared key nothing supplies. A credential that no
//! channel delivers renders cleanly, passes every gate, and fails at the first request that needs
//! it.
//!
//! Two answers live here, and they are deliberately separate:
//!
//! **The inventory** is what the contracts say, and needs no render. It reads the vendored
//! documents and nothing else, so it stays cheap enough to run while writing a values file. It
//! deliberately skips the staleness interlock [`bind`] applies: a digest bump produces one run
//! where the pinned digest and the vendored contract disagree, and refusing to print the inventory
//! then would withhold the document at exactly the moment a reviewer is reading a credential
//! change. Whether the vendored copy is current is a different check's question.
//!
//! **The reconciliation** is what a rendered chart actually delivers, against that inventory. It
//! does apply the interlock, because it reasons about which image a container runs and an answer
//! derived from a contract belonging to a different digest is worse than no answer.
//!
//! Three conclusions, and the scope of each is load-bearing:
//!
//! - `undeliverable`: a contract key no rendered configuration can supply through any channel.
//!   Judged across *every* values file the chart ships, because a credential delivered under one
//!   fixture is delivered: the chart has a way to supply it.
//! - `unclaimed`: a delivered Secret file name that no contract of the chart names, by neither an
//!   exact spelling nor a dynamic-map leaf. Scoped against every contract the chart vendors, which
//!   is what keeps it disjoint from over-projection.
//! - `over-projected`: a Secret file whose name a *sibling* image's contract claims and the
//!   container's own image's contract does not. A pod holding a credential its binary never reads
//!   is a least-privilege defect, and it is scoped per container against that one image's contract,
//!   never against the union.
//!
//! # Not duplicating gate 3
//!
//! Gate 3 already errors on a name unknown to the contract, but only for a file inside a *resolved*
//! secrets directory on a container the declaration lists as a consumer of that document. Both
//! halves of that condition are recorded per file here, and a file gate 3 would have judged is
//! reported by neither `unclaimed` nor `over_projected`, since it is already one line in the check,
//! and a second line saying the same thing in a different report trains a reader to skim both. What
//! remains is precisely gate 3's blind spot, and it is not hypothetical: a Deployment running an
//! init container that no declaration names as a consumer has nothing checking what it mounts.
//!
//! # Nothing here builds a union across images, and the word is worth avoiding
//!
//! Every judgement below is made against one image's own contract, which is the only scope that can
//! distinguish a key one service reads from one another reads. The single chart-wide structure is a
//! digest-to-contract lookup, and it exists to *find* the right contract for a container rather than
//! to widen one: an init container inside a Deployment may run an image the document it belongs to
//! has never heard of.
//!
//! # Every container, not only the declared consumers
//!
//! The reconciliation walks every container of every rendered workload and identifies its image by
//! digest, because the question "who can see this credential" is about the pod rather than about
//! what the declaration chose to describe. A container whose image the chart pins no contract for is
//! skipped: nothing here can say what it reads.
//!
//! # Aliases are deliberately not consulted
//!
//! A key may carry `env_aliases`, `env_file_aliases` and `secrets_file_aliases`; [`mod@crate::classify`]
//! and [`Union::key_by`] ignore them, and this module agrees with the normative reader rather than
//! being independently cleverer than the gate whose findings it sits beside.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value as Json;

use crate::classify::{Kind, classify};
use crate::error::Error;
use crate::gate::container::ContainerView;
use crate::gate::document_keys;
use crate::k8s::{containers_of, digest_of, load_manifests, pod_spec, secret_file_names};
use crate::report::{Report, error, warning};
use crate::union::{Merged, Union};

use super::declaration::{Binding, Declaration, Document, bind, declared, vendored_for};

/// The four ways a value can reach the loader, named as the report prints them.
///
/// The rendered document is among them and is not a mistake: a key written into the plaintext
/// `ConfigMap` *is* supplied, and leaving that channel out would report a key as undeliverable while a
/// running pod reads it. Whether a credential belongs in a `ConfigMap` is a different question from
/// whether it arrives.
const SECRETS_DIRECTORY: &str = "the secrets directory";
const INDIRECTION: &str = "`_FILE` indirection";
const ENVIRONMENT: &str = "the environment";
const DOCUMENT: &str = "the rendered document";

/// Extensions of the files under a chart that describe what it *delivers*.
///
/// The vendored contracts are excluded on purpose: they are the other side of the comparison, and
/// every credential is named in one of them by definition.
const CHART_TEXT: [&str; 6] = ["yaml", "yml", "tpl", "json", "md", "txt"];

/// Directories under a chart that describe something other than what this chart delivers.
const EXCLUDED_DIRS: [&str; 2] = ["contracts", "charts"];

// ------------------------------------------------------------------------------------------------
// The inventory: what the vendored contracts declare
// ------------------------------------------------------------------------------------------------

/// One `secret: true` key, as one image's contract spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The chart that vendors the contract.
    pub chart: String,
    /// The declared document the contract belongs to.
    pub document: String,
    /// The vendored file as [`bind`] labels it: `<chart>/contracts/<name>.json`. The same spelling
    /// [`Union::sources`] carries, so a declaration and the contract a rendered container was
    /// matched against are comparable without a second naming convention.
    pub contract: String,
    /// The application the contract describes.
    pub image: String,
    /// The configuration path.
    pub path: String,
    /// The file name that supplies it from a secrets directory.
    pub secrets_file: String,
    /// The environment variable that supplies it.
    pub env: String,
    /// The variable naming a file whose contents supply it.
    pub env_file: String,
    /// What the raw text is read as.
    pub text_form: String,
    /// Whether the image refuses to start without it.
    pub required: bool,
    /// The entries the key's map must contain, read from its `constraint`, empty for anything but a
    /// map a producer refined.
    pub entries: Vec<String>,
    /// The contract's own first line about the key.
    ///
    /// Carried into the report because it is what turns a triage question into an answered one: a
    /// key is undeliverable here and its own documentation says it is retired, which no amount of
    /// chart reading would have said.
    pub summary: String,
}

/// One credential of one chart: every declaration of it, merged.
///
/// Merged by spelling rather than by path alone. Two documents of one chart spelling one path
/// differently would be two different files on disk and two different variables, so they stay two
/// rows: the inventory's job is to name the artefacts an operator has to create, and collapsing
/// them would name one that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    /// The chart.
    pub chart: String,
    /// The configuration path every declaration of it spells.
    pub path: String,
    /// The secrets-directory file name.
    pub secrets_file: String,
    /// The environment variable.
    pub env: String,
    /// The indirection variable.
    pub env_file: String,
    /// Whether any reader requires it.
    pub required: bool,
    /// Every entry any reader requires its map to contain, sorted.
    pub entries: Vec<String>,
    /// The contract's first line about it.
    pub summary: String,
    /// The declared documents it appears in.
    pub documents: Vec<String>,
    /// The applications that read it.
    pub images: Vec<String>,
    /// The vendored contracts that declare it.
    pub contracts: Vec<String>,
}

/// Every chart that declares at least one document, in directory order.
///
/// # Errors
/// [`Error::Invalid`] when a declaration cannot be read, [`Error::Io`] when the tree cannot be
/// walked.
pub fn contracted_charts(charts: &Path) -> Result<Vec<(PathBuf, Declaration)>, Error> {
    declared(charts, true)
}

/// Every `secret: true` key of every contract one chart vendors.
///
/// # Errors
/// [`Error::Invalid`] when a vendored contract cannot be read.
pub fn declared_credentials(
    chart_dir: &Path,
    declaration: &Declaration,
) -> Result<Vec<Declared>, Error> {
    let mut found = Vec::new();
    for document in &declaration.documents {
        for item in vendored_for(chart_dir, document)? {
            let contract = &item.vendored.contract;
            let app = contract
                .get("app")
                .and_then(|app| app.get("name"))
                .and_then(Json::as_str)
                .unwrap_or(&item.vendored.image)
                .to_owned();
            for key in keys_of(contract) {
                if key.get("secret").and_then(Json::as_bool) != Some(true) {
                    continue;
                }
                found.push(Declared {
                    chart: declaration.chart.clone(),
                    document: document.name.clone(),
                    contract: item.label.clone(),
                    image: app.clone(),
                    path: text(key, "path"),
                    secrets_file: text(key, "secrets_file"),
                    env: text(key, "env"),
                    env_file: text(key, "env_file"),
                    text_form: text(key, "text_form"),
                    required: key.get("required").and_then(Json::as_bool) == Some(true),
                    entries: crate::document::required_entries(key.get("constraint"))
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                    summary: first_line(key.get("docs")),
                });
            }
        }
    }
    Ok(found)
}

/// Collapse declarations into one row per credential, keeping every reader's name.
#[must_use]
pub fn credentials(declared: &[Declared]) -> Vec<Credential> {
    // Insertion-ordered rather than sorted, so the stable sort below leaves declaration order
    // intact where the sort key ties. Two keys with one path and one secrets file differ only in
    // their variables, and printing them in the order the contract wrote them is the readable one.
    /// Chart, path, and the three spellings: what makes two declarations one credential.
    type Spelling<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

    let mut merged: Vec<(Spelling<'_>, Vec<&Declared>)> = Vec::new();
    for entry in declared {
        let identity = (
            entry.chart.as_str(),
            entry.path.as_str(),
            entry.secrets_file.as_str(),
            entry.env.as_str(),
            entry.env_file.as_str(),
        );
        if let Some(held) = merged.iter_mut().find(|(seen, _)| *seen == identity) {
            held.1.push(entry);
        } else {
            merged.push((identity, vec![entry]));
        }
    }

    let mut rows: Vec<Credential> = merged
        .into_iter()
        .map(
            |((chart, path, secrets_file, env, env_file), entries)| Credential {
                chart: chart.to_owned(),
                path: path.to_owned(),
                secrets_file: secrets_file.to_owned(),
                env: env.to_owned(),
                env_file: env_file.to_owned(),
                // Unioned exactly as the contract merge unions it: a key any reader requires is a key
                // the deployment must carry.
                required: entries.iter().any(|entry| entry.required),
                // The same union one level down: an entry any reader requires is one the Secret
                // must carry.
                entries: sorted(
                    entries
                        .iter()
                        .flat_map(|entry| entry.entries.iter().cloned()),
                ),
                // The merge refuses two contracts that document one key differently, so every entry
                // here carries the same line and the first will do.
                summary: entries[0].summary.clone(),
                documents: sorted(entries.iter().map(|entry| entry.document.clone())),
                images: sorted(entries.iter().map(|entry| entry.image.clone())),
                contracts: sorted(entries.iter().map(|entry| entry.contract.clone())),
            },
        )
        .collect();
    rows.sort_by(|left, right| {
        (&left.chart, &left.path, &left.secrets_file).cmp(&(
            &right.chart,
            &right.path,
            &right.secrets_file,
        ))
    });
    rows
}

/// Every secret declaration in the repository, chart by chart.
///
/// # Errors
/// [`Error::Invalid`] when a declaration or a vendored contract cannot be read.
pub fn inventory(charts: &Path) -> Result<Vec<Declared>, Error> {
    let mut found = Vec::new();
    for (chart_dir, declaration) in contracted_charts(charts)? {
        found.extend(declared_credentials(&chart_dir, &declaration)?);
    }
    Ok(found)
}

// ------------------------------------------------------------------------------------------------
// The reconciliation
// ------------------------------------------------------------------------------------------------

/// One Secret-sourced file, as one rendered container receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// The chart.
    pub chart: String,
    /// The values file whose render produced it.
    pub values_file: String,
    /// The workload, as `Kind name`.
    pub workload: String,
    /// The container's name.
    pub container: String,
    /// The vendored contract of the image it runs, without the path around it.
    pub image: String,
    /// Where the volume is mounted.
    pub mount_path: String,
    /// The file's name inside it.
    pub file_name: String,
    /// Whether gate 3 already judged this file: it inspects the contents of a *resolved* secrets
    /// directory on a container the declaration lists as a consumer, and nothing else.
    pub judged_by_gate_three: bool,
}

/// A credential the chart declares and no rendering of it supplies.
///
/// `named_by_chart` separates the two very different reasons that happens, and it is the whole
/// difference between a defect and a coverage gap. A chart that names a credential somewhere in its
/// values, templates or documentation has a channel for it and simply no fixture that exercises one;
/// a chart that names it nowhere has no channel at all, and an operator reading the values file will
/// never learn the credential exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undeliverable {
    /// What is not supplied.
    pub credential: Credential,
    /// The values files that were rendered and checked.
    pub values_files: Vec<String>,
    /// Whether the chart mentions any spelling of it outside its vendored contracts.
    pub named_by_chart: bool,
    /// Where it mentions them.
    pub named_in: Vec<String>,
}

/// A key the contract calls ordinary configuration and the chart delivers as a Secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elevated {
    /// The chart.
    pub chart: String,
    /// The configuration path.
    pub path: String,
    /// The Secret file that delivers it.
    pub file_name: String,
    /// What the raw text is read as.
    pub text_form: String,
    /// The containers that receive it.
    pub containers: Vec<String>,
}

/// Everything one pass over the rendered manifests found.
#[derive(Debug, Default)]
pub struct Surface {
    /// The charts reconciled.
    pub charts: Vec<String>,
    /// Every values file any of them was rendered under.
    pub values_files: Vec<String>,
    /// Credentials nothing supplies.
    pub undeliverable: Vec<Undeliverable>,
    /// Delivered file names no contract of the chart claims.
    pub unclaimed: Vec<Mount>,
    /// Delivered file names only a sibling image's contract claims.
    pub over_projected: Vec<Mount>,
    /// Ordinary keys the chart chose to deliver as Secrets.
    pub elevated: Vec<Elevated>,
    /// Anything that stopped a chart being reconciled, rather than anything found wrong in one.
    pub notes: Vec<String>,
}

/// What one chart's renders were found to deliver, accumulated across every values file.
///
/// The same shape gate 2 and 3's supplier table takes one level down, and for the same reason:
/// neither question is answerable from a single container, so the answer has to outlive the walk
/// over one. Keyed by the vendored contract as well as the config path, because "who supplies this"
/// is a question about one image: two images declaring one path are two credentials to deliver, and
/// collapsing them would let either one cover for the other.
#[derive(Debug, Default)]
struct Ledger {
    supplied: BTreeMap<(String, String), BTreeSet<String>>,
    elevated: BTreeMap<(String, String, String), BTreeSet<String>>,
}

impl Ledger {
    fn supply(&mut self, contract: &str, path: &str, channel: &str) {
        self.supplied
            .entry((contract.to_owned(), path.to_owned()))
            .or_default()
            .insert(channel.to_owned());
    }

    fn supplies(&self, contract: &str, path: &str) -> bool {
        self.supplied
            .contains_key(&(contract.to_owned(), path.to_owned()))
    }

    fn elevate(&mut self, key: &Merged, file_name: &str, container: String) {
        let identity = (
            key.text("path").unwrap_or_default().to_owned(),
            file_name.to_owned(),
            key.text("text_form").unwrap_or_default().to_owned(),
        );
        self.elevated.entry(identity).or_default().insert(container);
    }
}

/// Reconcile every contracted chart's declared credentials against what its renders deliver.
///
/// # Errors
/// [`Error::Invalid`] when a declaration or a contract cannot be read at all, [`Error::Io`] when the
/// trees cannot be walked. A chart whose contracts are merely *stale* is a note on the surface
/// rather than an error: the run still answers for every other chart.
pub fn reconcile(charts: &Path, rendered: &Path) -> Result<Surface, Error> {
    let mut surface = Surface::default();
    for (chart_dir, declaration) in contracted_charts(charts)? {
        reconcile_chart(&chart_dir, &declaration, rendered, &mut surface)?;
    }
    surface.charts.sort();
    surface.values_files.sort();
    Ok(surface)
}

/// One chart: bind its contracts, walk its renders, compare the two.
fn reconcile_chart(
    chart_dir: &Path,
    declaration: &Declaration,
    rendered: &Path,
    surface: &mut Surface,
) -> Result<(), Error> {
    let values = super::read_file(&chart_dir.join("values.yaml"))?;
    let chart_yaml = super::read_file(&chart_dir.join("Chart.yaml"))?;
    let app_version = chart_yaml.get("appVersion").and_then(Json::as_str);

    let mut bindings: BTreeMap<String, Binding> = BTreeMap::new();
    for document in &declaration.documents {
        let (binding, problems) = bind(chart_dir, document, &values, app_version)?;
        let Some(binding) = binding else {
            for problem in problems {
                surface.notes.push(format!(
                    "{}: {}: not reconciled: {problem}",
                    declaration.chart, document.name
                ));
            }
            continue;
        };
        bindings.insert(document.name.clone(), binding);
    }
    if bindings.is_empty() {
        return Ok(());
    }

    // Which contract describes the image a container runs, chart-wide rather than per document. An
    // init container inside one Deployment may run an image that document's binding has never heard
    // of, which is exactly the case the over-projection check exists to see.
    let mut by_digest: BTreeMap<&str, &Union> = BTreeMap::new();
    let mut contracts: Vec<&Union> = Vec::new();
    for binding in bindings.values() {
        for (digest, union) in &binding.by_digest {
            by_digest.insert(digest.as_str(), union);
            contracts.push(union);
        }
    }

    let renders = renders_of(rendered, &declaration.chart)?;
    if renders.is_empty() {
        surface.notes.push(format!(
            "{}: no rendered manifests; nothing to reconcile against",
            declaration.chart
        ));
        return Ok(());
    }

    surface.charts.push(declaration.chart.clone());
    let mut ledger = Ledger::default();
    let mut seen_values: Vec<String> = Vec::new();

    for render in &renders {
        let values_file = values_file_of(render);
        seen_values.push(values_file.clone());
        if !surface.values_files.contains(&values_file) {
            surface.values_files.push(values_file.clone());
        }
        let text = std::fs::read_to_string(render).map_err(|e| Error::io(render.display(), e))?;
        let manifests = load_manifests(&text)?;
        scan_render(
            declaration,
            &bindings,
            &by_digest,
            &contracts,
            &manifests,
            &values_file,
            &mut ledger,
            surface,
        );
    }

    collect(chart_dir, declaration, &ledger, &seen_values, surface)
}

/// One rendered values file, both halves: what the document sets and what each container receives.
#[expect(
    clippy::too_many_arguments,
    reason = "the walk carries one chart's whole context; bundling it into a struct would name \
              the same fields once more without making any of them optional"
)]
fn scan_render(
    declaration: &Declaration,
    bindings: &BTreeMap<String, Binding>,
    by_digest: &BTreeMap<&str, &Union>,
    contracts: &[&Union],
    manifests: &[Json],
    values_file: &str,
    ledger: &mut Ledger,
    surface: &mut Surface,
) {
    let judged = gate_three_reach(declaration, manifests);

    for document in &declaration.documents {
        if let Some(binding) = bindings.get(&document.name) {
            scan_document(document, binding, manifests, ledger);
        }
    }

    // Every workload, found by shape rather than by a list of kinds: anything carrying a pod
    // template has containers, and anything else yields none. A kind list here would need editing
    // the first time a chart grew a workload nobody thought of.
    for manifest in manifests {
        let spec = pod_spec(manifest);
        let found = containers_of(spec);
        if found.is_empty() {
            continue;
        }
        let workload = workload_of(manifest);
        let mut ordered = found;
        ordered.sort_by_key(|(name, _)| *name);
        for (name, container) in ordered {
            let digest = container
                .get("image")
                .and_then(Json::as_str)
                .and_then(digest_of)
                .unwrap_or_default();
            let Some(mine) = by_digest.get(digest) else {
                continue;
            };
            scan_container(
                declaration,
                contracts,
                manifests,
                spec,
                mine,
                Where {
                    values_file,
                    workload: &workload,
                    container: name,
                },
                container,
                &judged,
                ledger,
                surface,
            );
        }
    }
}

/// Where in the render one file is found, carried whole so the scan reads as one subject.
#[derive(Debug, Clone, Copy)]
struct Where<'a> {
    values_file: &'a str,
    workload: &'a str,
    container: &'a str,
}

/// Records the keys the rendered plaintext document sets, as one delivery channel.
fn scan_document(document: &Document, binding: &Binding, manifests: &[Json], ledger: &mut Ledger) {
    let Some(present) = document_keys(manifests, &document.source) else {
        return;
    };
    for union in binding.by_digest.values() {
        let label = union.sources.first().map_or("", String::as_str);
        for path in union.keys.names() {
            if present.contains(path) {
                ledger.supply(label, path, DOCUMENT);
            }
        }
    }
}

/// One container: its environment as delivery channels, then every Secret file it mounts.
#[expect(
    clippy::too_many_arguments,
    reason = "the same whole-chart context as the walk above, one level down"
)]
fn scan_container(
    declaration: &Declaration,
    contracts: &[&Union],
    manifests: &[Json],
    spec: &Json,
    mine: &Union,
    at: Where<'_>,
    container: &Json,
    judged: &BTreeSet<(String, String)>,
    ledger: &mut Ledger,
    surface: &mut Surface,
) {
    let label = mine.sources.first().map_or("", String::as_str);
    let view = ContainerView::read(manifests, container, mine);
    let secrets_dir = view
        .secrets_dir
        .as_deref()
        .map(|path| path.trim_end_matches('/').to_owned());

    for (variable, _) in view.environment.sorted() {
        let decision = classify(mine, variable);
        let Some(entry) = decision.entry else {
            continue;
        };
        let path = entry.text("path").unwrap_or_default();
        match decision.kind {
            Kind::KeyEnv => ledger.supply(label, path, ENVIRONMENT),
            Kind::KeyEnvFile => ledger.supply(label, path, INDIRECTION),
            _ => {}
        }
    }

    for mount in container
        .get("volumeMounts")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let Some(volume) = mount
            .get("name")
            .and_then(Json::as_str)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let path = mount
            .get("mountPath")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .trim_end_matches('/');
        for file_name in secret_file_names(manifests, spec, volume) {
            scan_file(
                declaration,
                contracts,
                mine,
                &view,
                secrets_dir.as_deref(),
                at,
                path,
                &file_name,
                judged,
                ledger,
                surface,
            );
        }
    }
}

/// One Secret file: supplied, elevated, over-projected, unclaimed, or already gate 3's.
#[expect(
    clippy::too_many_arguments,
    reason = "the same whole-chart context as the walks above, one level further down"
)]
fn scan_file(
    declaration: &Declaration,
    contracts: &[&Union],
    mine: &Union,
    view: &ContainerView<'_>,
    secrets_dir: Option<&str>,
    at: Where<'_>,
    mount_path: &str,
    file_name: &str,
    judged: &BTreeSet<(String, String)>,
    ledger: &mut Ledger,
    surface: &mut Surface,
) {
    let label = mine.sources.first().map_or("", String::as_str);
    let in_secrets_dir = secrets_dir == Some(mount_path);
    let by_indirection = view
        .indirect
        .contains_key(&format!("{mount_path}/{file_name}"));

    let exact = mine.key_by("secrets_file", file_name);
    let owned = exact.or_else(|| mine.container_of("secrets_file", file_name));

    if let Some(owned) = owned {
        let path = owned.text("path").unwrap_or_default();
        if in_secrets_dir {
            ledger.supply(label, path, SECRETS_DIRECTORY);
        } else if by_indirection {
            ledger.supply(label, path, INDIRECTION);
        }
        // A file whose exact key the contract calls ordinary configuration, delivered as a Secret
        // anyway. Legitimate (chart policy may treat a value as more sensitive than the image does,
        // and gate 3 permits it for any `text` key), so it is stated and never counted against the
        // chart. Only an exact match is judged: a dynamic map's leaves have no sensitivity of their
        // own for the contract to state. And only a file-supplyable key, because the same mount of
        // a key of any other form is gate 3's error rather than a policy choice.
        if let Some(key) = exact {
            let ordinary = key.fields.get("secret").and_then(Json::as_bool) != Some(true);
            if ordinary && key.entry().file_supplyable().unwrap_or(false) {
                ledger.elevate(
                    key,
                    file_name,
                    format!("{} ({})", at.container, short(label)),
                );
            }
        }
        return;
    }

    let judged_by_gate_three =
        in_secrets_dir && judged.contains(&(at.workload.to_owned(), at.container.to_owned()));
    if judged_by_gate_three {
        return;
    }

    let mount = Mount {
        chart: declaration.chart.clone(),
        values_file: at.values_file.to_owned(),
        workload: at.workload.to_owned(),
        container: at.container.to_owned(),
        image: short(label),
        mount_path: mount_path.to_owned(),
        file_name: file_name.to_owned(),
        judged_by_gate_three,
    };
    if named_anywhere(contracts, file_name) {
        surface.over_projected.push(mount);
    } else {
        surface.unclaimed.push(mount);
    }
}

/// The `(workload, container)` pairs the check already inspects the mounts of.
///
/// The check walks the consumers a declaration names and the containers each one names, so a
/// container absent from that list (an init container, a sidecar, a workload whose selector matches
/// nothing) is a container gate 3 never opens. Recomputing the same selection here is what lets
/// this report stay silent about files gate 3 has already reported and speak about the ones it
/// cannot see.
fn gate_three_reach(declaration: &Declaration, manifests: &[Json]) -> BTreeSet<(String, String)> {
    let mut reach = BTreeSet::new();
    for document in &declaration.documents {
        for consumer in document.consumers_in(manifests).iter() {
            for workload in consumer.select(manifests) {
                let identity = workload_of(workload);
                let present = containers_of(pod_spec(workload));
                for name in &consumer.containers {
                    if present.iter().any(|(seen, _)| seen == name) {
                        reach.insert((identity.clone(), name.clone()));
                    }
                }
            }
        }
    }
    reach
}

/// Turns one chart's scan into the findings that outlive it.
fn collect(
    chart_dir: &Path,
    declaration: &Declaration,
    ledger: &Ledger,
    values_files: &[String],
    surface: &mut Surface,
) -> Result<(), Error> {
    for credential in credentials(&declared_credentials(chart_dir, declaration)?) {
        // Delivered by any one of the images that declare it is delivered: the credential is one
        // artefact an operator creates, and the report is about that artefact existing. Which image
        // reads it where is the over-projection question, one report down.
        if credential
            .contracts
            .iter()
            .any(|contract| ledger.supplies(contract, &credential.path))
        {
            continue;
        }
        let named_in = names_credential(chart_dir, &credential)?;
        surface.undeliverable.push(Undeliverable {
            credential,
            values_files: values_files.to_vec(),
            named_by_chart: !named_in.is_empty(),
            named_in,
        });
    }

    for ((path, file_name, text_form), containers) in &ledger.elevated {
        surface.elevated.push(Elevated {
            chart: declaration.chart.clone(),
            path: path.clone(),
            file_name: file_name.clone(),
            text_form: text_form.clone(),
            containers: containers.iter().cloned().collect(),
        });
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// The report
// ------------------------------------------------------------------------------------------------

/// Turns one scan into findings, at the level each conclusion has earned.
///
/// An error is reserved for a chart that has no channel for a credential at all, and for a
/// credential reaching a pod that cannot read it. A warning is for everything that is a gap in what
/// was *checked* rather than in what the chart does: a credential no fixture exercises, a chart
/// policy being recorded, a document that could not be reconciled. That is the same division
/// [`crate::report`] already documents, applied to a report rather than a gate.
#[must_use]
pub fn report_of(surface: &Surface) -> Report {
    let mut report = Report::new();

    for note in &surface.notes {
        report.add("not reconciled", warning(note.clone()));
    }

    for finding in &surface.undeliverable {
        let row = &finding.credential;
        let channels = format!(
            "not as the file {} in a secrets directory, not through {}, not as {}, and not by the \
             rendered document",
            crate::gate::quoted(&row.secrets_file),
            row.env_file,
            row.env
        );
        let summary = if row.summary.is_empty() {
            String::new()
        } else {
            format!(" The contract says: {}", row.summary)
        };
        if finding.named_by_chart {
            report.add(
                format!("unsupplied: {}", row.chart),
                warning(format!(
                    "{} is declared secret by {} and none of the {} `ci/` values file(s) supplies \
                     it: {channels}. The chart does mention it, in {}, so it has heard of the \
                     credential: this is a fixture that never sets it, or a spelling the chart \
                     deliberately handles some other way, rather than a credential nothing in the \
                     chart knows about.{summary}",
                    row.path,
                    row.images.join(", "),
                    finding.values_files.len(),
                    finding.named_in.join(", ")
                )),
            );
            continue;
        }
        report.add(
            format!("undeliverable: {}", row.chart),
            error(format!(
                "{} is declared secret by {}, no rendering of this chart supplies it ({channels}), \
                 and no file of the chart outside its vendored contracts mentions any of those \
                 spellings. Nothing can supply this credential and nobody reading the values file \
                 would learn it exists. Checked against all {} `ci/` values file(s); {} to the \
                 image.{summary}",
                row.path,
                row.images.join(", "),
                finding.values_files.len(),
                if row.required { "required" } else { "optional" }
            )),
        );
    }

    for row in by_container(&surface.unclaimed) {
        report.add(
            format!("unclaimed: {}", row.chart),
            error(format!(
                "{} container {} mounts {} from a Secret, and no contract this chart vendors \
                 spells any of those names, as a key or as a leaf of a dynamic map. Under {}. Gate \
                 3 did not judge them: it inspects a resolved secrets directory on a declared \
                 consumer, and these are outside that reach.",
                row.workload,
                crate::gate::quoted(&row.container),
                row.files.join(", "),
                row.values_files.join(", ")
            )),
        );
    }

    for row in by_container(&surface.over_projected) {
        report.add(
            format!("over-projected: {}", row.chart),
            error(format!(
                "{} container {} runs the {} image and is given {}, none of which that image's own \
                 contract declares; a sibling image's does. The pod holds credentials the binary \
                 inside it never reads. Under {}. Gate 3 did not judge them: it inspects a \
                 resolved secrets directory on a declared consumer, and this container is outside \
                 that reach.",
                row.workload,
                crate::gate::quoted(&row.container),
                row.image,
                row.files.join(", "),
                row.values_files.join(", ")
            )),
        );
    }

    for elevated in &surface.elevated {
        report.add(
            format!("elevated: {}", elevated.chart),
            warning(format!(
                "{} is `secret: false` in the contract and this chart delivers it as the Secret \
                 file {} to {}. Chart policy electing to treat a value as more sensitive than the \
                 image does; its text_form is {}, so a file can supply it and gate 3 permits it. \
                 Recorded, not counted against the chart.",
                elevated.path,
                crate::gate::quoted(&elevated.file_name),
                elevated.containers.join(", "),
                crate::gate::quoted(&elevated.text_form)
            )),
        );
    }

    report
}

/// One container's worth of mounts, which is one problem rather than one per file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// The chart.
    pub chart: String,
    /// The workload.
    pub workload: String,
    /// The container's name.
    pub container: String,
    /// The image's vendored contract, without the path around it.
    pub image: String,
    /// The file names it received.
    pub files: Vec<String>,
    /// The values files that produced them.
    pub values_files: Vec<String>,
}

/// Groups mounts by container rather than by file.
///
/// A container over-projected six credentials has one problem, not six, and six near-identical lines
/// is how a reader learns to skim a report. The values files stay in the row because which fixture
/// produced it is what somebody reproducing this needs.
#[must_use]
pub fn by_container(mounts: &[Mount]) -> Vec<Container> {
    /// Chart, workload, container and image: the container one row is about.
    type Identity<'a> = (&'a str, &'a str, &'a str, &'a str);
    /// The file names it received, and the values files that produced them.
    type Received<'a> = (BTreeSet<&'a str>, BTreeSet<&'a str>);

    let mut grouped: BTreeMap<Identity<'_>, Received<'_>> = BTreeMap::new();
    for mount in mounts {
        let identity = (
            mount.chart.as_str(),
            mount.workload.as_str(),
            mount.container.as_str(),
            mount.image.as_str(),
        );
        let held = grouped.entry(identity).or_default();
        held.0.insert(&mount.file_name);
        held.1.insert(&mount.values_file);
    }
    grouped
        .into_iter()
        .map(
            |((chart, workload, container, image), (files, values_files))| Container {
                chart: chart.to_owned(),
                workload: workload.to_owned(),
                container: container.to_owned(),
                image: image.to_owned(),
                files: files.into_iter().map(str::to_owned).collect(),
                values_files: values_files.into_iter().map(str::to_owned).collect(),
            },
        )
        .collect()
}

// ------------------------------------------------------------------------------------------------
// The pieces the walks are built from
// ------------------------------------------------------------------------------------------------

/// Whether any contract this chart vendors claims the file name, exactly or as a map leaf.
///
/// Deliberately not a union: merging every contract a chart vendors is a merge that can fail on a
/// disagreement between two of them, and a report that refuses to run because two images document
/// one key differently has traded the answer for a check the gates already make.
fn named_anywhere(contracts: &[&Union], file_name: &str) -> bool {
    contracts.iter().any(|union| {
        union.key_by("secrets_file", file_name).is_some()
            || union.container_of("secrets_file", file_name).is_some()
    })
}

/// The chart's own files that mention any spelling of one credential.
///
/// Answers the question no fixed set of renders can: could *some* values file supply this? A chart
/// that writes a key into its document when an operator sets a value names the key in a template and
/// in `values.yaml`, and the only thing missing is a fixture that sets it. A chart that names it
/// nowhere has no channel at all.
///
/// Matched with boundaries rather than as a substring, because the spellings nest: `a__tokens__api`
/// contains `a__token`, and a substring search would report a retired key as deliverable on the
/// strength of an unrelated one. The boundary is "not another character the spelling could continue
/// with", which is what makes those two distinguishable at all.
///
/// A heuristic, and deliberately one that only chooses between two reports rather than producing
/// one: a chart that assembled a file name by concatenation would name no spelling and be read as
/// having no channel. Every finding is still grounded in what the render did. This decides how
/// loudly to say it, never whether there is anything to say.
///
/// # Errors
/// [`Error::Io`] when the chart tree cannot be walked. An individual unreadable file is skipped: it
/// is one input to a heuristic, and refusing the whole report over it would be the larger mistake.
fn names_credential(chart_dir: &Path, credential: &Credential) -> Result<Vec<String>, Error> {
    let spellings: Vec<&str> = [
        credential.path.as_str(),
        credential.secrets_file.as_str(),
        credential.env.as_str(),
        credential.env_file.as_str(),
    ]
    .into_iter()
    .filter(|spelling| !spelling.is_empty())
    .collect();
    if spellings.is_empty() {
        return Ok(Vec::new());
    }

    // Rust's `regex` has no lookaround, so the boundary is expressed as what may *precede* and
    // *follow* a match instead: the alternatives spell "start of text or a character the spelling
    // could not continue from". The match is then found by searching each candidate position, which
    // is what `captures_iter` over the whole text does anyway.
    let alternatives: Vec<String> = spellings.iter().map(|s| regex::escape(s)).collect();
    let pattern = Regex::new(&format!(
        r"(?:^|[^\w.])(?:{})(?:$|[^\w])",
        alternatives.join("|")
    ))
    .map_err(|failure| {
        Error::Invalid(format!("a credential spelling is not matchable: {failure}"))
    })?;

    let mut found: Vec<String> = Vec::new();
    for path in chart_files(chart_dir)? {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if pattern.is_match(&text) {
            let relative = path
                .strip_prefix(chart_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            found.push(relative);
        }
    }
    found.sort();
    Ok(found)
}

/// Everything under a chart that describes what it delivers, dependencies excluded.
fn chart_files(chart_dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    walk(chart_dir, chart_dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(root: &Path, at: &Path, into: &mut Vec<PathBuf>) -> Result<(), Error> {
    for entry in std::fs::read_dir(at).map_err(|e| Error::io(at.display(), e))? {
        let path = entry.map_err(|e| Error::io(at.display(), e))?.path();
        if path.is_dir() {
            let top = path
                .strip_prefix(root)
                .ok()
                .and_then(|relative| relative.components().next())
                .map(|component| component.as_os_str().to_string_lossy().into_owned());
            if top.is_some_and(|name| EXCLUDED_DIRS.contains(&name.as_str())) {
                continue;
            }
            walk(root, &path, into)?;
        } else if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| CHART_TEXT.contains(&extension))
        {
            into.push(path);
        }
    }
    Ok(())
}

/// Every rendered manifest file of one chart, sorted.
fn renders_of(rendered: &Path, chart: &str) -> Result<Vec<PathBuf>, Error> {
    let opening = format!("{chart}--");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(rendered).map_err(|e| Error::io(rendered.display(), e))? {
        let path = entry.map_err(|e| Error::io(rendered.display(), e))?.path();
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_owned();
        if name.starts_with(&opening) && path.extension() == Some(std::ffi::OsStr::new("yaml")) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// The values file whose render produced this manifest file.
fn values_file_of(render: &Path) -> String {
    render
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| name.split_once("--"))
        .map_or_else(String::new, |(_, values)| values.to_owned())
}

/// One workload as a report names it.
fn workload_of(manifest: &Json) -> String {
    format!(
        "{} {}",
        manifest
            .get("kind")
            .and_then(Json::as_str)
            .unwrap_or("None"),
        manifest
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Json::as_str)
            .unwrap_or("?")
    )
}

/// `chart/contracts/api.json` as `api`: the vendored file, without the path around it.
fn short(label: &str) -> String {
    Path::new(label)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(label)
        .to_owned()
}

/// Every key one contract declares.
fn keys_of(contract: &Json) -> impl Iterator<Item = &serde_json::Map<String, Json>> {
    contract
        .get("schema")
        .and_then(|schema| schema.get("keys"))
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_object)
}

/// One string field of a key, empty when it is absent or is not one.
fn text(key: &serde_json::Map<String, Json>, name: &str) -> String {
    key.get(name)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// A key's first non-blank documentation line.
fn first_line(docs: Option<&Json>) -> String {
    docs.and_then(Json::as_str)
        .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or_default()
        .to_owned()
}

/// Sorted and deduplicated, which is what every merged list here is.
fn sorted(items: impl Iterator<Item = String>) -> Vec<String> {
    let held: BTreeSet<String> = items.collect();
    held.into_iter().collect()
}

/// The dialect prefix every environment spelling shares, or nothing when they differ.
///
/// A derivation with a check under it rather than a convenience for a table. The spellings *are*
/// derived from the config path by the dialect, and stating that once above a table is both shorter
/// and more useful than a column every row of which restates it, but only while it is true, so the
/// caller prints the literal spellings under any row this does not cover.
#[must_use]
pub fn common_prefix<'a>(spellings: impl IntoIterator<Item = &'a str>) -> &'a str {
    let mut held: Option<&str> = None;
    for spelling in spellings {
        held = Some(match held {
            None => spelling,
            Some(seen) => {
                let shared = seen
                    .char_indices()
                    .zip(spelling.chars())
                    .take_while(|((_, left), right)| left == right)
                    .last()
                    .map_or(0, |((at, left), _)| at + left.len_utf8());
                &seen[..shared]
            }
        });
    }
    held.unwrap_or_default()
}

#[cfg(test)]
mod tests;
