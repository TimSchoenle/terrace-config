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
//!   whichever came second; [`tree_at`] refuses the pair as a mapping to write by hand.
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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde_json::Value as Json;

use crate::error::Error;
use crate::report::{Level, Report};
use crate::text::quoted;
use crate::union::Merged;

use super::bindings::{has_path, missing_keys, premises};
use super::declaration::{Bound, Declaration, load_declaration, read_yaml};
use super::files;
use super::markers::{MARKER, Marker, read as read_markers};
use super::patch::{Row, unified};
use super::regions::{Region, regions, split_lines};
use super::scaffold::{
    Destination, Key, Placement, Plan, Tree, branch_prefix, branches, credential_entries_for,
    default_for, derived_for, invented, plan_keys, render_tree, secrets_file_name, service_key_for,
    tree_at, values_scalar, write_off_groups, write_off_lines,
};

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

// ------------------------------------------------------------------------------------------
// What one chart is owed
// ------------------------------------------------------------------------------------------

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

// ------------------------------------------------------------------------------------------
// Where the blocks go
// ------------------------------------------------------------------------------------------

/// The values path one level up, or the empty path for a top-level value.
fn parent_of(values_path: &str) -> &str {
    values_path
        .rsplit_once('.')
        .map_or("", |(parent, _)| parent)
}

/// The values paths whose subtree binds at least one contract key.
///
/// A block bears a binding if it or anything under it carries a marker, which is what makes
/// `telemetry` part of the configuration surface when the markers are three levels down.
fn bearing(regions: &[Region], markers: &[Marker]) -> BTreeSet<String> {
    regions
        .iter()
        .filter(|region| {
            let nested = format!("{}.", region.values_path);
            markers.iter().any(|marker| {
                marker.values_path == region.values_path || marker.values_path.starts_with(&nested)
            })
        })
        .map(|region| region.values_path.clone())
        .collect()
}

/// The line a new child of `ancestor` goes after.
///
/// After the last child that binds something, so the new value lands at the end of the
/// configuration surface rather than at the end of the file; after the ancestor's own last line
/// when none of its children binds anything; and at the end of the file for a top-level block in a
/// chart none of whose top-level blocks binds anything.
fn insertion_point(
    regions: &[Region],
    bound_paths: &BTreeSet<String>,
    ancestor: &str,
    lines: usize,
) -> usize {
    let binding = regions
        .iter()
        .filter(|region| {
            parent_of(&region.values_path) == ancestor && bound_paths.contains(&region.values_path)
        })
        .map(|region| region.end)
        .max();
    if let Some(end) = binding {
        return end;
    }
    if ancestor.is_empty() {
        return lines;
    }
    regions
        .iter()
        .find(|region| region.values_path == ancestor)
        .map_or(lines, |region| region.end)
}

/// The blocks to insert, and the keys no block can be written for.
///
/// Placements are grouped by the existing block they land in, so two keys arriving under one new
/// `telemetry.sentry` produce one `sentry:` heading rather than two.
fn insertions_for(
    values: &Json,
    regions: &[Region],
    markers: &[Marker],
    plan: &Plan,
    lines: usize,
) -> Result<(Vec<Insertion>, Vec<Refusal>), Error> {
    let placed: BTreeSet<&str> = regions
        .iter()
        .map(|region| region.values_path.as_str())
        .collect();
    // A key written twice is read as its last occurrence, which is what YAML makes of it.
    let by_path: HashMap<&str, &Region> = regions
        .iter()
        .map(|region| (region.values_path.as_str(), region))
        .collect();
    let bound_paths = bearing(regions, markers);

    let mut refusals = Vec::new();
    let mut groups: Vec<(String, Vec<Placement>)> = Vec::new();

    for placement in plan.surfaced() {
        let values_path = placement.values_path.as_str();
        if placed.contains(values_path) || has_path(values, values_path) {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: occupied_refusal(placement),
                occupied: true,
            });
            continue;
        }

        let segments: Vec<&str> = values_path.split('.').collect();
        let ancestor = (1..segments.len())
            .rev()
            .map(|cut| segments[..cut].join("."))
            .find(|candidate| placed.contains(candidate.as_str()))
            .unwrap_or_default();

        // A mapping key written with nothing under it — `feed:` whose only child was removed — is
        // a parent to insert into rather than a leaf to refuse: `null` there is the absence of
        // children, and every other scalar is a value that cannot also be a mapping.
        let occupant = super::dig(values, &ancestor).filter(|held| !held.is_null());
        if !ancestor.is_empty() && occupant.is_some_and(|held| !held.is_object()) {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: format!(
                    "needs the chart value {}, and {} is a leaf value on the way there. The two \
                     cannot both exist, so this mapping has to be written by hand",
                    quoted(values_path),
                    quoted(&ancestor)
                ),
                occupied: false,
            });
            continue;
        }

        // `auth: {}` and `telemetry: null` are mappings to YAML, but written on one line: a block
        // inserted under either is a second value for the same key, and the file no longer parses.
        // Turning one into a bare `auth:` is a rewrite of a line somebody wrote, which an
        // insertion-only writer does not do.
        if let Some(region) = by_path.get(ancestor.as_str())
            && !ancestor.is_empty()
            && !region.bare
        {
            refusals.push(Refusal {
                path: placement.path().to_owned(),
                reason: format!(
                    "needs the chart value {}, and {} is written with its value on the key's own \
                     line, so nothing can be inserted under it. Rewrite it as a bare `{}:` with \
                     its children indented below, then run this again",
                    quoted(values_path),
                    quoted(&ancestor),
                    ancestor.rsplit('.').next().unwrap_or_default()
                ),
                occupied: false,
            });
            continue;
        }

        match groups.iter_mut().find(|(held, _)| *held == ancestor) {
            Some((_, members)) => members.push(placement.clone()),
            None => groups.push((ancestor, vec![placement.clone()])),
        }
    }

    let mut insertions = Vec::new();
    for (ancestor, placements) in groups {
        let (depth, indent, ancestor_line) = match by_path.get(ancestor.as_str()) {
            // Read off the block being inserted into rather than computed from the depth, so a
            // file indented by something other than two spaces is extended in its own style.
            Some(region) if !ancestor.is_empty() => (
                ancestor.split('.').count(),
                " ".repeat(region.indent + 2),
                Some(region.line),
            ),
            _ => (0, String::new(), None),
        };
        let tree = tree_at(&placements, depth)?;
        let after = insertion_point(regions, &bound_paths, &ancestor, lines);
        let mut paths: Vec<String> = placements
            .iter()
            .map(|item| item.path().to_owned())
            .collect();
        paths.sort();
        insertions.push(Insertion {
            after,
            lines: render_tree(&tree, &indent, depth, &branch_description)?,
            paths,
            branches: branches(&tree, &ancestor),
            spaced: ancestor_line.is_none_or(|line| line != after),
            depth,
        });
    }

    // Listed by the line each goes after and otherwise in the order planned, which is the order
    // the report has always printed them in. Where two of them meet at one line is the writer's
    // concern, not the listing's: see [`splices`].
    insertions.sort_by_key(|item| item.after);
    Ok((insertions, refusals))
}

/// Why a key whose chart value already exists is refused, and what to write instead.
///
/// The one refusal that is a rule rather than an obstacle — see this module's documentation. A
/// credential gets a different sentence because it never carries a marker: what an existing value
/// is owed there is the write-off and the Secret line, not a binding.
fn occupied_refusal(placement: &Placement) -> String {
    let values_path = quoted(&placement.values_path);
    if placement.destination == Destination::SecretFile {
        return format!(
            "needs the chart value {values_path}, which this chart already has. A credential \
             carries no marker, so what is owed is an `unbound` entry naming this key and a \
             `secretData` line delivering that value"
        );
    }
    let optional = if placement.optional() {
        " optional"
    } else {
        ""
    };
    format!(
        "needs the chart value {values_path}, which this chart already has. Whether that value \
         feeds this key is a claim only a person can make, so write `# # {MARKER} {} {}{optional}` \
         into its `@schema` block by hand",
        placement.marker_class().label(),
        placement.path()
    )
}

/// The `# --` line a grouping block created here is given, and why it is not a `TODO`.
///
/// A contract says nothing about the blocks a chart groups its keys into, so the sentence somebody
/// would write cannot be derived. A scaffold's answer is a `TODO` an author reads while writing the
/// chart; here that answer was measured, twice, on one pull request: the placeholder was deleted
/// rather than filled in, the values-documentation gate went red on a block with no description at
/// all, and in between a `TODO` was published into the README table and into the values schema.
///
/// So this writes the plainest true sentence instead — the same fallback a key whose producer wrote
/// no prose gets. Nothing is left to delete, the README row is dull rather than wrong, and the
/// closing output still names every block worth a better sentence.
fn branch_description(node: &Tree, depth: usize) -> Result<String, Error> {
    Ok(format!("The `{}` settings.", branch_prefix(node, depth)?))
}

// ------------------------------------------------------------------------------------------
// The declaration: the write-offs and the credential columns
// ------------------------------------------------------------------------------------------

/// The block a declaration gets when it has no `unbound`, in a scaffolded declaration's wording.
const UNBOUND_HEADER: [&str; 5] = [
    "# Contract keys no chart value binds, each group with the reason it does not.",
    "#",
    "# Chart-level: a key nothing surfaces is unsurfaced in every document that declares",
    "# it. `documents:` would narrow an entry where that is untrue.",
    "unbound:",
];

/// The block a declaration gets when it has no `credentials`.
const CREDENTIALS_HEADER: [&str; 7] = [
    "# What this chart adds to the credential reference in its README, which",
    "# `just config-readme` generates from the contract. The rows are the contract's — every",
    "# key it marks `secret: true`, and whether the image requires it — and these columns are",
    "# the chart's: the value an operator may set instead of pre-creating the Secret, and the",
    "# condition under which a release needs the credential at all. The second is nobody's to",
    "# derive and is written by hand, as a `note:` beside the `value:`.",
    "credentials:",
];

