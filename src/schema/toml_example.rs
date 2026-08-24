//! The `config.example.toml` rendering: the file an operator edits, generated rather than kept.
//!
//! The artefact this replaces is the one that drifts fastest. A reference table drifts and reads
//! wrong; an example file drifts and is *copied* — into a deployment, as the config the service
//! actually loads — so a key added six months after the example was written is a key nobody in
//! that deployment knows exists.
//!
//! Two things separate this from the Markdown rendering, and both come from the output being a
//! file rather than a page:
//!
//! - **It has to parse.** A table cell reads better as `public` than as `"public"`; a file that
//!   says `dist_dir = public` does not load. Every value here comes from
//!   [`Key::default_value`], which is the value rather than a picture of it, and is written as
//!   the TOML literal it is.
//! - **It has to be safe to commit.** A [`secret`](Key::secret) key is rendered as a placeholder
//!   whatever its default was, with the environment and secrets-directory spellings that should
//!   carry the real value on the line above it.
//!
//! Everything with a default is commented out, so that the generated file and an empty file mean
//! the same thing to the loader. What is left uncommented is exactly what has to be filled in.

use std::fmt::Write as _;

use super::tree::{self, Node};
use super::{Docs, Key, Schema};

/// The placeholder written in place of a secret's value.
const SECRET: &str = "<secret>";

/// The placeholder written where a key has no default to show.
const VALUE: &str = "<value>";

/// The column the comments this module writes wrap at.
///
/// A generated file is read in a terminal beside the service's logs, not only in an editor that
/// soft wraps: a spellings line naming three variables for a three-level key runs past 160
/// columns unwrapped, and what falls off the right is the part an operator was looking for.
const WIDTH: usize = 96;

/// How [`Schema::to_toml_example`] renders.
///
/// The default is the whole file: a preamble naming the variables the loader reads, and every key
/// under its purpose, its type and its spellings. Turn parts off for a shorter file — a service
/// whose README already carries the reference table wants
/// `TomlExample::new().spellings(false)`, and one whose configuration is generated into a
/// container image wants no preamble.
///
/// ```
/// # use terrace_config::Terrace;
/// # use terrace_config::schema::{Describe, Docs, Leaf, Sink, TomlExample};
/// # struct Config;
/// # impl Describe for Config {
/// #     fn describe(sink: &mut Sink) {
/// #         sink.leaf(Leaf { name: "ttl_secs", docs: "How long a page is served.\n\nThe rest.",
/// #             ty: Some("u64"), values: None, aliases: &[], note: None, required: false,
/// #             secret: false });
/// #     }
/// # }
/// let options = TomlExample::new().header(false).docs(Docs::None).spellings(false);
/// let example = Terrace::new("MYSERVICE_").schema::<Config>().to_toml_example_with(&options);
///
/// let expected = concat!(
///     "# Type: u64\n",
///     "# Unset by default: the value below is only the shape.\n",
///     "# ttl_secs = 0\n",
/// );
/// assert_eq!(example, expected);
/// ```
#[derive(Debug, Clone)]
pub struct TomlExample {
    /// The preamble: what the file is, and the variables read before it exists.
    header: bool,
    /// How much of each key's `///` comment to carry.
    docs: Docs,
    /// The other spellings that supply each key.
    spellings: bool,
    /// What a secret's value is written as.
    secret: String,
    /// What a key with no default is written as, when its type suggests nothing better.
    placeholder: String,
}

impl Default for TomlExample {
    fn default() -> Self {
        Self {
            header: true,
            docs: Docs::Summary,
            spellings: true,
            secret: SECRET.to_owned(),
            placeholder: VALUE.to_owned(),
        }
    }
}

impl TomlExample {
    /// The whole file: preamble, summaries, and every spelling.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether to emit the preamble.
    ///
    /// Defaults to `true`.
    #[must_use]
    pub fn header(mut self, header: bool) -> Self {
        self.header = header;
        self
    }

    /// How much of each key's `///` comment to carry.
    ///
    /// Defaults to [`Docs::Summary`].
    #[must_use]
    pub fn docs(mut self, docs: Docs) -> Self {
        self.docs = docs;
        self
    }

    /// Whether to name the environment and file spellings above each key.
    ///
    /// Defaults to `true`.
    #[must_use]
    pub fn spellings(mut self, spellings: bool) -> Self {
        self.spellings = spellings;
        self
    }

