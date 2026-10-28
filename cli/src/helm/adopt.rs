//! Writing the chart values for the contract keys a chart surfaces nothing for.
//!
//! The bindings gate reports the failure a chart repository keeps hitting: an image release adds a
//! setting, the automated bump repins the digest and omits everything else, and the new key is
//! reached by no chart value. It reports it per key and names the two repairs — bind the key to a
//! value, or write it off with a reason — and both are hand work. The first is the same block every
//! time:
//!
//! ```text
//! # @schema
//! # # @config projection telemetry.sentry.sample_rate optional
//! # type: [number, 'null']
//! # minimum: 0
//! # maximum: 1
//! # @schema
//! # -- Fraction of events sent to Sentry (`telemetry.sentry.sample_rate`).
//! sampleRate: null
//! ```
//!
//! Every line of that is in the contract already: the constraint is the shapes writer's rendering
//! of the key's own JSON Schema, the description is the first paragraph of the producer's prose, the
//! default is the one the binary compiles in, and the marker names the key the block was derived
//! from. So it is written from there, and what is left for a person is the judgement this cannot
//! make — whether the setting deserves a first-class value at all, and what the chart's template
//! does with it.
//!
//! It reads the vendored contract and nothing else, so what it can see is whatever the last refresh
//! fetched — and that refresh resolves the image `values.yaml` pins. A newer build reaches this only
//! once the pin moves.
//!
//! # Why this may generate a marker when the bindings gate says nothing does
//!
//! On a value that already exists the *value* is the fact and a marker is a claim about it, and a
//! wrong claim reads exactly like a right one. This never touches a value that exists. It creates
//! one, from a key, and emits the marker in the same breath — so there is no prior mapping for the
//! marker to be wrong about. That is [`super::scaffold`]'s argument for a chart being created; the
//! only thing new here is that the chart is already on disk.
//!
//! **The boundary is enforced rather than assumed.** A key whose chart value already exists is
//! refused, with the marker to write by hand — the one case where the two facts could disagree.
//!
//! # What it writes, and what it only prints
//!
//! Written into `values.yaml`:
//!
//! | what | why |
//! |---|---|
//! | an ordinary key | a value typed and defaulted from the contract, with its marker |
//! | a credential | a value with no marker, for the Secret rather than the document |
//! | a missing grouping block | the `telemetry:` the new value needs, described from its key prefix |
//!
//! Written into `config-contract.yaml`:
//!
//! | what | why |
//! |---|---|
//! | an `unbound` group | for a `reserved` key and a credential — the two write-offs whose reason the contract states rather than a person deciding it |
//! | a `credentials` row | the `value:` column, which is the chart value just written |
//!
//! Written into the template file that opens the define:
//!
//! | what | why |
//! |---|---|
//! | the `derivedConfig` lines | appended at the define's own depth, only for a key whose root the define does not already write — a second `telemetry:` beside an existing one is dropped by `fromYaml` in silence |
//! | the `secretData` lines | the same rule, by secrets-file name |
//!
//! Printed and never written: a projection whose root the define already carries (where it belongs
//! inside a hand-written block is a person's question), a credential's `note:` (when a release needs
//! it is the chart's knowledge), a better sentence for a grouping block (a contract says nothing
//! about the blocks a chart groups keys into), and a chosen default for a required key whose image
//! publishes none.
//!
//! So a written run leaves the chart binding, rendering and testing the new settings. Two generated
//! files it does *not* touch are owed immediately afterwards, and the closing output leads with
//! them: `values.schema.json`, without which every render fails on a values block the schema has not
//! seen, and the round-trip suite, whose new cases prove the projection actually arrived.
//!
//! # Where the block goes
//!
//! After the last sibling that binds a contract key, inside the deepest ancestor of the new value
//! that already exists. Alphabetically would be wrong — these files are ordered by how often a value
//! is edited, the image first and thirty blocks of chassis at the bottom — and the end of the file
//! would be worse, a screen and a half below the values the new one belongs with. The last *bound*
//! sibling is where the configuration surface ends, and it is derived from the markers rather than
//! declared, which is what makes it right for a chart nobody wrote the rule for.
//!
//! # Textual, on purpose
//!
//! Every edit is an insertion of whole lines; nothing is parsed and re-serialised. A YAML emitter
//! would drop every comment in the file, and the comments are the documentation, the `@schema`
//! blocks and the markers. Line endings are kept, so a CRLF file stays CRLF.
//!
//! # Where it departs from the script it replaces
//!
//! The report and every file this writes are byte for byte what `adopt-config.py` printed and
//! wrote, over every chart in the repository it served and over mutations of it built to make each
//! chart owe keys. They differ only where the script was wrong:
//!
//! - **Two blocks meeting at one line.** When a new leaf at the end of a block and a new sibling of
//!   that block go after the same line, the script wrote them in the reverse of the order it
//!   listed them in. Whether the leaf ended up inside its block depended on how the two paths
//!   sorted. When it did not, the leaf became a child of the new sibling, either moved in silence
//!   or making YAML that does not parse. [`splices`] writes the deeper block first, every time. The
//!   report still lists the blocks in the script's order.
//! - **A credentials row and a new `unbound` block meeting at one line.** The same reversal put the
//!   row under the new `unbound:`, where the declaration refuses it. Here the runs land in the
//!   order they are planned.
//! - **Two keys spelling one chart value.** `a.b_c` and `a.bC` both need `a.bC`. The script kept
//!   whichever came second; [`tree_at`](super::scaffold::tree_at) refuses the pair as a mapping to write by hand.
//! - **Two documents agreeing on a key.** The script compared the fields' Python `repr`, so the
//!   same constraint published with its members in another order read as a disagreement. Here
//!   the fields are compared as JSON, where member order means nothing.
//! - **A block scalar whose text looks like a comment.** The script's line walk read `# ...` and
//!   `# @schema` lines inside a `|` or `>` scalar as a comment and a delimiter, so the value ended
//!   early, and the delimiter swallowed every line after it. A new block then went into the middle
//!   of the scalar's text. [`super::regions`] counts every line of a scalar body as content.
//! - **A parent written with its value on one line.** `telemetry: null`, `auth: {}`, `unbound: []`
//!   and `credentials: []` cannot take a block child: a line inserted under one is a second value
//!   for the same key, and the file no longer parses. The script inserted anyway. Here a value
//!   under such a parent is refused, naming the rewrite, and a declaration block in that shape
//!   blocks the chart until it is rewritten by hand — values without their write-offs would leave
//!   the chart failing the gate the run was for.
//! - **A define whose closing `end` shares its line.** Lines are inserted above the define's
//!   closing `end`. When that `end` followed an inner `{{- end }}` on the same line, the script put
//!   the projection inside the inner `with`; when the whole define was on one line, above the
//!   define. Here the closing `end` has to be alone on a line of its own, or the lines are printed
//!   for a person instead.
//! - **A `text_form` this build does not implement.** Refused per key, with the rest of the
//!   contract adopted. The script typed such a key as text, or failed the run when it was a
//!   credential.
//! - **A string with a line break in it.** A default or an enum member holding a newline, a tab,
//!   another control character or a Unicode line separator is written escaped, on one line. The
//!   script wrote the raw character, which broke the `@schema` comment or the value it was in.
//! - **Writing.** The script wrote file by file as it reported, so a failure part-way left some
//!   charts adopted and others not, and a file could be left half-written. [`write_all`] plans every
//!   file of every chart first, stages each beside its target, and only then renames them into
//!   place: a failure before the renames leaves every file as it was. A file that resolves outside
//!   its chart — a symbolic link to somebody else's template — fails the run before anything is
//!   written.
//! - **Unreadable input.** A tree the script could not read ended in a Python traceback. Here it
//!   ends in one `error:` line. With `--check` that exits 2, so a gate can tell "owes a key" from
//!   "could not answer"; otherwise it exits 1, as the traceback did.
//! - **`--check` and `--diff`** are new. The script had only a plan and `--write`.
//!
//! Three smaller differences, none of which any chart in the repository reaches:
//!
//! - A number is written as Rust prints it, so a default of `0.0000001` is `1e-7` where the
//!   script wrote `1e-07`. Both read back as the same number.
//! - A file starting with a byte-order mark is refused as YAML that does not parse, as every other
//!   reader in this crate refuses it. The script's reader skipped the mark.
//! - Lines are split where Python splits them, so line numbers agree with the script's. The
//!   `migrate` command counts lines as an editor does instead; the two differ only on a lone `\r`
//!   and the Unicode separators, which no chart file contains.

