//! Turning a document into the artefacts a build and a page want.
//!
//! Every rendering here is a pure function of the document. That is not an incidental property —
//! it is the reason this module can exist at all. A rendering that needed the producer's types
//! would have to be written once per language, and the moment there are two of those, a Java
//! service's README table and a Rust service's differ in whitespace nobody can explain.
//!
//! `spec/v1/conformance/<case>/rendered/` is what holds this honest: the same bytes, checked
//! against this renderer and against every implementation that renders in-process.

pub mod image;
pub mod json_schema;
pub mod markdown;
pub mod toml;
pub mod tree;

use std::fmt;
use std::str::FromStr;

use serde_json::Value as Json;

use crate::Error;
use crate::document::Contract;

pub use json_schema::{DRAFT_07, DRAFT_2020_12, Docs, JsonSchema};
pub use markdown::Column;
pub use toml::TomlExample;

/// One of the renderings a build can ask for.
///
/// Separate from the argument parser so that a consumer who parses arguments some other way — a
/// Gradle plugin, a build script — gets the vocabulary and the [`Self::whole_image`] distinction
/// without an argument parser they did not ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Format {
    /// The schema half, for a pipeline that renders its own tables.
    ///
    /// The default, because it is the rendering that loses nothing: every field of every key is in
    /// it, and the others are derivable from it by a consumer who wants them.
    #[default]
    Json,
    /// GitHub-flavoured tables, for a pipeline whose next step is `>> README.md`.
    Markdown,
    /// The loader's own variables alone, as a table.
    ///
    /// A separate format rather than a section of [`Markdown`](Self::Markdown) because a README
    /// documents the two apart: the layers are prose in one section and the keys are a table in
    /// another, and the variables that *select* the layers belong with the prose.
    MarkdownLoader,
    /// The configuration keys alone, as a table.
    MarkdownKeys,
    /// The commented file an operator copies to `config.toml`.
    Toml,
    /// A JSON Schema, for an editor to validate a configuration file against.
    JsonSchema,
    /// The whole document, which is what a build embeds in its image.
    Contract,
    /// The image labels that make that document discoverable, one `NAME=value` per line.
    Labels,
    /// The same labels as a marked `LABEL` block to paste into a Dockerfile.
    Dockerfile,
}

impl Format {
    /// Every format, in the order a usage message lists them.
    pub const ALL: &'static [Self] = &[
        Self::Json,
        Self::Markdown,
        Self::MarkdownLoader,
        Self::MarkdownKeys,
        Self::Toml,
        Self::JsonSchema,
        Self::Contract,
        Self::Labels,
        Self::Dockerfile,
    ];

    /// The canonical spelling, which is the one `--format` takes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::MarkdownLoader => "markdown-loader",
            Self::MarkdownKeys => "markdown-keys",
            Self::Toml => "toml",
            Self::JsonSchema => "json-schema",
            Self::Contract => "contract",
            Self::Labels => "labels",
            Self::Dockerfile => "dockerfile",
        }
    }

    /// Whether the rendering describes the whole image rather than a slice of its configuration.
    ///
    /// The three that do are the ones an image is *discovered* by. A slice of a configuration
    /// rendered as a contract would claim the image reads only those keys, which is a lie a chart
    /// would then gate on — so a request that slices and asks for one of these is refused rather
    /// than served.
    pub const fn whole_image(self) -> bool {
        matches!(self, Self::Contract | Self::Labels | Self::Dockerfile)
    }

    /// Whether the rendering carries configuration keys at all.
    ///
    /// [`MarkdownLoader`](Self::MarkdownLoader) does not, so slicing does not apply to it.
    pub const fn reads_keys(self) -> bool {
        !matches!(self, Self::MarkdownLoader)
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `--format` value that names no rendering.
///
/// Lists what does exist rather than only what does not: the vocabulary is small enough to print,
/// and a misspelling is by far the likeliest cause of getting here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFormat(pub String);

impl fmt::Display for UnknownFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a format. Try one of: ", self.0)?;
        for (index, format) in Format::ALL.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(format.as_str())?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownFormat {}

impl FromStr for Format {
    type Err = UnknownFormat;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .find(|format| format.as_str() == text)
            .copied()
            .ok_or_else(|| UnknownFormat(text.to_owned()))
    }
}