    /// What a [`secret`](Key::secret) key is written as.
    ///
    /// Defaults to `<secret>`.
    ///
    /// Written as a TOML string whatever it says, because a secret is an opaque byte string
    /// everywhere else in this crate and a placeholder that changed its type would not be one.
    #[must_use]
    pub fn secret_placeholder(mut self, secret: impl Into<String>) -> Self {
        self.secret = secret.into();
        self
    }

    /// What a key with no default is written as.
    ///
    /// Defaults to `<value>`.
    ///
    /// Every key with nothing to show, not only a required one: an optional key with no default
    /// has no value either, and is written with the placeholder above a line saying so.
    ///
    /// Only reached when the key's type suggests nothing better: a `bool` with no default is
    /// written as `false` and a `Vec<_>` as `[]`, because those are values of the right type that
    /// are still obviously not answers. A type this crate does not recognise has no such value,
    /// so the placeholder goes in as a string — wrong-typed on purpose, so that a file left
    /// unedited fails at the key that was never filled in rather than at whatever it broke later.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }
}

impl Schema {
    /// The schema as a commented `config.toml`, ready to be copied and edited.
    ///
    /// Every key, in declaration order, grouped into the TOML tables its path describes. Each one
    /// carries its purpose, its type, and the other spellings that supply it, above an assignment
    /// showing the value it already has:
    ///
    /// ```toml
    /// [github]
    /// # Bearer token lifting the GitHub API rate limit.
    /// # Type: String
    /// # Also from: PORTFOLIO_GITHUB__TOKEN, PORTFOLIO_GITHUB__TOKEN_FILE=/path/to/file,
    /// #   github__token in the secrets directory
    /// # Secret: the value below is a placeholder.
    /// # token = "<secret>"
    /// ```
    ///
    /// A key is commented out when leaving it out changes nothing — it has a default, or the
    /// loader [reserves](Key::reserved) it and no file may supply it. What is left uncommented is
    /// exactly the set of [`required`](Key::required) keys, so the generated file is both a
    /// complete reference and the shortest file that loads.
    ///
    /// Use [`Self::to_toml_example_with`] for a shorter one. Ends with a newline unless it is
    /// empty, which only a schema with no keys and no preamble is.
    #[must_use]
    pub fn to_toml_example(&self) -> String {
        self.to_toml_example_with(&TomlExample::default())
    }

    /// The same file, with a chosen set of parts.
    ///
    /// See [`TomlExample`].
    #[must_use]
    pub fn to_toml_example_with(&self, options: &TomlExample) -> String {
        let mut blocks = Vec::new();
        if options.header {
            blocks.push(self.toml_preamble());
        }
        let root = Node::of(&self.keys);
        collect(&mut blocks, &root, "", options);
        blocks.join("\n")
    }

    /// What the file is, and the variables that decide whether it is read at all.
    fn toml_preamble(&self) -> String {
        let prefix = &self.dialect.prefix;
        let suffix = &self.dialect.indirection_suffix;
        let mut out = String::new();

        paragraph(
            &mut out,
            &format!("Configuration for a service reading {prefix}-prefixed keys."),
        );
        comment(&mut out, "");
        paragraph(
            &mut out,
            "Generated from the configuration type, so it lists every key that type can carry              and nothing else. Each key shows the value it already has, commented out: a              commented key and a deleted key mean the same thing to the loader, so uncomment one              only to change it. A key that is not commented out has no default, and nothing              loads until something supplies it.",
        );
        comment(&mut out, "");
        paragraph(
            &mut out,
            &format!(
                "Three layers can supply any key below, and all three win over this file: the                  environment variable named above the key, a file named by that variable plus                  `{suffix}`, and a key-named file in the secrets directory. A secret belongs in                  one of those — this file is usually committed."
            ),
        );

        if !self.loader.is_empty() {
            comment(&mut out, "");
            comment(&mut out, "Read before this file exists:");
            for var in &self.loader {
                comment(&mut out, "");
                let default = var
                    .default
                    .as_ref()
                    .map_or_else(String::new, |default| format!(", default `{default}`"));
                comment(
                    &mut out,
                    &format!("  {} — {}{default}", var.env, var.role.label()),
                );
                // Indented under the variable it belongs to, so the block reads as a list rather
                // than as one paragraph that happens to contain variable names.
                flowed(&mut out, "    ", "    ", &super::summary(&var.docs));
            }
        }

        out
    }
}