mod apply;
mod placement;
mod report;
mod template;
mod write_offs;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde_json::Value as Json;

use crate::error::Error;
use crate::report::{Level, Report};
use crate::union::Merged;

use super::bindings::{missing_keys, premises};
use super::declaration::{Bound, load_declaration, read_yaml};
use super::markers::{Marker, read as read_markers};
use super::regions::{regions, split_lines};
use super::scaffold::{Key, Placement, Plan, plan_keys, service_key_for};

pub use self::apply::{Splice, splices, write, write_all};
use self::placement::insertions_for;
pub use self::report::{closing, counted, report};
use self::template::projection_edits;
use self::write_offs::declaration_edits;

/// The fields of a contract key that decide what its chart value looks like.
///
/// Two documents declaring one path have to agree on every one of them before a single value can
/// feed both, and the list is the planner's inputs rather than a subset chosen by hand: the planner
/// branches on `reserved`, `secret` and `text_form`, the schema lines read `constraint` and
/// `required`, the default reads `default_value`, and the delivery reads `secrets_file`.
const SHAPE_FIELDS: [&str; 7] = [
    "constraint",
    "text_form",
    "required",
    "secret",
    "reserved",
    "default_value",
    "secrets_file",
];

/// One key no value is written for, and what to do instead.
///
/// Reported per key rather than failing the chart, so a contract that adds five ordinary keys and
/// one awkward one still gets the five. Every refusal names the hand edit that settles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The contract key.
    pub path: String,
    /// Why, as the rest of a sentence that starts with the key.
    pub reason: String,
    /// Whether this is "the chart already has this value". A credential refused that way is still
    /// owed its `unbound` entry — the write-off is a property of the key, not of whoever wrote the
    /// value — so the two kinds of refusal are told apart rather than counted together.
    pub occupied: bool,
}

