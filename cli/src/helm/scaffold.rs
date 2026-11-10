//! Planning a chart's configuration surface from the contract its image publishes.
//!
//! Every other module here *reads* a chart and holds it against a contract. This runs the other
//! way: given contract keys, it decides where each one goes in a chart and writes the lines that
//! put it there: the `values.yaml` block with its marker and its `@schema`, the `derivedConfig`
//! line that projects the value into the document, the `secretData` line that delivers a
//! credential, and the `unbound` entry for a key no value surfaces.
//!
//! Two callers share it, and the split between them is the reason it is a module of its own.
//! [`super::adopt`] writes the values a chart that already exists is owed; a scaffold for a chart
//! that does not exist yet writes the same lines for a whole contract at once. The rules (which
//! keys get a value, which go to the Secret, what "unset" renders as) are judgements made once, and
//! two generators carrying a copy each is how they come to disagree about where a credential
//! belongs.
//!
//! # Why generating a `# @config` marker here is not what the gate refuses
//!
//! The marker gate says nothing generates a marker onto a value that already exists, and it is
//! right about that case: there the *value* is the fact and the marker is a claim about it, and a
//! generator guessing whether `telemetry.logLevel` or `logging.level` feeds `telemetry.log_level`
//! produces a wrong guess that reads exactly like a right one.
//!
//! That objection does not reach this module, because here the value does not exist yet. The key is
//! the fact, and the value and its marker are emitted together from that one key by one rule, so
//! there is no prior mapping for the marker to be wrong about. The marker *is* the mapping, and the
//! value was named to match it. The gate is unchanged either way: what comes out of here is held
//! against the same contract, so a plan whose rules are wrong fails on the chart's first run.
//!
//! # Where each contract key goes
//!
//! One rule per row, applied in order; the first that matches wins.
//!
//! | The key is...                 | Destination                        | Why                     |
//! |-------------------------------|------------------------------------|-------------------------|
//! | `reserved`                    | nothing, plus an `unbound` entry   | the loader owns it      |
//! | `secret`, and file-supplyable | `secretData`, plus `unbound`       | never a `ConfigMap`     |
//! | `secret`, not file-supplyable | nothing, plus an `unbound` entry   | no safe channel exists  |
//! | anything else                 | a chart value, typed and defaulted | the ordinary case       |
//!
//! The credential rule is the one worth arguing with. A `secret: true` key is delivered as a *file*
//! and never written into the rendered document, because a `ConfigMap` is readable by anything that
//! can read the namespace. Some charts do project a credential into their document, which is a
//! judgement about what that credential is worth; a generator cannot make it, and the conservative
//! direction is the only defensible default: moving one key from the Secret into the `ConfigMap`
//! afterwards is a deliberate edit somebody signs, where the opposite mistake is silent.
//!
//! # Two layers
//!
//! Everything in this file is what writing *into* a chart needs: one key's block, one key's
//! template line, one write-off group. [`chart`] composes them into the whole files a new chart
//! starts with (`values.yaml` from the image block to the chassis, the helper file, the
//! declaration, the round-trip enrolment and the fixtures), so each of those is a composition of
//! the rules here rather than a second copy of them.

pub mod chart;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use regex::Regex;
use serde_json::Value as Json;

use crate::error::Error;
use crate::text::quoted;
use crate::union::Merged;

use super::markers::{Class, MARKER};
use super::shapes::{Added, expected, render, scalar};
use super::testgen::satisfying;

/// The column a generated comment wraps at, matching the charts it is written into.
pub const WIDTH: usize = 96;

// ------------------------------------------------------------------------------------------
// Naming
// ------------------------------------------------------------------------------------------

/// Helm's limit on a release name. `helm install` enforces it, and the generated declaration
/// selects the release by the chart's name, so a chart named past it cannot be installed.
const MAX_CHART_NAME: usize = 53;

/// Refuses a chart name Helm, Kubernetes or the generated files would reject later.
///
/// At most 53 lower-case alphanumerics and hyphens, starting and ending with an alphanumeric: the
/// rule a release name and a DNS label share, with Helm's release-name limit, the tighter of the
/// two. Checked before anything is planned rather than left to `helm lint` or `helm install`,
/// because every generated file embeds the name: the helper names, the release selector in the
/// declaration, the container the declaration names. A name that fails there fails after all of it
/// is on disk.
///
/// # Errors
/// [`Error::Invalid`] naming the rule.
pub fn check_chart_name(name: &str) -> Result<(), Error> {
    let alphanumeric = |held: char| held.is_ascii_lowercase() || held.is_ascii_digit();
    let valid = name.len() <= MAX_CHART_NAME
        && name.starts_with(alphanumeric)
        && name.ends_with(alphanumeric)
        && name.chars().all(|held| alphanumeric(held) || held == '-');
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{} is not a valid chart name: at most {MAX_CHART_NAME} lower-case alphanumerics and \
             hyphens, starting and ending with an alphanumeric",
            quoted(name)
        )))
    }
}

/// The dialect prefix as the library's environment partial wants it: without the trailing
/// separator.
///
/// A contract states `NETCUP_OFFER_BOT_` because that is what it prepends to a variable name; the
/// partial appends the separator itself, so passing the contract's spelling through would produce
/// `NETCUP_OFFER_BOT__CONFIG`.
pub fn env_prefix(prefix: &str) -> &str {
    prefix.trim_end_matches('_')
}

