//! Shared input parsing and oracle helpers.
//!
//! Every oracle takes **readable text** rather than a derived `Arbitrary` struct. Seeds stay
//! greppable, a crash artefact can be read without a decoder, and mutation time is not spent
//! generating a prefix that `Env::prefixed` immediately discards. This module is the grammar
//! they share, plus the containment guards that keep a campaign inside its temporary directory.

use std::path::Path;

use figment::value::Value;

/// The prefix every target's dialect uses. Supplied by the target rather than by the input.
pub const PREFIX: &str = "TEST_";

/// The most directives one iteration will act on.
///
/// An unbounded input here becomes an unbounded `set_env` loop or an unbounded number of files
/// written per iteration, which turns a fuzz campaign into a disk-fill.
pub const MAX_DIRECTIVES: usize = 48;

/// The longest file name a target will create.
pub const MAX_NAME_LEN: usize = 100;

/// The longest file body a target will write.
pub const MAX_CONTENT_LEN: usize = 4096;

/// One line of input, after parsing.
#[derive(Debug)]
pub enum Directive<'a> {
    /// `f:<name>=<content>` — a file in the directory under test.
    File {
        /// Verbatim from the input, and not yet through [`is_safe_name`].
        name: &'a str,
        /// Unescaped, so one input line can describe a file body holding newlines.
        content: String,
    },
    /// `e:<SUFFIX>=<value>` — the environment variable `TEST_<SUFFIX>`.
    Env {
        /// Appended to [`PREFIX`] verbatim, dialect separator and all.
        suffix: &'a str,
        /// The value the variable is set to, verbatim: a variable holds one line, so there is
        /// nothing to unescape.
        value: &'a str,
    },
    /// `p:<SUFFIX>=<content>` — a `TEST_<SUFFIX>_FILE` indirection, pointing at a file this
    /// target creates and names.
    ///
    /// The path is **never** taken from the input. An indirection variable holds a path, and a
    /// fuzzer-chosen one would have the target reading arbitrary files on the host machine.
    Indirect {
        /// The key half, between [`PREFIX`] and the `_FILE` suffix.
        suffix: &'a str,
        /// What lands in the file the variable points at, unescaped as for [`Self::File`].
        content: String,
    },
}

/// Parse the line grammar, skipping anything that does not fit it.
///
/// A malformed line is skipped rather than rejected so that a mutation which corrupts one line
/// still exercises the rest, instead of the whole iteration collapsing to a no-op.
pub fn directives(data: &str) -> impl Iterator<Item = Directive<'_>> {
    data.lines()
        .take(MAX_DIRECTIVES)
        .filter_map(|line| match line.split_once(':') {
            Some(("f" | "t", rest)) => {
                rest.split_once('=').map(|(name, content)| Directive::File {
                    name,
                    content: unescape(content),
                })
            }
            Some(("e", rest)) => rest
                .split_once('=')
                .map(|(suffix, value)| Directive::Env { suffix, value }),
            Some(("p", rest)) => {
                rest.split_once('=')
                    .map(|(suffix, content)| Directive::Indirect {
                        suffix,
                        content: unescape(content),
                    })
            }
            _ => None,
        })
}

/// Decode `\n`, `\r` and `\\` so a single input line can describe a multi-line file body.
///
/// Without this the trailing-terminator contract — the one rule about *which* bytes a value
/// loses — would be unreachable from a line-oriented grammar, which is the one rule most worth
/// reaching.
pub fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            // An escaped backslash, and a trailing backslash with nothing after it, both mean
            // one literal backslash.
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Whether a fuzzer-supplied name may be used as a file name inside the jail.
///
/// **This is a containment guard, not a property under test.** The names it rejects are the ones
/// that would escape the temporary directory or fail at the platform layer, and a fuzzer that
/// found one would be reporting a bug in this harness rather than in the crate. Path separators,
/// `..`, drive-letter colons and the Windows device names are all refused; everything else,
/// Unicode included, is allowed through, because a `Secret` key really can be any of it.
pub fn is_safe_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    // `<>:"|?*` are illegal on Windows and the separators would escape the directory. A
    // trailing space or dot is silently stripped by Win32, which would make the name written
    // differ from the name asserted about.
    if name.contains(['\0', '/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.ends_with(' ')
        || name.ends_with('.') && name != ".."
    {
        return false;
    }
    // `CON`, `NUL`, `COM1`… resolve to devices on Windows regardless of extension.
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Whether a fuzzer-supplied environment name and value can be set at all.
///
/// `set_var` panics on a NUL in either half and on `=` in the name — platform preconditions,
/// not loader behaviour, so reaching them would prove nothing.
pub fn is_safe_env(suffix: &str, value: &str) -> bool {
    !suffix.is_empty()
        && suffix.len() <= MAX_NAME_LEN
        && !suffix.contains(['\0', '='])
        && !value.contains('\0')
        && value.len() <= MAX_CONTENT_LEN
}

/// Write `content` to `dir/name`, returning whether it landed.
///
/// A refusal by [`is_safe_name`] or by the platform is not a finding: the target simply does not
/// assert anything about a file it never created.
pub fn write_file(dir: &Path, name: &str, content: &str) -> bool {
    if !is_safe_name(name) || content.len() > MAX_CONTENT_LEN {
        return false;
    }
    std::fs::write(dir.join(name), content).is_ok()
}

/// Follow a dot-separated key path through an extracted figment value.
pub fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        let Value::Dict(_, dict) = current else {
            return None;
        };
        current = dict.get(segment)?;
    }
    Some(current)
}

/// Whether `value` contains `needle` as a key anywhere in its tree.
///
/// Used by the skip contracts: a target plants a sentinel under a name the loader must ignore,
/// then asserts the name never surfaces. Checking the whole tree rather than one path means the
/// assertion still holds if a future bug nests the leak somewhere unexpected.
pub fn contains_key(value: &Value, needle: &str) -> bool {
    match value {
        Value::Dict(_, dict) => dict
            .iter()
            .any(|(key, child)| key == needle || contains_key(child, needle)),
        Value::Array(_, items) => items.iter().any(|item| contains_key(item, needle)),
        _ => false,
    }
}

/// Whether `value` contains `needle` as a string leaf anywhere in its tree.
///
/// The companion to [`contains_key`], for a sentinel whose *name* would be rewritten on the way
/// in. A planted `.sentinel` reached by stripping its dot rather than skipping it arrives under
/// a perfectly ordinary key; only its contents give it away.
pub fn contains_value(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(_, text) => text == needle,
        Value::Dict(_, dict) => dict.values().any(|child| contains_value(child, needle)),
        Value::Array(_, items) => items.iter().any(|item| contains_value(item, needle)),
        _ => false,
    }
}

/// The value a file's contents resolve to: everything but the trailing line terminators.
///
/// The contract restated rather than called through, so the target is comparing against an
/// independent statement of the rule instead of against the implementation of it.
pub fn expected_value(content: &str) -> &str {
    content.trim_end_matches(['\r', '\n'])
}