/// One run of lines for `values.yaml`, and the line it goes after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insertion {
    /// The 1-based line the block goes after; `0` is the top of the file.
    pub after: usize,
    /// The block, without the blank line [`Insertion::spaced`] adds above it.
    pub lines: Vec<String>,
    /// The contract keys it writes values for, sorted.
    pub paths: Vec<String>,
    /// The grouping blocks it creates, as values paths, each described only from its key prefix.
    pub branches: Vec<String>,
    /// Whether a blank line goes above the block. Not where it is the first thing under the
    /// mapping key it belongs to: every values file opens a block with its first value.
    pub spaced: bool,
    /// How many segments deep the block is written, which orders two blocks meeting at one line.
    depth: usize,
}

/// One run of lines for a file other than `values.yaml`, and the line it goes after.
///
/// An adoption touches three files, and the two beyond `values.yaml` are the ones a chart is red
/// without: a key surfaced by no value is owed an `unbound` entry, and a value bound to a key is
/// owed the template line that projects it. A run that wrote the values and left either alone would
/// leave the chart failing the very gates it was run to satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    /// The file.
    pub path: PathBuf,
    /// The 1-based line the lines go after.
    pub after: usize,
    /// The lines.
    pub lines: Vec<String>,
    /// The block that grows, for the report.
    pub what: String,
}

/// What one chart is owed, and the edits that would settle it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    /// The chart's directory name.
    pub chart: String,
    /// Its `values.yaml`.
    pub values: PathBuf,
    /// The keys owed a value, sorted into where each goes.
    pub plan: Plan,
    /// The `values.yaml` blocks, in file order.
    pub insertions: Vec<Insertion>,
    /// The declaration and template lines, in the order they are reported.
    pub edits: Vec<FileEdit>,
    /// The keys nothing is written for, and why.
    pub refusals: Vec<Refusal>,
    /// Why nothing was planned at all, when the chart fails a rule the plan depends on.
    pub blocked: Vec<String>,
    /// Whether `blocked` is a hand edit the declaration needs before anything can be appended to
    /// it, rather than the bindings gate's own findings — which decides what the report says
    /// will find it again.
    pub by_hand: bool,
    /// The projections that could not be appended, printed for a person to place.
    pub owed_projected: Vec<Placement>,
    /// The credentials whose `secretData` line could not be appended, printed likewise.
    pub owed_credentials: Vec<Placement>,
}