/// `check_interval_secs` as `checkIntervalSecs`: one contract path segment as a chart value.
///
/// The convention every chart that maps values onto a contract already follows:
/// `feed.check_interval_secs` is `feed.checkIntervalSecs`, `discord.webhook_url` is
/// `discord.webhookUrl`, and a segment with no underscore (`metrics.ip`) is left exactly as it is.
pub fn camel(segment: &str) -> String {
    let mut parts = segment.split('_');
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            out.extend(first.to_uppercase());
            out.push_str(characters.as_str());
        }
    }
    out
}

/// The dotted chart value that carries one contract key.
pub fn values_path_for(path: &str) -> String {
    path.split('.').map(camel).collect::<Vec<_>>().join(".")
}

// ------------------------------------------------------------------------------------------
// Planning
// ------------------------------------------------------------------------------------------

/// Where one contract key ends up in a chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// A chart value, projected into the configuration document.
    Projected,
    /// A chart value, delivered as a file in the secrets directory and never into the document.
    SecretFile,
    /// No chart value at all, and an `unbound` entry saying why.
    WrittenOff,
}

/// One contract key handed to the planner.
///
/// `documents` names which of a multi-document chart's documents declare the key, when the caller
/// resolved that: a generated `derivedConfig` block for such a chart needs it to know whether its
/// root wants a `$service` guard, and which one. [`None`] for a single-document chart's key, and for
/// any caller to whom the question does not arise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The merged contract entry.
    pub entry: Merged,
    /// The documents declaring it, sorted, when that was resolved.
    pub documents: Option<Vec<String>>,
}

impl From<Merged> for Key {
    fn from(entry: Merged) -> Self {
        Self {
            entry,
            documents: None,
        }
    }
}

/// One contract key, and what the plan does with it.
///
/// `values_path` is set for everything but a write-off: a credential still gets a chart value, it
/// simply carries into the Secret rather than into the document. `reason` is set for a write-off
/// alone, and is the sentence its `unbound` entry carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// The contract key.
    pub key: Merged,
    /// The documents declaring it; see [`Key::documents`].
    pub documents: Option<Vec<String>>,
    /// Where it goes.
    pub destination: Destination,
    /// The chart value carrying it, empty for a write-off.
    pub values_path: String,
    /// Why it is written off, empty otherwise.
    pub reason: String,
}

impl Placement {
    /// The contract path.
    pub fn path(&self) -> &str {
        &self.key.name
    }

    /// The marker class that binds a value to this key.
    pub fn marker_class(&self) -> Class {
        if self.key.text("text_form") == Some("structured") {
            Class::Structured
        } else {
            Class::Projection
        }
    }

    /// Whether the image may run without this key, which is what an `optional` marker states.
    pub fn optional(&self) -> bool {
        !truthy(self.key.fields.get("required"))
    }
}

/// Every key handed to [`plan_keys`], sorted into where it goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Keys that become a chart value projected into the document.
    pub projected: Vec<Placement>,
    /// Credentials that become a chart value delivered as a file.
    pub credentials: Vec<Placement>,
    /// Keys no chart value surfaces, each with its reason.
    pub written_off: Vec<Placement>,
}

impl Plan {
    /// The keys that get a chart value, projected first and credentials after, each in path order.
    pub fn surfaced(&self) -> impl Iterator<Item = &Placement> {
        self.projected.iter().chain(&self.credentials)
    }
}

/// The reason a reserved key is written off.
const RESERVED: &str = "Reserved by the loader: the image sets it itself, so a chart value \
                        feeding it would be overwritten at boot and would read to an operator as \
                        a setting that does nothing.";

/// Sorts keys by the table in this module's documentation, in contract path order.
///
/// # Errors
/// [`Error::Invalid`] when a credential's `text_form` is one this build does not implement, so
/// whether a file could supply it is unknown, which is a question the plan must not guess at.
pub fn plan_keys(keys: impl IntoIterator<Item = Key>) -> Result<Plan, Error> {
    let mut keys: Vec<Key> = keys.into_iter().collect();
    keys.sort_by(|left, right| left.entry.name.cmp(&right.entry.name));

    let mut plan = Plan::default();
    for Key { entry, documents } in keys {
        let values_path = values_path_for(&entry.name);
        let placement = |destination, values_path: String, reason: String| Placement {
            key: entry.clone(),
            documents: documents.clone(),
            destination,
            values_path,
            reason,
        };

        if truthy(entry.fields.get("reserved")) {
            plan.written_off.push(placement(
                Destination::WrittenOff,
                String::new(),
                RESERVED.to_owned(),
            ));
        } else if truthy(entry.fields.get("secret")) {
            if entry.entry().file_supplyable()? {
                plan.credentials.push(placement(
                    Destination::SecretFile,
                    values_path,
                    String::new(),
                ));
            } else {
                let reason = format!(
                    "A credential whose `text_form` is {}, so no file can supply it — only the \
                     configuration document or the environment can, and a bearer credential in a \
                     ConfigMap is readable by anything that can read the namespace. Surfacing it \
                     would mean choosing one of those two, which is a decision for whoever knows \
                     what this credential is worth.",
                    super::shown(entry.fields.get("text_form"))
                );
                plan.written_off
                    .push(placement(Destination::WrittenOff, String::new(), reason));
            }
        } else {
            plan.projected.push(placement(
                Destination::Projected,
                values_path,
                String::new(),
            ));
        }
    }
    Ok(plan)
}

