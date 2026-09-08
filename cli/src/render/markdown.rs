//! The Markdown renderings: GitHub-flavoured tables, ready to paste into a README.
//!
//! The rendering whose consumer is a person reading a page, which is what every choice here is
//! about. A cell has a page width to stay inside, so [`Column::DEFAULT`] is narrower than the set
//! of columns that exist; a cell is prose, so `|` and `\` are escaped rather than trusted; and a
//! cell shows the *summary* of a doc comment rather than the whole of it.
//!
//! Nothing here interprets a value. [`Key::default`](crate::Key::default) already arrived as the
//! string a table should show — the producer rendered it, in its own language, from a value this
//! crate never sees — so `12` prints as `12` and `public` prints as `public`, quotes and all left
//! off. That is the whole reason a renderer can be shared across producers: by the time a document
//! exists, every language-shaped decision has already been made.

use std::fmt::Write as _;

use crate::document::{Key, Schema};

/// Both tables, under [`Column::DEFAULT`].
///
/// The loader-variable table leads when there is one: its three columns are not the key columns,
/// and an operator who cannot find `<PREFIX>CONFIG` cannot use any of the rest.
///
/// Ends with a newline, so appending another section needs no separator of its own.
pub fn markdown(schema: &Schema, columns: &[Column]) -> String {
    let loader = markdown_loader(schema);
    let keys = markdown_keys(schema, columns);
    if loader.is_empty() {
        keys
    } else {
        // The blank line between them: two tables run together are one malformed table.
        format!("{loader}\n{keys}")
    }
}

/// The loader-variable table alone.
///
/// Empty when the schema has no loader variables — a header with no rows under it would be a table
/// promising variables that do not exist.
pub fn markdown_loader(schema: &Schema) -> String {
    let mut out = String::new();
    if schema.loader.is_empty() {
        return out;
    }

    out.push_str("| Variable | Role | Default | Purpose |\n");
    out.push_str("|---|---|---|---|\n");
    for var in &schema.loader {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            escape(&var.env),
            var.role.label(),
            optional_code(var.default.as_deref()),
            cell(&var.docs),
        );
    }

    out
}

/// The configuration-key table alone.
///
/// A schema with no keys still renders its header, unlike the loader table. An empty configuration
/// section is a real shape — a subsystem that reads nothing yet — and the header is what says the
/// section was generated rather than forgotten.
pub fn markdown_keys(schema: &Schema, columns: &[Column]) -> String {
    let mut out = String::new();
    let header: Vec<&str> = columns.iter().map(|c| c.heading()).collect();
    let _ = writeln!(out, "| {} |", header.join(" | "));
    let _ = writeln!(out, "|{}|", vec!["---"; columns.len()].join("|"));
    for key in &schema.keys {
        let cells: Vec<String> = columns.iter().map(|c| c.render(key)).collect();
        let _ = writeln!(out, "| {} |", cells.join(" | "));
    }

    out
}

/// One column of the Markdown key table.
///
/// The full set is deliberately wider than [`Self::DEFAULT`]: everything is available to a caller
/// who wants it, and the default stays narrow enough to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Column {
    /// The TOML key path.
    Path,
    /// What kind of value the key takes — its type, or the choices it accepts.
    Type,
    /// Other key paths that supply the same key.
    Aliases,
    /// The environment variable supplying the value directly.
    Env,
    /// The variable naming a file holding the value.
    EnvFile,
    /// The file name inside the secrets directory.
    SecretsFile,
    /// The value when nothing supplies the key, with its note in parentheses.
    Default,
    /// That value alone, with no note folded in. Pair it with [`Self::Note`].
    DefaultValue,
    /// The producer's prose about the default, on its own.
    Note,
    /// `required`, `secret` and `reserved`, collapsed into one cell.
    Flags,
    /// Whether the key must be supplied.
    Required,
    /// Whether the value is secret.
    Secret,
    /// The doc comment.
    Docs,
}

