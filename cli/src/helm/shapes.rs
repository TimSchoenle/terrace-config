//! Deriving a chart value's `@schema` block from the element schema its image publishes.
//!
//! A contract states a key's type as a JSON Schema object, and until `schema_version: 2` that
//! object was flat: no way to say what one *element* of a container held. A `Vec<RouteConfig>`
//! arrived as `{"type": "array"}`, which was all the producer could say, and a chart that wanted an
//! operator's editor to catch a misspelt field before the service did had to transcribe the struct
//! by hand — a copy of a fact somebody else owns, and a copy nothing regenerates is the one that
//! goes stale.
//!
//! `schema_version: 2` publishes that shape. This is what turns it into the block the chart ships.
//!
//! # Generated is the default, and the markers are the departures from it
//!
//! **Every value carrying a `projection`, `structured` or `external` binding has its block written
//! from the contract.** Nothing enrols it: the marker naming the key is the enrolment, because a
//! value that says which key it feeds has said everything a generator needs, and a second marker
//! repeating "yes, really" is one more thing to forget.
//!
//! It was the other way round once, and the cost was measured rather than argued: nine values were
//! enrolled and 185 were not, while 114 of the 185 carried a block byte-identical to the one their
//! contract describes — hand copies that happened to still be right, with nothing holding them
//! there. The other 71 differed, and every one of those differences was invisible.
//!
//! Three markers remain, and each says the *opposite* of what enrolment used to: not "derive this",
//! but "here is where the chart departs from what was derived".
//!
//! ```text
//! # @config-shape <values-path> handwritten <release> <source>
//! # @config-shape-except <values-path> <sub-path> <reason>
//! # @config-shape-narrow <values-path> <sub-path> <reason>
//! ```
//!
//! Written as plain comments *above* the block and separated from it by nothing but comments — the
//! placement measured to be invisible to both generators, and the reason a `@config` marker cannot
//! use it: inside the delimiters the text *is* the schema.
//!
//! `handwritten` is the escape hatch generation is refused for, and two things earn it: a container
//! whose element the producer has not described, where a block built from `{"type": "array"}` alone
//! would type-check nothing, and a key the producer publishes no constraint for at all, where the
//! derived block would name all six JSON types and check nothing.

mod divergences;
mod expected;
mod parse;
mod render;
mod resolve;

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::error::Error;

use super::declaration::Bound;
use super::markers::{Block, Class, MARKER, Marker};

pub use self::divergences::{Added, apply_divergences};
use self::expected::scalar_name;
pub use self::expected::{describes_element, describing, element_schema, expected};
pub use self::parse::{block_schema, parse_markers, split_block};
pub use self::render::render;
pub(super) use self::render::{quoted, scalar};
pub use self::resolve::{covers, generated, resolve, superseded};

/// The mode a marker still declares. Generation is what every bound value gets without declaring.
pub const HANDWRITTEN: &str = "handwritten";
/// The word the retired enrolment marker used, kept so that it can be refused *by name*.
const GENERATED: &str = "generated";

/// The marker that declares a hand transcription.
pub const SHAPE_MARKER: &str = "@config-shape";
/// The marker that overrides one position the contract also describes.
pub const EXCEPT_MARKER: &str = "@config-shape-except";
/// The marker that keeps one position the contract describes not at all.
pub const NARROW_MARKER: &str = "@config-shape-narrow";

/// Keywords in the order a generated block spells them: what the value *is*, then its bounds, then
/// what it holds.
///
/// Anything outside this list is appended alphabetically rather than dropped — under-reporting a
/// constraint is the failure this whole toolchain exists to remove, and it would be no better
/// coming from a renderer.
const ORDER: [&str; 20] = [
    "type",
    "enum",
    "const",
    "pattern",
    "format",
    "minimum",
    "exclusiveMinimum",
    "maximum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "uniqueItems",
    "required",
    "propertyNames",
    "properties",
    "additionalProperties",
    "items",
];

