//! Which rendering to emit.

use std::fmt;
use std::str::FromStr;

/// One of the renderings a generator can be asked for.
///
/// Separate from [`Request`](super::Request) so that a consumer who parses arguments with `clap`
/// or reads a format out of a build file gets the vocabulary — the spellings, the aliases and the
/// [`Self::whole_image`] distinction — without also getting an argument parser they did not ask
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Format {
    /// The versioned schema, for a pipeline that renders its own tables.
    ///
    /// The default, because it is the rendering that loses nothing: every field of every key is
    /// in it, and the other five are derivable from it by a consumer who wants them.
    #[default]
    Json,
    /// GitHub-flavoured tables, for a pipeline whose next step is `>> README.md`.
    Markdown,
    /// The commented file an operator copies to `config.toml`.
    Toml,
    /// A JSON Schema, for an editor to validate that file against.
    JsonSchema,
    /// The document a build embeds in its image and attaches to its digest.
    Contract,
    /// The image labels that make that document discoverable, one `NAME=value` per line.
    Labels,
    /// The same labels as a marked `LABEL` block to paste into a Dockerfile.
    Dockerfile,
}

impl Format {
    /// Every format, in the order [`USAGE`](super::USAGE) lists them.
    pub const ALL: &'static [Self] = &[
        Self::Json,
        Self::Markdown,
        Self::Toml,
        Self::JsonSchema,
        Self::Contract,
        Self::Labels,
        Self::Dockerfile,
    ];

    /// The canonical spelling, which is the one [`ALL`](Self::ALL) and `--format` use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Toml => "toml",
            Self::JsonSchema => "json-schema",
            Self::Contract => "contract",
            Self::Labels => "labels",
            Self::Dockerfile => "dockerfile",
        }
    }

    /// Whether this rendering describes a whole image rather than a slice of a configuration.
    ///
    /// The three build outputs are one set and they are claims about an image: the document says
    /// what the image reads, the labels say where to find the document, and the `LABEL` block is
    /// those labels in the form a Dockerfile can carry. A contract built from a subset would
    /// assert that the image does not read the keys the subset cut, which is the one claim in the
    /// document that must never be wrong — so [`Request::parse`](super::Request::parse) refuses
    /// the combination rather than publishing it.
    #[must_use]
    pub const fn whole_image(self) -> bool {
        matches!(self, Self::Contract | Self::Labels | Self::Dockerfile)
    }
}

impl FromStr for Format {
    type Err = UnknownFormat;

    /// Accepts the canonical spelling of each format and the two abbreviations that get typed
    /// anyway — `md` and `jsonschema`. Nothing else, because a format nobody meant is better
    /// refused than guessed: the output is redirected into a committed file.
    fn from_str(spelling: &str) -> Result<Self, Self::Err> {
        match spelling {
            "json" => Ok(Self::Json),
            "markdown" | "md" => Ok(Self::Markdown),
            "toml" => Ok(Self::Toml),
            "json-schema" | "jsonschema" => Ok(Self::JsonSchema),
            "contract" => Ok(Self::Contract),
            "labels" => Ok(Self::Labels),
            "dockerfile" => Ok(Self::Dockerfile),
            other => Err(UnknownFormat(other.to_owned())),
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `--format` value that names no rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownFormat(pub String);

impl fmt::Display for UnknownFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown format `{}`", self.0)
    }
}

impl std::error::Error for UnknownFormat {}
