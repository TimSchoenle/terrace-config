//! The Markdown rendering: GitHub-flavoured tables, ready to paste into a README.
//!
//! The rendering whose consumer is a person reading a page, which is what every choice here is
//! about. A cell has a page width to stay inside, so [`Column::DEFAULT`] is narrower than the
//! set of columns that exist; a cell is prose, so `|` and `\` are escaped rather than trusted;
//! and a cell shows the *summary* of a `///` comment rather than the whole of it.
//!
//! Nothing here interprets a value. `12` prints as `12` and `public` prints as `public`, quotes
//! and all left off, because that is what reads well in a table — and it is exactly why the
//! renderings that write a file read [`Key::default_value`] instead.

use std::fmt::Write as _;

use super::{Key, Schema, summary};

impl Schema {
    /// The schema as GitHub-flavoured Markdown, ready to paste into a README.
    ///
    /// Two tables: the variables the loader reads, then the configuration keys under
    /// [`Column::DEFAULT`]. Use [`Self::to_markdown_with`] to choose the columns.
    ///
    /// A [`Column::Docs`] cell carries the *summary* of the `///` comment — its first paragraph,
    /// as rustdoc means the word — rather than the whole of it. [`Key::docs`] keeps the whole
    /// text for [`Self::to_json`], so nothing is lost; a table cell is not where the four
    /// paragraphs below the summary belong.
    ///
    /// Ends with a newline, so appending another section needs no separator of its own.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        self.to_markdown_with(Column::DEFAULT)
    }

    /// Both tables, with a chosen set of key columns.
    ///
    /// The loader-variable table leads when there is one: its three columns are not the key
    /// columns, and an operator who cannot find `<PREFIX>CONFIG` cannot use any of the rest.
    ///
    /// [`Self::to_markdown_loader`] and [`Self::to_markdown_keys`] are the two halves on their
    /// own, for a page that wants them apart.
    ///
    /// Ends with a newline, as [`Self::to_markdown`] does.
    #[must_use]
    pub fn to_markdown_with(&self, columns: &[Column]) -> String {
        let loader = self.to_markdown_loader();
        let keys = self.to_markdown_keys(columns);
        if loader.is_empty() {
            keys
        } else {
            // The blank line between them: two tables run together are one malformed table.
            format!("{loader}\n{keys}")
        }
    }

    /// The loader-variable table alone.
    ///
    /// A documentation page with one key table per subsystem wants these variables once, not
    /// repeated above every table — and the subsystem pages want [`Self::to_markdown_keys`] with
    /// no loader table at all. Emitting the pair together is the common case, not the only one,
    /// so each half is reachable on its own rather than through clearing a field.
    ///
    /// Empty when the schema has no loader variables, which is what
    /// [`Schema::describe`](Self::describe) produces on its own — a header with no rows under it
    /// would be a table promising variables that do not exist.
    ///
    /// Ends with a newline when it is not empty.
    #[must_use]
    pub fn to_markdown_loader(&self) -> String {
        let mut out = String::new();
        if self.loader.is_empty() {
            return out;
        }

        out.push_str("| Variable | Role | Default | Purpose |\n");
        out.push_str("|---|---|---|---|\n");
        for var in &self.loader {
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

    /// The configuration-key table alone, with a chosen set of columns.
    ///
    /// The counterpart to [`Self::to_markdown_loader`]: the table for a page that documents one
    /// subsystem and has said where the configuration file comes from somewhere else.
    ///
    /// A schema with no keys still renders its header, unlike the loader table. An empty
    /// configuration section is a real shape — a subsystem that reads nothing yet — and the
    /// header is what says the section was generated rather than forgotten.
    ///
    /// Ends with a newline.
    #[must_use]
    pub fn to_markdown_keys(&self, columns: &[Column]) -> String {
        let mut out = String::new();
        let header: Vec<&str> = columns.iter().map(|c| c.heading()).collect();
        let _ = writeln!(out, "| {} |", header.join(" | "));
        let _ = writeln!(out, "|{}|", vec!["---"; columns.len()].join("|"));
        for key in &self.keys {
            let cells: Vec<String> = columns.iter().map(|c| c.render(key)).collect();
            let _ = writeln!(out, "| {} |", cells.join(" | "));
        }

        out
    }
}

/// One column of the Markdown key table.
///
/// The full set is deliberately wider than [`Self::DEFAULT`]: everything is available to a
/// caller who wants it, and the default stays narrow enough to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Column {
    /// The TOML/figment key path.
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
    /// The `#[config(note = "…")]` prose on its own, for a table that keeps the two apart.
    ///
    /// Pair it with [`Self::DefaultValue`], not [`Self::Default`], which already carries the
    /// note. A column that rendered differently depending on which *other* columns were asked
    /// for would be the kind of surprise a generated table cannot afford.
    Note,
    /// `required`, `secret` and `reserved`, collapsed into one cell.
    Flags,
    /// Whether the key must be supplied.
    Required,
    /// Whether the value is secret.
    Secret,
    /// The `///` comment.
    Docs,
}