/// Push this level's block, then every level below it.
///
/// Depth first and leaves first, which is what TOML requires rather than a preference: a bare
/// assignment belongs to the table header above it, so a key written after `[csp.cloudflare]`
/// would land in `csp.cloudflare` however it was declared.
fn collect(blocks: &mut Vec<String>, node: &Node<'_>, header: &str, options: &TomlExample) {
    let mut block = String::new();
    if !header.is_empty() {
        let _ = writeln!(block, "[{header}]");
    }
    let keys: Vec<String> = node
        .keys
        .iter()
        .map(|key| key_block(key, node, options))
        .collect();
    block.push_str(&keys.join("\n"));

    if !block.is_empty() {
        blocks.push(block);
    }

    for child in &node.children {
        let segment = toml_key(child.segment);
        let child_header = if header.is_empty() {
            segment
        } else {
            format!("{header}.{segment}")
        };
        collect(blocks, child, &child_header, options);
    }
}

/// One key: what it is for, above what it is set to.
fn key_block(key: &Key, parent: &Node<'_>, options: &TomlExample) -> String {
    let mut out = String::new();

    if let Some(docs) = options.docs.of(&key.docs) {
        for line in docs.lines() {
            comment(&mut out, line);
        }
    }

    match (key.ty.as_deref(), key.values.as_slice()) {
        (Some(ty), []) => comment(&mut out, &format!("Type: {ty}")),
        (Some(ty), values) => comment(
            &mut out,
            &format!("Type: {ty} — one of: {}", values.join(", ")),
        ),
        (None, []) => {}
        (None, values) => comment(&mut out, &format!("One of: {}", values.join(", "))),
    }

    if !key.aliases.is_empty() {
        comment(
            &mut out,
            &format!("Also accepted as: {}", key.aliases.join(", ")),
        );
    }

    if key.reserved {
        let env = key.env.as_deref().unwrap_or("the environment");
        wrapped(
            &mut out,
            &format!("Reserved: only {env} supplies this key; a file may not."),
        );
    } else if options.spellings {
        wrapped(&mut out, &spellings(key));
    }

    if key.required && !key.reserved {
        comment(
            &mut out,
            "Required: nothing loads until this key is supplied.",
        );
    }
    if key.secret {
        comment(&mut out, "Secret: the value below is a placeholder.");
    } else if !key.required && key.default_value.is_none() {
        // Without this the line below reads as a default. `repos = []` is the *shape* of the
        // value and not what the key is when nothing sets it, which for an optional key is
        // nothing at all — the distinction the whole `Option<Vec<_>>` spelling exists to make.
        comment(
            &mut out,
            "Unset by default: the value below is only the shape.",
        );
    }

    // The one shape TOML cannot write down: a key and a table of the same name in one parent.
    // It means a field wanted `#[config(nested)]` and did not get it, and the table below is the
    // half a reader can act on — so the key is commented out and told why.
    let name = tree::name(&key.path);
    let shadowed = parent.opens(name);
    if shadowed {
        wrapped(
            &mut out,
            "Shadowed by the table of the same name below: TOML cannot carry both.",
        );
    }

    // Commented unless leaving it out would stop the file loading. A reserved key is commented
    // whatever else it is, because no file supplies one.
    if !key.required || key.reserved || shadowed {
        out.push_str("# ");
    }
    let _ = writeln!(out, "{} = {}", toml_key(name), literal(key, options));

    out
}

/// The other ways this key can be supplied, as one comment line.
///
/// Never asked of a [`reserved`](Key::reserved) key: no file supplies one, so the only thing
/// worth saying about it is that.
fn spellings(key: &Key) -> String {
    let mut ways = Vec::new();
    if let Some(env) = &key.env {
        ways.push(env.clone());
    }
    if let Some(env_file) = &key.env_file {
        ways.push(format!("{env_file}=/path/to/file"));
    }
    if let Some(secrets_file) = &key.secrets_file {
        ways.push(format!("{secrets_file} in the secrets directory"));
    }
    if ways.is_empty() {
        // Not a footnote: `#[serde(rename_all = "camelCase")]` produces exactly this, and an
        // operator who assumed the usual environment spelling would set a variable that does
        // nothing at all.
        return "Only this file supplies this key: no environment or secrets-directory spelling \
                reaches it."
            .to_owned();
    }
    format!("Also from: {}", ways.join(", "))
}

/// The value written for a key: its default, a redaction, or a placeholder of the right shape.
fn literal(key: &Key, options: &TomlExample) -> String {
    if key.secret {
        return toml_string(&options.secret);
    }
    if let Some(value) = &key.default_value
        && let Some(literal) = toml_literal(value, 0)
    {
        return literal;
    }
    placeholder(key.ty.as_deref(), &options.placeholder)
}