/// Whether a binding class has a constraint of its own to derive a block from.
///
/// `composed` is the one left out, and the reason is that such a value is *an input* to the key's
/// text rather than the key's value — a port inside a `printf` — so the key's constraint describes
/// the composition and generating from it would type the part as the whole.
fn derives(class: Class) -> bool {
    matches!(
        class,
        Class::Projection | Class::Structured | Class::External
    )
}

/// One value whose block is owned here, generated or transcribed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    /// The chart.
    pub chart: String,
    /// The line a reader has to go to: a declared shape's marker, or a generated one's opening
    /// delimiter.
    pub line: usize,
    /// The value.
    pub values_path: String,
    /// Whether the chart owns the block rather than deriving it.
    pub handwritten: bool,
    /// The release the struct was read at, for a transcription.
    pub version: Option<String>,
    /// The file it was read from.
    pub source: Option<String>,
    /// Whether a marker declared this. A generated shape cannot be told to move a marker it does
    /// not have.
    pub declared: bool,
}

impl Shape {
    /// `chart/values.yaml:LINE`, for a message a reader can jump to.
    pub fn at(&self) -> String {
        format!("{}/values.yaml:{}", self.chart, self.line)
    }
}

/// Which of the two things a declared departure is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Departure {
    /// Replaces a position the contract also describes.
    Override,
    /// Keeps a position the contract does not describe at all.
    Narrowing,
}

impl Departure {
    /// The marker this was written as.
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Override => EXCEPT_MARKER,
            Self::Narrowing => NARROW_MARKER,
        }
    }
}

/// One declared departure from the generated shape: a position the chart keeps as its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The chart.
    pub chart: String,
    /// The marker's own line.
    pub line: usize,
    /// The value whose block it departs from.
    pub values_path: String,
    /// A dotted walk of *schema keywords* — `items.properties.guild_id` — because that is what the
    /// thing being addressed is.
    pub sub_path: String,
    /// Why. Mandatory: a departure from the contract without a reason is one nobody can review.
    pub reason: String,
    /// Which kind.
    pub kind: Departure,
}

impl Divergence {
    /// `chart/values.yaml:LINE`, for a message a reader can jump to.
    pub fn at(&self) -> String {
        format!("{}/values.yaml:{}", self.chart, self.line)
    }
}

/// One image publishing a key, and the release its vendored contract was published at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publisher {
    /// The document that carries the key.
    pub document: String,
    /// The release its image was built at.
    pub version: String,
}

/// The contract key one value feeds, and everything a caller needs about it.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// What the producer says the parsed value must satisfy.
    pub constraint: Option<Json>,
    /// Whether the marker called the binding optional.
    pub optional: bool,
    /// Whether every document that carries it calls it `structured`.
    pub structured: bool,
    /// The producer's own name for the type, carried for one purpose: a refusal that asks for a
    /// transcription can name what is being transcribed, so the marker it asks for can be pasted
    /// rather than composed. Empty when the documents disagree, or publish none.
    pub ty: String,
    /// Every document that carries the key, paired with the release its image was built at.
    pub publishers: Vec<Publisher>,
}

impl Resolved {
    /// The furthest-ahead release publishing this key — what a transcription is read at.
    ///
    /// A hand copy is one description of a type several images share, so the release to read it at
    /// is the newest of them: read at the newest it is a true statement about that image and at
    /// worst a narrowing of an older one, whereas read at the oldest it says nothing about the
    /// field the newest added.
    pub fn newest(&self) -> &str {
        self.publishers
            .iter()
            .max_by_key(|held| ordinal(&held.version))
            .map_or("", |held| held.version.as_str())
    }
}

/// A release as a comparable tuple, for the one comparison made here.
///
/// Leading numeric segments only: `8.9.1` sorts under `8.10.0`, which a string comparison gets
/// backwards, and a tag nobody writes — a pre-release, a date, a bare word — degrades to nothing
/// rather than failing. Two of those compare equal, which the caller turns into "re-read it" rather
/// than "it is current".
fn ordinal(version: &str) -> Vec<u64> {
    let mut parts = Vec::new();
    for segment in super::release(version).split('.') {
        match segment.parse::<u64>() {
            Ok(number) => parts.push(number),
            Err(_) => break,
        }
    }
    parts
}