/// Whether a field holds something, as the planner's rules read a flag.
///
/// The contract publishes these as booleans; a value of another type is read as present and
/// non-empty rather than refused, since nothing downstream distinguishes the two.
fn truthy(value: Option<&Json>) -> bool {
    match value {
        None | Some(Json::Null) => false,
        Some(Json::Bool(held)) => *held,
        Some(Json::Number(number)) => number.as_f64().is_some_and(|held| held != 0.0),
        Some(Json::String(text)) => !text.is_empty(),
        Some(Json::Array(items)) => !items.is_empty(),
        Some(Json::Object(fields)) => !fields.is_empty(),
    }
}

// ------------------------------------------------------------------------------------------
// One value's block in `values.yaml`
// ------------------------------------------------------------------------------------------

/// Comment lines, wrapped at `width`, each carrying `prefix`. Paragraph breaks are collapsed.
///
/// A `docs` block is the producer's own prose and routinely runs to several paragraphs; a values
/// description is one paragraph by the README generator's convention, so the paragraphs are joined
/// rather than reproduced. The full text stays available through `explain`.
///
/// Width is counted in characters. A single word longer than the width is never broken: a line
/// over the limit is uglier than a URL split in two.
pub fn wrap(text: &str, prefix: &str, width: usize) -> Vec<String> {
    let bare = prefix.trim();
    let mut lines = Vec::new();
    let mut current = prefix.trim_end().to_owned();
    for word in text.split_whitespace() {
        let started = current.trim() != bare;
        let candidate = if started {
            format!("{current} {word}")
        } else {
            format!("{prefix}{word}")
        };
        if candidate.chars().count() > width && started {
            lines.push(std::mem::replace(&mut current, format!("{prefix}{word}")));
        } else {
            current = candidate;
        }
    }
    if current.trim() != bare {
        lines.push(current);
    }
    lines
}

/// The first paragraph of a key's documentation, or a sentence naming the key.
///
/// Never empty: the README generator renders `# --` descriptions into the published values table,
/// and a blank row there reads as a value nobody thought about.
pub fn summary_of(key: &Merged) -> String {
    match key.text("docs") {
        Some(docs) if !docs.trim().is_empty() => docs
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        _ => format!("The `{}` setting.", key.name),
    }
}

/// The `@schema` lines for one contract key, from its constraint.
///
/// A thin call into [`super::shapes`], where the rules and the measurements behind them live. Two
/// generators write these blocks (this one for a value being created, the shapes writer for one
/// that exists), and a value created with a block the shapes gate would rewrite fails that gate the
/// moment it is written.
pub fn schema_lines(key: &Merged, indent: &str) -> Vec<String> {
    render(
        &expected(
            key.fields.get("constraint"),
            !truthy(key.fields.get("required")),
            key.text("text_form") == Some("structured"),
        ),
        indent.len(),
        &Added::default(),
    )
}

/// The value written for one key.
///
/// Three cases, and only the first is a real answer:
///
/// **The image publishes a default.** `default_value` is that default already parsed, which is
/// exactly what a values file wants. Written through, so the chart's default *is* the binary's and
/// the two cannot drift.
///
/// **Optional, with no default.** `null`, and the projection wraps the key in a guard, so nothing
/// is written into the document and the binary falls back to its compiled default. That is what
/// "unset" has to mean; writing an empty string instead would *supply* the key. [`unset_line`]
/// writes the description line that says so.
///
/// **Required, with no default.** The image demands a value and names none, so one has to be
/// invented and every invented value is wrong. What it must not be is *invalid*: a required integer
/// bounded at `minimum: 1` cannot be written as `0`, or the chart fails the document gate on its
/// first run for a reason that reads like a bug in the gate. So the value comes from
/// [`satisfying`], the constraint-satisfying search the round-trip probes use, and every key that
/// needed one is reported by [`invented`] as something to replace with a value somebody chose.
pub fn default_for(key: &Merged) -> Json {
    if let Some(published) = key
        .fields
        .get("default_value")
        .filter(|held| !held.is_null())
    {
        return published.clone();
    }
    if !truthy(key.fields.get("required")) {
        return Json::Null;
    }

    let declared = first_type(key);
    if key.text("text_form") == Some("structured") {
        // Empty, in the shape the constraint names. An empty table where the constraint says
        // `array` is a default the chart's own generated values schema rejects.
        return if declared.as_deref() == Some("array") {
            Json::Array(Vec::new())
        } else {
            Json::Object(serde_json::Map::new())
        };
    }

    if let Some(candidate) = satisfying(key) {
        return candidate;
    }
    match declared.as_deref() {
        Some("integer" | "number") => Json::from(0),
        Some("boolean") => Json::Bool(false),
        Some("array") => Json::Array(Vec::new()),
        Some("object") => Json::Object(serde_json::Map::new()),
        _ => Json::String(String::new()),
    }
}