/// How a rendering was asked for: the parts that are the caller's rather than the document's.
#[derive(Debug, Clone)]
pub struct Options<'a> {
    /// Where the document lives inside the image, for the two label formats.
    pub path: &'a str,
    /// Which columns the key table carries.
    pub columns: &'a [Column],
    /// How the JSON Schema rendering is shaped.
    pub json_schema: JsonSchema,
    /// How the TOML rendering is shaped.
    pub toml_example: TomlExample,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            path: crate::document::DEFAULT_PATH,
            columns: Column::DEFAULT,
            json_schema: JsonSchema::default(),
            toml_example: TomlExample::default(),
        }
    }
}

/// Render one document.
///
/// The returned string is whatever the rendering produced, and the renderings disagree about a
/// trailing newline. Nothing is normalised here, because a caller writing into a buffer wants what
/// was rendered; [`one_newline`] is where it becomes exactly one, for the file a shell redirects
/// into.
///
/// # Errors
/// [`Error::Invalid`] when a rendering that serialises JSON fails to.
pub fn render(contract: &Contract, format: Format, options: &Options<'_>) -> Result<String, Error> {
    Ok(match format {
        Format::Json => to_json_pretty(&contract.schema, "schema")?,
        Format::Markdown => markdown::markdown(&contract.schema, options.columns),
        Format::MarkdownLoader => markdown::markdown_loader(&contract.schema),
        Format::MarkdownKeys => markdown::markdown_keys(&contract.schema, options.columns),
        Format::Toml => toml::toml_example(&contract.schema, &options.toml_example),
        Format::JsonSchema => to_json_pretty(
            &Json::Object(json_schema::document(
                &contract.schema,
                &options.json_schema,
            )),
            "JSON Schema",
        )?,
        Format::Contract => to_json_pretty(contract, "contract")?,
        Format::Labels => image::labels(contract, options.path),
        Format::Dockerfile => image::dockerfile_block(contract, options.path),
    })
}

/// Exactly one trailing newline, whatever the rendering ended with.
///
/// The renderings disagree: the tables end in one, the labels do not. A build redirects this into
/// a committed file, and a trailing blank line is invisible on a terminal and a diff in the file.
pub fn one_newline(mut text: String) -> String {
    while text.ends_with('\n') {
        text.pop();
    }
    text.push('\n');
    text
}

/// Pretty-printed JSON, two-space indented, in the document's own field order.
///
/// The order matters more than it looks: `serde_json`'s `preserve_order` keeps the `json_schema`
/// half as the producer wrote it, and the model's field order mirrors the meta-schema's. Together
/// those make `--format contract` a byte-for-byte round trip of the document it was given, which
/// is what lets `stamp` change the build identity and nothing else.
fn to_json_pretty<T: serde::Serialize>(value: &T, what: &str) -> Result<String, Error> {
    serde_json::to_string_pretty(value)
        .map_err(|e| Error::Invalid(format!("the {what} could not be written as JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::Format;

    #[test]
    fn every_format_round_trips_through_its_spelling() {
        for format in Format::ALL {
            assert_eq!(
                format
                    .as_str()
                    .parse::<Format>()
                    .expect("a canonical spelling parses"),
                *format
            );
        }
    }

    #[test]
    fn an_unknown_format_lists_the_ones_that_exist() {
        let error = "yaml"
            .parse::<Format>()
            .expect_err("yaml is not a rendering");
        assert!(error.to_string().contains("markdown-keys"), "{error}");
    }
}