/// The `unbound` groups and `credentials` rows this adoption owes the declaration.
///
/// **Every reason written here is read off the contract rather than decided.** That is the whole
/// licence for writing them, and it is narrower than it looks: a write-off says *why no chart value
/// surfaces this key*, and this produces exactly two answers — `reserved`, which is the image saying
/// the loader sets the key itself, and `secret`, where the standing rule sends a credential down the
/// secrets directory and never through a `ConfigMap`. Both are facts in the document. The third kind
/// of write-off — a chart choosing not to expose an ordinary setting — is a judgement, and it is
/// never produced here, because an ordinary key gets a value instead.
///
/// The `credentials` row is the same argument once more. `value` is the chart value carrying the
/// credential, which is a fact here because this just wrote that value. `note` — when a release
/// needs the credential at all — is the chart's, and is left for a person.
///
/// The second half of the answer is the blocks that exist but cannot take a line: `unbound: []`
/// and `credentials: []` are lists, but written on the key's own line, and an entry appended
/// below either is a second value for one key that no YAML parser accepts.
fn declaration_edits(
    path: &Path,
    declaration: &Declaration,
    plan: &Plan,
    occupied: &[Placement],
) -> Result<(Vec<FileEdit>, Vec<String>), Error> {
    let text = read_text(path)?;
    // Keyed by path, a key written twice read as its last occurrence.
    let regions: HashMap<String, Region> = regions(&text)
        .into_iter()
        .map(|region| (region.values_path.clone(), region))
        .collect();

    let mut edits = Vec::new();
    let mut unwritable = Vec::new();
    let mut fresh: Vec<(&str, Vec<String>)> = Vec::new();
    let mut inline = |name: &str| {
        unwritable.push(format!(
            "{}: `{name}` is written with its value on the key's own line, so the entries this \
             adoption owes cannot be appended to it. Rewrite it as a bare `{name}:` with its \
             entries indented below, then run this again",
            posix(path)
        ));
    };

    let rows = credential_rows(declaration, plan);
    if !rows.is_empty() {
        match regions.get("credentials") {
            Some(region) if !region.bare => inline("credentials"),
            Some(region) => edits.push(FileEdit {
                path: path.to_path_buf(),
                after: region.end,
                lines: rows,
                what: "credentials".to_owned(),
            }),
            None => fresh.push(("credentials", with_header(&CREDENTIALS_HEADER, rows))),
        }
    }

    let groups = owed_write_offs(plan, occupied, declaration)?;
    if !groups.is_empty() {
        match regions.get("unbound") {
            Some(region) if !region.bare => inline("unbound"),
            Some(region) => edits.push(FileEdit {
                path: path.to_path_buf(),
                after: region.end,
                lines: groups,
                what: "unbound".to_owned(),
            }),
            None => fresh.push(("unbound", with_header(&UNBOUND_HEADER, groups))),
        }
    }

    if !fresh.is_empty() {
        let mut body = Vec::new();
        for (_, lines) in &fresh {
            body.push(String::new());
            body.extend(lines.iter().cloned());
        }
        edits.push(FileEdit {
            path: path.to_path_buf(),
            after: before_documents(&regions, path)?,
            lines: body,
            what: fresh
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(" and "),
        });
    }
    Ok((edits, unwritable))
}

fn with_header(header: &[&str], body: Vec<String>) -> Vec<String> {
    header
        .iter()
        .map(|line| (*line).to_owned())
        .chain(body)
        .collect()
}

/// The line a block that must sit above `documents:` goes after.
///
/// The end of the last top-level key before it, rather than the line above `documents:` itself:
/// every declaration opens each of its blocks with a paragraph of prose, and an insertion between
/// that paragraph and the key it explains would attach it to the wrong one.
fn before_documents(regions: &HashMap<String, Region>, path: &Path) -> Result<usize, Error> {
    let documents = regions.get("documents").ok_or_else(|| {
        Error::Invalid(format!(
            "{}: has no `documents:` key this writer can find, so there is no line to place a \
             new `unbound` or `credentials` block above",
            posix(path)
        ))
    })?;
    Ok(regions
        .values()
        .filter(|region| region.line < documents.line)
        .map(|region| region.end)
        .max()
        .unwrap_or(0))
}

/// A `credentials` row per credential this adoption surfaced and the chart has not described.
fn credential_rows(declaration: &Declaration, plan: &Plan) -> Vec<String> {
    plan.credentials
        .iter()
        .filter(|item| !declaration.credentials.contains_key(item.path()))
        .flat_map(|item| {
            [
                format!("  - key: {}", item.path()),
                format!("    value: {}", item.values_path),
            ]
        })
        .collect()
}

/// Why a credential the chart already carries a value for is bound by no marker.
fn occupied_reason(placement: &Placement) -> Result<String, Error> {
    Ok(format!(
        "Delivered as a file in the secrets directory — from the Secret this chart renders, or \
         from `existingSecret` under the file name `{}` — and never written into the \
         configuration document, because a credential in a ConfigMap is readable by anything that \
         can read the namespace. `just check-config-secrets` is what reconciles that channel, and \
         it does it from the rendered manifests rather than from a comment.",
        secrets_file_name(placement)?
    ))
}

/// The `unbound` groups this adoption owes, in a scaffolded declaration's layout.
///
/// A group every key of which the declaration already writes off is not written again — a second
/// entry for a key that has one is the same drift as a missing entry, one reason saying what
/// another already said, and the gate would not catch it because both are true. `occupied` are the
/// credentials whose value the chart already had; they get the same sentence with the clause naming
/// the chart value removed, since that is the claim this refuses to make about a value it did not
/// write.
fn owed_write_offs(
    plan: &Plan,
    occupied: &[Placement],
    declaration: &Declaration,
) -> Result<Vec<String>, Error> {
    let already: BTreeSet<&str> = declaration
        .unbound
        .iter()
        .flat_map(|entry| entry.keys.iter().map(String::as_str))
        .collect();

    let mut groups: Vec<(Vec<String>, String)> = write_off_groups(plan)?
        .into_iter()
        .filter(|(paths, _)| !paths.iter().all(|path| already.contains(path.as_str())))
        .collect();
    for item in occupied {
        if !already.contains(item.path()) {
            groups.push((vec![item.path().to_owned()], occupied_reason(item)?));
        }
    }

    Ok(groups
        .iter()
        .flat_map(|(paths, reason)| write_off_lines(paths, reason))
        .collect())
}

// ------------------------------------------------------------------------------------------
// The template: projecting the values into the document
// ------------------------------------------------------------------------------------------

/// The Go-template actions that open a nesting level. `end` closes one; `else` is neither.
const OPENERS: [&str; 5] = ["if", "with", "range", "define", "block"];

/// One `define` in a chart's templates, as the lines a caller may append to.
///
/// `end` is the line of the define's own closing action, so a new top-level entry is inserted after
/// `end - 1` and lands inside the define. `roots` are the mapping keys already written at column
/// zero — the ones a second block of the same name would shadow — whether or not an
/// `if`/`with`/`range` wraps them, since a chart's helpers never let one of those add YAML
/// indentation of its own: every nested value reaches its indent through an explicit `nindent`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Define {
    end: usize,
    roots: BTreeSet<String>,
}

/// Find one define and the top-level keys it already writes; [`None`] when it cannot be read.
///
/// A helper is hand-written Go template, and this reads exactly as much of it as an append needs:
/// where the define ends, and which keys already occupy its rendered top level. Depth is tracked
/// only to find the closing `end` — a projection is always appended at the define's own depth,
/// outside every `if`/`with`/`range`, so it is never silently written under somebody else's
/// condition.
///
/// An occupied root, however, is detected by column alone, at any depth: a key written from two or
/// three actions deep — a `scheduler:` guarded by `{{- if eq $service ... }}` around a
/// `{{- with include ... }}` — still lands at column zero in the rendered document. Gating this on
/// depth 1 once missed exactly that, and a new key under the root was appended as a second,
/// colliding block that `fromYaml` resolved by dropping the first in silence.
///
/// [`None`] when the define is absent, or when its actions do not balance: a template shape this
/// cannot read is one it must not edit, and the caller prints the lines instead.
fn read_define(text: &str, name: &str) -> Option<Define> {
    let lines = split_lines(text, false);
    let opening = format!("define \"{name}\"");
    let start = lines.iter().position(|line| line.contains(&opening))?;

    let mut depth: i64 = 0;
    let mut roots = BTreeSet::new();
    for (number, line) in lines.iter().enumerate().skip(start) {
        if depth >= 1
            && let Some(key) = template_patterns().top_level_key.captures(line)
        {
            roots.insert(key["name"].to_owned());
        }
        let actions: Vec<&str> = template_patterns()
            .action
            .captures_iter(line)
            .filter_map(|found| found.get(1))
            .map(|found| found.as_str())
            .collect();
        for action in &actions {
            if OPENERS.contains(action) {
                depth += 1;
            } else if *action == "end" {
                depth -= 1;
                if depth == 0 {
                    // Lines are inserted *above* the closing `end`, so it has to own its line.
                    // Sharing it with the define's opening would put them above the define, and
                    // sharing it with an inner `{{- end }}` would put them inside that inner
                    // block, under a condition nobody wrote for them. Either way the shape is not
                    // one an append can be trusted with, and the lines are printed instead.
                    let alone = number != start && actions.len() == 1;
                    return alone.then_some(Define {
                        end: number + 1,
                        roots,
                    });
                }
            }
        }
    }
    None
}

/// The patterns the template reader uses, compiled once.
struct TemplatePatterns {
    /// An action's opening word.
    action: Regex,
    /// A mapping key at column zero.
    top_level_key: Regex,
    /// The line that unwraps a `(dict "ctx" ... "service" ...)` argument.
    context_assignment: Regex,
}

fn template_patterns() -> &'static TemplatePatterns {
    static HELD: std::sync::OnceLock<TemplatePatterns> = std::sync::OnceLock::new();
    HELD.get_or_init(|| TemplatePatterns {
        action: Regex::new(r"\{\{-?\s*(\w+)").expect("a literal pattern compiles"),
        top_level_key: Regex::new(r"^(?P<name>[A-Za-z0-9_][A-Za-z0-9_.\-]*):")
            .expect("a literal pattern compiles"),
        context_assignment: Regex::new(r"\$ctx\s*:=\s*\.ctx\b")
            .expect("a literal pattern compiles"),
    })
}

/// Whether a `derivedConfig` define is included with `.` set to `(dict "ctx" ... "service" ...)`
/// rather than to the root context.
///
/// A multi-document chart's `derivedConfig` needs to know which service is rendering, and the shape
/// this knows wraps the root context and the service together, unwrapping it again with the define's
/// own `{{- $ctx := .ctx -}}`. Every reference written has to agree with that: `.Values` inside such
/// a define is a nil pointer. Read from the text rather than assumed from the document count — a
/// multi-document chart shaped some other way would make `$ctx.Values` exactly as wrong a guess as
/// `.Values` is, and there is no third convention to fall back on, so such a chart is treated the
/// same as one with no define at all.
fn wraps_context(text: &str) -> bool {
    template_patterns().context_assignment.is_match(text)
}

/// The template file that opens define `name`, its text, and what [`read_define`] found there.
///
/// Every `templates/*.tpl` is a candidate, not only `_helpers.tpl`: a chart whose services split
/// their configuration across several partials keeps `derivedConfig` in `_config.tpl`, and reading
/// one file only made every projection of such a chart print as still owed, silently. A fixed order
/// makes two files that both open a define resolve the same way every run.
fn find_define(chart_dir: &Path, name: &str) -> Result<Option<(PathBuf, String, Define)>, Error> {
    let templates = chart_dir.join("templates");
    let Ok(entries) = std::fs::read_dir(&templates) else {
        return Ok(None);
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Error::io(templates.display(), e))?.path();
        let named = path.extension().is_some_and(|extension| extension == "tpl");
        if named && path.is_file() {
            candidates.push(path);
        }
    }
    candidates.sort();

    for path in candidates {
        let text = read_text(&path)?;
        if let Some(define) = read_define(&text, name) {
            return Ok(Some((path, text, define)));
        }
    }
    Ok(None)
}