/// The first type a key's constraint names, spelled as text.
fn first_type(key: &Merged) -> Option<String> {
    let declared = key.fields.get("constraint")?.get("type")?;
    let declared = match declared {
        Json::Array(names) => names.first()?,
        other => other,
    };
    Some(
        declared
            .as_str()
            .map_or_else(|| declared.to_string(), str::to_owned),
    )
}

/// The surfaced keys whose value was chosen here rather than read from the contract.
///
/// Reported rather than hidden. A required key with no published default is the one place this
/// module has to guess, and a guess nobody was told about is the same defect as a missing key.
pub fn invented(plan: &Plan) -> Vec<&Placement> {
    plan.projected
        .iter()
        .filter(|item| {
            !item.optional()
                && item
                    .key
                    .fields
                    .get("default_value")
                    .is_none_or(Json::is_null)
        })
        .collect()
}

/// One default as it is written into `values.yaml`.
pub fn values_scalar(value: &Json) -> String {
    scalar(value)
}

/// The one line that binds a chart value to a contract key.
///
/// Written inside the `@schema` block, which is why it reads as a comment within a comment: the
/// text between the delimiters is the schema and is parsed as YAML, so a YAML comment there is
/// discarded by the only parser that reads it. [`super::markers`] records why that is the one
/// position both generators ignore.
pub fn marker_line(placement: &Placement, indent: &str) -> String {
    let suffix = if placement.optional() {
        " optional"
    } else {
        ""
    };
    format!(
        "{indent}# # {MARKER} {} {}{suffix}",
        placement.marker_class().label(),
        placement.path()
    )
}

/// One README-generator description: `# --` on the first line, `#` on the rest.
///
/// The marker belongs to the description as a whole and is read that way: repeating it on every
/// wrapped line puts a literal `--` in the middle of the rendered sentence.
pub fn described(text: &str, indent: &str) -> Vec<String> {
    let wrapped = wrap(text, "", WIDTH.saturating_sub(indent.len() + 5));
    wrapped
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{indent}# -- {line}")
            } else {
                format!("{indent}# {line}")
            }
        })
        .collect()
}

/// The description of one contract key's value, naming the key and, for a credential, its file.
///
/// # Errors
/// As [`secrets_file_name`], for a credential.
pub fn description_lines(placement: &Placement, indent: &str) -> Result<Vec<String>, Error> {
    let summary = summary_of(&placement.key);
    let mut text = format!(
        "{} (`{}`).",
        summary.trim_end_matches('.'),
        placement.path()
    );
    if placement.destination == Destination::SecretFile {
        let _ = write!(
            text,
            " Delivered as the secrets-directory file `{}`.",
            secrets_file_name(placement)?
        );
    }
    Ok(described(&text, indent))
}

/// The line saying what a value written as `null` leaves in effect, when it is one.
///
/// Written for an optional key bound by a marker and left unset, which is the one case where the
/// value on the page says nothing about what the image will do. The shapes writer owns the line
/// from then on and rewrites it when the contract's default moves; see [`super::unset`].
pub fn unset_line(placement: &Placement, value: &Json, indent: &str) -> Option<String> {
    if placement.destination != Destination::Projected || !placement.optional() || !value.is_null()
    {
        return None;
    }
    let published = super::unset::Published::of(None, &placement.key.fields);
    Some(super::unset::line(
        &super::unset::sentence(&[published]),
        indent,
    ))
}

/// One contract key as the `values.yaml` block that feeds it.
///
/// No `@config-shape` marker is written, and none is needed: a block belonging to a value that
/// carries a `# @config` marker is generated from the contract by definition, so it is owned by the
/// shapes writer from the moment it exists.
///
/// A credential gets no `# @config` marker, and the two facts that make that right belong together:
/// every marker class names a way a value reaches the *configuration document*, and a credential
/// delivered as a file never reaches it. The marker gate says the same from the other side: it
/// refuses a key both bound by a marker and written off in `unbound`, which a marker here would
/// make every credential.
///
/// # Errors
/// As [`secrets_file_name`], for a credential.
pub fn value_block(placement: &Placement, indent: &str) -> Result<Vec<String>, Error> {
    let leaf = placement.values_path.rsplit('.').next().unwrap_or_default();
    let secret = placement.destination == Destination::SecretFile;

    let mut lines = vec![format!("{indent}# @schema")];
    if !secret {
        lines.push(marker_line(placement, indent));
    }
    lines.extend(schema_lines(&placement.key, indent));
    lines.push(format!("{indent}# @schema"));
    lines.extend(description_lines(placement, indent)?);

    let value = if secret {
        Json::String(String::new())
    } else {
        default_for(&placement.key)
    };
    if let Some(annotation) = unset_line(placement, &value, indent) {
        lines.push(annotation);
    }
    lines.push(format!("{indent}{leaf}: {}", values_scalar(&value)));
    Ok(lines)
}

// ------------------------------------------------------------------------------------------
// Grouping values into blocks
// ------------------------------------------------------------------------------------------

/// One level of the values tree the placements make.
pub type Tree = BTreeMap<String, Node>;