impl Adoption {
    fn new(chart: String, values: PathBuf) -> Self {
        Self {
            chart,
            values,
            plan: Plan::default(),
            insertions: Vec::new(),
            edits: Vec::new(),
            refusals: Vec::new(),
            blocked: Vec::new(),
            by_hand: false,
            owed_projected: Vec::new(),
            owed_credentials: Vec::new(),
        }
    }

    /// Whether the chart owes anything at all: a value, a line, a refused key or a fixed rule.
    pub fn owed(&self) -> bool {
        !(self.insertions.is_empty()
            && self.edits.is_empty()
            && self.refusals.is_empty()
            && self.blocked.is_empty())
    }

    /// Whether `--write` changes a file for this chart.
    pub fn writes(&self) -> bool {
        !(self.insertions.is_empty() && self.edits.is_empty())
    }

    /// The credentials whose chart value was already there, so nothing was written for them.
    ///
    /// Still owed the write-off: a credential travels the secrets directory rather than the
    /// configuration document, and that is a property of the key. The reason written for one of
    /// these names the secrets file and not the chart value — naming the value would be the claim
    /// the refusal was issued to avoid making.
    pub fn occupied(&self) -> Vec<Placement> {
        let refused: BTreeSet<&str> = self
            .refusals
            .iter()
            .filter(|item| item.occupied)
            .map(|item| item.path.as_str())
            .collect();
        self.plan
            .credentials
            .iter()
            .filter(|item| refused.contains(item.path()))
            .cloned()
            .collect()
    }

    /// The plan restricted to the values actually being written.
    ///
    /// Everything the closing report says is about these. A refused key is one the chart already
    /// has a value for, so its projection line is a line the template most likely already carries
    /// and its invented default is a default nobody is about to write. The write-offs stay whole:
    /// nothing inserts them, and they are owed a reason whatever else could be written.
    pub fn placed(&self) -> Plan {
        let written: BTreeSet<&str> = self
            .insertions
            .iter()
            .flat_map(|insertion| insertion.paths.iter().map(String::as_str))
            .collect();
        let keep = |items: &[Placement]| -> Vec<Placement> {
            items
                .iter()
                .filter(|item| written.contains(item.path()))
                .cloned()
                .collect()
        };
        Plan {
            projected: keep(&self.plan.projected),
            credentials: keep(&self.plan.credentials),
            written_off: self.plan.written_off.clone(),
        }
    }
}

