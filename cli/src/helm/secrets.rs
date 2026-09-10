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
//! deliberately skips the staleness interlock [`bind`] applies — a digest bump produces one run
//! where the pinned digest and the vendored contract disagree, and refusing to print the inventory
//! then would withhold the document at exactly the moment a reviewer is reading a credential change.
//! Whether the vendored copy is current is a different check's question.
//!
//! **The reconciliation** is what a rendered chart actually delivers, against that inventory. It
//! does apply the interlock, because it reasons about which image a container runs and an answer
//! derived from a contract belonging to a different digest is worse than no answer.
//!
//! Three conclusions, and the scope of each is load-bearing:
//!
//! - `undeliverable` — a contract key no rendered configuration can supply through any channel.
//!   Judged across *every* values file the chart ships, because a credential delivered under one
//!   fixture is delivered: the chart has a way to supply it.
//! - `unclaimed` — a delivered Secret file name that no contract of the chart names, by neither an
//!   exact spelling nor a dynamic-map leaf. Scoped against every contract the chart vendors, which
//!   is what keeps it disjoint from over-projection.
//! - `over-projected` — a Secret file whose name a *sibling* image's contract claims and the
//!   container's own image's contract does not. A pod holding a credential its binary never reads is
//!   a least-privilege defect, and it is scoped per container against that one image's contract,
//!   never against the union.
//!
//! # Not duplicating gate 3
//!
//! Gate 3 already errors on a name unknown to the contract, but only for a file inside a *resolved*
//! secrets directory on a container the declaration lists as a consumer of that document. Both
//! halves of that condition are recorded per file here, and a file gate 3 would have judged is
//! reported by neither `unclaimed` nor `over_projected` — it is already one line in the check, and a
//! second line saying the same thing in a different report trains a reader to skim both. What
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
//! A key may carry `env_aliases`, `env_file_aliases` and `secrets_file_aliases`; [`crate::classify`]
//! and [`Union::key_by`] ignore them, and this module agrees with the normative reader rather than
//! being independently cleverer than the gate whose findings it sits beside.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value as Json;

use crate::classify::{Kind, classify};
use crate::error::Error;
use crate::gate::container::ContainerView;
use crate::k8s::{containers_of, digest_of, load_manifests, pod_spec, secret_file_names, select};
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
// The inventory — what the vendored contracts declare
// ------------------------------------------------------------------------------------------------

/// One `secret: true` key, as one image's contract spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The chart that vendors the contract.
    pub chart: String,
    /// The declared document the contract belongs to.
    pub document: String,
    /// The vendored file as [`bind`] labels it — `<chart>/contracts/<name>.json`. The same spelling
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
/// rows — the inventory's job is to name the artefacts an operator has to create, and collapsing
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
pub fn declared_secrets(
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
        found.extend(declared_secrets(&chart_dir, &declaration)?);
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
/// is a question about one image — two images declaring one path are two credentials to deliver, and
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
    // of — which is exactly the case the over-projection check exists to see.
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

/// Where in the render one file was found, carried whole so the scan reads as one subject.
#[derive(Debug, Clone, Copy)]
struct Where<'a> {
    values_file: &'a str,
    workload: &'a str,
    container: &'a str,
}