/// A leaf value, or a grouping block holding more of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A value, from the placement that produced it.
    Leaf(Placement),
    /// A grouping block.
    Branch(Tree),
}

/// The placements' chart values as a nested tree, rooted `depth` segments down.
///
/// Rooted at zero for a whole chart. A caller inserting under a block that already exists roots the
/// tree at that block's depth instead, and hands the same depth to [`render_tree`], which reads a
/// grouping block's contract prefix off the depth it was given, so the two have to agree.
///
/// Sorted at every level by the map that holds it, so what is rendered is a property of the keys
/// rather than of the order they arrived in.
///
/// # Errors
/// [`Error::Invalid`] when two placements cannot both exist: one value is a leaf on the way to
/// another, or two keys spell one chart value. Either is a mapping that has to be written by hand,
/// and rendering whichever happened to win would drop the other in silence.
pub fn tree_at(placements: &[Placement], depth: usize) -> Result<Tree, Error> {
    let mut tree = Tree::new();
    for placement in placements {
        let segments: Vec<&str> = placement.values_path.split('.').skip(depth).collect();
        let Some((leaf, branches)) = segments.split_last() else {
            continue;
        };
        let mut node = &mut tree;
        for segment in branches {
            let entry = node
                .entry((*segment).to_owned())
                .or_insert_with(|| Node::Branch(Tree::new()));
            node = match entry {
                Node::Branch(children) => children,
                Node::Leaf(_) => {
                    return Err(Error::Invalid(format!(
                        "the contract key {} needs the chart value {}, but {} is already a \
                         leaf value on the way there; the two cannot both exist and this \
                         mapping has to be written by hand",
                        quoted(placement.path()),
                        quoted(&placement.values_path),
                        quoted(segment)
                    )));
                }
            };
        }
        if node.contains_key(*leaf) {
            return Err(Error::Invalid(format!(
                "the contract key {} needs the chart value {}, which another key already needs; \
                 one chart value cannot feed two keys, so this mapping has to be written by hand",
                quoted(placement.path()),
                quoted(&placement.values_path)
            )));
        }
        node.insert((*leaf).to_owned(), Node::Leaf(placement.clone()));
    }
    Ok(tree)
}

/// The `# --` sentence of one grouping block, given the block and the depth it sits at.
///
/// A parameter rather than a rule, because the callers want different things from a sentence
/// nobody can derive well; see [`render_tree`].
pub type Describe<'a> = &'a dyn Fn(&Tree, usize) -> Result<String, Error>;

/// The values blocks for one level of the tree, and everything under it.
///
/// `describe` writes a grouping block's `# --` line. A contract describes keys, not the blocks a
/// chart groups them into, so there is no honest sentence to derive: a chart being scaffolded gets
/// [`branch_placeholder`]'s `TODO`, read by an author looking at all of them at once, and a block
/// adopted into a chart that already exists gets a plain derived sentence instead.
///
/// # Errors
/// As [`value_block`] and as `describe`.
pub fn render_tree(
    tree: &Tree,
    indent: &str,
    depth: usize,
    describe: Describe<'_>,
) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    for (name, node) in tree {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        match node {
            Node::Leaf(placement) => lines.extend(value_block(placement, indent)?),
            Node::Branch(children) => {
                lines.push(format!("{indent}# @schema"));
                lines.push(format!("{indent}# additionalProperties: true"));
                lines.push(format!("{indent}# @schema"));
                lines.extend(described(&describe(children, depth + 1)?, indent));
                lines.push(format!("{indent}{name}:"));
                lines.extend(render_tree(
                    children,
                    &format!("{indent}  "),
                    depth + 1,
                    describe,
                )?);
            }
        }
    }
    Ok(lines)
}

/// The placeholder a grouping block is scaffolded with, naming the prefix it holds.
///
/// A `TODO` in the spelling a chart's other placeholders use, because a value with no description
/// fails the values-documentation gate and an invented sentence would be published unread.
///
/// # Errors
/// As [`branch_prefix`].
pub fn branch_placeholder(node: &Tree, depth: usize) -> Result<String, Error> {
    Ok(format!(
        "TODO: what the `{}` settings have in common, in one sentence.",
        branch_prefix(node, depth)?
    ))
}

/// The contract path prefix one grouping block holds.
///
/// Read off a key underneath it rather than off the values path: [`values_path_for`] camel-cases
/// each segment, so the two have the same segment count and the contract spelling (which is what a
/// description should name) is only recoverable from a key.
///
/// # Errors
/// [`Error::Invalid`] for a branch holding no key, which [`tree_at`] never builds.
pub fn branch_prefix(node: &Tree, depth: usize) -> Result<String, Error> {
    let placement = any_placement(node).ok_or_else(|| {
        Error::Invalid("a values branch was built with no key underneath it".to_owned())
    })?;
    Ok(placement
        .path()
        .split('.')
        .take(depth)
        .collect::<Vec<_>>()
        .join("."))
}

/// The first placement under a branch, in tree order. They all share its prefix.
fn any_placement(node: &Tree) -> Option<&Placement> {
    node.values().find_map(|child| match child {
        Node::Leaf(placement) => Some(placement),
        Node::Branch(children) => any_placement(children),
    })
}

