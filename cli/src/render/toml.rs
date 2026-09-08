//! The `config.example.toml` rendering: the file an operator edits, generated rather than kept.
//!
//! The artefact this replaces is the one that drifts fastest. A reference table drifts and reads
//! wrong; an example file drifts and is *copied* — into a deployment, as the config the service
//! actually loads — so a key added six months after the example was written is a key nobody in
//! that deployment knows exists.
//!
//! Two things separate this from the Markdown renderings, and both come from the output being a
//! file rather than a page:
//!
//! - **It has to parse.** A table cell reads better as `public` than as `"public"`; a file that
//!   says `dist_dir = public` does not load. Every value here comes from
//!   [`Key::default_value`](crate::Key::default_value), which is the value rather than a picture
//!   of it, and is written as the TOML literal it is.
//! - **It has to be safe to commit.** A secret key is rendered as a placeholder whatever its
//!   default was, with the environment and secrets-directory spellings that should carry the real
//!   value on the line above it.
//!
//! Everything with a default is commented out, so the generated file and an empty file mean the
//! same thing to the loader. What is left uncommented is exactly what has to be filled in.
//!
//! # The one place this renderer improves on the producer it was ported from
//!
//! A key with no default gets a placeholder of the right *shape* — `0` for a number, `[]` for a
//! list. The Rust implementation picks that shape by interpreting `Key::ty`, because inside a
//! producer the type name is a fact about a type it can see. Here it would be a fact about a
//! language: `ty` is token text in the producer's own vocabulary, and a `match` arm spelling
//! `Vec<T>` produces `"<value>"` for the `java.util.List` that means exactly the same thing.
//!
//! So the shape is read from [`Key::constraint`](crate::Key::constraint) instead, which is the
//! same answer arrived at by the producer that *did* have the type — and it is right for every
//! producer rather than for one.

use std::fmt::Write as _;

use serde_json::Value as Json;

use super::json_schema::Docs;
use super::tree::{self, Node};
use crate::document::{Key, Schema};

/// The placeholder written in place of a secret's value.
const SECRET: &str = "<secret>";

/// The placeholder written where a key has no default to show.
const VALUE: &str = "<value>";

/// The column the comments this module writes wrap at.
///
/// A generated file is read in a terminal beside the service's logs, not only in an editor that
/// soft wraps: a spellings line naming three variables for a three-level key runs past 160 columns
/// unwrapped, and what falls off the right is the part an operator was looking for.
const WIDTH: usize = 96;

/// How deep a default value may nest before it is left out.
///
/// A document is read from a registry, so a default deep enough to overflow the stack here would
/// be a denial of service in a documentation generator.
const MAX_DEPTH: usize = 32;

/// How the TOML rendering is shaped.
#[derive(Debug, Clone)]
pub struct TomlExample {
    /// The preamble: what the file is, and the variables read before it exists.
    pub header: bool,
    /// How much of each key's comment to carry.
    pub docs: Docs,
    /// The other spellings that supply each key.
    pub spellings: bool,
    /// What a secret's value is written as.
    pub secret: String,
    /// What a key with no default is written as, when its shape suggests nothing better.
    pub placeholder: String,
}

impl Default for TomlExample {
    fn default() -> Self {
        Self {
            header: true,
            // The summary, not the whole comment: this file is read while being filled in, and a
            // four-paragraph comment above every key buries the keys.
            docs: Docs::Summary,
            spellings: true,
            secret: SECRET.to_owned(),
            placeholder: VALUE.to_owned(),
        }
    }
}

/// The whole file.
pub fn toml_example(schema: &Schema, options: &TomlExample) -> String {
    let mut blocks = Vec::new();
    if options.header {
        blocks.push(preamble(schema));
    }
    let root = Node::of(&schema.keys);
    collect(&mut blocks, &root, "", options);
    blocks.join("\n")
}

/// What the file is, and the variables that decide whether it is read at all.
fn preamble(schema: &Schema) -> String {
    let prefix = &schema.dialect.prefix;
    let suffix = &schema.dialect.indirection_suffix;
    let mut out = String::new();

    paragraph(
        &mut out,
        &format!("Configuration for a service reading {prefix}-prefixed keys."),
    );
    comment(&mut out, "");
    paragraph(
        &mut out,
        "Generated from the configuration type, so it lists every key that type can carry and \
         nothing else. Each key shows the value it already has, commented out: a commented key and \
         a deleted key mean the same thing to the loader, so uncomment one only to change it. A \
         key that is not commented out has no default, and nothing loads until something supplies \
         it.",
    );
    comment(&mut out, "");
    paragraph(
        &mut out,
        &format!(
            "Three layers can supply any key below, and all three win over this file: the \
             environment variable named above the key, a file named by that variable plus \
             `{suffix}`, and a key-named file in the secrets directory. A secret belongs in one of \
             those — this file is usually committed."
        ),
    );

    if !schema.loader.is_empty() {
        comment(&mut out, "");
        comment(&mut out, "Read before this file exists:");
        for var in &schema.loader {
            comment(&mut out, "");
            let default = var
                .default
                .as_ref()
                .map_or_else(String::new, |default| format!(", default `{default}`"));
            comment(
                &mut out,
                &format!("  {} — {}{default}", var.env, var.role.label()),
            );
            // Indented under the variable it belongs to, so the block reads as a list rather than
            // as one paragraph that happens to contain variable names.
            flowed(
                &mut out,
                "    ",
                "    ",
                &super::markdown::summary(&var.docs),
            );
        }
    }

    out
}