/// Record the keys the rendered plaintext document sets, as one delivery channel.
fn scan_document(document: &Document, binding: &Binding, manifests: &[Json], ledger: &mut Ledger) {
    let matched = select(manifests, &document.source.kind, &document.source.selector);
    if matched.len() != 1 {
        return;
    }
    let Some(text) = matched[0]
        .get("data")
        .and_then(|data| data.get(&document.source.key))
        .and_then(Json::as_str)
    else {
        return;
    };
    let Ok(instance) = document.source.format.read(text) else {
        return;
    };

    let mut present = BTreeSet::new();
    document_paths(&instance, "", &mut present);
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
    let view = ContainerView::read(container, mine);
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
        // anyway. Legitimate — chart policy may treat a value as more sensitive than the image does,
        // and gate 3 permits it for any `text` key — so it is stated and never counted against the
        // chart. Only an exact match is judged: a dynamic map's leaves have no sensitivity of their
        // own for the contract to state. And only a file-supplyable key, because the same mount of a
        // key of any other form is gate 3's error rather than a policy choice.
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
/// container absent from that list — an init container, a sidecar, a workload whose selector matches
/// nothing — is a container gate 3 never opens. Recomputing the same selection here is what lets
/// this report stay silent about files gate 3 has already reported and speak about the ones it
/// cannot see.
fn gate_three_reach(declaration: &Declaration, manifests: &[Json]) -> BTreeSet<(String, String)> {
    let mut reach = BTreeSet::new();
    for document in &declaration.documents {
        for consumer in &document.consumers {
            for workload in select(manifests, &consumer.kind, &consumer.selector) {
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

/// Turn one chart's scan into the findings that outlive it.
fn collect(
    chart_dir: &Path,
    declaration: &Declaration,
    ledger: &Ledger,
    values_files: &[String],
    surface: &mut Surface,
) -> Result<(), Error> {
    for credential in credentials(&declared_secrets(chart_dir, declaration)?) {
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

/// Turn one scan into findings, at the level each conclusion has earned.
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

/// Group mounts by container rather than by file.
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

/// Every dotted path the parsed configuration document sets, leaves and tables alike.
///
/// Tables are included so a `structured` key supplied as a whole table counts as supplied, which is
/// what the loader sees.
fn document_paths(instance: &Json, prefix: &str, into: &mut BTreeSet<String>) {
    let Some(fields) = instance.as_object() else {
        return;
    };
    for (name, value) in fields {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        document_paths(value, &full, into);
        into.insert(full);
    }
}

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
/// having no channel. Every finding is still grounded in what the render did — this decides how
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

/// `chart/contracts/api.json` as `api` — the vendored file, without the path around it.
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
/// and more useful than a column every row of which restates it — but only while it is true, so the
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
mod tests {
    use serde_json::{Value as Json, json};

    use super::{
        Credential, Declared, Ledger, Mount, Where, by_container, common_prefix, credentials,
        document_paths, gate_three_reach, names_credential, scan_file,
    };
    use crate::gate::container::ContainerView;
    use crate::gate::document::DocumentSource;
    use crate::helm::declaration::{Consumer, Declaration, Document};
    use crate::k8s::secret_file_names;
    use crate::union::{Union, union_contracts};

    /// The two fixtures every scope question here is asked against: one image's contract, and a
    /// sibling's. A file only the sibling declares is over-projection; a file neither declares is
    /// unclaimed, and collapsing the two into a union would erase the distinction the whole report
    /// rests on.
    const API: &str = include_str!("../../tests/fixtures/api.json");
    const WORKER: &str = include_str!("../../tests/fixtures/worker.json");

    fn union(label: &str, contract: &str) -> Union {
        union_contracts(&[(
            label.to_owned(),
            serde_json::from_str(contract).expect("a fixture contract"),
        )])
        .expect("one contract merges with itself")
    }

    /// The `api` fixture plus one `text` key the image does *not* consider secret.
    ///
    /// No fixture on disk carries that combination, and it is the exact shape of a key a chart
    /// elevates: a value the image treats as ordinary configuration and the chart delivers as a
    /// Secret file anyway.
    fn with_plain_text_key() -> Union {
        let mut contract: Json = serde_json::from_str(API).expect("a fixture contract");
        contract["schema"]["keys"]
            .as_array_mut()
            .expect("a list of keys")
            .push(json!({
                "path": "email.username",
                "env": "FIXTURE_EMAIL__USERNAME",
                "env_file": "FIXTURE_EMAIL__USERNAME_FILE",
                "secrets_file": "email__username",
                "docs": "The mailbox login.",
                "ty": "String",
                "values": [],
                "constraint": {"type": "string"},
                "text_constraint": null,
                "text_form": "text",
                "aliases": [],
                "default": null,
                "default_value": null,
                "note": null,
                "required": false,
                "secret": false,
                "reserved": false,
            }));
        union_contracts(&[("api".to_owned(), contract)]).expect("one contract merges with itself")
    }

    fn declaration(documents: Vec<Document>) -> Declaration {
        Declaration {
            chart: "fixture".to_owned(),
            path: std::path::PathBuf::from("fixture"),
            documents,
            reason: None,
            unconfigured: Vec::new(),
            bindings: false,
            unbound: Vec::new(),
            credentials: std::collections::BTreeMap::new(),
        }
    }

    fn container(env: &[(&str, &str)]) -> Json {
        json!({
            "name": "app",
            "image": format!("example/app:v1@sha256:{}", "0".repeat(64)),
            "env": env
                .iter()
                .map(|(name, value)| json!({"name": name, "value": value}))
                .collect::<Vec<_>>(),
            "volumeMounts": [],
        })
    }

    // -- what a volume delivers ------------------------------------------------------------

    #[test]
    fn a_projected_volume_reports_only_its_secret_sources() {
        // `config.toml` arriving from a ConfigMap in the same volume is not a credential, and
        // counting it would make every pod in a repository look wrong at once.
        let spec = json!({
            "volumes": [{
                "name": "config",
                "projected": {"sources": [
                    {"configMap": {"name": "cm", "items": [{"key": "config.toml"}]}},
                    {"secret": {"name": "s", "items": [{"key": "k", "path": "db__url"}]}},
                ]},
            }]
        });
        assert_eq!(secret_file_names(&[], &spec, "config"), ["db__url"]);
    }

    #[test]
    fn a_source_without_items_presents_every_key_of_the_rendered_secret() {
        let spec = json!({"volumes": [{"name": "creds", "secret": {"secretName": "existing"}}]});
        let manifests = [json!({
            "kind": "Secret",
            "metadata": {"name": "existing"},
            "data": {"database__url": "eA=="},
            "stringData": {"auth__jwt_secret": "x"},
        })];
        assert_eq!(
            secret_file_names(&manifests, &spec, "creds"),
            ["auth__jwt_secret", "database__url"]
        );
    }

    #[test]
    fn a_secret_the_operator_supplies_contributes_no_names() {
        // Nothing renders it, so nothing can be read from it. Silence is the only honest answer.
        let spec =
            json!({"volumes": [{"name": "creds", "secret": {"secretName": "not-rendered"}}]});
        assert!(secret_file_names(&[], &spec, "creds").is_empty());
    }

    #[test]
    fn a_volume_the_pod_does_not_have_is_not_an_error() {
        assert!(secret_file_names(&[], &json!({"volumes": []}), "missing").is_empty());
    }

    #[test]
    fn every_table_and_every_leaf_of_a_document_is_a_path() {
        let mut found = std::collections::BTreeSet::new();
        document_paths(
            &json!({"auth": {"jwt_secret": "x"}, "port": 8080}),
            "",
            &mut found,
        );
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            ["auth", "auth.jwt_secret", "port"]
        );
    }

    // -- the inventory ---------------------------------------------------------------------

    fn declared(document: &str, contract: &str, image: &str, required: bool) -> Declared {
        Declared {
            chart: "c".to_owned(),
            document: document.to_owned(),
            contract: contract.to_owned(),
            image: image.to_owned(),
            path: "database.url".to_owned(),
            secrets_file: "database__url".to_owned(),
            env: "FIXTURE_DATABASE__URL".to_owned(),
            env_file: "FIXTURE_DATABASE__URL_FILE".to_owned(),
            text_form: "text".to_owned(),
            required,
            summary: "The connection string.".to_owned(),
        }
    }

    #[test]
    fn two_images_reading_one_key_are_one_row_naming_both() {
        let rows = credentials(&[
            declared("api", "c/contracts/api.json", "api", false),
            declared("worker", "c/contracts/worker.json", "worker", false),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].images, ["api", "worker"]);
        assert_eq!(rows[0].documents, ["api", "worker"]);
    }

    #[test]
    fn required_unions_the_way_the_contract_model_unions_it() {
        // A key any reader requires is a key the deployment must carry.
        let rows = credentials(&[
            declared("d", "c/contracts/a.json", "a", false),
            declared("d", "c/contracts/b.json", "b", true),
        ]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].required);
    }

    #[test]
    fn two_spellings_of_one_path_stay_two_rows() {
        // Two files on disk and two variables. Collapsing them would name an artefact that does not
        // exist, which is the opposite of what an inventory is for.
        let mut renamed = declared("d", "c/contracts/b.json", "b", false);
        renamed.secrets_file = "db__url".to_owned();
        let rows = credentials(&[declared("d", "c/contracts/a.json", "a", false), renamed]);
        assert_eq!(rows.len(), 2);
    }

    // -- which files of a chart name a credential -------------------------------------------

    fn credential(path: &str, secrets_file: &str) -> Credential {
        Credential {
            chart: "c".to_owned(),
            path: path.to_owned(),
            secrets_file: secrets_file.to_owned(),
            env: "TANKOVAULT_INTERNAL__TOKEN".to_owned(),
            env_file: "TANKOVAULT_INTERNAL__TOKEN_FILE".to_owned(),
            required: false,
            summary: String::new(),
            documents: vec!["d".to_owned()],
            images: vec!["a".to_owned()],
            contracts: vec!["c/contracts/a.json".to_owned()],
        }
    }

    /// A chart directory with the files a test wants in it, removed when it goes out of scope.
    ///
    /// Hand-rolled rather than a dependency: three of these tests need a directory on disk, and the
    /// crate that would supply one would be carried by every consumer of this library's test build.
    struct Chart(std::path::PathBuf);

    impl Chart {
        fn of(files: &[(&str, &str)]) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir().join(format!(
                "terrace-secrets-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&at);
            for (name, body) in files {
                let path = at.join(name);
                std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
                std::fs::write(path, body).expect("a file is written");
            }
            Self(at)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Chart {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_longer_spelling_that_contains_the_shorter_one_does_not_name_it() {
        // `internal__tokens__api` contains `internal__token`, so a substring search calls the
        // retired tier-wide key deliverable on the strength of an unrelated one — and downgrades
        // the only finding that mattered.
        let chart = Chart::of(&[(
            "templates/_secrets.tpl",
            "key: {{ printf \"internal__tokens__%s\" . }}\n{{ .Values.internal.tokens }}\n",
        )]);
        assert!(
            names_credential(
                chart.path(),
                &credential("internal.token", "internal__token")
            )
            .expect("the chart is readable")
            .is_empty()
        );
    }

    #[test]
    fn the_spelling_itself_names_it() {
        let chart = Chart::of(&[("values.yaml", "# internal.token is gone\n")]);
        assert_eq!(
            names_credential(
                chart.path(),
                &credential("internal.token", "internal__token")
            )
            .expect("the chart is readable"),
            ["values.yaml"]
        );
    }

    #[test]
    fn the_vendored_contracts_are_not_evidence_that_the_chart_delivers_it() {
        // They are the other side of the comparison: every credential is named in one by
        // definition, so counting them would make the question unanswerable.
        let chart = Chart::of(&[(
            "contracts/a.json",
            "{\"secrets_file\": \"internal__token\"}",
        )]);
        assert!(
            names_credential(
                chart.path(),
                &credential("internal.token", "internal__token")
            )
            .expect("the chart is readable")
            .is_empty()
        );
    }

    // -- one mounted file, which is where each of the three reports is decided ---------------

    /// One call of [`scan_file`], with everything a test does not care about held constant.
    struct Scan {
        mine: Union,
        sibling: Union,
        ledger: Ledger,
        surface: super::Surface,
    }

    impl Scan {
        fn new() -> Self {
            Self {
                mine: union("worker", WORKER),
                sibling: union("api", API),
                ledger: Ledger::default(),
                surface: super::Surface::default(),
            }
        }

        fn scan(
            &mut self,
            file_name: &str,
            mount_path: &str,
            secrets_dir: Option<&str>,
            judged: &[(&str, &str)],
            env: &[(&str, &str)],
        ) {
            let held = container(env);
            let view = ContainerView::read(&held, &self.mine);
            let contracts = [&self.mine, &self.sibling];
            let judged = judged
                .iter()
                .map(|(workload, container)| ((*workload).to_owned(), (*container).to_owned()))
                .collect();
            scan_file(
                &declaration(Vec::new()),
                &contracts,
                &self.mine,
                &view,
                secrets_dir,
                Where {
                    values_file: "default.yaml",
                    workload: "Deployment app",
                    container: "app",
                },
                mount_path,
                file_name,
                &judged,
                &mut self.ledger,
                &mut self.surface,
            );
        }

        fn supplied(&self) -> Vec<(&str, &str, Vec<&str>)> {
            self.ledger
                .supplied
                .iter()
                .map(|((contract, path), channels)| {
                    (
                        contract.as_str(),
                        path.as_str(),
                        channels.iter().map(String::as_str).collect(),
                    )
                })
                .collect()
        }
    }

    #[test]
    fn a_key_the_container_reads_is_recorded_as_supplied_and_reported_nowhere() {
        let mut scan = Scan::new();
        scan.scan("database__url", "/secrets", Some("/secrets"), &[], &[]);
        assert_eq!(
            scan.supplied(),
            [("worker", "database.url", vec!["the secrets directory"])]
        );
        assert!(scan.surface.unclaimed.is_empty());
        assert!(scan.surface.over_projected.is_empty());
    }

    #[test]
    fn a_file_outside_the_secrets_directory_supplies_nothing_without_indirection() {
        // Nothing opens it. A loader reads the directory it was told about and the files a `_FILE`
        // variable names, and a file lying anywhere else is a file lying anywhere else.
        let mut scan = Scan::new();
        scan.scan("database__url", "/elsewhere", Some("/secrets"), &[], &[]);
        assert!(scan.supplied().is_empty());
    }

    #[test]
    fn a_file_named_by_a_file_variable_supplies_its_key_wherever_it_is_mounted() {
        let mut scan = Scan::new();
        scan.scan(
            "database__url",
            "/elsewhere",
            Some("/secrets"),
            &[],
            &[("FIXTURE_DATABASE__URL_FILE", "/elsewhere/database__url")],
        );
        assert_eq!(
            scan.supplied(),
            [("worker", "database.url", vec!["`_FILE` indirection"])]
        );
    }

    #[test]
    fn a_sibling_images_key_is_over_projection_rather_than_an_unknown_name() {
        let mut scan = Scan::new();
        scan.scan("auth__session_ttl", "/secrets", Some("/other"), &[], &[]);
        assert!(scan.surface.unclaimed.is_empty());
        assert_eq!(scan.surface.over_projected.len(), 1);
        assert_eq!(
            scan.surface.over_projected[0].file_name,
            "auth__session_ttl"
        );
    }

    #[test]
    fn a_name_no_contract_of_the_chart_spells_is_unclaimed() {
        let mut scan = Scan::new();
        scan.scan("nothing__spells_this", "/secrets", Some("/other"), &[], &[]);
        assert!(scan.surface.over_projected.is_empty());
        assert_eq!(scan.surface.unclaimed.len(), 1);
    }

    #[test]
    fn a_file_gate_three_already_judged_produces_no_second_line() {
        // It is already one line in the check, and a second line saying the same thing in a
        // different report is how a reader learns to skim both.
        let mut scan = Scan::new();
        scan.scan(
            "nothing__spells_this",
            "/secrets",
            Some("/secrets"),
            &[("Deployment app", "app")],
            &[],
        );
        assert!(scan.surface.unclaimed.is_empty());
        assert!(scan.surface.over_projected.is_empty());
    }

    #[test]
    fn gate_three_needs_both_halves_before_it_owns_the_verdict() {
        // The declaration lists the container, but the file is outside the secrets directory gate 3
        // resolves — so gate 3 never inspected it and this report must still speak.
        let mut scan = Scan::new();
        scan.scan(
            "nothing__spells_this",
            "/elsewhere",
            Some("/secrets"),
            &[("Deployment app", "app")],
            &[],
        );
        assert_eq!(scan.surface.unclaimed.len(), 1);
    }

    #[test]
    fn a_non_secret_text_key_delivered_as_a_secret_is_recorded_as_policy() {
        let mut scan = Scan::new();
        scan.mine = with_plain_text_key();
        scan.sibling = union("api", API);
        scan.scan("email__username", "/secrets", Some("/secrets"), &[], &[]);
        assert_eq!(
            scan.ledger.elevated.keys().collect::<Vec<_>>(),
            [&(
                "email.username".to_owned(),
                "email__username".to_owned(),
                "text".to_owned()
            )]
        );
        assert!(scan.surface.over_projected.is_empty());
    }

    #[test]
    fn a_non_secret_key_no_file_can_supply_is_left_to_gate_three() {
        // `worker.concurrency` is an integer, and the mount of it is gate 3's error rather than a
        // policy choice — which it already says at length.
        let mut scan = Scan::new();
        scan.scan(
            "worker__concurrency",
            "/secrets",
            Some("/secrets"),
            &[],
            &[],
        );
        assert!(scan.ledger.elevated.is_empty());
    }

    // -- which containers the check already opens -------------------------------------------

    fn workload() -> Json {
        json!({
            "kind": "Deployment",
            "metadata": {"name": "app", "labels": {"app.kubernetes.io/component": "api"}},
            "spec": {"template": {"spec": {
                "initContainers": [{"name": "migrate"}],
                "containers": [{"name": "api"}],
            }}},
        })
    }

    fn naming(container: &str) -> Declaration {
        declaration(vec![Document {
            name: "api".to_owned(),
            source: DocumentSource {
                kind: "ConfigMap".to_owned(),
                selector: Vec::new(),
                key: "config.toml".to_owned(),
                format: crate::gate::document::DocumentFormat::Toml,
            },
            images: Vec::new(),
            consumers: vec![Consumer {
                kind: "Deployment".to_owned(),
                selector: vec![("app.kubernetes.io/component".to_owned(), "api".to_owned())],
                containers: vec![container.to_owned()],
            }],
            exempt: Vec::new(),
        }])
    }

    #[test]
    fn a_declared_container_is_within_reach_and_an_init_container_is_not() {
        // The blind spot this whole report exists for: a container no declaration names is a
        // container gate 3 never opens, and nothing else checks what it mounts.
        let reach = gate_three_reach(&naming("api"), &[workload()]);
        assert_eq!(
            reach.iter().collect::<Vec<_>>(),
            [&("Deployment app".to_owned(), "api".to_owned())]
        );
        assert!(!reach.contains(&("Deployment app".to_owned(), "migrate".to_owned())));
    }

    // -- the rendering ----------------------------------------------------------------------

    #[test]
    fn one_container_over_projecting_several_files_is_one_row() {
        // Six near-identical lines is how a reader learns to skim a report.
        let mount = |file_name: &str, values_file: &str| Mount {
            chart: "c".to_owned(),
            values_file: values_file.to_owned(),
            workload: "Deployment app".to_owned(),
            container: "migrate".to_owned(),
            image: "bootstrap".to_owned(),
            mount_path: "/secrets".to_owned(),
            file_name: file_name.to_owned(),
            judged_by_gate_three: false,
        };
        let rows = by_container(&[
            mount("a", "one.yaml"),
            mount("b", "one.yaml"),
            mount("a", "two.yaml"),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].files, ["a", "b"]);
        assert_eq!(rows[0].values_files, ["one.yaml", "two.yaml"]);
    }

    #[test]
    fn the_dialect_prefix_is_what_every_spelling_shares() {
        assert_eq!(common_prefix(["FIXTURE_A__B", "FIXTURE_C__D"]), "FIXTURE_");
        assert_eq!(common_prefix(["A_ONE", "B_TWO"]), "");
        assert_eq!(common_prefix([]), "");
    }
}