/// The grouping blocks a tree creates, as values paths under `prefix`, in tree order.
pub fn branches(tree: &Tree, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (name, node) in tree {
        if let Node::Branch(children) = node {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            found.push(path.clone());
            found.extend(branches(children, &path));
        }
    }
    found
}

// ------------------------------------------------------------------------------------------
// The template lines: projecting values into the document, and credentials into the Secret
// ------------------------------------------------------------------------------------------

/// The `$service` value one document's `derivedConfig` branch is gated on, or [`None`].
///
/// Read from the document's own `images[].values`: a document naming `services.controlPlane.image`
/// is the `controlPlane` branch of a chart whose `derivedConfig` is included once per
/// `.Values.services` key and compares `$service` against that key. Nothing in the contract states
/// this convention (it is read off a field recorded for image resolution), so a document whose
/// image path does not match it resolves to [`None`] rather than a guess, and its keys are left for
/// a person to place.
pub fn service_key_for(image_values_path: &str) -> Option<String> {
    service_image()
        .captures(image_values_path)
        .map(|found| found[1].to_owned())
}

/// An image path of the `services.<name>.image` shape, compiled once.
fn service_image() -> &'static Regex {
    static HELD: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    HELD.get_or_init(|| {
        Regex::new(r"^services\.([A-Za-z][A-Za-z0-9]*)\.image$")
            .expect("a literal pattern compiles")
    })
}

/// The placements' contract paths as a nested tree, which is the shape the document has.
fn document_tree(placements: &[Placement]) -> Tree {
    let mut tree = Tree::new();
    for placement in placements {
        let segments: Vec<&str> = placement.path().split('.').collect();
        let Some((leaf, branches)) = segments.split_last() else {
            continue;
        };
        let mut node = &mut tree;
        for segment in branches {
            let entry = node
                .entry((*segment).to_owned())
                .or_insert_with(|| Node::Branch(Tree::new()));
            // Unreachable from a plan whose values tree was built: [`tree_at`] refuses a key that
            // is a leaf on the way to another, and the two trees have the same segments. Read as
            // the table rather than refused so that this stays a total function of its input.
            if let Node::Leaf(_) = entry {
                *entry = Node::Branch(Tree::new());
            }
            let Node::Branch(children) = entry else {
                unreachable!("the entry was made a branch above");
            };
            node = children;
        }
        node.insert((*leaf).to_owned(), Node::Leaf(placement.clone()));
    }
    tree
}

/// The YAML the `derivedConfig` helper emits for a tree of placements.
///
/// An optional setting must not reach the document at all when it is unset, and that distinction
/// is the whole reason the helper is generated rather than left to a per-chart hand. Two guards say
/// "unset", because one of them is wrong half the time; see [`Unset`].
///
/// **A table is guarded too, on any of its contents being set.** A table written unconditionally
/// with every child guarded renders as `audit:` and nothing under it when nothing is set, which
/// parses as `audit: null`, or, one level down, as an empty mapping that leaves a bare `[security]`
/// header in TOML. A chart that merges this layer over the operator's own `config` tree (the
/// ordinary layering: `config` under the derived values under a service's own overrides) then
/// replaces the operator's whole table with that null. So a table whose contents can all be absent
/// is wrapped in the disjunction of its leaves' own guards, built from the same [`Unset`] the leaf
/// guards are, so the two cannot disagree. A table holding a required leaf always has content and
/// is written bare.
///
/// The guard repeats the leaf guards rather than pruning the rendered map afterwards, because a
/// pruning pass is a define the chart would have to carry, and an adoption cannot assume one exists
/// or add one without rewriting how the chart merges its layers. A guard is local: what renders is
/// readable off the lines themselves.
///
/// `values_prefix` is `.Values` where the define is included with `.` set to the root context, and
/// `$ctx.Values` where it is included with `(dict "ctx" $ctx "service" $service)`, where `.Values`
/// is not merely wrong but a nil pointer, because the dict has no `Values` field.
fn derived_lines(tree: &Tree, indent: &str, values_prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, node) in tree {
        let placement = match node {
            Node::Branch(children) => {
                let guard = presence(children, values_prefix);
                if let Some(condition) = &guard {
                    lines.push(format!("{indent}{{{{- if {condition} }}}}"));
                }
                lines.push(format!("{indent}{name}:"));
                lines.extend(derived_lines(
                    children,
                    &format!("{indent}  "),
                    values_prefix,
                ));
                if guard.is_some() {
                    lines.push(format!("{indent}{{{{- end }}}}"));
                }
                continue;
            }
            Node::Leaf(placement) => placement,
        };

        let reference = reference_to(placement, values_prefix);
        let structured = placement.key.text("text_form") == Some("structured");
        let nested = indent.len() + 2;

        match (Unset::of(placement), structured) {
            (Unset::Nil, _) => {
                lines.push(format!(
                    "{indent}{{{{- if not (kindIs \"invalid\" {reference}) }}}}"
                ));
                lines.push(format!("{indent}{name}: {{{{ {reference} }}}}"));
                lines.push(format!("{indent}{{{{- end }}}}"));
            }
            (Unset::Falsy, true) => {
                lines.push(format!("{indent}{{{{- with {reference} }}}}"));
                lines.push(format!("{indent}{name}:"));
                lines.push(format!("{indent}  {{{{- toYaml . | nindent {nested} }}}}"));
                lines.push(format!("{indent}{{{{- end }}}}"));
            }
            (Unset::Falsy, false) => {
                lines.push(format!("{indent}{{{{- with {reference} }}}}"));
                lines.push(format!("{indent}{name}: {{{{ . | quote }}}}"));
                lines.push(format!("{indent}{{{{- end }}}}"));
            }
            (Unset::Never, true) => {
                lines.push(format!("{indent}{name}:"));
                lines.push(format!(
                    "{indent}  {{{{- toYaml {reference} | nindent {nested} }}}}"
                ));
            }
            (Unset::Never, false) if bare(placement) => {
                lines.push(format!("{indent}{name}: {{{{ {reference} }}}}"));
            }
            (Unset::Never, false) => {
                lines.push(format!("{indent}{name}: {{{{ {reference} | quote }}}}"));
            }
        }
    }
    lines
}