/// One chart's shape markers, resolved against its contracts.
///
/// Everything that can fail before a single block is looked at fails here, with the chart named
/// once: a marker naming a value that does not exist, a departure with no shape to depart from, a
/// value that binds no key. The two callers then differ only in what they do with a block that does
/// not match.
pub struct Chart {
    /// The chart's directory name.
    pub name: String,
    /// The values file, verbatim. Read without newline translation, because a rewrite that
    /// normalised line endings would reflow every line of every file it touched.
    pub text: String,
    /// Everything found wrong, in the order it was found.
    pub problems: Vec<String>,
    /// The values whose block the chart owns.
    pub transcribed: Vec<Shape>,
    /// The declared departures from a generated block.
    pub divergences: Vec<Divergence>,
    /// Every `@schema` block, by the value it belongs to.
    pub blocks: BTreeMap<String, Block>,
    /// Every binding marker, by the value it was written in.
    pub markers: BTreeMap<String, Vec<Marker>>,
    /// The chart's documents, resolved to their contracts.
    pub bound: Option<Bound>,
    /// The chart's own `appVersion`, for a transcription no contract carries.
    pub app_version: String,
    /// Every shape: the generated ones, then the transcribed.
    pub shapes: Vec<Shape>,
}

impl Chart {
    /// Read one chart, resolving everything that can be resolved before a block is compared.
    ///
    /// # Errors
    /// [`Error::Invalid`] when the markers or the declaration cannot be read at all, and
    /// [`Error::Io`] when the values file cannot be.
    pub fn read(chart_dir: &std::path::Path) -> Result<Self, Error> {
        let name = chart_dir
            .file_name()
            .map_or_else(String::new, |held| held.to_string_lossy().into_owned());
        let values = chart_dir.join("values.yaml");
        let bytes = std::fs::read(&values).map_err(|e| Error::io(values.display(), e))?;
        let text = String::from_utf8(bytes)
            .map_err(|e| Error::Invalid(format!("{}: is not UTF-8: {e}", values.display())))?;

        let (transcribed, divergences) = parse_markers(&text, &name)?;
        let (found, blocks) = super::markers::read(&text, &name)?;

        let mut by_value: BTreeMap<String, Vec<Marker>> = BTreeMap::new();
        for marker in found {
            by_value
                .entry(marker.values_path.clone())
                .or_default()
                .push(marker);
        }
        let blocks: BTreeMap<String, Block> = blocks
            .into_iter()
            .map(|block| (block.values_path.clone(), block))
            .collect();

        let declaration = super::load_declaration(chart_dir)?;
        let bound = match declaration {
            Some(declaration) => Some(Bound::of(chart_dir, declaration)?),
            None => None,
        };

        let chart_yaml = chart_dir.join("Chart.yaml");
        let app_version = if chart_yaml.is_file() {
            let held = std::fs::read_to_string(&chart_yaml)
                .map_err(|e| Error::io(chart_yaml.display(), e))?;
            super::declaration::read_yaml(&held, &chart_yaml)?
                .get("appVersion")
                .map(scalar_name)
                .unwrap_or_default()
                .trim()
                .to_owned()
        } else {
            String::new()
        };

        let mut shapes = generated(&name, &by_value, &blocks, &transcribed);
        shapes.extend(transcribed.iter().cloned());

        let mut chart = Self {
            name,
            text,
            problems: Vec::new(),
            transcribed,
            divergences,
            blocks,
            markers: by_value,
            bound,
            app_version,
            shapes,
        };
        chart.check_markers();
        Ok(chart)
    }

