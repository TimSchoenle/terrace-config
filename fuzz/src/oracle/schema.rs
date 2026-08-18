//! Does a spelling the schema advertises actually reach the key it says it does?
//!
//! The other oracles fuzz the loader. This one fuzzes the *claim made about* the loader, which
//! is a different kind of bug and a worse one: a loader that mishandles a key fails loudly at
//! boot, whereas a reference table that names the wrong environment variable sends an operator
//! to set something that silently does nothing.
//!
//! So the property is not "the schema is internally consistent" — it is end-to-end. For every
//! key, take the spelling the schema printed, *set it that way*, run the real
//! [`Terrace`](terrace_config::Terrace) over the result, and require the value to arrive at the
//! path the schema promised. Three mechanisms, three passes, each in its own jail:
//!
//! | Advertised as | Set by | Must arrive at |
//! |---|---|---|
//! | [`Key::env`] | the environment variable | `key.path` |
//! | [`Key::secrets_file`] | a file in the secrets directory | `key.path` |
//! | [`Key::env_file`] | a file, named by the variable | `key.path` |
//!
//! And the negative, which is the half that catches an over-eager round-trip check: where the
//! schema reports **no** spelling, setting the obvious one must *not* produce the key. A schema
//! that said "unreachable" about a reachable key would be merely unhelpful; one that said
//! "reachable" about an unreachable key is the bug this exists to find.
//!
//! # The input
//!
//! ```text
//! s:<separator>      the nesting separator, default `__`
//! x:<suffix>         the indirection suffix, default `_FILE`
//! r:<SPELLING>       reserve a full environment spelling
//! k:<a>/<b>/<c>=docs declare a leaf at `a.b.c`, with `docs` as its `///` comment
//! m:<a>/<b>=note     annotate that leaf
//! S:<a>/<b>          mark that leaf secret
//! t:<a>/<b>=Type      the type the key takes
//! v:<a>/<b>=x,y,z     the values the key accepts
//! A:<a>/<b>=alt       an extra name the key answers to
//! ```
//!
//! Segments are split on `/` rather than on `.` so that a segment can *contain* a `.` — which is
//! the case where a key path and a figment path stop meaning the same thing, and so exactly the
//! case worth reaching.

use std::collections::{BTreeMap, BTreeSet};

use terrace_config::Terrace;
use terrace_config::schema::{Describe, Key, Leaf, Schema, Sink};
use terrace_config::testing::Harness;

use crate::support::{MAX_DIRECTIVES, MAX_NAME_LEN, PREFIX, is_safe_name, lookup};

/// The value every pass writes, and the value every assertion looks for.
///
/// One marker rather than a fuzzer-chosen value: what is under test is *which key* a spelling
/// lands on, and a value that could be confused with a key path would make a mismatch ambiguous.
const MARKER: &str = "sentinel-value";

/// The most leaves one iteration will describe.
const MAX_LEAVES: usize = 24;

/// The deepest a declared path will nest, well inside the schema's own limit.
const MAX_DEPTH: usize = 8;

/// One leaf, as the input asked for it.
#[derive(Debug, Clone, Default)]
struct LeafSpec {
    /// The path segments, innermost last. Never empty.
    segments: Vec<String>,
    /// The `///` comment.
    docs: String,
    /// The `#[config(note = "…")]` prose.
    note: Option<String>,
    /// Whether it is marked secret.
    secret: bool,
    /// The field's type as written.
    ty: Option<String>,
    /// The values the key accepts, when it is a choice.
    values: Vec<String>,
    /// Extra names the key answers to.
    aliases: Vec<String>,
}

/// What the input asked for, in full.
#[derive(Debug, Default)]
struct Spec {
    separator: Option<String>,
    suffix: Option<String>,
    reserved: Vec<String>,
    leaves: Vec<LeafSpec>,
}