/// A value of the key's type that is still obviously not an answer.
fn placeholder(ty: Option<&str>, text: &str) -> String {
    let interpreted = ty.and_then(super::rust_type::interpret);
    let shape = interpreted
        .as_ref()
        .and_then(|schema| schema.get("type"))
        .and_then(serde_json::Value::as_str);

    match shape {
        Some("boolean") => "false".to_owned(),
        Some("integer") => "0".to_owned(),
        Some("number") => "0.0".to_owned(),
        Some("array") => "[]".to_owned(),
        Some("object") => "{}".to_owned(),
        _ => toml_string(text),
    }
}

/// A default value as TOML, or [`None`] for one TOML cannot carry.
///
/// The counterpart to [`super::render_value`], which renders the same values for a table cell:
/// this one quotes, escapes and refuses, because what it writes has to parse back into the value
/// it came from.
fn toml_literal(value: &figment::value::Value, depth: usize) -> Option<String> {
    use figment::value::{Empty, Value};

    // The same bound as `Sink::nested`, for the same reason: a default deep enough to overflow
    // the stack here would be a denial of service in a documentation generator.
    if depth > super::MAX_DEPTH {
        return None;
    }

    Some(match value {
        Value::String(_, string) => toml_string(string),
        Value::Char(_, character) => toml_string(&character.to_string()),
        Value::Bool(_, boolean) => boolean.to_string(),
        Value::Num(_, number) => toml_number(*number)?,
        // TOML has no null: an absent key *is* the absent value, so there is nothing to write.
        Value::Empty(_, Empty::None | Empty::Unit) => return None,
        Value::Array(_, items) => {
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                // An array element cannot be left out the way a table entry can — dropping one
                // would write a shorter array than the default actually is.
                rendered.push(toml_literal(item, depth + 1)?);
            }
            format!("[{}]", rendered.join(", "))
        }
        Value::Dict(_, dict) => {
            let rendered: Vec<String> = dict
                .iter()
                // An entry with no value is an absent entry, which is what leaving it out says.
                .filter_map(|(key, value)| {
                    Some(format!(
                        "{} = {}",
                        toml_key(key),
                        toml_literal(value, depth + 1)?
                    ))
                })
                .collect();
            if rendered.is_empty() {
                "{}".to_owned()
            } else {
                format!("{{ {} }}", rendered.join(", "))
            }
        }
    })
}

/// A number as TOML, or [`None`] for one TOML cannot hold.
///
/// TOML integers are 64-bit and signed, so a `u64` or `u128` default above `i64::MAX` has no
/// spelling in the file at all. Writing it anyway would produce an example that fails to parse,
/// which is worse than an example that leaves one default out.
fn toml_number(number: figment::value::Num) -> Option<String> {
    use figment::value::Num;

    let signed: i128 = match number {
        Num::U8(value) => value.into(),
        Num::U16(value) => value.into(),
        Num::U32(value) => value.into(),
        Num::U64(value) => value.into(),
        Num::USize(value) => value as i128,
        Num::U128(value) => i128::try_from(value).ok()?,
        Num::I8(value) => value.into(),
        Num::I16(value) => value.into(),
        Num::I32(value) => value.into(),
        Num::I64(value) => value.into(),
        Num::ISize(value) => value as i128,
        Num::I128(value) => value,
        Num::F32(value) => return Some(toml_float(value.into())),
        Num::F64(value) => return Some(toml_float(value)),
    };
    i64::try_from(signed).ok().map(|value| value.to_string())
}

/// A float as TOML: never bare digits, because bare digits are a TOML *integer*.
fn toml_float(value: f64) -> String {
    if value.is_nan() {
        // Rust prints `NaN`; TOML spells it `nan`, and the sign is not meaningful either way.
        return "nan".to_owned();
    }
    let rendered = value.to_string();
    if rendered.contains(['.', 'e', 'E']) || value.is_infinite() {
        return rendered;
    }
    format!("{rendered}.0")
}

/// A TOML basic string: quoted, with everything the format cannot carry raw escaped.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            // Every other control character, which a basic string may not carry unescaped.
            character if character < ' ' || character == '\u{7f}' => {
                let _ = write!(out, "\\u{:04X}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A key name as TOML spells it: bare where it can be, quoted where it cannot.
///
/// `#[serde(rename = "…")]` accepts any string at all, and a bare key accepts none of the
/// interesting ones. Quoting is not cosmetic here — an unquoted `a.b` is two levels of nesting.
fn toml_key(name: &str) -> String {
    let bare = !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        });
    if bare {
        name.to_owned()
    } else {
        toml_string(name)
    }
}