/// When a projected value counts as unset, and so is left out of the document.
///
/// `with` treats every falsy value as absent, which is right for a string or a structured value and
/// wrong for a number or a boolean, where `0` and `false` are settings in their own right. Those
/// are tested with `kindIs "invalid"` instead, a nil test that only catches the value actually
/// being absent. Getting this backwards silently drops every deliberate zero an operator writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unset {
    /// Required: always written.
    Never,
    /// An optional number or boolean: unset only when it is null.
    Nil,
    /// Any other optional value: unset when it is falsy, as `with` reads it.
    Falsy,
}

impl Unset {
    fn of(placement: &Placement) -> Self {
        if !placement.optional() {
            Self::Never
        } else if bare(placement) {
            Self::Nil
        } else {
            Self::Falsy
        }
    }

    /// The template condition that holds when the value at `reference` is set; [`None`] for a
    /// value that is never unset.
    fn set(self, reference: &str) -> Option<String> {
        match self {
            Self::Never => None,
            Self::Nil => Some(format!("not (kindIs \"invalid\" {reference})")),
            Self::Falsy => Some(reference.to_owned()),
        }
    }
}

/// Whether a key's value is written into the document bare rather than quoted: a number or a
/// boolean.
fn bare(placement: &Placement) -> bool {
    constraint_types(&placement.key)
        .iter()
        .any(|name| matches!(name.as_str(), "integer" | "number" | "boolean"))
}

/// The template reference to one placement's chart value.
fn reference_to(placement: &Placement, values_prefix: &str) -> String {
    format!("{values_prefix}.{}", placement.values_path)
}

/// The condition under which a table renders anything, or [`None`] when it always does.
///
/// One leaf's own guard where the table holds one, and `or` over all of them otherwise, each
/// parenthesised where it is more than a reference. A table with a required leaf anywhere beneath
/// it always has content, so it needs no guard at all.
fn presence(tree: &Tree, values_prefix: &str) -> Option<String> {
    let mut terms = Vec::new();
    if !collect_presence(tree, values_prefix, &mut terms) {
        return None;
    }
    if let [only] = terms.as_slice() {
        return Some(only.clone());
    }
    let parenthesised: Vec<String> = terms
        .iter()
        .map(|term| {
            if term.contains(' ') {
                format!("({term})")
            } else {
                term.clone()
            }
        })
        .collect();
    Some(format!("or {}", parenthesised.join(" ")))
}

/// Every leaf guard under a table, in tree order; `false` as soon as a leaf needs none.
fn collect_presence(tree: &Tree, values_prefix: &str, terms: &mut Vec<String>) -> bool {
    for node in tree.values() {
        match node {
            Node::Branch(children) => {
                if !collect_presence(children, values_prefix, terms) {
                    return false;
                }
            }
            Node::Leaf(placement) => {
                match Unset::of(placement).set(&reference_to(placement, values_prefix)) {
                    Some(term) => terms.push(term),
                    None => return false,
                }
            }
        }
    }
    true
}

/// The type names a key's constraint declares, as a list whether it was published as one or not.
fn constraint_types(key: &Merged) -> Vec<String> {
    match key
        .fields
        .get("constraint")
        .and_then(|held| held.get("type"))
    {
        Some(Json::Array(names)) => names
            .iter()
            .map(|name| {
                name.as_str()
                    .map_or_else(|| name.to_string(), str::to_owned)
            })
            .collect(),
        Some(Json::String(name)) => vec![name.clone()],
        _ => Vec::new(),
    }
}

/// Some placements, and the `$service` values they are rendered for.
///
/// A multi-document chart's `derivedConfig` is included once per service and gated by hand on
/// `$service`, and a run is one such gate: the placements in it render only where `$service` is
/// one of `services`. A run with no services renders for every service, unguarded, which is also
/// what every run of a one-document chart is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The `$service` values the run is gated on; empty for every service.
    pub services: Vec<String>,
    /// What renders there.
    pub placements: Vec<Placement>,
}

impl Run {
    /// A run rendered for every service, unguarded.
    pub fn everywhere(placements: Vec<Placement>) -> Self {
        Self {
            services: Vec::new(),
            placements,
        }
    }
}