/// Brand-new roots split into what can be written and what must be printed, with each written
/// root's `$service` guard.
///
/// A single-document chart's roots always come back writable and unguarded. A multi-document
/// chart's `derivedConfig` takes a `$service` argument: a root every document declares renders the
/// same way for all of them, and a root only some declare needs the matching guard.
///
/// That guard is written only when it cannot be wrong. Every key under the root has to agree which
/// documents declare it — a root with keys split across scopes is two settings sharing a table
/// name — and every one of those documents has to resolve to a real `$service` value. Either failure
/// sends the whole root to the printed-for-a-person pile, the same one an occupied root lands in.
fn scope_new_roots(
    candidates: Vec<Placement>,
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> (Vec<Placement>, BTreeMap<String, Vec<String>>) {
    let mut by_root: Vec<(String, Vec<Placement>)> = Vec::new();
    for item in candidates {
        let root = root_of(&item).to_owned();
        match by_root.iter_mut().find(|(held, _)| *held == root) {
            Some((_, members)) => members.push(item),
            None => by_root.push((root, vec![item])),
        }
    }

    let mut writable = Vec::new();
    let mut scoped = BTreeMap::new();
    for (root, items) in by_root {
        let declaring: BTreeSet<Option<&Vec<String>>> =
            items.iter().map(|item| item.documents.as_ref()).collect();
        let [documents] = declaring.into_iter().collect::<Vec<_>>()[..] else {
            continue;
        };
        let everywhere = documents
            .is_none_or(|named| named.iter().cloned().collect::<BTreeSet<_>>() == *all_documents);
        if everywhere {
            writable.extend(items);
            continue;
        }

        let named = documents.map_or(&[][..], Vec::as_slice);
        let service_keys: Vec<String> = named
            .iter()
            .filter_map(|name| service_key_of.get(name).cloned())
            .collect();
        if service_keys.len() != named.len() {
            continue;
        }
        writable.extend(items);
        scoped.insert(root, service_keys);
    }
    (writable, scoped)
}

/// A placement's top-level contract segment, which is the root it is projected under.
fn root_of(placement: &Placement) -> &str {
    placement.path().split('.').next().unwrap_or_default()
}

/// The template lines an adoption appends, and the ones it prints instead.
struct Projections {
    edits: Vec<FileEdit>,
    owed_projected: Vec<Placement>,
    owed_credentials: Vec<Placement>,
}

/// The `derivedConfig` and `secretData` lines to append, and the ones that must be printed.
///
/// **One rule decides whether a line can be written: does the define already have this key's
/// root?** A projection is appended at the define's own depth, so it cannot land inside somebody's
/// `if`; what it *can* do is write a second `telemetry:` beside an existing one, and the document is
/// built by `fromYaml`, which keeps the last of two mapping keys and drops everything under the
/// first. The values file parses, the chart renders, and a setting has vanished. So a key whose
/// root is already in the define is printed for a person to place, and only a new root is written.
///
/// On a multi-document chart, [`scope_new_roots`] draws a second such line, and anything it leaves
/// uncertain joins the same printed pile — never a guess.
fn projection_edits(
    chart_dir: &Path,
    chart: &str,
    plan: &Plan,
    all_documents: &BTreeSet<String>,
    service_key_of: &BTreeMap<String, String>,
) -> Result<Projections, Error> {
    let mut edits = Vec::new();

    let mut projected: Vec<Placement> = Vec::new();
    if let Some((path, text, derived)) = find_define(chart_dir, &format!("{chart}.derivedConfig"))?
    {
        let multi_document = all_documents.len() > 1;
        let mut values_prefix = ".Values";
        let mut scoped = BTreeMap::new();
        // A multi-document chart's define is unusable here unless it is provably shaped the one
        // way this knows how to address; refusing is the same posture as finding no define.
        if !multi_document || wraps_context(&text) {
            projected = plan
                .projected
                .iter()
                .filter(|item| !derived.roots.contains(root_of(item)))
                .cloned()
                .collect();
            if multi_document {
                values_prefix = "$ctx.Values";
                (projected, scoped) = scope_new_roots(projected, all_documents, service_key_of);
            }
        }
        if !projected.is_empty() {
            edits.push(FileEdit {
                path,
                after: derived.end - 1,
                lines: derived_for(&projected, "", Some(&scoped), values_prefix),
                what: "derivedConfig".to_owned(),
            });
        }
    }
    let written: BTreeSet<&str> = projected.iter().map(Placement::path).collect();
    let owed_projected = plan
        .projected
        .iter()
        .filter(|item| !written.contains(item.path()))
        .cloned()
        .collect();

    let mut owed_credentials = plan.credentials.clone();
    if let Some((path, _, secrets)) = find_define(chart_dir, &format!("{chart}.secretData"))? {
        let mut delivered = Vec::new();
        owed_credentials.clear();
        for item in &plan.credentials {
            if secrets.roots.contains(&secrets_file_name(item)?) {
                owed_credentials.push(item.clone());
            } else {
                delivered.push(item.clone());
            }
        }
        if !delivered.is_empty() {
            edits.push(FileEdit {
                path,
                after: secrets.end - 1,
                lines: credential_entries_for(&delivered)?,
                what: "secretData".to_owned(),
            });
        }
    }

    Ok(Projections {
        edits,
        owed_projected,
        owed_credentials,
    })
}

// ------------------------------------------------------------------------------------------
// Applying the edits
// ------------------------------------------------------------------------------------------

/// One file and the runs of lines inserted into it, before anything is written.
///
/// The one representation both `--write` and `--diff` read, so the diff a reviewer is shown is the
/// edit that would be made rather than a second rendering of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The file.
    pub path: PathBuf,
    /// Its text as it is now.
    pub original: String,
    /// Each run of lines and the 1-based line it goes after, in the order they land in the file.
    pub insertions: Vec<(usize, Vec<String>)>,
}

impl Splice {
    /// The lines of the original, each with its terminator, the last one terminated if it was not.
    ///
    /// Returns whether the last line had to be terminated: a last line with no terminator would
    /// otherwise have the first inserted line appended to it.
    fn lines(&self) -> (Vec<String>, &'static str, bool) {
        // One CRLF anywhere makes the file a CRLF file: a rewrite that normalised it would reflow
        // every line of every file it touched.
        let ending = if self.original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut lines: Vec<String> = split_lines(&self.original, true)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut terminated = false;
        if let Some(last) = lines.last_mut()
            && !last.ends_with(['\n', '\r'])
        {
            last.push_str(ending);
            terminated = true;
        }
        (lines, ending, terminated)
    }

    /// The runs inserted before each original line, by index, with the runs past the end at
    /// `lines.len()`.
    fn runs(&self, count: usize, ending: &str) -> BTreeMap<usize, Vec<String>> {
        let mut runs: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (after, body) in &self.insertions {
            runs.entry((*after).min(count))
                .or_default()
                .extend(body.iter().map(|line| format!("{line}{ending}")));
        }
        runs
    }

    /// The text after every insertion.
    pub fn apply(&self) -> String {
        let (lines, ending, _) = self.lines();
        let mut runs = self.runs(lines.len(), ending);
        let mut out = String::new();
        for (index, line) in lines.iter().enumerate() {
            for inserted in runs.remove(&index).unwrap_or_default() {
                out.push_str(&inserted);
            }
            out.push_str(line);
        }
        for inserted in runs.into_values().flatten() {
            out.push_str(&inserted);
        }
        out
    }

    /// The change as a unified diff with three lines of context, against the file's own path.
    ///
    /// Built from the insertions rather than by comparing two texts: every edit here is an
    /// insertion, so the alignment is known exactly, and a general diff could only rediscover it —
    /// or, where a new blank line matches an old one, choose a different but equally valid one.
    pub fn diff(&self) -> String {
        let (lines, ending, terminated) = self.lines();
        let mut runs = self.runs(lines.len(), ending);
        let mut rows: Vec<Row> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            for inserted in runs.remove(&index).unwrap_or_default() {
                rows.push(Row::Added(inserted));
            }
            if terminated && index + 1 == lines.len() {
                let original = line.strip_suffix(ending).unwrap_or(line).to_owned();
                rows.push(Row::Removed(original));
                rows.push(Row::Added(line.clone()));
            } else {
                rows.push(Row::Context(line.clone()));
            }
        }
        rows.extend(runs.into_values().flatten().map(Row::Added));

        let shown = posix(&self.path);
        unified(&shown, &shown, &rows)
    }
}

/// Every file one adoption changes, in the order they are written.
///
/// `values.yaml` first, then the other files in path order, each with its runs in the order they
/// land in the file. For the declaration and the templates that is the order they were planned,
/// so two runs meeting at one line keep the order the plan gave them; for the values it is that
/// order corrected for depth, as the comment in the body explains.
///
/// # Errors
/// [`Error::Io`] when a file cannot be read.
pub fn splices(adoption: &Adoption) -> Result<Vec<Splice>, Error> {
    let mut found = Vec::new();
    if !adoption.insertions.is_empty() {
        // Two blocks can meet at one line: a new leaf at the end of a block, and a new sibling of
        // that block after it. The deeper one belongs *inside* the block that ends there, so it is
        // written first; the other way round, the leaf would land under the new sibling and become
        // its child — or, when the sibling is a scalar, turn the file into YAML that does not
        // parse. Blocks of one depth never meet at one line (one ancestor is one block), so the
        // last key only keeps the order total, laying such ties out the way the script this
        // replaces did: the later planned above the earlier.
        let mut ordered: Vec<(usize, &Insertion)> =
            adoption.insertions.iter().enumerate().collect();
        ordered.sort_by_key(|(index, insertion)| {
            (
                insertion.after,
                std::cmp::Reverse(insertion.depth),
                std::cmp::Reverse(*index),
            )
        });
        found.push(Splice {
            path: adoption.values.clone(),
            original: read_text(&adoption.values)?,
            insertions: ordered
                .into_iter()
                .map(|(_, insertion)| {
                    let mut body = Vec::new();
                    if insertion.spaced {
                        body.push(String::new());
                    }
                    body.extend(insertion.lines.iter().cloned());
                    (insertion.after, body)
                })
                .collect(),
        });
    }

    let paths: BTreeSet<&PathBuf> = adoption.edits.iter().map(|edit| &edit.path).collect();
    for path in paths {
        let mut insertions: Vec<(usize, Vec<String>)> = adoption
            .edits
            .iter()
            .filter(|edit| &edit.path == path)
            .map(|edit| (edit.after, edit.lines.clone()))
            .collect();
        insertions.sort_by_key(|(after, _)| *after);
        found.push(Splice {
            path: path.clone(),
            original: read_text(path)?,
            insertions,
        });
    }
    Ok(found)
}