/// One comment line, with no trailing space on an empty one.
///
/// A comment is the one part of this file that carries text nobody chose: a `///` comment, a key
/// name, an alias. TOML permits no control character but tab in a comment, so what cannot be
/// written raw is written as an escape — the same escape a quoted key two lines below would use,
/// so the two spellings of one name read as the same name.
///
/// Found by the `schema` fuzz oracle, which described a key whose name contained a NUL and got
/// back an example that does not parse.
fn comment(out: &mut String, text: &str) {
    if text.is_empty() {
        out.push_str("#\n");
        return;
    }
    out.push_str("# ");
    for character in text.chars() {
        if character == '\t' || (character >= ' ' && character != '\u{7f}') {
            out.push(character);
        } else {
            let _ = write!(out, "\\u{:04X}", character as u32);
        }
    }
    out.push('\n');
}

/// Prose as comment lines, wrapped flush left.
fn paragraph(out: &mut String, text: &str) {
    flowed(out, "", "", text);
}

/// One assembled line, wrapped, with a hanging indent marking what is a continuation.
///
/// The indent is what separates a spellings line that ran long from the next thing said about
/// the key, which is the only reason these two differ.
fn wrapped(out: &mut String, text: &str) {
    flowed(out, "", "  ", text);
}

/// As [`wrapped`], with a chosen indent on the first line and on the rest.
///
/// Wrapping is for the text this module writes and for the spellings it assembles, both of which
/// are one paragraph with no structure to lose. A `///` comment is *not* wrapped here: its line
/// breaks are the author's, and a fenced block inside one does not survive being reflowed.
fn flowed(out: &mut String, first: &str, rest: &str, text: &str) {
    let mut line = String::new();
    let mut indent = first;
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > WIDTH {
            comment(out, &line);
            line.clear();
            indent = rest;
        }
        if line.is_empty() {
            line.push_str(indent);
        } else {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        comment(out, &line);
    }
}

#[cfg(test)]
mod tests {
    use figment::value::{Dict, Value};

    use super::{toml_key, toml_literal, toml_string};

    /// A string default that lost its quotes does not parse, and one that kept an unescaped
    /// backslash parses as something else.
    #[test]
    fn a_string_is_quoted_and_escaped() {
        assert_eq!(toml_string("public"), r#""public""#);
        assert_eq!(toml_string(r"C:\logs"), r#""C:\\logs""#);
        assert_eq!(toml_string("a\nb"), r#""a\nb""#);
        assert_eq!(toml_string("\u{1}"), r#""\u0001""#);
    }

    /// An unquoted key containing a dot is two levels of nesting, which is a different key.
    #[test]
    fn a_key_is_quoted_when_it_has_to_be() {
        assert_eq!(toml_key("dist_dir"), "dist_dir");
        assert_eq!(toml_key("a.b"), r#""a.b""#);
        assert_eq!(toml_key(""), r#""""#);
    }

    /// Bare digits are a TOML integer, and a float key given one fails to deserialise.
    #[test]
    fn a_whole_float_keeps_a_decimal_point() {
        let whole = Value::from(1.0_f64);
        assert_eq!(toml_literal(&whole, 0).as_deref(), Some("1.0"));
    }

    /// TOML integers are signed and 64-bit, so this one has no spelling rather than a wrong one.
    #[test]
    fn an_integer_too_large_for_toml_is_left_out() {
        let huge = Value::from(u64::MAX);
        assert_eq!(toml_literal(&huge, 0), None);
    }

    /// An absent entry is what an absent value means in TOML, so the table keeps the rest.
    #[test]
    fn a_dict_drops_the_entries_toml_cannot_carry() {
        let mut dict = Dict::new();
        dict.insert("kept".to_owned(), Value::from("yes"));
        dict.insert(
            "dropped".to_owned(),
            Value::serialize(Option::<u8>::None).unwrap(),
        );
        assert_eq!(
            toml_literal(&Value::from(dict), 0).as_deref(),
            Some(r#"{ kept = "yes" }"#)
        );
    }

    /// An array cannot lose an element the way a table can lose an entry.
    #[test]
    fn an_array_with_an_absent_element_has_no_spelling() {
        let array = Value::from(vec![
            Value::from("a"),
            Value::serialize(Option::<u8>::None).unwrap(),
        ]);
        assert_eq!(toml_literal(&array, 0), None);
    }
}