/// The `derivedConfig` lines for some runs, at a caller's indent, each inside its `$service` gate.
///
/// The runs are rendered in the order given, each as one document tree. Two runs naming the same
/// root must never both render for one service, or `fromYaml` keeps the second and drops the
/// first in silence; ordering and partitioning them is the caller's job.
///
/// `values_prefix` applies to every run alike, gated or not: it is a fact about how the *define* is
/// included, not about any one root.
pub fn derived_for(runs: &[Run], indent: &str, values_prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for run in runs {
        let body = derived_lines(&document_tree(&run.placements), indent, values_prefix);
        if run.services.is_empty() {
            lines.extend(body);
        } else {
            lines.extend(guarded(body, &run.services, indent));
        }
    }
    lines
}

/// A root's rendered lines inside the `$service` guard a multi-document chart writes by hand.
///
/// One document: `eq $service "x"`. Several: `or (eq $service "x") (eq $service "y")`.
fn guarded(body: Vec<String>, service_keys: &[String], indent: &str) -> Vec<String> {
    let condition = if let [only] = service_keys {
        format!("eq $service \"{only}\"")
    } else {
        let checks: Vec<String> = service_keys
            .iter()
            .map(|name| format!("(eq $service \"{name}\")"))
            .collect();
        format!("or {}", checks.join(" "))
    };
    let mut lines = vec![format!("{indent}{{{{- if {condition} }}}}")];
    lines.extend(body);
    lines.push(format!("{indent}{{{{- end }}}}"));
    lines
}

/// The `secretData` lines for some credentials, keyed by the file the loader reads each from.
///
/// # Errors
/// As [`secrets_file_name`].
pub fn credential_entries_for(placements: &[Placement]) -> Result<Vec<String>, Error> {
    placements
        .iter()
        .map(|item| {
            Ok(format!(
                "{}: {{{{ .Values.{} | quote }}}}",
                secrets_file_name(item)?,
                item.values_path
            ))
        })
        .collect()
}

/// The secrets-directory file name one credential is read from.
///
/// Taken from the contract rather than derived from the path: `secrets_file` is what the producer
/// published, and a chart that computed its own spelling would be right until the day the producer
/// changed a separator.
///
/// # Errors
/// [`Error::Invalid`] when the key declares no `secrets_file`, so there is no name to deliver it
/// under.
pub fn secrets_file_name(placement: &Placement) -> Result<String, Error> {
    match placement
        .key
        .fields
        .get("secrets_file")
        .filter(|held| truthy(Some(held)))
    {
        Some(Json::String(name)) => Ok(name.clone()),
        Some(other) => Ok(other.to_string()),
        None => Err(Error::Invalid(format!(
            "the contract key {} is `secret: true` and declares no `secrets_file`, so there is no \
             file name to deliver it under",
            quoted(placement.path())
        ))),
    }
}

// ------------------------------------------------------------------------------------------
// The declaration's write-offs
// ------------------------------------------------------------------------------------------

/// Every unsurfaced key of a plan, grouped by the reason it is unsurfaced, as `(keys, reason)`.
///
/// Grouped rather than listed one entry per key: a chart with many reserved keys would otherwise
/// turn a handful of sentences into several hundred lines. The keys are still written out one by
/// one inside the group, so an image release that adds one turns the gate red until somebody puts
/// it in a list on purpose.
///
/// A credential is in here too, though it has a value: it travels the secrets directory rather than
/// the document the markers describe, so no marker binds it, and the gate asks for the reason.
///
/// # Errors
/// As [`secrets_file_name`].
pub fn write_off_groups(plan: &Plan) -> Result<Vec<(Vec<String>, String)>, Error> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let mut add =
        |reason: String, path: &str| match groups.iter_mut().find(|(held, _)| *held == reason) {
            Some((_, paths)) => paths.push(path.to_owned()),
            None => groups.push((reason, vec![path.to_owned()])),
        };

    for item in &plan.credentials {
        add(
            format!(
                "Delivered as a file in the secrets directory — from the Secret this chart \
                 renders, or from `existingSecret` under the file name `{}` — and never written \
                 into the configuration document, because a credential in a ConfigMap is readable \
                 by anything that can read the namespace. `{}` is the value that carries it, so \
                 the binding does exist; it simply does not run through the document the markers \
                 describe. `just check-config-secrets` is what reconciles that channel, and it \
                 does it from the rendered manifests rather than from a comment.",
                secrets_file_name(item)?,
                item.values_path
            ),
            item.path(),
        );
    }
    for item in &plan.written_off {
        add(item.reason.clone(), item.path());
    }

    Ok(groups
        .into_iter()
        .map(|(reason, mut paths)| {
            paths.sort();
            (paths, reason)
        })
        .collect())
}

/// One `unbound` group as declaration lines, in the layout every declaration uses.
pub fn write_off_lines(paths: &[String], reason: &str) -> Vec<String> {
    let mut lines = vec!["  - keys:".to_owned()];
    lines.extend(paths.iter().map(|path| format!("      - {path}")));
    lines.push("    reason: >-".to_owned());
    lines.extend(wrap(reason, "      ", WIDTH));
    lines
}

#[cfg(test)]
mod tests;