thread_local! {
    /// The leaves the current iteration is describing.
    ///
    /// [`Describe::describe`] takes no value — the whole point is that it is a property of the
    /// *type* — so a fuzzer-driven implementation has nowhere else to read its input from. Thread
    /// local rather than global because the replay suite runs oracles in parallel.
    static LEAVES: std::cell::RefCell<Vec<LeafSpec>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A stand-in for a consumer's config type, whose keys come from the input.
struct Fuzzed;

impl Describe for Fuzzed {
    fn describe(sink: &mut Sink) {
        LEAVES.with_borrow(|leaves| {
            for leaf in leaves {
                describe_one(sink, &leaf.segments, leaf);
            }
        });
    }
}

/// Walk `segments`, opening a subtree per segment and reporting the leaf at the end.
fn describe_one(sink: &mut Sink, segments: &[String], leaf: &LeafSpec) {
    match segments {
        [] => unreachable!("a spec always has at least one segment"),
        [name] => {
            // Borrowed views, which live exactly as long as the call: `Leaf` takes slices so a
            // hand-written `Describe` can report keys it computed rather than only literals.
            let values: Vec<&str> = leaf.values.iter().map(String::as_str).collect();
            let aliases: Vec<&str> = leaf.aliases.iter().map(String::as_str).collect();
            sink.leaf(Leaf {
                name,
                docs: &leaf.docs,
                ty: leaf.ty.as_deref(),
                values: (!values.is_empty()).then_some(values.as_slice()),
                aliases: &aliases,
                note: leaf.note.as_deref(),
                required: false,
                secret: leaf.secret,
            });
        }
        [head, tail @ ..] => sink.nested(head, |sink| describe_one(sink, tail, leaf)),
    }
}

/// Parse the line grammar, skipping anything that does not fit it.
fn parse(data: &str) -> Spec {
    let mut spec = Spec::default();
    // Keyed on the *joined* path, which is what `Sink` keys on. Keying on the segments instead
    // let `a/b` and a single segment literally named `a.b` both through, and they are one key
    // path — a duplicate `Sink::leaf` panics on by design, so feeding it would be fuzzing the
    // assertion rather than the code under it. Found by a long sweep.
    let mut by_path: BTreeMap<String, usize> = BTreeMap::new();

    for line in data.lines().take(MAX_DIRECTIVES) {
        let Some((kind, rest)) = line.split_once(':') else {
            continue;
        };
        match kind {
            "s" if !rest.is_empty() && rest.len() <= MAX_NAME_LEN => {
                spec.separator = Some(rest.to_owned());
            }
            "x" if !rest.is_empty() && rest.len() <= MAX_NAME_LEN => {
                spec.suffix = Some(rest.to_owned());
            }
            "r" if !rest.is_empty() && rest.len() <= MAX_NAME_LEN => {
                spec.reserved.push(rest.to_owned());
            }
            "k" => {
                let Some((path, docs)) = rest.split_once('=') else {
                    continue;
                };
                let Some(segments) = segments(path) else {
                    continue;
                };
                let joined = segments.join(".");
                if spec.leaves.len() >= MAX_LEAVES || by_path.contains_key(&joined) {
                    continue;
                }
                by_path.insert(joined, spec.leaves.len());
                spec.leaves.push(LeafSpec {
                    segments,
                    docs: docs.to_owned(),
                    ..LeafSpec::default()
                });
            }
            "m" => {
                let Some((path, note)) = rest.split_once('=') else {
                    continue;
                };
                if let Some(index) = segments(path).and_then(|s| by_path.get(&s.join("."))) {
                    spec.leaves[*index].note = Some(note.to_owned());
                }
            }
            "S" => {
                if let Some(index) = segments(rest).and_then(|s| by_path.get(&s.join("."))) {
                    spec.leaves[*index].secret = true;
                }
            }
            "t" => {
                let Some((path, ty)) = rest.split_once('=') else {
                    continue;
                };
                if let Some(index) = segments(path).and_then(|s| by_path.get(&s.join("."))) {
                    spec.leaves[*index].ty = Some(ty.to_owned());
                }
            }
            "v" => {
                let Some((path, values)) = rest.split_once('=') else {
                    continue;
                };
                if let Some(index) = segments(path).and_then(|s| by_path.get(&s.join("."))) {
                    spec.leaves[*index].values = values.split(',').map(ToOwned::to_owned).collect();
                }
            }
            "A" => {
                let Some((path, alias)) = rest.split_once('=') else {
                    continue;
                };
                if alias.is_empty() || alias.len() > MAX_NAME_LEN {
                    continue;
                }
                if let Some(index) = segments(path).and_then(|s| by_path.get(&s.join("."))) {
                    spec.leaves[*index].aliases.push(alias.to_owned());
                }
            }
            _ => {}
        }
    }
    spec
}

/// The path segments a `k:` directive names, or [`None`] if it names nothing usable.
fn segments(path: &str) -> Option<Vec<String>> {
    let segments: Vec<String> = path
        .split('/')
        .take(MAX_DEPTH)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (!segments.is_empty() && segments.iter().all(|s| s.len() <= MAX_NAME_LEN)).then_some(segments)
}

/// Run every property over one input.
pub fn check(data: &str) {
    let spec = parse(data);
    if spec.leaves.is_empty() {
        return;
    }

    let terrace = terrace(&spec);
    let schema = LEAVES.with(|cell| {
        cell.replace(spec.leaves.clone());
        let schema = terrace.schema::<Fuzzed>();
        cell.replace(Vec::new());
        schema
    });

    // Properties that need no filesystem, over every key the input produced.
    aliases_sit_beside_the_key_they_alias(&spec, &schema);
    rendering_is_well_formed(&schema);
    json_round_trips(&schema);
    subsets_are_prefixes(&schema);

    // The end-to-end passes. Only over keys that can be set *independently* — see `settable`.
    let settable = settable(&schema);
    env_spelling_reaches_its_key(&spec, &settable);
    secrets_file_reaches_its_key(&spec, &settable);
    indirection_reaches_its_key(&spec, &settable);
    an_unreachable_key_stays_unreachable(&spec, &schema);
}

/// The loader the schema was built from, rebuilt identically.
fn terrace(spec: &Spec) -> Terrace {
    let mut terrace = Terrace::new(PREFIX);
    if let Some(separator) = &spec.separator {
        terrace = terrace.nesting_separator(separator.clone());
    }
    if let Some(suffix) = &spec.suffix {
        terrace = terrace.file_suffix(suffix.clone());
    }
    for reserved in &spec.reserved {
        terrace = terrace.reserve(reserved.clone());
    }
    terrace
}

/// The keys that can each be set on their own without the others interfering.
///
/// Two exclusions, both of which are real conflicts rather than defects:
///
/// - A path that is a strict prefix of another. `a` and `a.b` cannot both hold a value: one is a
///   scalar and the other is a table, and figment must pick.
/// - A spelling shared by two paths. A weird separator can map two distinct paths onto one
///   environment name, and then setting it necessarily lands on only one of them.
fn settable(schema: &Schema) -> Vec<Key> {
    let paths: BTreeSet<&str> = schema.keys.iter().map(|k| k.path.as_str()).collect();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for key in &schema.keys {
        for spelling in [&key.env, &key.secrets_file, &key.env_file]
            .into_iter()
            .flatten()
        {
            *seen.entry(spelling.as_str()).or_default() += 1;
        }
    }

    schema
        .keys
        .iter()
        .filter(|key| {
            let prefix = format!("{}.", key.path);
            !paths.iter().any(|other| other.starts_with(&prefix))
        })
        .filter(|key| {
            [&key.env, &key.secrets_file, &key.env_file]
                .into_iter()
                .flatten()
                .all(|spelling| seen.get(spelling.as_str()) == Some(&1))
        })
        .cloned()
        .collect()
}

/// Setting [`Key::env`] must put a value at [`Key::path`].
fn env_spelling_reaches_its_key(spec: &Spec, keys: &[Key]) {
    let named: Vec<&Key> = keys
        .iter()
        .filter(|k| k.env.is_some() && !k.reserved)
        .collect();
    if named.is_empty() {
        return;
    }

    Harness::over(terrace(spec)).run(|jail| {
        for key in &named {
            jail.env(key.env.as_ref().expect("filtered"), MARKER);
        }
        assert_arrives(&terrace(spec), &named, "the environment spelling");
        Ok(())
    });
}

/// Writing [`Key::secrets_file`] into the secrets directory must put a value at [`Key::path`].
fn secrets_file_reaches_its_key(spec: &Spec, keys: &[Key]) {
    let named: Vec<&Key> = keys
        .iter()
        .filter(|k| k.secrets_file.as_deref().is_some_and(is_safe_name))
        .collect();
    if named.is_empty() {
        return;
    }

    Harness::over(terrace(spec)).run(|jail| {
        // `secrets_dir` also points the loader's own variable at the directory, whatever the
        // spec named it.
        if jail.secrets_dir().is_err() {
            return Ok(());
        }
        for key in &named {
            let name = key.secrets_file.as_ref().expect("filtered");
            if jail.secret(name, MARKER).is_err() {
                return Ok(());
            }
        }
        assert_arrives(&terrace(spec), &named, "the secrets-directory file name");
        Ok(())
    });
}

/// Pointing [`Key::env_file`] at a file must put that file's contents at [`Key::path`].
fn indirection_reaches_its_key(spec: &Spec, keys: &[Key]) {
    let named: Vec<&Key> = keys.iter().filter(|k| k.env_file.is_some()).collect();
    if named.is_empty() {
        return;
    }

    Harness::over(terrace(spec)).run(|jail| {
        for (index, key) in named.iter().enumerate() {
            // The path is named by this oracle, never by the input: an indirection variable holds
            // a path, and a fuzzer-chosen one would have the target reading the host's files.
            //
            // The *variable* is the schema's own spelling rather than the one `Jail::indirection`
            // would derive, because that spelling is exactly what this oracle is checking.
            let Ok(path) = jail.write(format!("indirect/value-{index}"), MARKER) else {
                return Ok(());
            };
            jail.env(key.env_file.as_ref().expect("filtered"), path.display());
        }
        assert_arrives(&terrace(spec), &named, "the indirection variable");
        Ok(())
    });
}

/// Load through the real loader and require every key to have arrived.
fn assert_arrives(terrace: &Terrace, keys: &[&Key], mechanism: &str) {
    // A refusal is a legitimate outcome — a reserved key supplied by a file, a shadowed key —
    // and not a claim about spelling. What must never happen is a *silent* miss.
    let Ok(figment) = terrace.figment() else {
        return;
    };
    let Ok(value) = figment.extract::<figment::value::Value>() else {
        return;
    };

    for key in keys {
        // `support::lookup`, not `Value::find_ref`: figment's own lookup *stops early* on an empty
        // path segment and hands back whatever it had reached, so `auth..ab` resolves to the value
        // at `auth`. That lets a neighbouring key's value answer for this one, in both directions,
        // and a strict walk is what the question actually is.
        let found = lookup(&value, &key.path);
        assert!(
            found.is_some(),
            "the schema advertised {mechanism} for `{}`, but setting it that way produced \
             nothing at that path.\n  env: {:?}\n  secrets file: {:?}\n  indirection: {:?}\n  \
             merged: {value:?}",
            key.path,
            key.env,
            key.secrets_file,
            key.env_file,
        );
    }
}

/// Where the schema reports no environment spelling, the obvious one must not work.
///
/// This is the half that catches a round-trip check that gave up too easily *and* one that did
/// not give up when it should have. Only the environment is checked: it is the mechanism whose
/// spelling is derived rather than chosen, and so the one that can be wrong.
fn an_unreachable_key_stays_unreachable(spec: &Spec, schema: &Schema) {
    let unreachable: Vec<&Key> = schema.keys.iter().filter(|k| k.env.is_none()).collect();
    if unreachable.is_empty() {
        return;
    }

    let dialect = terrace(spec).dialect();
    // The obvious spelling is only worth trying when an operating system could hold it. A name
    // carrying a NUL or an `=` cannot be created at all, so "setting it does not reach the key"
    // would be true for a reason that has nothing to do with the schema — and `set_var` panics
    // rather than failing, which is what took down the first libFuzzer campaign to run this.
    let unreachable: Vec<&Key> = unreachable
        .into_iter()
        .filter(|key| settable_name(&dialect.env_spelling(&key.path)))
        .collect();
    if unreachable.is_empty() {
        return;
    }

    Harness::over(terrace(spec)).run(|jail| {
        for key in &unreachable {
            jail.env(dialect.env_spelling(&key.path), MARKER);
        }
        let Ok(figment) = terrace(spec).figment() else {
            return Ok(());
        };
        let Ok(value) = figment.extract::<figment::value::Value>() else {
            return Ok(());
        };
        for key in &unreachable {
            assert!(
                lookup(&value, &key.path).and_then(figment::value::Value::as_str) != Some(MARKER),
                "the schema reported no environment spelling for `{}`, but `{}` reached it \
                 anyway — the key is documented as unsettable when it is not.",
                key.path,
                dialect.env_spelling(&key.path),
            );
        }
        Ok(())
    });
}

/// An alias is reported as a full key path under the same parent as the key it aliases.
///
/// A bare alias name would be useless: its environment and file spellings have to derive the same
/// way the canonical path's do, and they only can if it *is* a path.
fn aliases_sit_beside_the_key_they_alias(spec: &Spec, schema: &Schema) {
    for (leaf, key) in spec.leaves.iter().zip(&schema.keys) {
        assert_eq!(leaf.aliases.len(), key.aliases.len(), "`{}`", key.path);
        // The parent comes from the *declared segments*, never from splitting the path. A segment
        // may itself contain a `.` — `a.b` as one name — and then the path no longer says where
        // the segment boundaries were. The crate is right here because it keeps a prefix stack;
        // this assertion was splitting strings, and the sweep found it in four iterations.
        let mut parent = leaf.segments[..leaf.segments.len() - 1].join(".");
        if !parent.is_empty() {
            parent.push('.');
        }
        for (declared, reported) in leaf.aliases.iter().zip(&key.aliases) {
            assert_eq!(
                reported,
                &format!("{parent}{declared}"),
                "an alias of `{}` was not reported beside it",
                key.path
            );
        }
    }
}

/// Whether an operating system could hold an environment variable of this name.
///
/// The same rule the crate applies before advertising a spelling, restated here rather than
/// called: an oracle that asked the code under test what to try would agree with it by
/// construction, and this is the one place whose whole job is to disagree.
fn settable_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['\0', '='])
}