/// Apply one chart's edits: the values, the declaration and the template lines they oblige.
///
/// All three, because that set is what a green chart needs: values with no write-off fail the
/// bindings gate, and values with no projection fail the round trip the same run regenerates. A
/// command that wrote one of the three would be a command whose output is never the finished state.
///
/// # Errors
/// As [`write_all`].
pub fn write(adoption: &Adoption) -> Result<(), Error> {
    write_all(std::slice::from_ref(adoption))
}

/// Apply every chart's edits, all of them or none.
///
/// Every splice of every chart is planned first, reading each file once, and the whole set is
/// handed to [`files::write_all`], which refuses a file that resolves outside its chart and
/// replaces the rest, putting back what it replaced if a later replacement fails.
///
/// # Errors
/// [`Error::Io`] when a file cannot be read, resolved or written, and [`Error::Invalid`] when one
/// resolves outside its chart. No file has changed, unless putting one back failed too, as
/// [`files::write_all`] describes.
pub fn write_all(adoptions: &[Adoption]) -> Result<(), Error> {
    let mut planned = Vec::new();
    for adoption in adoptions.iter().filter(|item| item.writes()) {
        let chart = adoption.values.parent().unwrap_or_else(|| Path::new("."));
        for splice in splices(adoption)? {
            planned.push(files::Planned {
                chart: chart.to_path_buf(),
                text: splice.apply(),
                path: splice.path,
            });
        }
    }
    files::write_all(&planned)
}

// ------------------------------------------------------------------------------------------
// Saying what happened
// ------------------------------------------------------------------------------------------

/// One chart's result: what was inserted, what was refused, and what is still owed.
///
/// # Errors
/// As [`secrets_file_name`], for a credential still owed its `secretData` line.
pub fn report(adoption: &Adoption, written: bool) -> Result<String, Error> {
    let rule = "=".repeat(92);
    let mut out = format!("\n{rule}\n{}\n{rule}\n", adoption.chart);

    if !adoption.blocked.is_empty() {
        for message in &adoption.blocked {
            let _ = writeln!(out, "  blocked: {message}");
        }
        out.push_str(if adoption.by_hand {
            "\n  Nothing was planned. Once the declaration is fixed by hand, this plans the rest.\n"
        } else {
            "\n  Nothing was planned. `just check-config-bindings` reports the same thing.\n"
        });
        return Ok(out);
    }

    let values = posix(&adoption.values);
    for insertion in &adoption.insertions {
        let state = if written {
            "written after"
        } else {
            "goes after"
        };
        let _ = writeln!(out, "\n  {state} {values}:{}\n", insertion.after);
        indented(&mut out, &insertion.lines);
    }

    for refusal in &adoption.refusals {
        let _ = writeln!(out, "\n  refused: {} {}", refusal.path, refusal.reason);
    }

    still_owed(&mut out, adoption, written)?;
    Ok(out)
}

/// Lines under a report heading, four spaces in, a blank line staying blank.
fn indented(out: &mut String, lines: &[String]) {
    for line in lines {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "    {line}");
        }
    }
}

/// What is left for a person, and the edits that are not.
///
/// Printed on a dry run as well, because the values block on its own is the half that reads as
/// done: a value that is typed, documented and bound but that no template projects is a setting an
/// operator can set and nothing honours, which is the state this output exists to make loud.
fn still_owed(out: &mut String, adoption: &Adoption, written: bool) -> Result<(), Error> {
    let plan = adoption.placed();
    let chart = &adoption.chart;

    if !adoption.owed_projected.is_empty() {
        let _ = writeln!(
            out,
            "\n  still owed — {chart}/templates/_helpers.tpl, in `derivedConfig`:\n"
        );
        for line in derived_for(&adoption.owed_projected, "  ", None, ".Values") {
            let _ = writeln!(out, "  {line}");
        }
        out.push_str(
            "\n    The define already writes this key's root, and a second one would shadow\n    \
             the first — `fromYaml` keeps the last. Place these in the block that exists.\n",
        );
    }

    if !adoption.owed_credentials.is_empty() {
        let _ = writeln!(
            out,
            "\n  still owed — {chart}/templates/_helpers.tpl, in `secretData`:\n"
        );
        for line in credential_entries_for(&adoption.owed_credentials)? {
            let _ = writeln!(out, "    {line}");
        }
    }

    let chosen = invented(&plan);
    if !chosen.is_empty() {
        out.push_str("\n  still owed — a chosen default, in place of the one invented for:\n\n");
        for item in chosen {
            let _ = writeln!(
                out,
                "    {}: {}",
                item.values_path,
                values_scalar(&default_for(&item.key))
            );
        }
        out.push_str(
            "\n    Each is a key its image requires and publishes no default for, so the value\n    \
             above is a legal one drawn from the constraint rather than a chosen one.\n",
        );
    }

    let described: Vec<&String> = adoption
        .insertions
        .iter()
        .flat_map(|insertion| &insertion.branches)
        .collect();
    if !described.is_empty() {
        out.push_str(
            "\n  worth a sentence — these blocks were described from their key prefix:\n\n",
        );
        for path in described {
            let _ = writeln!(out, "    {path}");
        }
    }

    for edit in &adoption.edits {
        let state = if written { "written into" } else { "goes into" };
        let _ = writeln!(
            out,
            "\n  {state} {}:{}, under `{}`:\n",
            posix(&edit.path),
            edit.after,
            edit.what
        );
        indented(out, &edit.lines);
    }
    Ok(())
}

/// What a run added up to, and the work that has to follow it.
///
/// `rows` are the bindings gate's per-chart counts for the charts this run wrote values into, read
/// back after the write; see [`counted`].
pub fn closing(adoptions: &[Adoption], written: bool, rows: &[String]) -> String {
    let values: usize = adoptions
        .iter()
        .map(|item| {
            let placed = item.placed();
            placed.projected.len() + placed.credentials.len()
        })
        .sum();
    let charts: Vec<&str> = adoptions
        .iter()
        .filter(|item| !item.insertions.is_empty())
        .map(|item| item.chart.as_str())
        .collect();

    if !written {
        return format!(
            "\n==> {values} value(s) across {} chart(s); nothing was written.\n    Re-run with \
             --write to insert them.\n",
            charts.len()
        );
    }

    let mut out = format!(
        "\n==> wrote {values} value(s) into {} chart(s): {}\n\n",
        charts.len(),
        charts.join(", ")
    );
    out.push_str(AFTERWARDS);
    if !rows.is_empty() {
        out.push('\n');
        for row in rows {
            let _ = writeln!(out, "{row}");
        }
    }
    out
}

/// The work a written run leaves, in the order it has to be done.
///
/// Led by the two generated files the run does not touch and that are owed at once: the values
/// schema, without which every render fails on a block it has never seen, and the round-trip suite,
/// whose new cases are what prove the projection arrived.
const AFTERWARDS: &str = "    Then, in order — `just sync-config --write` does 1 and 5 for you:
      1. `just schema`. values.schema.json is committed and Helm validates every
         render against it, so a new block it does not know fails every job
      2. read the `derivedConfig` and `secretData` lines that were written, and
         place by hand any the output above says could not be
      3. replace the text after each `TODO: ` — keep the `# --` line, since a
         value with no description fails `just check-values-docs`
      4. add a `note:` to each credential row saying when a release needs it, and
         choose a default for each invented value named above
      5. `just contract-tests <chart>`, then `just update-snapshots <chart>` —
         the round trip gains a case per marker, and every snapshot case that
         covers the ConfigMap fails until the committed `.snap` is rewritten
      6. paste the row(s) below into tests/test_contract_bindings.py, in
         `test_every_enrolled_chart_passes_the_gate`
      7. `just config-readme` — the `credentials` row is a column of the
         generated table — then bump the chart version and run `just docs`
      8. `just check-config-bindings`, `just check-config-wired`,
         `just check-values-docs`, `just test`
";

/// The rows a chart repository's tree-level test asserts, for the charts this run wrote into.
///
/// That test holds the per-chart marker counts as literals, so every adoption moves one of them and
/// the suite goes red until somebody edits it. The number is the bindings gate's own — markers
/// times the documents each binds — and re-deriving it here would be a second implementation of
/// exactly the arithmetic that test pins. So it is read back out of the gate after the write, and
/// printed as the line to paste rather than written: a test whose expected values a generator edits
/// silently is a test nobody reads.
///
/// # Errors
/// As [`super::bindings::check`].
pub fn counted(charts: &Path, adoptions: &[Adoption]) -> Result<Vec<String>, Error> {
    let touched: BTreeSet<&str> = adoptions
        .iter()
        .filter(|item| !item.insertions.is_empty())
        .map(|item| item.chart.as_str())
        .collect();
    let found = super::bindings::check(charts, &super::declaration::Selection::all())?;
    Ok(found
        .enrolled
        .iter()
        .filter(|(chart, _, _)| touched.contains(chart.as_str()))
        .map(|(chart, keys, external)| format!("        (\"{chart}\", {keys}, {external}),"))
        .collect())
}

// ------------------------------------------------------------------------------------------
// Small helpers
// ------------------------------------------------------------------------------------------

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
mod tests {
    //! Adopting owed contract keys into a chart that already exists.
    //!
    //! The scaffold writes a chart nobody has edited, where every rule is about the *output*. This
    //! writes into a file somebody else wrote, so half of what can go wrong is about the file rather
    //! than about the contract, and none of it is visible from a run over a complete repository —
    //! every enrolled chart there binds every key, which is exactly why a key going unbound is worth
    //! a command in the first place.
    //!
    //! Four of the cases are rules a plausible implementation gets wrong: a value that already
    //! exists is refused rather than guessed at; a new block lands after the last bound sibling
    //! rather than at the end of the file; a key several documents declare becomes one value; and a
    //! credential carries no marker. The round trip ties the rest together: what comes out is handed
    //! to the bindings gate and the shapes gate — the two that decide whether the chart is complete
    //! — rather than compared against bytes written out by hand, which would only prove the
    //! generator agrees with itself.
    //!
    //! Ported case for case from the suite of the script this module replaces, plus the cases for
    //! what the port changed: the tie at one line, the diff, and the line endings.

    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use serde_json::{Map, Value as Json, json};

    use super::{Adoption, Refusal, adopt, splices, write};
    use crate::helm::declaration::{Selection, load_declaration, read_yaml};
    use crate::helm::{bindings, shapes};
    use crate::report::Level;

    /// The contract envelope every reader walks — the dialect, the loader variables, `external`,
    /// `json_schema` — as the format publishes it, so a case changes only the keys.
    const ENVELOPE: &str = include_str!("../../tests/fixtures/api.json");