    /// What can be decided from the markers alone, before a contract is opened.
    ///
    /// A value carries at most one shape marker, and a departure belongs to a generated block and
    /// says so by naming the same value.
    fn check_markers(&mut self) {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut problems = Vec::new();
        for shape in &self.transcribed {
            if let Some(first) = seen.get(&shape.values_path) {
                problems.push(format!(
                    "{}: '{}' already declares a shape on line {first}. One value has one block, \
                     so the second marker is either a duplicate or the migration of the first left \
                     half-done",
                    shape.at(),
                    shape.values_path
                ));
                continue;
            }
            seen.insert(shape.values_path.clone(), shape.line);
        }

        let derived: Vec<&str> = self
            .shapes
            .iter()
            .filter(|shape| !shape.handwritten)
            .map(|shape| shape.values_path.as_str())
            .collect();
        for divergence in &self.divergences {
            if derived.contains(&divergence.values_path.as_str()) {
                continue;
            }
            if seen.contains_key(&divergence.values_path) {
                problems.push(format!(
                    "{}: departs from the generated shape of '{}', whose block is `{HANDWRITTEN}` \
                     and so is not generated at all. A departure from a hand-written block is just \
                     the block",
                    divergence.at(),
                    divergence.values_path
                ));
                continue;
            }
            problems.push(format!(
                "{}: departs from the generated shape of '{}', which carries no `{MARKER}` marker \
                 naming a contract key, so nothing is generated there to depart from",
                divergence.at(),
                divergence.values_path
            ));
        }
        self.problems.extend(problems);
    }

    /// The contract key one value feeds.
    pub fn constraint_for(&mut self, shape: &Shape, report: bool) -> Option<Resolved> {
        let markers = self
            .markers
            .get(&shape.values_path)
            .cloned()
            .unwrap_or_default();
        let mut problems = Vec::new();
        let resolved = resolve(self.bound.as_ref(), shape, &markers, &mut problems, report);
        self.problems.extend(problems);
        resolved
    }

    /// The `@schema` block one shape describes, refusing a marker that is not above it.
    ///
    /// Placement is checked rather than assumed, for a shape a marker declares. The marker is
    /// matched to its value by name, so one written at the other end of the file would still
    /// resolve — and would then assert something about a block nobody reading that block can see.
    ///
    /// A generated shape is not declared anywhere, so there is nothing to place: it *is* the block,
    /// found through the `@config` marker written inside it.
    pub fn block_for(&mut self, shape: &Shape) -> Option<Block> {
        let Some(block) = self.blocks.get(&shape.values_path).cloned() else {
            self.problems.push(format!(
                "{}: names '{}', which has no `@schema` block for this marker to describe",
                shape.at(),
                shape.values_path
            ));
            return None;
        };
        if !shape.declared {
            return Some(block);
        }

        let lines: Vec<&str> = self.text.lines().collect();
        let between = lines
            .get(shape.line..block.start.saturating_sub(1))
            .unwrap_or(&[]);
        let interrupted = between
            .iter()
            .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
        if shape.line >= block.start || interrupted {
            self.problems.push(format!(
                "{}: sits away from the `@schema` block for '{}' on line {}. A marker is read next \
                 to the thing it describes or it is not read at all; write it in the comment run \
                 directly above the block",
                shape.at(),
                shape.values_path,
                block.start
            ));
            return None;
        }
        Some(block)
    }

