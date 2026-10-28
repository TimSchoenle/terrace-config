//! Rendering a surface: the compact table, the full per-setting entries and the environment-
//! variable sections.

use serde_json::{Map, Value as Json};

use crate::value::Entry;

use super::constraint::{describe_constraint, plain};
use super::text::{clip, prose, wrap};
use super::{Reader, Setting, Surface, WIDTH, is_empty, select};

/// Column ceilings for the compact listing.
///
/// The columns size themselves to the selection (a filtered listing is narrower than the whole
/// chart), and these only stop one 60-character type from setting the width for 167 rows.
pub(super) const MAX_PATH: usize = 46;
pub(super) const MAX_TYPE: usize = 30;
pub(super) const MAX_DEFAULT: usize = 22;

/// The images that read one setting, or `all N` when that is every image the chart pins.
///
/// `all 9` rather than nine names is not an abbreviation: the nine are listed in the header, and the
/// fact worth reading here is that the setting is fleet-wide. It is also what keeps a key read by
/// eight visibly different from a loader variable read by all nine.
#[must_use]
pub fn reader_list(setting: &Setting, total: usize) -> String {
    if total > 1 && setting.occurrences.len() == total {
        return format!("all {total}");
    }
    setting.readers().join(", ")
}

/// The two-character flag column: `R` required, `S` secret.
#[must_use]
pub fn markers(entry: &Map<String, Json>) -> String {
    let held = |name: &str| entry.get(name).and_then(Json::as_bool) == Some(true);
    format!(
        "{}{}",
        if held("required") { "R" } else { "." },
        if held("secret") { "S" } else { "." }
    )
}