    /// One contract key, with the fields every consumer of one reads, and `overrides` over them.
    fn key(path: &str, overrides: &Json) -> Json {
        let spelt = path.replace('.', "__");
        let mut entry: Map<String, Json> = json!({
            "path": path,
            "env": format!("FIXTURE_{}", spelt.to_uppercase()),
            "env_file": format!("FIXTURE_{}_FILE", spelt.to_uppercase()),
            "secrets_file": spelt,
            "docs": format!("The {path} setting."),
            "ty": "String",
            "text_form": "text",
            "values": [],
            "constraint": {"type": "string"},
            "text_constraint": null,
            "aliases": [],
            "default": null,
            "default_value": null,
            "note": null,
            "required": false,
            "secret": false,
            "reserved": false,
        })
        .as_object()
        .expect("an object")
        .clone();
        for (name, value) in overrides.as_object().expect("overrides are an object") {
            entry.insert(name.clone(), value.clone());
        }
        Json::Object(entry)
    }

    /// The key every case's chart already binds, so a case is about the keys it adds.
    fn session_ttl() -> Json {
        key(
            "auth.session_ttl",
            &json!({"text_form": "integer", "constraint": {"type": "integer"}}),
        )
    }

    fn integer(path: &str, default: i64) -> Json {
        key(
            path,
            &json!({
                "text_form": "integer",
                "constraint": {"type": "integer"},
                "default_value": default,
            }),
        )
    }

    /// A contract document carrying these keys.
    fn contract_of(keys: &[Json]) -> Json {
        let mut document: Json = serde_json::from_str(ENVELOPE).expect("the fixture is JSON");
        document["schema"]["keys"] = Json::Array(keys.to_vec());
        document
    }

    const DECLARATION: &str = "\
bindings: true
documents:
  - name: api
    source:
      kind: ConfigMap
      selector: { app: fixture }
      key: config.toml
    images:
      - values: image
        contract: contracts/api.json
";

    fn two_documents() -> String {
        format!(
            "{DECLARATION}  - name: worker
    source:
      kind: ConfigMap
      selector: {{ app: fixture-worker }}
      key: config.toml
    images:
      - values: image
        contract: contracts/worker.json
"
        )
    }

    /// Two documents whose `images[].values` follow the `services.<name>.image` convention — the
    /// one a generated root's `$service` guard is read from. [`two_documents`] deliberately does
    /// not: its flat `image` path is the ordinary shape, and is what proves a scope that cannot be
    /// resolved is printed rather than guessed.
    const SCOPED_DOCUMENTS: &str = "\
bindings: true
documents:
  - name: api
    source:
      kind: ConfigMap
      selector: { app.kubernetes.io/component: api }
      key: config.toml
    images:
      - values: services.api.image
        contract: contracts/api.json
  - name: worker
    source:
      kind: ConfigMap
      selector: { app.kubernetes.io/component: worker }
      key: config.toml
    images:
      - values: services.worker.image
        contract: contracts/worker.json
";