    /// The lines one generated block's schema half should carry, or [`None`] on a problem.
    pub fn target(&mut self, shape: &Shape) -> Option<Vec<String>> {
        let block = self.block_for(shape)?;
        let resolved = self.constraint_for(shape, true)?;

        let ty = resolved.ty.clone();
        let source = if !ty.is_empty() && !ty.contains([' ', ',']) {
            ty
        } else {
            "<source>".to_owned()
        };
        // The release the marker being asked for should record: the newest image publishing the
        // key, not the chart's `appVersion`, which in a multi-service chart is one service's.
        let version = if resolved.newest().is_empty() {
            if self.app_version.is_empty() {
                "<version>".to_owned()
            } else {
                self.app_version.clone()
            }
        } else {
            resolved.newest().to_owned()
        };

        if resolved.structured && !describes_element(resolved.constraint.as_ref()) {
            self.problems.push(format!(
                "{}: the contract describes '{}' as a container and says nothing about what one \
                 element holds, so a generated block would type-check nothing. Transcribe it and \
                 declare `{SHAPE_MARKER} {} {HANDWRITTEN} {version} {source}` until the image \
                 publishes the element",
                shape.at(),
                shape.values_path,
                shape.values_path
            ));
            return None;
        }
        if !describing(resolved.constraint.as_ref()) {
            self.problems.push(format!(
                "{}: the contract publishes no constraint for '{}', so a generated block would \
                 accept every JSON type and check nothing. Transcribe it and declare \
                 `{SHAPE_MARKER} {} {HANDWRITTEN} {version} {source}`, which is held against the \
                 version it was read at",
                shape.at(),
                shape.values_path,
                shape.values_path
            ));
            return None;
        }

        let wanted = expected(
            resolved.constraint.as_ref(),
            resolved.optional,
            resolved.structured,
        );
        let present = match block_schema(&block) {
            Ok(present) => present,
            Err(failure) => {
                self.problems.push(format!("{}: {failure}", shape.at()));
                return None;
            }
        };

        let mine: Vec<&Divergence> = self
            .divergences
            .iter()
            .filter(|divergence| divergence.values_path == shape.values_path)
            .collect();
        let (settled, added, problems) = apply_divergences(&wanted, &present, &mine);
        let failed = !problems.is_empty();
        self.problems.extend(problems);
        if failed {
            return None;
        }

        Some(render(&settled, block.indent, &added))
    }

    /// The older marker: a copy of a struct, held against the release it was copied at.
    ///
    /// Reached from the checker only. Every other problem here is one the writer repairs on the
    /// same pass, so raising it from both halves costs nothing; this one it cannot repair —
    /// re-reading a struct somebody else owns is a person's job, and a writer that refused over it
    /// would turn the job that repairs the tree into a gate over the one thing in it no repair
    /// reaches.
    ///
    /// The release is the newest *image publishing the key*, and only the chart's own when no
    /// contract publishes it at all. The difference is the whole of this check on a multi-service
    /// chart: an automated bump moves the one image that had a release, so holding every
    /// transcription against `appVersion` demands that a struct owned by the images that did not
    /// move be re-read at a release those images were never built at.
    pub fn check_handwritten(&mut self, shape: &Shape) {
        if self.block_for(shape).is_none() {
            return;
        }
        let resolved = self.constraint_for(shape, false);
        if resolved.as_ref().is_some_and(superseded) {
            self.problems.push(format!(
                "{}: '{}' was transcribed by hand, and the contract now describes it itself. \
                 Delete the marker and regenerate; declare any position the chart keeps with \
                 `{EXCEPT_MARKER}` or `{NARROW_MARKER}`",
                shape.at(),
                shape.values_path
            ));
            return;
        }

        let published = resolved
            .as_ref()
            .map(|held| held.newest().to_owned())
            .unwrap_or_default();
        if published.is_empty() && self.app_version.is_empty() {
            self.problems.push(format!(
                "{}: declares a shape for '{}', but no contract records the release the key was \
                 published at and the chart has no appVersion either, so there is nothing to hold \
                 the transcription against",
                shape.at(),
                shape.values_path
            ));
            return;
        }

        let read_at = shape.version.clone().unwrap_or_default();
        let source = shape.source.clone().unwrap_or_default();
        if published.is_empty() {
            // No contract carries the key — an unbound transcription, whose only release is the
            // chart's own. Nothing here can be more precise, and equality is the check.
            if super::release(&read_at) != super::release(&self.app_version) {
                self.problems.push(format!(
                    "{}: '{}' was transcribed at {read_at}, and the chart now pins appVersion {}\n    re-read {source} at {}, bring the `@schema` block for '{}' up to it, and move the marker",
                    shape.at(),
                    shape.values_path,
                    self.app_version,
                    self.app_version,
                    shape.values_path
                ));
            }
            return;
        }

        if covers(&read_at, &published) {
            return;
        }
        let mut ahead: Vec<&str> = resolved
            .as_ref()
            .map(|held| {
                held.publishers
                    .iter()
                    .filter(|one| !covers(&read_at, &one.version))
                    .map(|one| one.document.as_str())
                    .collect()
            })
            .unwrap_or_default();
        ahead.sort_unstable();
        ahead.dedup();
        self.problems.push(format!(
            "{}: '{}' was transcribed at {read_at}, and {} now publish{} it at {published}\n    re-read {source} at {published}, bring the `@schema` block for '{}' up to it, and move the marker",
            shape.at(),
            shape.values_path,
            ahead.join(", "),
            if ahead.len() == 1 { "es" } else { "" },
            shape.values_path
        ));
    }
}