impl Column {
    /// The columns the plain Markdown rendering emits.
    ///
    /// The two file spellings are left out because both are mechanical, and dropping the pair
    /// keeps the table inside a page. [`Self::Flags`] carries what [`Self::Required`] and
    /// [`Self::Secret`] would have taken two columns to say, and [`Self::Aliases`] is empty for
    /// almost every key.
    pub const DEFAULT: &'static [Self] = &[
        Self::Path,
        Self::Type,
        Self::Env,
        Self::Default,
        Self::Flags,
        Self::Docs,
    ];

    /// The `--columns` spelling of one column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Type => "type",
            Self::Aliases => "aliases",
            Self::Env => "env",
            Self::EnvFile => "env-file",
            Self::SecretsFile => "secrets-file",
            Self::Default => "default",
            Self::DefaultValue => "default-value",
            Self::Note => "note",
            Self::Flags => "flags",
            Self::Required => "required",
            Self::Secret => "secret",
            Self::Docs => "docs",
        }
    }

    /// Every column, in the order a usage message lists them.
    pub const ALL: &'static [Self] = &[
        Self::Path,
        Self::Type,
        Self::Aliases,
        Self::Env,
        Self::EnvFile,
        Self::SecretsFile,
        Self::Default,
        Self::DefaultValue,
        Self::Note,
        Self::Flags,
        Self::Required,
        Self::Secret,
        Self::Docs,
    ];

    const fn heading(self) -> &'static str {
        match self {
            Self::Path => "TOML",
            Self::Type => "Type",
            Self::Aliases => "Also accepts",
            Self::Env => "Environment",
            Self::EnvFile => "File indirection",
            Self::SecretsFile => "Secrets file",
            Self::Default | Self::DefaultValue => "Default",
            Self::Note => "Note",
            Self::Flags => "Flags",
            Self::Required => "Required",
            Self::Secret => "Secret",
            Self::Docs => "Purpose",
        }
    }

    fn render(self, key: &Key) -> String {
        match self {
            // Escaped like every other cell. A key path is not prose, but it is not the table
            // author's to choose either — a rename can put a cell separator in it, and an
            // unescaped one adds a column to the row.
            Self::Path => format!("`{}`", escape(&key.path)),
            // The choices when there are any, because a type name tells an operator nothing they
            // can act on and `trace | debug | info` tells them exactly what to type. The type name
            // stays in front of them, since it is what they will see in the source.
            Self::Type => match (&key.ty, key.values.as_slice()) {
                (_, []) => optional_code(key.ty.as_deref()),
                (ty, values) => {
                    let choices = values
                        .iter()
                        .map(|value| format!("`{}`", escape(value)))
                        .collect::<Vec<_>>()
                        .join(r" \| ");
                    match ty {
                        Some(ty) => format!("`{}`: {choices}", escape(ty)),
                        None => choices,
                    }
                }
            },
            Self::Aliases => {
                if key.aliases.is_empty() {
                    EM_DASH.to_owned()
                } else {
                    key.aliases
                        .iter()
                        .map(|alias| format!("`{}`", escape(alias)))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            }
            Self::Env => optional_code(key.env.as_deref()),
            Self::EnvFile => optional_code(key.env_file.as_deref()),
            Self::SecretsFile => optional_code(key.secrets_file.as_deref()),
            // The exact value leads, because that is what an operator compares against what they
            // set; the note explains what it means. An unset default is written as prose rather
            // than as an empty code span, so `unset (ISR off)` reads as one phrase.
            Self::Default | Self::DefaultValue => {
                let value = match &key.default {
                    Some(default) => format!("`{}`", escape(default)),
                    None if key.required => EM_DASH.to_owned(),
                    None => "unset".to_owned(),
                };
                match (self, &key.note) {
                    (Self::Default, Some(note)) => format!("{value} ({})", cell(note)),
                    _ => value,
                }
            }
            Self::Flags => {
                let mut notes = Vec::new();
                if key.required {
                    notes.push("required");
                }
                if key.secret {
                    notes.push("secret");
                }
                if key.reserved {
                    notes.push("reserved");
                }
                if notes.is_empty() {
                    EM_DASH.to_owned()
                } else {
                    notes.join(", ")
                }
            }
            Self::Required => yes_or_dash(key.required),
            Self::Secret => yes_or_dash(key.secret),
            Self::Note => key.note.as_deref().map_or_else(|| EM_DASH.to_owned(), cell),
            Self::Docs => summary_cell(&key.docs),
        }
    }
}

/// A `--columns` value that names no column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownColumn(pub String);

impl std::fmt::Display for UnknownColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not a column. Try one of: ", self.0)?;
        for (index, column) in Column::ALL.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(column.as_str())?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownColumn {}

impl std::str::FromStr for Column {
    type Err = UnknownColumn;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .find(|column| column.as_str() == text)
            .copied()
            .ok_or_else(|| UnknownColumn(text.to_owned()))
    }
}

/// What every column shows when it has nothing to show.
const EM_DASH: &str = "—";

fn yes_or_dash(flag: bool) -> String {
    if flag { "yes" } else { EM_DASH }.to_owned()
}

/// A spelling as inline code, or an em dash when there is none.
fn optional_code(value: Option<&str>) -> String {
    value.map_or_else(
        || EM_DASH.to_owned(),
        |value| format!("`{}`", escape(value)),
    )
}

/// Prose in a table cell: newlines become breaks, and `|` stops ending the cell early.
fn cell(text: &str) -> String {
    if text.is_empty() {
        return EM_DASH.to_owned();
    }
    escape(text).replace('\n', "<br>")
}

/// A doc comment in a table cell: its summary, on one line.
fn summary_cell(text: &str) -> String {
    let summary = summary(text);
    if summary.is_empty() {
        return EM_DASH.to_owned();
    }
    escape(&summary)
}

/// The first paragraph of a doc comment, as rustdoc means the word.
///
/// The whole comment would put every paragraph of a field's documentation into one cell and make a
/// table out of an essay. This is the producer's own convention rather than a new annotation to
/// keep in step, and it is stated in `FORMAT.md` rather than being this renderer's invention —
/// which matters, because a Java producer's Javadoc has to reach the same cell.
pub fn summary(docs: &str) -> String {
    docs.lines()
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The characters that would otherwise be read as table structure.
fn escape(text: &str) -> String {
    text.replace('\\', r"\\").replace('|', r"\|")
}

#[cfg(test)]
mod tests {
    use super::{escape, summary};

    #[test]
    fn a_pipe_in_a_doc_comment_does_not_end_the_cell() {
        assert_eq!(escape("a | b"), r"a \| b");
        assert_eq!(escape(r"a \ b"), r"a \\ b");
    }

    #[test]
    fn the_summary_stops_at_the_first_blank_line() {
        assert_eq!(summary("One.\nStill one.\n\nTwo."), "One. Still one.");
    }
}