    const SCOPED_HELPERS: &str = r#"{{- define "fixture.derivedConfig" -}}
{{- $ctx := .ctx -}}
{{- $service := .service -}}
auth:
  {{- with $ctx.Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#;

    /// One bound value under a grouping block, then a block with no marker at all — the shape of
    /// every chart: the configuration surface first, the chassis after it.
    const VALUES: &str = "\
# @schema
# additionalProperties: true
# @schema
# -- The authentication settings.
auth:
  # @schema
  # # @config projection auth.session_ttl
  # type: integer
  # @schema
  # -- How long a session lives.
  sessionTtl: 3600

# @schema
# type: object
# @schema
# -- Requests and limits for the container.
resources: {}
";

    /// The shape every contracted chart's helper has: one define per partial, mapping keys at the
    /// define's own depth, and `with` blocks under them. Written out rather than generated,
    /// because what these cases are about is a *hand-written* template.
    const HELPERS: &str = r#"{{- define "fixture.derivedConfig" -}}
auth:
  {{- with .Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#;

    /// A throwaway `charts/` tree holding one chart, removed when the case ends.
    struct Chart {
        root: PathBuf,
        dir: PathBuf,
    }

    impl Drop for Chart {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Chart {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "terrace-adopt-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            let dir = root.join("fixture");
            std::fs::create_dir_all(dir.join("contracts")).expect("a contracts directory");
            std::fs::create_dir_all(dir.join("templates")).expect("a templates directory");
            std::fs::write(
                dir.join("Chart.yaml"),
                "name: fixture\nversion: 1.0.0\nappVersion: v1\n",
            )
            .expect("a chart file");
            let chart = Self { root, dir };
            chart.declaration(DECLARATION);
            chart.values(VALUES);
            chart.template("_helpers.tpl", HELPERS);
            chart
        }

        fn contract(&self, document: &str, keys: &[Json]) -> &Self {
            let vendored = json!({
                "source": {
                    "image": "docker.io/example/app",
                    "digest": format!("sha256:{}", "1".repeat(64)),
                    "sha256": "0".repeat(64),
                    "fetched": "2026-01-01T00:00:00Z",
                },
                "contract": contract_of(keys),
            });
            std::fs::write(
                self.dir.join("contracts").join(format!("{document}.json")),
                vendored.to_string(),
            )
            .expect("a vendored contract");
            self
        }

        fn declaration(&self, body: &str) -> &Self {
            std::fs::write(self.dir.join("config-contract.yaml"), body).expect("a declaration");
            self
        }

        fn values(&self, text: &str) -> &Self {
            std::fs::write(self.dir.join("values.yaml"), text).expect("a values file");
            self
        }

        fn template(&self, name: &str, text: &str) -> &Self {
            std::fs::write(self.dir.join("templates").join(name), text).expect("a template");
            self
        }

        fn read(&self, relative: &str) -> String {
            std::fs::read_to_string(self.dir.join(relative)).expect("the file is there")
        }

        fn text(&self) -> String {
            self.read("values.yaml")
        }

        fn helper_text(&self) -> String {
            self.read("templates/_helpers.tpl")
        }

        fn declaration_text(&self) -> String {
            self.read("config-contract.yaml")
        }

        fn parsed(&self) -> Json {
            read_yaml(&self.text(), Path::new("values.yaml")).expect("the values still parse")
        }

        fn plan_for(&self, wanted: &[&str]) -> Adoption {
            let wanted: BTreeSet<String> = wanted.iter().map(|path| (*path).to_owned()).collect();
            adopt(&self.dir, &wanted)
                .expect("the chart is readable")
                .expect("the fixture chart is enrolled")
        }

        fn plan(&self) -> Adoption {
            self.plan_for(&[])
        }

        fn write(&self) -> Adoption {
            let planned = self.plan();
            write(&planned).expect("the edits are written");
            planned
        }

        /// The bindings gate over the chart as it now stands, as its error messages.
        fn gate(&self) -> String {
            let found = bindings::check(&self.root, &Selection::all()).expect("the gate runs");
            found
                .report
                .at_level(Level::Error)
                .map(|entry| entry.finding.message.clone())
                .collect::<Vec<_>>()
                .join("\n")
        }

        fn declared(&self) -> crate::helm::declaration::Declaration {
            load_declaration(&self.dir)
                .expect("the declaration loads")
                .expect("there is one")
        }
    }

    fn refusals(planned: &Adoption) -> String {
        planned
            .refusals
            .iter()
            .map(|Refusal { path, reason, .. }| format!("{path} {reason}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn owed_projected(planned: &Adoption) -> Vec<&str> {
        planned
            .owed_projected
            .iter()
            .map(super::Placement::path)
            .collect()
    }

    /// Every `@schema` block in a values file, keyed by the key it describes, read as helm-schema
    /// reads it: the marker is a YAML comment inside the block, so what comes back is the schema.
    fn blocks(text: &str) -> BTreeMap<String, Json> {
        let lines: Vec<&str> = text.lines().collect();
        let mut found = BTreeMap::new();
        let mut number = 0;
        while number < lines.len() {
            if lines[number].trim() != "# @schema" {
                number += 1;
                continue;
            }
            let start = number + 1;
            let mut end = start;
            while lines[end].trim() != "# @schema" {
                end += 1;
            }
            let body: Vec<&str> = lines[start..end]
                .iter()
                .map(|line| {
                    let comment = line.trim();
                    comment.strip_prefix('#').unwrap_or(comment).trim()
                })
                .collect();
            let mut value = end + 1;
            while lines[value].trim().starts_with('#') {
                value += 1;
            }
            let name = lines[value].split(':').next().unwrap_or_default().trim();
            let schema = read_yaml(&body.join("\n"), Path::new("block")).expect("a block is YAML");
            found.insert(name.to_owned(), schema);
            number = value;
        }
        found
    }

    // --------------------------------------------------------------------------------------
    // What a new key becomes
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_new_key_becomes_a_value_with_its_marker_type_and_description() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key(
                    "log.level",
                    &json!({
                        "docs": "Least severe level that is logged.",
                        "constraint": {"type": "string", "enum": ["debug", "info"]},
                        "default_value": "info",
                    }),
                ),
            ],
        );
        chart.write();

        assert_eq!(chart.parsed()["log"]["level"], json!("info"));
        assert!(
            chart
                .text()
                .contains("# # @config projection log.level optional")
        );
        assert!(
            chart
                .text()
                .contains("Least severe level that is logged (`log.level`)")
        );
        assert_eq!(
            blocks(&chart.text())["level"],
            json!({"enum": ["debug", "info", null]})
        );
    }

    #[test]
    fn the_default_is_the_one_the_image_publishes() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), integer("worker.concurrency", 8)]);
        chart.write();
        assert_eq!(chart.parsed()["worker"]["concurrency"], json!(8));
    }

    #[test]
    fn two_keys_under_one_new_branch_share_a_grouping_block() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("sentry.dsn_url", &json!({})),
                key("sentry.environment", &json!({})),
            ],
        );
        let planned = chart.write();

        assert_eq!(planned.insertions.len(), 1);
        assert_eq!(chart.text().matches("\nsentry:").count(), 1);
        let sentry = chart.parsed()["sentry"].clone();
        let mut names: Vec<&String> = sentry.as_object().expect("a mapping").keys().collect();
        names.sort();
        assert_eq!(names, ["dsnUrl", "environment"]);
        // The block a contract says nothing about, and the plainest true sentence for it.
        assert!(chart.text().contains("-- The `sentry` settings."));
        assert_eq!(planned.insertions[0].branches, ["sentry"]);
    }

    #[test]
    fn a_credential_gets_a_value_with_no_marker_and_a_write_off() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        let planned = chart.write();

        assert_eq!(chart.parsed()["database"]["url"], json!(""));
        assert!(!chart.text().contains("@config projection database.url"));
        assert!(chart.text().contains("`database__url`"));
        let secrets: Vec<&str> = planned
            .plan
            .credentials
            .iter()
            .map(super::Placement::path)
            .collect();
        assert_eq!(secrets, ["database.url"]);
    }

    #[test]
    fn a_reserved_key_gets_no_value_at_all() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("runtime.node_id", &json!({"reserved": true})),
            ],
        );
        let planned = chart.write();

        assert!(chart.parsed().get("runtime").is_none());
        let written_off: Vec<&str> = planned
            .plan
            .written_off
            .iter()
            .map(super::Placement::path)
            .collect();
        assert_eq!(written_off, ["runtime.node_id"]);
        assert!(
            planned.plan.written_off[0]
                .reason
                .contains("Reserved by the loader")
        );
    }

    // --------------------------------------------------------------------------------------
    // Where the block goes
    // --------------------------------------------------------------------------------------

    fn line_of(text: &str, wanted: &str) -> usize {
        text.lines()
            .position(|line| line == wanted)
            .unwrap_or_else(|| panic!("no line {wanted:?} in\n{text}"))
    }

    #[test]
    fn a_new_top_level_block_lands_after_the_last_bound_block() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        chart.write();

        let text = chart.text();
        assert!(line_of(&text, "log:") < line_of(&text, "resources: {}"));
        assert!(line_of(&text, "log:") > line_of(&text, "auth:"));
    }

    #[test]
    fn a_new_leaf_lands_inside_the_block_it_belongs_to() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), key("auth.issuer", &json!({}))]);
        chart.write();

        let auth = chart.parsed()["auth"].clone();
        let mut names: Vec<&String> = auth.as_object().expect("a mapping").keys().collect();
        names.sort();
        assert_eq!(names, ["issuer", "sessionTtl"]);
        let text = chart.text();
        assert!(line_of(&text, "  issuer: null") < line_of(&text, "resources: {}"));
    }

    #[test]
    fn a_leaf_and_a_sibling_block_meeting_at_one_line_both_land_where_they_belong() {
        // `auth` ends on the last line of the bound values, and that same line is where a new
        // top-level block goes. The leaf has to be written first, inside `auth`; the other way
        // round it lands under `log:` and becomes `log.issuer`.
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("auth.issuer", &json!({})),
                key("log.level", &json!({})),
            ],
        );
        let planned = chart.write();

        assert_eq!(planned.insertions.len(), 2);
        assert_eq!(planned.insertions[0].after, planned.insertions[1].after);
        let values = chart.parsed();
        assert_eq!(values["auth"]["issuer"], Json::Null);
        assert!(values["auth"].get("issuer").is_some(), "{values}");
        assert!(values["log"].get("issuer").is_none(), "{values}");
        // Bound, but not yet rendered: `derivedConfig` already writes `auth:`, so where the
        // `issuer` line goes inside that hand-written block is printed for a person rather than
        // written. The gate's template rule is what keeps that from passing silently.
        assert_eq!(
            chart.gate(),
            "binds contract key 'auth.issuer', but no template under fixture/templates/ renders \
             a 'issuer' key. The value exists and is typed, but nothing projects it — \
             indistinguishable, to an operator, from the setting never having been added"
        );
        assert!(!chart.plan().owed());
    }

    #[test]
    fn the_file_keeps_its_line_endings() {
        let chart = Chart::new();
        chart.values(&VALUES.replace('\n', "\r\n"));
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        chart.write();

        let body = chart.text();
        assert_eq!(body.matches('\n').count(), body.matches("\r\n").count());
        assert!(body.contains("log:\r\n"));
    }

    #[test]
    fn a_last_line_with_no_terminator_is_terminated_before_anything_follows_it() {
        let chart = Chart::new();
        chart.values(VALUES.trim_end_matches('\n'));
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        chart.write();
        assert!(chart.text().contains("resources: {}\n"));
        assert_eq!(chart.parsed()["resources"], json!({}));
    }

    #[test]
    fn planning_writes_nothing() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let before = chart.text();
        let planned = chart.plan();

        assert_ne!(planned.insertions, [] as [super::Insertion; 0]);
        assert_eq!(chart.text(), before);
    }

    // --------------------------------------------------------------------------------------
    // What it refuses to write
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_key_whose_value_already_exists_is_refused_and_names_the_marker() {
        let chart = Chart::new();
        // The chart has `auth.sessionTtl`; the contract's key is the one it feeds, and the marker
        // that says so has been lost.
        chart.values(&VALUES.replace("  # # @config projection auth.session_ttl\n", ""));
        chart.contract("api", &[session_ttl()]);
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert!(refusals(&planned).contains("which this chart already has"));
        assert!(refusals(&planned).contains("# # @config projection auth.session_ttl optional"));
    }

    #[test]
    fn a_leaf_on_the_way_to_the_value_is_refused() {
        let chart = Chart::new();
        // `resources` is a scalar here, so `resources.limit` cannot exist beside it.
        chart.contract("api", &[session_ttl(), key("resources.limit", &json!({}))]);
        chart.values(&VALUES.replace("resources: {}", "resources: none"));
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert!(refusals(&planned).contains("is a leaf value on the way there"));
    }

    #[test]
    fn documents_that_disagree_about_a_key_are_refused() {
        let chart = Chart::new();
        chart.declaration(&two_documents());
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({"constraint": {"type": "string"}})),
            ],
        );
        chart.contract(
            "worker",
            &[key(
                "log.level",
                &json!({"constraint": {"type": "integer"}}),
            )],
        );
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert!(refusals(&planned).contains("do not agree what it is"));
    }

    #[test]
    fn a_key_bound_in_one_document_and_missing_in_another_is_refused() {
        let chart = Chart::new();
        chart.declaration(&two_documents());
        // The marker is scoped to `api`, so `worker` declares the key and nothing binds it there.
        // A second value under the same name is not the repair; the scope is.
        chart.values(&VALUES.replace(
            "@config projection auth.session_ttl",
            "@config projection api:auth.session_ttl",
        ));
        chart.contract("api", &[session_ttl()]);
        chart.contract("worker", &[session_ttl()]);
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert!(refusals(&planned).contains("what is owed is that marker's scope"));
    }

    #[test]
    fn a_chart_whose_markers_are_broken_is_not_written_to() {
        let chart = Chart::new();
        chart.values(&format!(
            "{}\n# @schema\n# type: string\n# @schema\n# -- A value.\nspare: \"\"\n",
            VALUES.replace("auth.session_ttl", "auth.session_ttl_typo")
        ));
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let before = chart.text();
        let planned = chart.plan();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert_ne!(planned.blocked, [] as [String; 0]);
        assert!(planned.blocked.join("\n").contains("auth.session_ttl_typo"));
        write(&planned).expect("writing nothing succeeds");
        assert_eq!(chart.text(), before);
    }

    // --------------------------------------------------------------------------------------
    // One value per key
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_key_several_documents_declare_becomes_one_value() {
        let chart = Chart::new();
        chart.declaration(&two_documents());
        let shared = key("log.level", &json!({"default_value": "info"}));
        chart.contract("api", &[session_ttl(), shared.clone()]);
        chart.contract("worker", &[shared]);
        let planned = chart.write();

        assert_eq!(planned.insertions.len(), 1);
        assert_eq!(
            chart.text().matches("@config projection log.level").count(),
            1
        );
        assert_eq!(chart.parsed()["log"]["level"], json!("info"));
    }

    // --------------------------------------------------------------------------------------
    // A new root only some of a multi-document chart's documents declare
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_key_only_one_document_declares_is_guarded_by_service() {
        let chart = Chart::new();
        chart.declaration(SCOPED_DOCUMENTS);
        chart.template("_helpers.tpl", SCOPED_HELPERS);
        chart.contract("api", &[session_ttl()]);
        chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
        let planned = chart.write();

        assert_eq!(planned.owed_projected, [] as [super::Placement; 0]);
        let body = chart.helper_text();
        assert!(body.contains("{{- if eq $service \"worker\" }}"), "{body}");
        assert!(body.contains("queue:"), "{body}");
        assert!(
            body.contains("workers: {{ $ctx.Values.queue.workers }}"),
            "{body}"
        );
        // The unguarded root stays exactly as it was; the guard wraps only the new one.
        assert!(body.find("auth:") < body.find("{{- if eq $service \"worker\" }}"));
    }

    #[test]
    fn a_new_key_under_a_root_already_guarded_by_service_is_printed_instead() {
        // A root already written, but from inside `{{- if eq $service ... }}` around a
        // `{{- with include ... }}`, not at the define's own depth. A root detector keyed to that
        // depth alone cannot see it, so a new key under the same root looks like a brand-new root
        // and is appended as a second, colliding block — which `fromYaml` resolves by keeping only
        // the second and dropping the first in silence.
        let chart = Chart::new();
        chart.declaration(SCOPED_DOCUMENTS);
        chart.template(
            "_helpers.tpl",
            r#"{{- define "fixture.derivedConfig" -}}
{{- $ctx := .ctx -}}
{{- $service := .service -}}
auth:
  {{- with $ctx.Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- if eq $service "worker" }}
{{- with include "fixture.queue.config" $ctx }}
queue:
  {{- . | nindent 2 }}
{{- end }}
{{- end }}
{{- end -}}

{{- define "fixture.queue.config" -}}
{{- if not (kindIs "invalid" .Values.queue.workers) }}
workers: {{ .Values.queue.workers }}
{{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#,
        );
        chart.contract("api", &[session_ttl()]);
        chart.contract("worker", &[session_ttl(), integer("queue.retries", 3)]);
        let planned = chart.write();

        assert_eq!(owed_projected(&planned), ["queue.retries"]);
        assert_eq!(chart.helper_text().matches("queue:").count(), 1);
    }

    #[test]
    fn new_keys_sharing_a_root_with_different_scopes_are_printed_not_guessed() {
        let chart = Chart::new();
        chart.declaration(SCOPED_DOCUMENTS);
        chart.template("_helpers.tpl", SCOPED_HELPERS);
        chart.contract("api", &[session_ttl(), integer("queue.retries", 3)]);
        chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
        let planned = chart.write();

        let mut owed = owed_projected(&planned);
        owed.sort_unstable();
        assert_eq!(owed, ["queue.retries", "queue.workers"]);
        assert!(!chart.helper_text().contains("queue:"));
    }

    #[test]
    fn a_scoped_key_with_no_resolvable_service_is_printed_not_guessed() {
        let chart = Chart::new();
        chart.declaration(&two_documents());
        chart.contract("api", &[session_ttl(), integer("queue.workers", 4)]);
        chart.contract("worker", &[session_ttl()]);
        let planned = chart.write();

        assert_eq!(owed_projected(&planned), ["queue.workers"]);
        assert!(!chart.helper_text().contains("queue:"));
    }

    #[test]
    fn a_define_that_does_not_wrap_context_is_never_guessed_at() {
        // Included as `(dict "ctx" $ctx "service" $service)`, `.Values` inside the define is a nil
        // pointer. A multi-document chart whose define is not provably shaped that way — no
        // `$ctx := .ctx` line — gets nothing written: bare `.Values` would be as much of a guess
        // as `$ctx.Values` would be wrong.
        let chart = Chart::new();
        chart.declaration(SCOPED_DOCUMENTS);
        chart.template(
            "_helpers.tpl",
            r#"{{- define "fixture.derivedConfig" -}}
{{- $service := .service -}}
auth:
  {{- with .Values.auth.sessionTtl }}
  session_ttl: {{ . }}
  {{- end }}
{{- end -}}

{{- define "fixture.secretData" -}}
{{- end -}}
"#,
        );
        chart.contract("api", &[session_ttl()]);
        chart.contract("worker", &[session_ttl(), integer("queue.workers", 4)]);
        let planned = chart.write();

        assert_eq!(owed_projected(&planned), ["queue.workers"]);
        assert!(!chart.helper_text().contains("queue:"));
    }

    // --------------------------------------------------------------------------------------
    // The declaration
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_credential_is_written_off_and_given_its_value_column() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        chart.write();
        let body = chart.declaration_text();

        assert!(body.contains("unbound:"));
        assert!(body.contains("      - database.url"));
        assert!(body.contains("`database__url`"));
        assert!(body.contains("  - key: database.url"));
        assert!(body.contains("    value: database.url"));

        let declared = chart.declared();
        let written_off: Vec<&Vec<String>> =
            declared.unbound.iter().map(|entry| &entry.keys).collect();
        assert_eq!(written_off, [&vec!["database.url".to_owned()]]);
        let credential = &declared.credentials["database.url"];
        assert_eq!(credential.value.as_deref(), Some("database.url"));
        assert_eq!(credential.note, None);
    }

    #[test]
    fn a_reserved_key_is_written_off_with_the_loader_reason() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("runtime.node_id", &json!({"reserved": true})),
            ],
        );
        chart.write();

        let declared = chart.declared();
        let written_off: Vec<&Vec<String>> =
            declared.unbound.iter().map(|entry| &entry.keys).collect();
        assert_eq!(written_off, [&vec!["runtime.node_id".to_owned()]]);
        assert!(
            declared.unbound[0]
                .reason
                .contains("Reserved by the loader")
        );
        assert!(declared.credentials.is_empty());
    }

    #[test]
    fn a_credential_the_chart_already_has_a_value_for_is_still_written_off() {
        // The state a run leaves behind if it wrote the value before the declaration. The refusal
        // stands — nothing claims that value feeds this key — but the write-off does not depend on
        // who wrote the value, so its reason drops the clause naming it and the gate is satisfied.
        // Without this, running the command twice leaves a chart it could not finish.
        let chart = Chart::new();
        chart.values(&format!("{VALUES}database:\n  url: \"\"\n"));
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert_ne!(planned.refusals, [] as [Refusal; 0]);
        let declared = chart.declared();
        let written_off: Vec<&Vec<String>> =
            declared.unbound.iter().map(|entry| &entry.keys).collect();
        assert_eq!(written_off, [&vec!["database.url".to_owned()]]);
        assert!(
            !declared.unbound[0]
                .reason
                .contains("is the value that carries it")
        );
        // The `value:` column would name a value this command did not write, so it is not claimed.
        assert!(declared.credentials.is_empty());
        assert_eq!(chart.gate(), "");
    }

    #[test]
    fn a_key_already_written_off_is_not_written_off_twice() {
        let chart = Chart::new();
        chart.declaration(&format!(
            "{DECLARATION}unbound:\n  - keys:\n      - database.url\n    reason: Already decided.\n"
        ));
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        chart.write();

        let reasons: Vec<String> = chart
            .declared()
            .unbound
            .into_iter()
            .map(|entry| entry.reason)
            .collect();
        assert_eq!(reasons, ["Already decided."]);
    }

    #[test]
    fn a_group_appends_to_the_block_a_chart_already_has() {
        let chart = Chart::new();
        chart.declaration(&format!(
            "{DECLARATION}unbound:\n  - keys:\n      - log.level\n    reason: Not surfaced.\n"
        ));
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({})),
                key("runtime.node_id", &json!({"reserved": true})),
            ],
        );
        chart.write();

        let written_off: Vec<Vec<String>> = chart
            .declared()
            .unbound
            .into_iter()
            .map(|entry| entry.keys)
            .collect();
        assert_eq!(
            written_off,
            [
                vec!["log.level".to_owned()],
                vec!["runtime.node_id".to_owned()]
            ]
        );
    }

    #[test]
    fn a_credential_row_lands_in_credentials_and_not_in_a_new_unbound_block() {
        // Both new blocks go after the last line of `credentials`: the row has to be written
        // first, so it stays in that block; the other way round it lands under the new `unbound:`
        // as an entry the declaration refuses.
        let chart = Chart::new();
        chart.declaration(&DECLARATION.replace(
            "documents:\n",
            "credentials:\n  - key: other.token\n    value: other.token\n\ndocuments:\n",
        ));
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("other.token", &json!({"secret": true})),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        chart.values(&format!("{VALUES}other:\n  # -- A token.\n  token: \"\"\n"));
        let planned = chart.write();

        let declaring: Vec<usize> = planned
            .edits
            .iter()
            .filter(|edit| edit.path.ends_with("config-contract.yaml"))
            .map(|edit| edit.after)
            .collect();
        assert_eq!(declaring.len(), 2, "{:?}", planned.edits);
        assert_eq!(
            declaring[0], declaring[1],
            "the two blocks meet at one line"
        );

        let declared = chart.declared();
        assert!(declared.credentials.contains_key("database.url"));
        assert!(declared.credentials.contains_key("other.token"));
        assert!(
            declared
                .unbound
                .iter()
                .any(|entry| entry.keys == ["database.url"])
        );
    }

    #[test]
    fn planning_writes_no_declaration() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("runtime.node_id", &json!({"reserved": true})),
            ],
        );
        let before = chart.declaration_text();
        let planned = chart.plan();

        assert_ne!(planned.edits, [] as [super::FileEdit; 0]);
        assert_eq!(chart.declaration_text(), before);
    }

    // --------------------------------------------------------------------------------------
    // The template lines
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_new_root_is_appended_to_the_derived_config() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({"default_value": "info"})),
            ],
        );
        chart.write();
        let body = chart.helper_text();

        assert!(body.contains("log:"));
        assert!(body.contains("{{- with .Values.log.level }}"));
        assert!(body.contains("level: {{ . | quote }}"));
        // Inside the define it belongs to, after everything that was already there.
        assert!(body.find("session_ttl") < body.find("log:"));
        assert!(body.find("log:") < body.find("define \"fixture.secretData\""));
    }

    #[test]
    fn a_root_the_define_already_writes_is_printed_instead() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), key("auth.issuer", &json!({}))]);
        let planned = chart.write();

        assert_eq!(chart.helper_text().matches("auth:").count(), 1);
        assert_eq!(owed_projected(&planned), ["auth.issuer"]);
    }

    #[test]
    fn a_credential_is_appended_to_the_secret_data() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("database.url", &json!({"secret": true, "required": true})),
            ],
        );
        chart.write();
        assert!(
            chart
                .helper_text()
                .contains("database__url: {{ .Values.database.url | quote }}")
        );
    }

    #[test]
    fn a_derived_config_split_into_another_file_is_still_found() {
        // A chart whose services split configuration across several partials keeps
        // `derivedConfig` in `_config.tpl`. Searching `_helpers.tpl` alone would print every
        // projection as still owed and write none, on a chart enrolled like any other.
        let chart = Chart::new();
        chart.template(
            "_helpers.tpl",
            "{{- define \"fixture.secretData\" -}}\n{{- end -}}\n",
        );
        chart.template(
            "_config.tpl",
            "{{- define \"fixture.derivedConfig\" -}}\nauth:\n  {{- with .Values.auth.sessionTtl }}\n  \
             session_ttl: {{ . }}\n  {{- end }}\n{{- end -}}\n",
        );
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({"default_value": "info"})),
            ],
        );
        let planned = chart.write();

        assert_eq!(planned.owed_projected, [] as [super::Placement; 0]);
        assert!(chart.read("templates/_config.tpl").contains("log:"));
        assert!(!chart.helper_text().contains("log:"));
    }

    #[test]
    fn a_chart_with_no_helper_prints_every_line() {
        let chart = Chart::new();
        std::fs::remove_file(chart.dir.join("templates").join("_helpers.tpl"))
            .expect("the helper is removed");
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let planned = chart.write();
        assert_eq!(owed_projected(&planned), ["log.level"]);
    }

    #[test]
    fn a_define_whose_actions_do_not_balance_is_left_alone() {
        // A template shape this cannot read is one it must not edit.
        let chart = Chart::new();
        chart.template(
            "_helpers.tpl",
            "{{- define \"fixture.derivedConfig\" -}}\nauth:\n",
        );
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let planned = chart.write();

        assert_eq!(owed_projected(&planned), ["log.level"]);
        assert!(!chart.helper_text().contains("log:"));
    }

    #[test]
    fn planning_writes_no_template() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let before = chart.helper_text();
        chart.plan();
        assert_eq!(chart.helper_text(), before);
    }

    // --------------------------------------------------------------------------------------
    // The round trip
    // --------------------------------------------------------------------------------------

    fn adopted() -> Chart {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key(
                    "log.level",
                    &json!({"constraint": {"type": "string", "enum": ["debug", "info"]}}),
                ),
                key(
                    "sentry.environment",
                    &json!({"default_value": "production"}),
                ),
                key(
                    "sentry.max_events",
                    &json!({"text_form": "integer", "constraint": {"type": "integer"}}),
                ),
            ],
        );
        chart.write();
        chart
    }

    #[test]
    fn the_gate_passes_over_what_was_written() {
        assert_eq!(adopted().gate(), "");
    }

    #[test]
    fn the_shape_gate_passes_over_what_was_written() {
        let chart = adopted();
        let mut read = shapes::Chart::read(&chart.dir).expect("the chart reads");
        shapes::check(&mut read);
        assert!(read.problems.is_empty(), "{:?}", read.problems);
    }

    #[test]
    fn the_values_file_still_parses_and_keeps_what_it_had() {
        let values = adopted().parsed();
        assert_eq!(values["auth"]["sessionTtl"], json!(3600));
        assert_eq!(values["resources"], json!({}));
        let mut names: Vec<&String> = values["sentry"]
            .as_object()
            .expect("a mapping")
            .keys()
            .collect();
        names.sort();
        assert_eq!(names, ["environment", "maxEvents"]);
    }

    #[test]
    fn a_second_run_has_nothing_left_to_do() {
        let again = adopted().plan();
        assert_eq!(again.insertions, [] as [super::Insertion; 0]);
        assert_eq!(again.refusals, [] as [Refusal; 0]);
        assert!(!again.owed());
    }

    // --------------------------------------------------------------------------------------
    // Selecting keys
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_named_key_is_adopted_and_the_rest_are_left() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({})),
                key("sentry.environment", &json!({})),
            ],
        );
        let planned = chart.plan_for(&["log.level"]);
        write(&planned).expect("the edits are written");

        assert!(chart.parsed().get("log").is_some());
        assert!(chart.parsed().get("sentry").is_none());
    }

    #[test]
    fn a_key_that_is_not_owed_is_reported_rather_than_ignored() {
        let chart = Chart::new();
        chart.contract("api", &[session_ttl()]);
        let planned = chart.plan_for(&["log.level"]);

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        assert!(refusals(&planned).contains("is not a key fixture is owed a value for"));
    }

    // --------------------------------------------------------------------------------------
    // Enrolment
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_chart_that_is_not_enrolled_is_passed_over() {
        let chart = Chart::new();
        chart.declaration(&DECLARATION.replace("bindings: true", "bindings: false"));
        chart.contract("api", &[key("log.level", &json!({}))]);
        assert!(
            adopt(&chart.dir, &BTreeSet::new())
                .expect("readable")
                .is_none()
        );
    }

    #[test]
    fn a_marker_is_never_written_for_a_chart_with_no_contract() {
        let chart = Chart::new();
        std::fs::remove_file(chart.dir.join("config-contract.yaml")).expect("removed");
        assert!(
            adopt(&chart.dir, &BTreeSet::new())
                .expect("readable")
                .is_none()
        );
    }

    // --------------------------------------------------------------------------------------
    // The diff
    // --------------------------------------------------------------------------------------

    #[test]
    fn the_diff_is_a_unified_diff_of_exactly_what_write_produces() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("auth.issuer", &json!({})),
                key("database.url", &json!({"secret": true})),
            ],
        );
        let planned = chart.plan();
        let found = splices(&planned).expect("the files read");
        assert_eq!(
            found
                .iter()
                .map(|splice| splice.path.file_name().expect("a file name").to_owned())
                .collect::<Vec<_>>(),
            ["values.yaml", "config-contract.yaml", "_helpers.tpl"]
        );

        let values = &found[0];
        let diff = values.diff();
        let shown = super::posix(&values.path);
        assert!(
            diff.starts_with(&format!("--- {shown}\n+++ {shown}\n@@ -")),
            "{diff}"
        );
        // Every line of the diff is a header, a hunk, or one marked line.
        for line in diff.lines().skip(2) {
            assert!(
                line.starts_with("@@ ") || line.starts_with([' ', '+', '-']),
                "{line:?}"
            );
        }
        assert!(diff.contains("\n+  issuer: null\n"), "{diff}");
        assert!(
            !diff
                .lines()
                .any(|line| line.starts_with('-') && !line.starts_with("---"))
        );

        // What the diff describes is what `write` puts on disk.
        write(&planned).expect("written");
        assert_eq!(chart.text(), values.apply());
    }

    #[test]
    fn the_diff_marks_a_last_line_that_had_no_terminator() {
        // The bound block is the whole file and its last line has no terminator, so a new block
        // after it rewrites that line: the old one is removed, saying it had none, and the same
        // text comes back terminated.
        let chart = Chart::new();
        let bound: Vec<&str> = VALUES.lines().take(11).collect();
        chart.values(&bound.join("\n"));
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let planned = chart.plan();
        assert_eq!(planned.insertions[0].after, 11);

        let found = splices(&planned).expect("the files read");
        let diff = found[0].diff();
        assert!(
            diff.contains(
                "-  sessionTtl: 3600\n\\ No newline at end of file\n+  sessionTtl: 3600\n"
            ),
            "{diff}"
        );
        write(&planned).expect("written");
        assert_eq!(chart.text(), found[0].apply());
        assert_eq!(chart.parsed()["log"]["level"], Json::Null);
    }

    // --------------------------------------------------------------------------------------
    // Shapes an insertion-only writer must not write into
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_block_scalar_that_looks_like_comments_is_not_cut_in_two() {
        // `auth` ends with a `|` scalar whose text has a comment-looking line and a `# @schema`
        // line. Read as a comment and a delimiter, they ended `auth` mid-scalar, and the new
        // top-level block went into the middle of the text.
        let chart = Chart::new();
        chart.values(&VALUES.replace(
            "  sessionTtl: 3600\n",
            "  sessionTtl: 3600\n\n  # -- A banner.\n  banner: |\n    hello\n    # @schema\n    \
             # not a comment\n    world\n",
        ));
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        chart.write();

        let values = chart.parsed();
        assert_eq!(
            values["auth"]["banner"],
            json!("hello\n# @schema\n# not a comment\nworld\n")
        );
        assert_eq!(values["log"]["level"], Json::Null);
        let text = chart.text();
        assert!(line_of(&text, "log:") > line_of(&text, "    world"));
    }

    #[test]
    fn a_parent_written_with_its_value_on_one_line_is_refused() {
        // `telemetry: null` and `limits: {}` are mappings to YAML, but a block below either is a
        // second value for one key, and the file would no longer parse.
        let chart = Chart::new();
        chart.values(&format!(
            "{VALUES}\n# -- Telemetry.\ntelemetry: null\n\n# -- Limits.\nlimits: {{}}\n"
        ));
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("telemetry.level", &json!({})),
                key("limits.burst", &json!({})),
            ],
        );
        let before = chart.text();
        let planned = chart.write();

        assert_eq!(planned.insertions, [] as [super::Insertion; 0]);
        let refused = refusals(&planned);
        assert!(
            refused.contains("telemetry.level needs the chart value 'telemetry.level'"),
            "{refused}"
        );
        assert!(refused.contains("limits.burst"), "{refused}");
        assert!(
            refused.contains("written with its value on the key's own line"),
            "{refused}"
        );
        assert_eq!(chart.text(), before);
        let _ = chart.parsed();
    }

    #[test]
    fn a_declaration_block_written_on_one_line_blocks_the_chart() {
        for (block, owed) in [
            (
                "unbound: []",
                key("runtime.node_id", &json!({"reserved": true})),
            ),
            (
                "credentials: []",
                key("database.url", &json!({"secret": true})),
            ),
        ] {
            let chart = Chart::new();
            chart.declaration(&format!("{DECLARATION}{block}\n"));
            chart.contract("api", &[session_ttl(), owed]);
            let values = chart.text();
            let declaration = chart.declaration_text();
            let planned = chart.write();

            assert!(planned.by_hand, "{block}");
            assert!(
                planned
                    .blocked
                    .join("\n")
                    .contains("is written with its value on the key's own line"),
                "{block}: {:?}",
                planned.blocked
            );
            assert_eq!(planned.insertions, [] as [super::Insertion; 0], "{block}");
            assert_eq!(chart.text(), values, "{block}");
            assert_eq!(chart.declaration_text(), declaration, "{block}");
            let shown = super::report(&planned, false).expect("a report");
            assert!(shown.contains("fixed by hand"), "{shown}");
        }
    }

    #[test]
    fn a_define_closed_on_a_line_shared_with_another_action_is_printed_not_written() {
        // Inserting above `{{- end }}{{- end -}}` lands inside the `with`, under a condition
        // nobody wrote for the new lines.
        let chart = Chart::new();
        chart.template(
            "_helpers.tpl",
            "{{- define \"fixture.derivedConfig\" -}}\nauth:\n  {{- with .Values.auth.sessionTtl }}\n  \
             session_ttl: {{ . }}\n  {{- end }}{{- end -}}\n\n{{- define \"fixture.secretData\" -}}\n\
             {{- end -}}\n",
        );
        let before = chart.helper_text();
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let planned = chart.write();

        assert_eq!(owed_projected(&planned), ["log.level"]);
        assert_eq!(chart.helper_text(), before);
    }

    #[test]
    fn a_define_opened_and_closed_on_one_line_is_printed_not_written() {
        // Inserting above the closing `end` would put the lines above the define itself.
        let chart = Chart::new();
        chart.template(
            "_helpers.tpl",
            &HELPERS.replace(
                "{{- define \"fixture.secretData\" -}}\n{{- end -}}",
                "{{- define \"fixture.secretData\" -}}{{- end -}}",
            ),
        );
        let before = chart.helper_text();
        chart.contract(
            "api",
            &[session_ttl(), key("database.url", &json!({"secret": true}))],
        );
        let planned = chart.write();

        let owed: Vec<&str> = planned
            .owed_credentials
            .iter()
            .map(super::Placement::path)
            .collect();
        assert_eq!(owed, ["database.url"]);
        assert_eq!(chart.helper_text(), before);
    }

    #[test]
    fn a_key_whose_text_form_this_build_does_not_know_is_refused_and_the_rest_adopted() {
        let chart = Chart::new();
        chart.contract(
            "api",
            &[
                session_ttl(),
                key("log.level", &json!({})),
                key("log.window", &json!({"text_form": "duration"})),
            ],
        );
        let planned = chart.write();

        assert!(
            refusals(&planned).contains(
                "log.window has `text_form` 'duration', which this build does not implement"
            ),
            "{}",
            refusals(&planned)
        );
        assert_eq!(chart.parsed()["log"]["level"], Json::Null);
        assert!(chart.parsed()["log"].get("window").is_none());
    }

    // --------------------------------------------------------------------------------------
    // Writing: all or nothing, and only inside the chart
    // --------------------------------------------------------------------------------------

    #[test]
    fn a_file_that_resolves_outside_its_chart_fails_the_whole_write_before_anything_lands() {
        // Two charts planned together; the second's helper is a link to a file outside it. The
        // first chart's files are untouched, the outside file is untouched, and no temporary is
        // left behind.
        let first = Chart::new();
        first.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let second = Chart::new();
        second.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        let outside = second.root.join("outside.tpl");
        std::fs::write(&outside, HELPERS).expect("the outside file");
        let helper = second.dir.join("templates").join("_helpers.tpl");
        std::fs::remove_file(&helper).expect("removed");
        std::os::unix::fs::symlink(&outside, &helper).expect("a symbolic link");

        let before = (first.text(), first.helper_text(), second.text());
        let planned = [first.plan(), second.plan()];
        let error = super::write_all(&planned).expect_err("the link escapes its chart");
        assert!(error.to_string().contains("outside the chart"), "{error}");

        assert_eq!((first.text(), first.helper_text(), second.text()), before);
        assert_eq!(std::fs::read_to_string(&outside).expect("outside"), HELPERS);
        for chart in [&first, &second] {
            for directory in [chart.dir.clone(), chart.dir.join("templates")] {
                for entry in std::fs::read_dir(&directory).expect("a directory") {
                    let name = entry.expect("an entry").file_name();
                    assert!(
                        !name.to_string_lossy().ends_with(".tmp"),
                        "a temporary was left: {name:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_write_keeps_each_files_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let chart = Chart::new();
        let values = chart.dir.join("values.yaml");
        std::fs::set_permissions(&values, std::fs::Permissions::from_mode(0o640))
            .expect("permissions set");
        chart.contract("api", &[session_ttl(), key("log.level", &json!({}))]);
        chart.write();
        let mode = std::fs::metadata(&values)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
        assert!(chart.text().contains("log:"));
    }
}
