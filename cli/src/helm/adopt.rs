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
mod tests;