/// What one chart is owed; [`None`] when its declaration does not enrol it.
///
/// The gate's own first four rules run before anything is planned, and a chart that fails one of
/// them is blocked rather than written to: a marker naming a key that does not exist means the
/// mapping is already wrong somewhere, and the keys this would then take to be unbound are whatever
/// that mistake left over.
///
/// `wanted` restricts the plan to those keys, and a wanted key the chart is not owed is refused
/// rather than ignored, so a typo cannot select nothing and report success.
///
/// # Errors
/// [`Error::Invalid`] when a declaration, a values file, a vendored contract or a template cannot be
/// read as one — as distinct from a chart that is wrong, which is a refusal or a block.
pub fn adopt(chart_dir: &Path, wanted: &BTreeSet<String>) -> Result<Option<Adoption>, Error> {
    let values_path = chart_dir.join("values.yaml");
    let Some(declaration) = load_declaration(chart_dir)?.filter(|held| held.bindings) else {
        return Ok(None);
    };
    if !values_path.is_file() {
        return Ok(None);
    }

    let chart = name_of(chart_dir);
    let mut adoption = Adoption::new(chart.clone(), values_path.clone());
    if declaration.documents.is_empty() {
        adoption.blocked.push(
            "declares `bindings: true` and no configuration contract, so there is nothing to \
             adopt keys from"
                .to_owned(),
        );
        return Ok(Some(adoption));
    }

    let declaration_path = chart_dir.join(super::DECLARATION);
    let bound = Bound::of(chart_dir, declaration)?;
    let text = read_text(&values_path)?;
    let (markers, _) = read_markers(&text, &chart)?;
    let values = read_yaml(&text, &values_path)?;

    // The `$service` vocabulary a multi-document chart's `derivedConfig` compares against, read off
    // each document's own `images[].values` rather than guessed from its name — see
    // `service_key_for`. Empty, harmlessly, for the ordinary one-document chart.
    let all_documents: BTreeSet<String> = bound.documents.keys().cloned().collect();
    let service_key_of: BTreeMap<String, String> = bound
        .documents
        .iter()
        .filter_map(|(name, document)| {
            document
                .images
                .iter()
                .find_map(|image| service_key_for(&image.values))
                .map(|key| (name.clone(), key))
        })
        .collect();

    let mut report = Report::new();
    let resolved = premises(&bound, &markers, &values, &mut report);
    let blocked: Vec<String> = report
        .at_level(Level::Error)
        .map(|entry| entry.finding.message.clone())
        .collect();
    if !blocked.is_empty() {
        adoption.blocked = blocked;
        return Ok(Some(adoption));
    }

    let (mut owed, refusals) = owed_keys(&bound, &missing_keys(&bound, &resolved), &markers);
    adoption.refusals = refusals;

    refuse_unknown_forms(&mut owed, &mut adoption.refusals);

    if !wanted.is_empty() {
        let known: BTreeSet<&str> = owed
            .iter()
            .map(|key| key.entry.name.as_str())
            .chain(adoption.refusals.iter().map(|item| item.path.as_str()))
            .collect();
        let unknown: Vec<Refusal> = wanted
            .iter()
            .filter(|path| !known.contains(path.as_str()))
            .map(|path| Refusal {
                path: path.clone(),
                reason: format!("is not a key {chart} is owed a value for"),
                occupied: false,
            })
            .collect();
        adoption.refusals.extend(unknown);
        owed.retain(|key| wanted.contains(&key.entry.name));
        adoption.refusals.retain(|item| wanted.contains(&item.path));
    }

    adoption.plan = plan_keys(owed)?;
    let (insertions, conflicts) = insertions_for(
        &values,
        &regions(&text),
        &markers,
        &adoption.plan,
        split_lines(&text, false).len(),
    )?;
    adoption.insertions = insertions;
    adoption.refusals.extend(conflicts);

    // After the insertions, because the write-offs owed are the ones for values that will be
    // written: a credential whose value was refused is one this chart already delivers, and an
    // entry claiming otherwise would describe an edit nobody made.
    let placed = adoption.placed();
    let (edits, unwritable) = declaration_edits(
        &declaration_path,
        &bound.declaration,
        &placed,
        &adoption.occupied(),
    )?;
    if !unwritable.is_empty() {
        // Values written without the write-offs they need leave the chart failing the gate this
        // was run to satisfy, so the whole chart waits for the hand edit rather than half of it.
        adoption.insertions.clear();
        adoption.blocked = unwritable;
        adoption.by_hand = true;
        return Ok(Some(adoption));
    }
    adoption.edits = edits;
    let projections =
        projection_edits(chart_dir, &chart, &placed, &all_documents, &service_key_of)?;
    adoption.edits.extend(projections.edits);
    adoption.owed_projected = projections.owed_projected;
    adoption.owed_credentials = projections.owed_credentials;
    Ok(Some(adoption))
}

/// Refuse every owed key whose `text_form` this build does not implement, and keep the rest.
///
/// A form never seen before says nothing about how the loader reads the value, so there is no type
/// to write, no default to trust and no telling whether a file can deliver it. Refused per key, so
/// the rest of the contract is still adopted.
fn refuse_unknown_forms(owed: &mut Vec<Key>, refusals: &mut Vec<Refusal>) {
    owed.retain(|key| {
        if key.entry.entry().text_form().is_ok() {
            return true;
        }
        refusals.push(Refusal {
            path: key.entry.name.clone(),
            reason: format!(
                "has `text_form` {}, which this build does not implement, so nothing says how its \
                 value is read. Upgrade terrace-contract, or write the value by hand",
                super::shown(key.entry.fields.get("text_form"))
            ),
            occupied: false,
        });
        false
    });
}