/// Push this level's block, then every level below it.
///
/// Depth first and leaves first, which is what TOML requires rather than a preference: a bare
/// assignment belongs to the table header above it, so a key written after `[github]` would land
/// in `github` however it was declared.
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

    if let Some(docs) = docs_of(options.docs, &key.docs) {
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
        // Without this the line below reads as a default. `repos = []` is the *shape* of the value
        // and not what the key is when nothing sets it, which for an optional key is nothing at
        // all — the distinction an optional container spelling exists to make.
        comment(
            &mut out,
            "Unset by default: the value below is only the shape.",
        );
    }

    // The one shape TOML cannot write down: a key and a table of the same name in one parent. The
    // table below is the half a reader can act on, so the key is commented out and told why.
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

/// How much of a comment this rendering carries.
fn docs_of(docs: Docs, text: &str) -> Option<String> {
    let carried = match docs {
        Docs::Full => text.to_owned(),
        Docs::Summary => super::markdown::summary(text),
        Docs::None => String::new(),
    };
    if carried.is_empty() {
        None
    } else {
        Some(carried)
    }
}

/// The other ways this key can be supplied, as one comment line.
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
        // Not a footnote: a camel-cased key path produces exactly this, and an operator who
        // assumed the usual environment spelling would set a variable that does nothing at all.
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
    placeholder(key, &options.placeholder)
}

/// A value of the key's shape that is still obviously not an answer.
///
/// The shape comes from the published `constraint`, not from the type name. See this module's own
/// note: a producer knows its types, and a consumer of a document knows only what the document
/// says — which for this purpose is strictly more portable and exactly as accurate, since the
/// producer derived `constraint` from the type it was reading.
fn placeholder(key: &Key, text: &str) -> String {
    let shape = key
        .constraint
        .as_ref()
        .and_then(|constraint| constraint.get("type"))
        .and_then(Json::as_str);

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
fn toml_literal(value: &Json, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }

    Some(match value {
        Json::String(string) => toml_string(string),
        Json::Bool(boolean) => boolean.to_string(),
        Json::Number(number) => toml_number(number)?,
        // TOML has no null: an absent key *is* the absent value, so there is nothing to write.
        Json::Null => return None,
        Json::Array(items) => {
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                // An array element cannot be left out the way a table entry can — dropping one
                // would write a shorter array than the default actually is.
                rendered.push(toml_literal(item, depth + 1)?);
            }
            format!("[{}]", rendered.join(", "))
        }
        Json::Object(entries) => {
            let rendered: Vec<String> = entries
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
/// TOML integers are 64-bit and signed, so an unsigned default above `i64::MAX` has no spelling in
/// the file at all. Writing it anyway would produce an example that fails to parse, which is worse
/// than an example that leaves one default out.
fn toml_number(number: &serde_json::Number) -> Option<String> {
    if let Some(value) = number.as_i64() {
        return Some(value.to_string());
    }
    if number.as_u64().is_some() {
        // Above `i64::MAX`: representable in JSON, not in TOML.
        return None;
    }
    number.as_f64().map(toml_float)
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
/// A rename accepts any string at all, and a bare key accepts none of the interesting ones.
/// Quoting is not cosmetic here — an unquoted `a.b` is two levels of nesting.
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
/// A comment is the one part of this file that carries text nobody chose: a doc comment, a key
/// name, an alias. TOML permits no control character but tab in a comment, so what cannot be
/// written raw is written as an escape — the same escape a quoted key two lines below would use,
/// so the two spellings of one name read as the same name.
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
fn wrapped(out: &mut String, text: &str) {
    flowed(out, "", "  ", text);
}

/// As [`wrapped`], with a chosen indent on the first line and on the rest.
///
/// Wrapping is for the text this module writes and for the spellings it assembles, both of which
/// are one paragraph with no structure to lose. A doc comment is *not* wrapped here: its line
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
    use serde_json::json;

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
        assert_eq!(toml_literal(&json!(1.0), 0).as_deref(), Some("1.0"));
        assert_eq!(toml_literal(&json!(0.0), 0).as_deref(), Some("0.0"));
        assert_eq!(toml_literal(&json!(1), 0).as_deref(), Some("1"));
    }

    /// An integer JSON can hold and TOML cannot is left out rather than written unparseably.
    #[test]
    fn an_integer_past_tomls_range_is_left_out() {
        let past = json!(u64::MAX);
        assert_eq!(toml_literal(&past, 0), None);
    }

    /// TOML has no null, and an absent key is what an absent value means.
    #[test]
    fn null_has_no_spelling() {
        assert_eq!(toml_literal(&json!(null), 0), None);
    }
}
