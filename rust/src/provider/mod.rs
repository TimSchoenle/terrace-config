//! The three figment providers, each usable on its own against a hand-built `Figment`.
//!
//! Two of them carry configuration values that arrive as *files* rather than as environment
//! variables, both shaped for Kubernetes: a directory of key-named files ([`SecretsDir`] — what
//! a `Secret` mounted as a volume looks like) and per-key indirection ([`FileSuffixEnv`],
//! `MYAPP_<KEY>_FILE=/path` — what Docker Compose `secrets:` looks like).
//!
//! A value in a pod's environment is readable from `/proc/<pid>/environ` by anything sharing
//! the namespace, is inherited by every child process, and is printed by anything that dumps
//! the environment. A mounted file is none of those things, and the kubelet updates it in
//! place when the `Secret` changes, which is what lets a running service pick up a rotated
//! credential without being restarted.
//!
//! **Values from these layers are emitted unparsed, as strings.** `figment::providers::Env`
//! runs a TOML-ish parse over every value, so `12345678` becomes a number — and
//! `Figment::extract` uses figment's default interpreter, which will not coerce a number back
//! into a string, so an all-digit password supplied that way fails to deserialise into
//! `SecretString`. These layers exist to carry secrets, and a secret is an opaque byte string.
//! Anything structured belongs in the TOML layer, which parses it properly.

mod file_suffix;
mod secrets_dir;
mod toml_layers;

pub use file_suffix::FileSuffixEnv;
pub use secrets_dir::SecretsDir;
pub use toml_layers::TomlLayers;

use std::path::{Path, PathBuf};

use figment::value::{Dict, Value};

use crate::error::Error;

/// One value and the file it came from.
///
/// The path is kept so an error can name its source. **No error in this crate ever prints the
/// value**, and neither does [`Debug`]: the whole point of the layer is that the value stays
/// out of anything ambient, and a public type whose derived `Debug` dumps a credential into
/// the first `tracing::debug!` that touches it would give that back.
#[derive(Clone)]
pub struct FileValue {
    /// The file this value was read from.
    path: PathBuf,
    /// The contents, minus trailing line terminators.
    value: String,
}

impl FileValue {
    /// The file this value was read from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The value itself, minus trailing line terminators.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for FileValue {
    /// The path, never the value.
    ///
    /// See the type's own documentation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileValue")
            .field("path", &self.path)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// The contents of one value file, minus trailing line terminators.
///
/// Only `\r` and `\n` are stripped, never spaces or tabs: `printf 'x\n' > f` and every text
/// editor add a newline nobody meant as part of the value, whereas a trailing space can be a
/// real character of a real password.
pub(crate) fn read_value(path: &Path) -> Result<String, Error> {
    let bytes = std::fs::read(path)
        .map_err(|e| Error::source(format!("reading {}: {e}", path.display())))?;
    // Named, not printed: the file holds a secret, so the invalid bytes stay out of the log.
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::source(format!("{} is not valid UTF-8", path.display())))?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

/// Insert `value` at a dot-separated `key` path, creating intermediate dictionaries.
///
/// Written out rather than using `figment::util::nest` because that returns one nested `Value`
/// per key and merging them needs figment's private `Coalescible`.
pub(crate) fn insert_nested(dict: &mut Dict, key: &str, value: Value) {
    match key.split_once('.') {
        None => {
            dict.insert(key.to_owned(), value);
        }
        Some((head, rest)) => {
            let entry = dict
                .entry(head.to_owned())
                .or_insert_with(|| Value::Dict(figment::value::Tag::Default, Dict::new()));
            // A non-dict here means two keys disagree about whether a segment is a leaf
            // (`a` and `a__b`). The later one wins by replacing the leaf, which is what the
            // environment layer does too.
            if !matches!(entry, Value::Dict(..)) {
                *entry = Value::Dict(figment::value::Tag::Default, Dict::new());
            }
            if let Value::Dict(_, inner) = entry {
                insert_nested(inner, rest, value);
            }
        }
    }
}