/// Every Markdown row has the same number of cells, whatever the prose contains.
///
/// One unescaped `|` in a `///` comment silently adds a column to the row it is in, which a
/// renderer shows as a broken table and a reviewer reads past.
fn rendering_is_well_formed(schema: &Schema) {
    let markdown = schema.to_markdown();
    // Two tables separated by a blank line; only the key table is fixed-width here, because the
    // loader table has its own column count.
    let Some(table) = markdown.split("\n\n").nth(1) else {
        return;
    };

    let widths: Vec<usize> = table
        .lines()
        .filter(|line| !line.is_empty())
        .map(cell_boundaries)
        .collect();
    assert!(
        widths.windows(2).all(|w| w[0] == w[1]),
        "a Markdown row has a different number of cells than its header:\n{table}"
    );
    // A `///` comment is prose and contains newlines: the `Purpose` column folds its summary onto
    // one line and every other prose cell turns them into `<br>`. One that survived either would
    // end its row early, which shows up as a line that is not a table row at all.
    for line in table.lines().filter(|line| !line.is_empty()) {
        assert!(
            line.starts_with('|') && line.ends_with('|'),
            "a cell broke out of its row:\n{table}"
        );
    }
}

/// The `|` characters in one row that actually end a cell.
fn cell_boundaries(row: &str) -> usize {
    let mut escaped = false;
    let mut boundaries = 0;
    for character in row.chars() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '|' => boundaries += 1,
            _ => {}
        }
    }
    boundaries
}