impl Column {
    /// The columns [`Schema::to_markdown`] emits: everything an operator needs, and nothing that
    /// pushes the table past the width of a page.
    ///
    /// The two file spellings are left out because both are mechanical — [`Self::SecretsFile`]
    /// is `path` with the separator substituted, and [`Self::EnvFile`] is [`Self::Env`] with the
    /// dialect's documented suffix appended. Neither adds anything the reader cannot derive from
    /// a column already in front of them plus one sentence of prose, and dropping the pair keeps
    /// the table inside a page. Ask for either by name through [`Schema::to_markdown_with`].
    ///
    /// [`Self::Flags`] carries what [`Self::Required`] and [`Self::Secret`] would have taken two
    /// columns to say, and [`Self::Aliases`] is empty for almost every key.
    ///
    /// [`Self::Type`] *is* here, because without it a required key shows an em dash for its
    /// default and the reader has no way to tell whether to supply a string, a number or a list.
    pub const DEFAULT: &'static [Self] = &[
        Self::Path,
        Self::Type,
        Self::Env,
        Self::Default,
        Self::Flags,
        Self::Docs,
    ];

    fn heading(self) -> &'static str {
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
            // Escaped like every other cell. A key path is not prose, but it is not the
            // table author's to choose either — `#[serde(rename = "a|b")]` puts a cell
            // separator in it, and an unescaped one adds a column to the row.
            Self::Path => format!("`{}`", escape(&key.path)),
            // The choices when there are any, because `LogLevel` tells an operator nothing they
            // can act on and `trace | debug | info` tells them exactly what to type. The type
            // name stays in front of them, since it is what they will see in the source.
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
                    "—".to_owned()
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
                    None if key.required => "—".to_owned(),
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
                    "—".to_owned()
                } else {
                    notes.join(", ")
                }
            }
            Self::Required => yes_or_dash(key.required),
            Self::Secret => yes_or_dash(key.secret),
            Self::Note => key.note.as_deref().map_or_else(|| "—".to_owned(), cell),
            Self::Docs => summary_cell(&key.docs),
        }
    }
}

fn yes_or_dash(flag: bool) -> String {
    if flag { "yes" } else { "—" }.to_owned()
}

/// A spelling as inline code, or an em dash when there is none.
fn optional_code(value: Option<&str>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("`{}`", escape(value)))
}

/// Prose in a table cell: newlines become breaks, and `|` stops ending the cell early.
fn cell(text: &str) -> String {
    if text.is_empty() {
        return "—".to_owned();
    }
    escape(text).replace('\n', "<br>")
}

/// A doc comment in a table cell: its summary, on one line.
///
/// The whole comment used to go in, which put every paragraph of a field's rustdoc into one cell
/// and made a table out of an essay. The fix is rustdoc's own convention rather than a new
/// annotation to keep in step: the first paragraph is the summary, and a comment written the way
/// rustdoc asks for one already reads correctly here with nothing to change.
///
/// [`Key::docs`] keeps the whole text, so the JSON contract loses nothing and a pipeline that
/// wants the paragraphs below the summary can still render them.
fn summary_cell(text: &str) -> String {
    let summary = summary(text);
    if summary.is_empty() {
        return "—".to_owned();
    }
    escape(&summary)
}

/// The characters that would otherwise be read as table structure.
fn escape(text: &str) -> String {
    text.replace('\\', r"\\").replace('|', r"\|")
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn a_pipe_in_a_doc_comment_does_not_end_the_cell() {
        assert_eq!(escape("a | b"), r"a \| b");
        assert_eq!(escape(r"a \ b"), r"a \\ b");
    }
}