/// The keys of one chart that are owed a value, each resolved to a single description.
///
/// A key declared by several documents is one value — that is what an unscoped marker means — so
/// the documents that declare it have to agree about what it is. The bindings gate refuses a marker
/// whose documents disagree about `text_form`; this refuses the same disagreement one field wider,
/// because the value it would write is typed, defaulted and delivered from all of
/// [`SHAPE_FIELDS`].
///
/// A key missing in one document and *bound* in another is refused as well, and it is the subtler
/// case: a scoped marker already maps it onto a value, so what is owed is the scope of that marker
/// rather than a second value under the same name.
fn owed_keys(
    bound: &Bound,
    missing: &BTreeMap<String, Vec<String>>,
    markers: &[Marker],
) -> (Vec<Key>, Vec<Refusal>) {
    let mut owed = Vec::new();
    let mut refusals = Vec::new();

    let everywhere: BTreeSet<&str> = missing.values().flatten().map(String::as_str).collect();
    let already: BTreeSet<&str> = markers
        .iter()
        .filter(|marker| marker.class.targets_a_key())
        .map(|marker| marker.target.as_str())
        .collect();

    for path in everywhere {
        let declares: Vec<&str> = missing
            .iter()
            .filter(|(_, paths)| paths.iter().any(|held| held == path))
            .map(|(name, _)| name.as_str())
            .collect();

        if already.contains(path) {
            let elsewhere: Vec<&str> = bound
                .documents
                .keys()
                .filter(|name| {
                    bound
                        .unions
                        .get(*name)
                        .is_some_and(|union| union.keys.contains(path))
                        && !missing
                            .get(*name)
                            .is_some_and(|paths| paths.iter().any(|held| held == path))
                })
                .map(String::as_str)
                .collect();
            refusals.push(Refusal {
                path: path.to_owned(),
                reason: format!(
                    "is bound in {} and unbound in {}. A scoped marker already maps it onto a \
                     value, so what is owed is that marker's scope — a second value under the same \
                     name would be two chart values for one setting, which rule 4 refuses",
                    elsewhere.join(", "),
                    declares.join(", ")
                ),
                occupied: false,
            });
            continue;
        }

        let entries: Vec<&Merged> = declares
            .iter()
            .filter_map(|name| bound.unions.get(*name)?.keys.get(path))
            .collect();
        let shapes: BTreeSet<Vec<Option<String>>> =
            entries.iter().map(|entry| shape_of(entry)).collect();
        if shapes.len() > 1 {
            refusals.push(Refusal {
                path: path.to_owned(),
                reason: format!(
                    "is declared by {}, which do not agree what it is. One path in two shapes is \
                     two settings that share a name, so the values that feed them are written by \
                     hand, one per document, with a scoped marker each",
                    declares.join(", ")
                ),
                occupied: false,
            });
            continue;
        }

        if let Some(entry) = entries.first() {
            owed.push(Key {
                entry: (*entry).clone(),
                documents: Some(declares.iter().map(|name| (*name).to_owned()).collect()),
            });
        }
    }

    (owed, refusals)
}

/// The fields of [`SHAPE_FIELDS`] as one comparable value. An absent field and a `null` one are
/// the same answer to "what is this key", so both read as [`None`].
fn shape_of(entry: &Merged) -> Vec<Option<String>> {
    SHAPE_FIELDS
        .iter()
        .map(|name| {
            entry
                .fields
                .get(*name)
                .filter(|held| !held.is_null())
                .map(Json::to_string)
        })
        .collect()
}

/// A path as the report spells it: separated by `/`, with no `.` segments.
///
/// The spelling every line of the report used before this was ported, which a reader's tooling may
/// match on, so `./charts` and `charts` are reported alike.
pub fn posix(path: &Path) -> String {
    let mut root = String::new();
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => root.push_str(&prefix.as_os_str().to_string_lossy()),
            Component::RootDir => root.push('/'),
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".to_owned()),
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
        }
    }
    let joined = format!("{root}{}", parts.join("/"));
    if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

/// A file's text.
fn read_text(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path.display(), e))
}

/// A chart directory's name.
fn name_of(chart_dir: &Path) -> String {
    chart_dir
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Every chart directory under `charts`, and the ones of them an adoption is owed for.
///
/// # Errors
/// As [`adopt`], for any chart.
pub fn adopt_all(
    chart_dirs: &[PathBuf],
    wanted: &BTreeSet<String>,
) -> Result<Vec<Adoption>, Error> {
    let mut owed = Vec::new();
    for chart_dir in chart_dirs {
        if let Some(adoption) = adopt(chart_dir, wanted)?.filter(Adoption::owed) {
            owed.push(adoption);
        }
    }
    Ok(owed)
}

#[cfg(test)]
mod tests;