/// The JSON contract survives a round trip, whatever the prose contains.
fn json_round_trips(schema: &Schema) {
    let json = schema.to_json().expect("a schema serialises");
    let parsed: Schema = serde_json::from_str(&json).expect("what it wrote, it can read");

    assert_eq!(parsed.keys.len(), schema.keys.len());
    for (before, after) in schema.keys.iter().zip(&parsed.keys) {
        assert_eq!(before.path, after.path);
        assert_eq!(before.env, after.env);
        assert_eq!(before.env_file, after.env_file);
        assert_eq!(before.secrets_file, after.secrets_file);
        assert_eq!(before.docs, after.docs);
        assert_eq!(before.note, after.note);
        assert_eq!(before.ty, after.ty);
        assert_eq!(before.values, after.values);
        assert_eq!(before.aliases, after.aliases);
        assert_eq!(before.secret, after.secret);
    }
}

/// A subset holds exactly the keys under its prefix, and nothing whose name merely starts the
/// same way.
fn subsets_are_prefixes(schema: &Schema) {
    for key in &schema.keys {
        let Some((head, _)) = key.path.split_once('.') else {
            continue;
        };
        let head = head.to_owned();
        // The contract has two branches, and this asserted only one of them. An empty prefix
        // keeps *everything* — it is how "the whole schema" is spelled — and a key whose own name
        // begins with a `.` is how a fuzzer reaches that branch through this loop.
        let expected: Vec<String> = if head.is_empty() {
            schema.keys.iter().map(|k| k.path.clone()).collect()
        } else {
            schema
                .keys
                .iter()
                .filter(|k| k.path == head || k.path.starts_with(&format!("{head}.")))
                .map(|k| k.path.clone())
                .collect()
        };
        let actual: Vec<String> = schema
            .clone()
            .subset(&head)
            .keys
            .iter()
            .map(|k| k.path.clone())
            .collect();
        assert_eq!(actual, expected, "subset(`{head}`) kept the wrong keys");
    }
}