/// The chart's values file with every generated block written from its contract.
///
/// Blocks are replaced from the bottom up so that an earlier replacement cannot move the line
/// numbers a later one was read at.
pub fn rewrite(chart: &mut Chart) -> (String, usize) {
    let shapes: Vec<Shape> = chart
        .shapes
        .iter()
        .filter(|shape| !shape.handwritten)
        .cloned()
        .collect();

    let mut pending: Vec<(Block, Vec<String>)> = Vec::new();
    for shape in &shapes {
        if let Some(target) = chart.target(shape)
            && let Some(block) = chart.blocks.get(&shape.values_path)
        {
            pending.push((block.clone(), target));
        }
    }
    pending.sort_by_key(|(block, _)| std::cmp::Reverse(block.start));

    let ending = if chart.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = split_keeping_ends(&chart.text);
    let mut written = 0;

    for (block, target) in pending {
        let (markers, current) = split_block(&block);
        if current == target {
            continue;
        }
        let body: Vec<String> = markers
            .into_iter()
            .chain(target)
            .map(|line| format!("{line}{ending}"))
            .collect();
        lines.splice(block.start..block.end - 1, body);
        written += 1;
    }

    (lines.concat(), written)
}

/// Every generated block against what its contract calls for, reported onto the chart.
pub fn check(chart: &mut Chart) {
    let shapes: Vec<Shape> = chart
        .shapes
        .iter()
        .filter(|shape| !shape.handwritten)
        .cloned()
        .collect();

    for shape in &shapes {
        let Some(target) = chart.target(shape) else {
            continue;
        };
        let Some(block) = chart.blocks.get(&shape.values_path).cloned() else {
            continue;
        };
        let (_, current) = split_block(&block);
        if current == target {
            continue;
        }
        // Compared as text rather than as meaning, and the writer compares the same way. A block
        // that differs only in quoting is still a block a regeneration would rewrite, so a gate
        // that passed it would leave the writer with a diff to commit on a tree it had just called
        // current — which is how a generated artefact ends up perpetually dirty.
        chart.problems.push(format!(
            "{}: the `@schema` block for '{}' is not what its contract describes; regenerate it\n{}",
            shape.at(),
            shape.values_path,
            difference(&current, &target)
        ));
    }
}

/// One values file's lines, each keeping the ending it was written with.
fn split_keeping_ends(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find('\n') {
        lines.push(rest[..=position].to_owned());
        rest = &rest[position + 1..];
    }
    if !rest.is_empty() {
        lines.push(rest.to_owned());
    }
    lines
}

/// Two blocks, side by side, for a message somebody has to act on.
///
/// Both in full rather than a minimal edit script: a generated block is a dozen lines, and showing
/// both is shorter to read than working out which of them moved.
fn difference(current: &[String], target: &[String]) -> String {
    let mut out = String::from("--- values.yaml\n");
    for line in current {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("--- the contract\n");
    for line in target {
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;