/// The default as the loader would read it, or `-` when the setting has none.
///
/// An empty string is a default, not an absence (`""` and "nothing is set" are two different
/// deployments), so it is spelt out rather than folded into the same dash.
#[must_use]
pub fn shown_default(entry: &Map<String, Json>) -> String {
    match entry.get("default") {
        None | Some(Json::Null) => "-".to_owned(),
        Some(Json::String(text)) if text.is_empty() => "(empty)".to_owned(),
        Some(Json::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// One line per setting: path, type, markers, default, and the images that read it.
///
/// The columns size themselves to the selection rather than to the chart, so a filtered listing is
/// narrow where the whole chart is wide, and the default column disappears entirely when nothing in
/// the selection has one. The reader list is last and unpadded: it is the widest field and the one
/// that varies most, and truncating it would hide the fact this listing exists to show.
#[must_use]
pub fn compact(settings: &[&Setting], total: usize) -> Vec<String> {
    let rows: Vec<[String; 5]> = settings
        .iter()
        .map(|setting| {
            let entry = setting.representative();
            let form = Entry(&entry)
                .text_form()
                .map_or_else(|_| "unknown".to_owned(), |form| form.label().to_owned());
            [
                clip(&setting.name, MAX_PATH),
                clip(
                    entry
                        .get("ty")
                        .and_then(Json::as_str)
                        .filter(|held| !held.is_empty())
                        .unwrap_or(&form),
                    MAX_TYPE,
                ),
                markers(&entry),
                clip(&shown_default(&entry), MAX_DEFAULT),
                if total > 1 {
                    reader_list(setting, total)
                } else {
                    String::new()
                },
            ]
        })
        .collect();

    let width = |column: usize| {
        rows.iter()
            .map(|row| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (path, ty, default) = (width(0), width(1), width(3));
    let defaults = rows.iter().any(|row| row[3] != "-");

    rows.iter()
        .map(|row| {
            let mut line = format!(
                "    {:<path$}  {:<ty$}  {}",
                row[0],
                row[1],
                row[2],
                path = path,
                ty = ty
            );
            if defaults {
                line = format!("{line}  {:<default$}", row[3], default = default);
            }
            if row[4].is_empty() {
                line.trim_end().to_owned()
            } else {
                format!("{}  {}", line.trim_end(), row[4])
            }
        })
        .collect()
}

/// One labelled line, wrapped under its own label rather than off the terminal.
///
/// A spelling or a type never needs this; a refusal explaining why a file cannot supply the key
/// always does, and an entry whose widest line is the one carrying the reason is the one an operator
/// will not read.
pub(super) fn row(into: &mut Vec<String>, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    let margin = format!("{:8}{label:<14}", "");
    let indent = " ".repeat(margin.chars().count());
    for (index, line) in wrap(value, WIDTH - margin.chars().count())
        .iter()
        .enumerate()
    {
        into.push(if index == 0 {
            format!("{margin}{line}")
        } else {
            format!("{indent}{line}")
        });
    }
}

/// Everything one contract says about one setting.
///
/// `readers` is every image the chart pins, for the one fact a key entry cannot carry alone: whether
/// the image that published it applies a change without restarting.
#[must_use]
pub fn full(
    setting: &Setting,
    readers: &[Reader],
    dialect: &Map<String, Json>,
    total: usize,
) -> Vec<String> {
    let entry = setting.representative();
    let form = Entry(&entry)
        .text_form()
        .map_or_else(|_| "unknown".to_owned(), |form| form.label().to_owned());
    let divergent = setting.divergent();

    let mut lines = vec![format!("    {}", setting.name)];

    let ty = entry.get("ty").and_then(Json::as_str).unwrap_or("?");
    row(&mut lines, "type", &format!("{ty}  (read as {form})"));
    row(&mut lines, "required", yes_no(&entry, "required"));
    row(&mut lines, "secret", yes_no(&entry, "secret"));
    if entry.get("reserved").and_then(Json::as_bool) == Some(true) {
        row(
            &mut lines,
            "reserved",
            "yes — the loader claims this path; a deployment must not set it",
        );
    }
    on_change(&mut lines, setting, readers);
    row(&mut lines, "default", &shown_default(&entry));
    // Beside the default rather than at the end with the prose, because that is what a note turns
    // out to be: it glosses the default rather than the key, and a gloss printed six lines from the
    // thing it glosses is a gloss nobody connects.
    row(
        &mut lines,
        "note",
        setting
            .value("note")
            .map(plain)
            .unwrap_or_default()
            .as_str(),
    );
    if let Some(values) = entry
        .get("values")
        .and_then(Json::as_array)
        .filter(|held| !held.is_empty())
    {
        row(
            &mut lines,
            "values",
            &values.iter().map(plain).collect::<Vec<_>>().join(" | "),
        );
    }
    row(
        &mut lines,
        "accepts",
        &describe_constraint(entry.get("constraint")),
    );
    row(
        &mut lines,
        "as text",
        &describe_constraint(entry.get("text_constraint")),
    );
    if total > 1 {
        row(&mut lines, "read by", &reader_list(setting, total));
    }

    spellings(&mut lines, &entry, dialect, &form);

    if divergent.iter().any(|name| name == "docs") {
        lines.push("        docs (the images disagree)".to_owned());
        for (value, readers) in setting.variants("docs") {
            lines.push(format!("          {}:", readers.join(", ")));
            let text = value.as_str().unwrap_or("(nothing)");
            lines.extend(prose(
                if text.is_empty() { "(nothing)" } else { text },
                &" ".repeat(12),
            ));
        }
    } else if let Some(docs) = setting.value("docs").and_then(Json::as_str) {
        lines.push("        docs".to_owned());
        lines.extend(prose(docs, &" ".repeat(12)));
    }

    let remaining: Vec<&str> = divergent
        .iter()
        .map(String::as_str)
        .filter(|name| *name != "docs")
        .collect();
    if !remaining.is_empty() {
        let first = setting
            .readers()
            .first()
            .copied()
            .unwrap_or_default()
            .to_owned();
        row(
            &mut lines,
            "disagreement",
            &format!(
                "the images describe {} differently; the values above are {first}'s",
                remaining.join(", ")
            ),
        );
    }

    lines.push(String::new());
    lines
}

/// What a change to one setting costs the running process, as each image that reads it says.
///
/// Answered per image and then collapsed, because the answer is a property of the pair: one image
/// may rebuild on a change and another may not, and a chart has to roll the pods of every workload
/// running the second. Where they agree it is one line; where they do not, each image is named.
///
/// The channel is not known here (this reads contracts, not a render), so a `live` answer says what
/// it depends on: the file being one the image watches, and the value not arriving through a
/// variable, which changes the pod and rolls it whatever the image can do.
pub(super) fn on_change(lines: &mut Vec<String>, setting: &Setting, readers: &[Reader]) {
    let answers: Vec<(&str, String)> = setting
        .occurrences
        .iter()
        .map(|(name, entry)| {
            let declared = readers
                .iter()
                .find(|reader| reader.name == *name)
                .and_then(|reader| reader.reload.as_ref());
            (name.as_str(), change_cost(declared, entry.get("reload")))
        })
        .collect();

    let Some((_, first)) = answers.first() else {
        return;
    };
    if answers.iter().all(|(_, answer)| answer == first) {
        row(lines, "on change", first);
        return;
    }
    row(lines, "on change", "the images disagree");
    for (name, answer) in answers {
        lines.push(format!("          {name}: {answer}"));
    }
}

/// One image's answer to "what does a change to this key cost".
pub(super) fn change_cost(declared: Option<&Json>, class: Option<&Json>) -> String {
    match crate::reload::class_of_raw(declared, class) {
        Ok(()) => {
            let layers: Vec<&str> = declared
                .and_then(|held| held.get("layers"))
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
                .collect();
            format!(
                "applied by a rebuild when it arrives through a watched file ({}); set through a \
                 variable, it rolls the pods",
                layers.join(", ")
            )
        }
        Err(why) if why.degraded() => format!(
            "rolls the pods — {} (read as a restart; this build is older than the document)",
            why.describe()
        ),
        Err(why) => format!("rolls the pods — {}", why.describe()),
    }
}

/// Every name the loader will accept for one key, and whether a file can supply it.
///
/// The file-supplyable reading is normative and counter-intuitive: the two file spellings exist for
/// every key, and for a key of any form but text neither of them can supply it: a file delivers a
/// string, and a loader will not coerce one into a number, a boolean or a literal. Printing the
/// spelling without printing that is how an operator ends up mounting a Secret that is never read.
pub(super) fn spellings(
    lines: &mut Vec<String>,
    entry: &Map<String, Json>,
    dialect: &Map<String, Json>,
    form: &str,
) {
    let suffix = dialect
        .get("indirection_suffix")
        .and_then(Json::as_str)
        .unwrap_or("_FILE");
    for (label, spelling) in [
        ("environment".to_owned(), "env"),
        (format!("{suffix} form"), "env_file"),
        ("secrets file".to_owned(), "secrets_file"),
    ] {
        if let Some(value) = entry.get(spelling).filter(|held| !is_empty(held)) {
            row(lines, &label, &plain(value));
        }
    }
    for (label, spelling) in [
        ("env aliases", "env_aliases"),
        ("aliases", "aliases"),
        ("secrets file aliases", "secrets_file_aliases"),
    ] {
        if let Some(values) = entry
            .get(spelling)
            .and_then(Json::as_array)
            .filter(|held| !held.is_empty())
        {
            row(
                lines,
                label,
                &values.iter().map(plain).collect::<Vec<_>>().join(", "),
            );
        }
    }

    let by_file = ["env_file", "secrets_file"]
        .iter()
        .any(|spelling| entry.get(*spelling).is_some_and(|held| !is_empty(held)));
    if !by_file {
        return;
    }
    if Entry(entry).file_supplyable().unwrap_or(false) {
        row(lines, "from a file", "yes");
    } else {
        row(
            lines,
            "from a file",
            &format!(
                "no — a file supplies text, and this key is read as {form}; set it in the document \
                 or the environment"
            ),
        );
    }
}

/// One boolean field, as the entry reads.
pub(super) fn yes_no(entry: &Map<String, Json>, name: &str) -> &'static str {
    if entry.get(name).and_then(Json::as_bool) == Some(true) {
        "yes"
    } else {
        "no"
    }
}

/// One loader or external variable: what it is, what it says, and who reads it.
///
/// Neither is a setting, so neither gets the full entry above: there is no path, no file spelling
/// and no secrets file to report. What they have in common is a name, a one-line summary and prose.
#[must_use]
pub fn variable(setting: &Setting, summary: &str, total: usize) -> Vec<String> {
    let mut lines = vec![format!("    {}  ({summary})", setting.name)];
    if let Some(docs) = setting
        .value("docs")
        .and_then(Json::as_str)
        .filter(|held| !held.is_empty())
    {
        lines.extend(prose(docs, &" ".repeat(8)));
    }
    if total > 1 {
        lines.push(format!("        read by  {}", reader_list(setting, total)));
    }
    lines
}

/// The whole surface, for a person.
#[must_use]
pub fn render(surface: &Surface, pattern: Option<&str>, show_full: bool) -> String {
    use std::fmt::Write as _;

    let (keys, loader, external) = (
        select(&surface.keys, pattern),
        select(&surface.loader, pattern),
        select(&surface.external, pattern),
    );
    let total = surface.readers.len();
    let dialect = |name: &str| {
        surface
            .dialect
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or_default()
    };
    let prefix = dialect("prefix");

    let mut out = format!(
        "==> {}: {} setting(s) across {total} image(s)\n",
        surface.chart,
        surface.keys.len()
    );
    let _ = writeln!(
        out,
        "    {prefix}* spellings, {} for nesting, {} for a file reference",
        dialect("nesting_separator"),
        dialect("indirection_suffix")
    );
    let mut readers: Vec<&Reader> = surface.readers.iter().collect();
    readers.sort_by(|left, right| left.name.cmp(&right.name));
    let width =
        |of: fn(&Reader) -> &str| readers.iter().map(|held| of(held).len()).max().unwrap_or(0) + 2;
    let (name_width, image_width) = (width(|held| &held.name), width(|held| &held.image));
    for reader in &readers {
        let _ = writeln!(
            out,
            "    {:<name_width$}{:<image_width$}{:<10}{}...",
            reader.name,
            reader.image,
            reader.version,
            reader.digest.chars().take(19).collect::<String>()
        );
    }

    if let Some(pattern) = pattern {
        let _ = writeln!(
            out,
            "\n    matching '{pattern}': {} setting(s), {} loader variable(s), {} external \
variable(s)",
            keys.len(),
            loader.len(),
            external.len()
        );
    }

    if !keys.is_empty() {
        let _ = write!(out, "\n==> settings\n\n");
        let entries = show_full || pattern.is_some();
        if entries {
            for setting in &keys {
                let _ = writeln!(
                    out,
                    "{}",
                    full(setting, &surface.readers, &surface.dialect, total).join("\n")
                );
            }
        } else {
            let mut columns = "path, type, flags, default".to_owned();
            if total > 1 {
                columns.push_str(", and the images that read it");
            }
            let _ = write!(out, "    {columns}; R marks required, S marks secret\n\n");
            let _ = writeln!(out, "{}", compact(&keys, total).join("\n"));
        }
    }

    let _ = write!(
        out,
        "{}",
        variable_sections(surface, &loader, &external, total)
    );
    out
}

/// The two sections that are not settings: what locates the document, and what is nobody's.
pub(super) fn variable_sections(
    surface: &Surface,
    loader: &[&Setting],
    external: &[&Setting],
    total: usize,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let field = |entry: &Map<String, Json>, name: &str| {
        entry
            .get(name)
            .and_then(Json::as_str)
            .unwrap_or("null")
            .to_owned()
    };

    if !loader.is_empty() {
        let _ = write!(
            out,
            "\n==> loader variables — where the settings are read from\n\n"
        );
        let blocks: Vec<String> = loader
            .iter()
            .map(|setting| {
                let entry = setting.representative();
                variable(
                    setting,
                    &format!(
                        "role {}, default {}",
                        field(&entry, "role"),
                        shown_default(&entry)
                    ),
                    total,
                )
                .join("\n")
            })
            .collect();
        let _ = writeln!(out, "{}", blocks.join("\n\n"));
    }

    if external.is_empty() {
        return out;
    }

    let _ = write!(
        out,
        "\n==> external variables — read by the image, and nobody's configuration\n\n"
    );
    let _ = writeln!(
        out,
        "{}",
        prose(
            "These are not settings. They belong to a runtime or a library the image carries, they \
             are outside the loader's namespace, and nothing in this chart's document can change \
             one. A value set here and expected to reach the application's configuration is a \
             value that is ignored.",
            "    "
        )
        .join("\n")
    );
    let _ = writeln!(out);

    let blocks: Vec<String> = external
        .iter()
        .map(|setting| {
            let entry = setting.representative();
            let constraint = describe_constraint(entry.get("constraint"));
            let summary = format!(
                "owner {}, {}{}, default {}",
                field(&entry, "owner"),
                field(&entry, "ty"),
                if constraint.is_empty() {
                    String::new()
                } else {
                    format!(", {constraint}")
                },
                shown_default(&entry)
            );
            variable(setting, &summary, total).join("\n")
        })
        .collect();
    let _ = writeln!(out, "{}", blocks.join("\n\n"));

    let prefix = surface
        .dialect
        .get("prefix")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "\n    unaccounted-for variables under {prefix}*: {}",
        surface.unknown
    );
    if !surface.ignore.is_empty() {
        let _ = writeln!(out, "    ignored by pattern: {}", surface.ignore.join(", "));
    }
    out
}
