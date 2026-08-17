//! The schema export: what the derive sees, and what the loader spells it as.
//!
//! These are the tests that keep generated documentation honest. A schema that lists a key the
//! loader cannot read, or omits one it can, is worse than no generated documentation at all —
//! the whole point is that the table cannot drift from the code.

#![cfg(feature = "schema")]
// Every struct here is a fixture that exists to be *described*. The derive reads each field at
// compile time and the assertions check what it produced, so no field is ever read at runtime —
// which is the whole point of the feature and not something to work around by adding uses.
#![expect(dead_code, reason = "fixtures are read by the derive, not at runtime")]

use serde::{Deserialize, Serialize};
use terrace_config::Terrace;
use terrace_config::schema::{Column, Describe, Key, SCHEMA_VERSION, Schema};

#[derive(Deserialize, Serialize, Describe)]
struct Config {
    /// Bundle directory the readiness probe checks.
    #[serde(default = "default_dist")]
    dist_dir: String,
    #[config(nested)]
    csp: Csp,
    #[config(nested)]
    github: Github,
    /// Not part of the documented surface.
    #[config(skip)]
    #[serde(default)]
    internal: bool,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Csp {
    /// Hash the document's inline scripts instead of
    /// allowing `'unsafe-inline'`.
    #[serde(default)]
    hash_inline_scripts: bool,
    #[config(nested)]
    cloudflare: Cloudflare,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Cloudflare {
    /// Admit the Turnstile widget.
    #[serde(default)]
    turnstile: bool,
}

#[derive(Deserialize, Serialize, Default, Describe)]
struct Github {
    /// User whose repositories `update-repos` lists.
    username: String,
    /// Bearer token lifting the GitHub API rate limit.
    #[config(secret)]
    token: Option<String>,
    /// Revalidation interval in seconds.
    #[config(note = "permanent")]
    #[serde(default)]
    ttl_secs: u64,
}

fn default_dist() -> String {
    "public".to_owned()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dist_dir: default_dist(),
            csp: Csp::default(),
            github: Github {
                username: "TimSchoenle".to_owned(),
                token: Some("hunter2".to_owned()),
                ttl_secs: 0,
            },
            internal: false,
        }
    }
}

fn schema() -> Schema {
    Terrace::new("PORTFOLIO_")
        .reserve("PORTFOLIO_PROFILE")
        .schema::<Config>()
}

fn key<'a>(schema: &'a Schema, path: &str) -> &'a Key {
    schema
        .keys
        .iter()
        .find(|k| k.path == path)
        .unwrap_or_else(|| panic!("no key `{path}` in {:?}", paths(schema)))
}

fn paths(schema: &Schema) -> Vec<&str> {
    schema.keys.iter().map(|k| k.path.as_str()).collect()
}

/// The `|` characters in one Markdown row that actually end a cell.
///
/// Counting every `|` would count the escaped ones too, which is exactly the difference between
/// a table that renders and one that does not.
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

/// Declaration order, not alphabetical: it is the order a hand-written table would have used,
/// and it carries grouping that sorting destroys.
#[test]
fn nesting_produces_dotted_paths_in_declaration_order() {
    assert_eq!(
        paths(&schema()),
        [
            "dist_dir",
            "csp.hash_inline_scripts",
            "csp.cloudflare.turnstile",
            "github.username",
            "github.token",
            "github.ttl_secs",
        ]
    );
}

#[test]
fn every_spelling_the_loader_accepts_is_reported() {
    let schema = schema();
    let turnstile = key(&schema, "csp.cloudflare.turnstile");

    assert_eq!(
        turnstile.env.as_deref(),
        Some("PORTFOLIO_CSP__CLOUDFLARE__TURNSTILE")
    );
    assert_eq!(
        turnstile.env_file.as_deref(),
        Some("PORTFOLIO_CSP__CLOUDFLARE__TURNSTILE_FILE")
    );
    assert_eq!(
        turnstile.secrets_file.as_deref(),
        Some("csp__cloudflare__turnstile")
    );
}

/// The separator is a parameter, so every spelling has to be derived from it rather than from
/// the `__` that happens to be the default.
#[test]
fn a_custom_dialect_changes_every_spelling() {
    let schema = Terrace::new("APP_")
        .nesting_separator("_")
        .file_suffix("_PATH")
        .schema::<Config>();
    let turnstile = key(&schema, "csp.cloudflare.turnstile");

    assert_eq!(
        turnstile.env.as_deref(),
        Some("APP_CSP_CLOUDFLARE_TURNSTILE")
    );
    assert_eq!(
        turnstile.env_file.as_deref(),
        Some("APP_CSP_CLOUDFLARE_TURNSTILE_PATH")
    );
    assert_eq!(
        turnstile.secrets_file.as_deref(),
        Some("csp_cloudflare_turnstile")
    );
}

/// A separator containing a letter used to break `Dialect::key_path`, which folded the input to
/// lower case before looking for `_X_` in it. The schema was the honest half — it reported no
/// spelling rather than one that did not work — and this is the same assertion the other way
/// round, now that `key_path` matches the separator case-insensitively.
#[test]
fn a_separator_containing_a_letter_reaches_every_key() {
    let schema = Terrace::new("APP_")
        .nesting_separator("_X_")
        .schema::<Config>();
    let turnstile = key(&schema, "csp.cloudflare.turnstile");

    assert_eq!(
        turnstile.env.as_deref(),
        Some("APP_CSP_X_CLOUDFLARE_X_TURNSTILE")
    );
    assert_eq!(
        turnstile.secrets_file.as_deref(),
        Some("csp_X_cloudflare_X_turnstile")
    );
}

#[test]
fn doc_comments_become_the_purpose_column() {
    let schema = schema();
    assert_eq!(
        key(&schema, "csp.hash_inline_scripts").docs,
        "Hash the document's inline scripts instead of\nallowing `'unsafe-inline'`."
    );
    // A field with no `///` reports an empty string rather than inventing prose.
    assert_eq!(
        key(&schema, "dist_dir").docs,
        "Bundle directory the readiness probe checks."
    );
}

#[test]
fn serde_decides_which_keys_are_required() {
    let schema = schema();
    // `#[serde(default = "…")]`.
    assert!(!key(&schema, "dist_dir").required);
    // Nothing at all.
    assert!(key(&schema, "github.username").required);
    // `Option<_>`.
    assert!(!key(&schema, "github.token").required);
}

#[test]
fn config_skip_omits_a_key_without_touching_deserialisation() {
    assert!(!paths(&schema()).contains(&"internal"));
    // The field still deserialises: this compiles only because it is a real field.
    let config: Config =
        figment::Figment::from(figment::providers::Serialized::defaults(Config::default()))
            .extract()
            .unwrap();
    assert!(!config.internal);
}

#[test]
fn a_secret_default_is_redacted_rather_than_documented() {
    let schema = schema().with_defaults_from(&Config::default()).unwrap();
    assert_eq!(
        key(&schema, "github.token").default.as_deref(),
        Some("<redacted>")
    );
    assert!(key(&schema, "github.token").secret);
    // And the real value is nowhere in the rendered output either.
    assert!(!schema.to_markdown().contains("hunter2"));
    assert!(!schema.to_json().unwrap().contains("hunter2"));
}

#[test]
fn defaults_come_from_the_supplied_value() {
    let schema = schema().with_defaults_from(&Config::default()).unwrap();
    assert_eq!(key(&schema, "dist_dir").default.as_deref(), Some("public"));
    assert_eq!(
        key(&schema, "csp.cloudflare.turnstile").default.as_deref(),
        Some("false")
    );
}

/// A required key has no default by definition: loading fails when nothing supplies it. Whatever
/// `Default` put in the field is an artefact of constructing the value, and printing it would
/// tell an operator they can leave the key out.
#[test]
fn a_required_key_reports_no_default_however_the_value_was_built() {
    let schema = schema().with_defaults_from(&Config::default()).unwrap();
    let username = key(&schema, "github.username");
    assert!(username.required);
    assert_eq!(username.default, None);
}

/// The `Option<_>` is `None`, so there is nothing to hide. `<redacted>` here would read as though
/// the service ships with a credential baked in.
#[test]
fn an_unset_secret_reports_unset_rather_than_redacted() {
    #[derive(Serialize, Default, Describe)]
    struct NoToken {
        /// Bearer token.
        #[config(secret)]
        token: Option<String>,
    }

    let schema = Schema::describe::<NoToken>(&Terrace::new("T_").dialect())
        .with_defaults_from(&NoToken::default())
        .unwrap();
    assert_eq!(key(&schema, "token").default, None);
    assert!(schema.to_markdown().contains("| unset |"));
}

/// The value and the prose answer different questions — `0` is what an operator compares against
/// what they set, "permanent" is why they would leave it alone — so both are reported. A note
/// that *replaced* the value, which is what this used to do, meant the observed value was thrown
/// away and had to be hand-copied into the prose to appear at all.
#[test]
fn a_note_accompanies_the_observed_default_rather_than_replacing_it() {
    let schema = schema().with_defaults_from(&Config::default()).unwrap();
    let ttl = key(&schema, "github.ttl_secs");

    assert_eq!(ttl.default.as_deref(), Some("0"));
    assert_eq!(ttl.note.as_deref(), Some("permanent"));
    assert!(
        schema.to_markdown().contains("| `0` (permanent) |"),
        "{}",
        schema.to_markdown()
    );
}

/// A note on a key whose value cannot be shown still carries: the prose is the only thing left
/// to say, and dropping it would leave the cell reading nothing but `<redacted>`.
#[test]
fn a_note_survives_redaction() {
    #[derive(Serialize, Describe)]
    struct Tokened {
        /// Bearer token.
        #[config(secret)]
        #[config(note = "issued by the operator")]
        token: Option<String>,
    }

    let schema = Schema::describe::<Tokened>(&Terrace::new("T_").dialect())
        .with_defaults_from(&Tokened {
            token: Some("hunter2".to_owned()),
        })
        .unwrap();
    let token = key(&schema, "token");

    assert_eq!(token.default.as_deref(), Some("<redacted>"));
    assert_eq!(token.note.as_deref(), Some("issued by the operator"));
    assert!(!schema.to_markdown().contains("hunter2"));
    assert!(
        schema
            .to_markdown()
            .contains("| `<redacted>` (issued by the operator) |")
    );
}

/// An unset default is written as prose, not as an empty code span, so the cell reads as one
/// phrase rather than as a missing value followed by an explanation.
#[test]
fn a_note_on_an_unset_default_reads_as_one_phrase() {
    #[derive(Serialize, Default, Describe)]
    struct Isr {
        /// Writable directory rendered pages are cached into.
        #[config(note = "ISR off; the image sets /tmp/isr")]
        cache_dir: Option<String>,
    }

    let markdown = Schema::describe::<Isr>(&Terrace::new("T_").dialect())
        .with_defaults_from(&Isr::default())
        .unwrap()
        .to_markdown();
    assert!(
        markdown.contains("| unset (ISR off; the image sets /tmp/isr) |"),
        "{markdown}"
    );
}

/// `Column::Note` keeps the two apart for a table that would rather have its own column.
#[test]
fn the_note_can_be_rendered_as_a_column_of_its_own() {
    let markdown = schema()
        .with_defaults_from(&Config::default())
        .unwrap()
        .to_markdown_with(&[Column::Path, Column::DefaultValue, Column::Note]);

    assert!(markdown.contains("| TOML | Default | Note |"));
    assert!(markdown.contains("| `github.ttl_secs` | `0` | permanent |"));
    // A key with no note reports an em dash rather than an empty cell.
    assert!(markdown.contains("| `dist_dir` | `public` | — |"));
}

#[test]
fn the_loader_variables_are_reported_alongside_the_keys() {
    let schema = schema();
    let names: Vec<&str> = schema.loader.iter().map(|v| v.env.as_str()).collect();
    assert_eq!(
        names,
        [
            "PORTFOLIO_CONFIG",
            "PORTFOLIO_SECRETS_DIR",
            "PORTFOLIO_PROFILE"
        ]
    );
    assert_eq!(schema.loader[0].default.as_deref(), Some("config.toml"));
}

#[test]
fn the_json_document_round_trips() {
    let schema = schema().with_defaults_from(&Config::default()).unwrap();
    let json = schema.to_json().unwrap();
    let parsed: Schema = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.schema_version, SCHEMA_VERSION);
    assert_eq!(parsed.dialect.prefix, "PORTFOLIO_");
    assert_eq!(parsed.dialect.nesting_separator, "__");
    assert_eq!(parsed.dialect.indirection_suffix, "_FILE");
    assert_eq!(paths(&parsed), paths(&schema));
}

#[test]
fn the_markdown_table_is_well_formed() {
    let markdown = schema()
        .with_defaults_from(&Config::default())
        .unwrap()
        .to_markdown();

    let key_table = markdown.split("\n\n").nth(1).expect("two tables");
    let widths: Vec<usize> = key_table
        .lines()
        .filter(|line| !line.is_empty())
        .map(cell_boundaries)
        .collect();
    // Header, separator, and one row per key, every one the same width.
    assert_eq!(widths.len(), 2 + schema().keys.len());
    assert!(widths.windows(2).all(|w| w[0] == w[1]), "{key_table}");
    assert!(key_table.contains("| `csp.cloudflare.turnstile` |"));
}

/// A doc comment is prose, and prose contains `|`. Unescaped, one such comment silently adds a
/// column to every table that renders it.
#[test]
fn table_structure_survives_prose() {
    /// Either `a` \| `b`.
    #[derive(Describe)]
    struct Pipes {
        /// Either `a` | `b`.
        choice: bool,
    }

    let markdown = Schema::describe::<Pipes>(&Terrace::new("T_").dialect()).to_markdown();
    let widths: Vec<usize> = markdown
        .lines()
        .filter(|line| !line.is_empty())
        .map(cell_boundaries)
        .collect();
    assert!(widths.windows(2).all(|w| w[0] == w[1]), "{markdown}");
    assert!(markdown.contains(r"`a` \| `b`"));
}

/// `rename_all` is read from the serde attribute rather than duplicated, because the key path
/// has to be the one serde will actually look for.
#[test]
fn serde_renaming_is_honoured() {
    #[derive(Describe)]
    #[serde(rename_all = "kebab-case")]
    struct Renamed {
        max_connections: u32,
        #[serde(rename = "url")]
        connection_string: String,
    }

    let schema = Schema::describe::<Renamed>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&schema), ["max-connections", "url"]);
}

/// A camelCase key cannot be named in the environment at all: figment folds an environment key
/// to lower case on the way in, so `distDir` never comes back. Reporting a spelling nobody can
/// use would be worse than reporting none.
#[test]
fn a_key_the_environment_cannot_reach_reports_no_spelling() {
    #[derive(Describe)]
    #[serde(rename_all = "camelCase")]
    struct Camel {
        dist_dir: String,
    }

    let schema = Schema::describe::<Camel>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&schema), ["distDir"]);
    assert_eq!(key(&schema, "distDir").env, None);
    assert_eq!(key(&schema, "distDir").env_file, None);
    assert_eq!(key(&schema, "distDir").secrets_file, None);
    assert!(schema.to_markdown().contains("| — |"));
}

/// A reserved key is read straight from the environment, so advertising a file spelling for it
/// would document a path that errors the moment anyone uses it.
#[test]
fn a_reserved_key_advertises_no_file_spelling() {
    #[derive(Describe)]
    struct WithProfile {
        profile: String,
    }

    let schema = Terrace::new("T_")
        .reserve("T_PROFILE")
        .schema::<WithProfile>();
    let profile = key(&schema, "profile");
    assert!(profile.reserved);
    assert_eq!(profile.env.as_deref(), Some("T_PROFILE"));
    assert_eq!(profile.env_file, None);
    assert_eq!(profile.secrets_file, None);
}

#[test]
fn flattening_merges_keys_at_the_current_level() {
    #[derive(Describe)]
    struct Outer {
        top: bool,
        #[serde(flatten)]
        #[config(nested)]
        inner: Inner,
    }

    #[derive(Describe)]
    struct Inner {
        /// Merged in without a segment of its own.
        buried: bool,
    }

    let schema = Schema::describe::<Outer>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&schema), ["top", "buried"]);
}

/// A `#[config(nested)] Option<Inner>` describes `Inner`: the keys underneath exist either way.
#[test]
fn an_optional_subtree_is_described_through_the_option() {
    #[derive(Describe)]
    struct Outer {
        #[config(nested)]
        inner: Option<Inner>,
    }

    #[derive(Describe)]
    struct Inner {
        leaf: bool,
    }

    let schema = Schema::describe::<Outer>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&schema), ["inner.leaf"]);
}

/// A configuration is not one file. Each subsystem derives `Describe` beside the code that
/// consumes it, and describing the root walks all of it — `#[config(nested)]` is a trait bound,
/// so it follows the type rather than the module.
#[test]
fn a_configuration_split_across_modules_is_described_as_one_tree() {
    mod csp {
        use terrace_config::schema::Describe;

        #[derive(Describe)]
        pub(crate) struct Csp {
            /// Documented next to the code that reads it.
            pub(crate) turnstile: bool,
        }
    }

    #[derive(Describe)]
    struct Root {
        #[config(nested)]
        csp: csp::Csp,
    }

    let schema = Schema::describe::<Root>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&schema), ["csp.turnstile"]);
    assert_eq!(
        key(&schema, "csp.turnstile").docs,
        "Documented next to the code that reads it."
    );
}

/// Describing a subsystem's own type gives paths relative to it, which appear in no file
/// anywhere. `schema_at` roots them where the operator will actually have to spell them.
#[test]
fn a_subsystem_can_be_described_at_the_path_it_occupies() {
    #[derive(Describe)]
    struct Csp {
        turnstile: bool,
    }

    let bare = Schema::describe::<Csp>(&Terrace::new("T_").dialect());
    assert_eq!(paths(&bare), ["turnstile"]);

    let rooted = Terrace::new("T_").schema_at::<Csp>("csp.cloudflare");
    assert_eq!(paths(&rooted), ["csp.cloudflare.turnstile"]);
    assert_eq!(
        key(&rooted, "csp.cloudflare.turnstile").env.as_deref(),
        Some("T_CSP__CLOUDFLARE__TURNSTILE")
    );
}

#[test]
fn a_subset_keeps_one_subtree_and_its_real_spellings() {
    let sliced = schema().subset("csp");
    assert_eq!(
        paths(&sliced),
        ["csp.hash_inline_scripts", "csp.cloudflare.turnstile"]
    );
    // The loader variables are not part of any subtree, and a page showing only `csp` still has
    // to say where the configuration file comes from.
    assert_eq!(sliced.loader.len(), 3);
    assert_eq!(
        key(&sliced, "csp.cloudflare.turnstile").env.as_deref(),
        Some("PORTFOLIO_CSP__CLOUDFLARE__TURNSTILE")
    );
}

/// The separating `.` is what stops `csp` from taking `cspx` with it.
#[test]
fn a_subset_matches_whole_segments_only() {
    #[derive(Describe)]
    struct Siblings {
        #[config(nested)]
        csp: Leafy,
        #[config(nested)]
        cspx: Leafy,
    }

    #[derive(Describe)]
    struct Leafy {
        enabled: bool,
    }

    let sliced = Schema::describe::<Siblings>(&Terrace::new("T_").dialect()).subset("csp");
    assert_eq!(paths(&sliced), ["csp.enabled"]);
}

#[test]
fn an_empty_subset_prefix_keeps_everything() {
    assert_eq!(paths(&schema().subset("")), paths(&schema()));
}

#[test]
fn a_custom_column_set_is_rendered_in_order() {
    let markdown = schema().to_markdown_with(&[Column::Path, Column::SecretsFile]);
    assert!(markdown.contains("| TOML | Secrets file |"));
    assert!(markdown.contains("| `github.token` | `github__token` |"));
}

/// Two fields resolving to one key path is a bug in the annotations, and a table that lists a
/// key twice is worse than one that refuses to be generated.
#[test]
#[should_panic(expected = "`same` is described twice")]
fn a_colliding_rename_is_refused() {
    #[derive(Describe)]
    struct Collides {
        same: bool,
        #[serde(rename = "same")]
        other: bool,
    }

    let _ = Schema::describe::<Collides>(&Terrace::new("T_").dialect());
}

/// Every shape a default value can take, rendered the way an operator would have to type it back
/// into a TOML file. A `Dict` at a leaf means the field wanted `#[config(nested)]`, and is shown
/// as an inline table rather than dropped, so the output says what is actually there.
#[test]
fn every_kind_of_default_value_renders() {
    // `#[serde(default)]` so none of these is *required* — a required key reports no default at
    // all, which is a different property, tested above.
    #[derive(Serialize, Deserialize, Default, Describe)]
    #[serde(default)]
    struct Shapes {
        text: String,
        empty: String,
        flag: bool,
        letter: char,
        whole: u64,
        negative: i64,
        fraction: f64,
        list: Vec<String>,
        nested_list: Vec<Vec<u8>>,
        table: std::collections::BTreeMap<String, u8>,
        absent: Option<u8>,
    }

    let schema = Schema::describe::<Shapes>(&Terrace::new("T_").dialect())
        .with_defaults_from(&Shapes {
            text: "public".to_owned(),
            empty: String::new(),
            flag: true,
            letter: 'x',
            whole: 16,
            negative: -1,
            fraction: 1.5,
            list: vec!["Portfolio".to_owned(), "actions".to_owned()],
            nested_list: vec![vec![1, 2]],
            table: [("a".to_owned(), 1u8)].into_iter().collect(),
            absent: None,
        })
        .unwrap();

    let rendered = |path: &str| key(&schema, path).default.clone();
    assert_eq!(rendered("text").as_deref(), Some("public"));
    // Quoted, so an empty default is distinguishable from an absent one in a rendered cell.
    assert_eq!(rendered("empty").as_deref(), Some("\"\""));
    assert_eq!(rendered("flag").as_deref(), Some("true"));
    assert_eq!(rendered("letter").as_deref(), Some("x"));
    assert_eq!(rendered("whole").as_deref(), Some("16"));
    assert_eq!(rendered("negative").as_deref(), Some("-1"));
    assert_eq!(rendered("fraction").as_deref(), Some("1.5"));
    assert_eq!(rendered("list").as_deref(), Some("[Portfolio, actions]"));
    assert_eq!(rendered("nested_list").as_deref(), Some("[[1, 2]]"));
    assert_eq!(rendered("table").as_deref(), Some("{ a = 1 }"));
    // Absent and null render the same, because they mean the same to an operator.
    assert_eq!(rendered("absent"), None);
}

/// A configuration type that contains itself has no finite set of keys, so there is no correct
/// output — only a stack overflow, or this.
#[test]
#[should_panic(expected = "nests more than 32 levels deep")]
fn a_type_that_contains_itself_is_refused_rather_than_overflowing_the_stack() {
    struct Cyclic;

    impl Describe for Cyclic {
        fn describe(sink: &mut terrace_config::schema::Sink) {
            sink.nested("down", Self::describe);
        }
    }

    let _ = Schema::describe::<Cyclic>(&Terrace::new("T_").dialect());
}

/// Nesting right up to the limit is legal — the guard is for a type with no bottom, not for a
/// configuration that happens to be deep.
#[test]
fn nesting_to_the_limit_is_allowed() {
    struct Deep;

    impl Describe for Deep {
        fn describe(sink: &mut terrace_config::schema::Sink) {
            fn down(sink: &mut terrace_config::schema::Sink, left: usize) {
                if left == 0 {
                    sink.leaf(terrace_config::schema::Leaf {
                        name: "bottom",
                        docs: "",
                        ty: None,
                        values: None,
                        aliases: &[],
                        note: None,
                        required: true,
                        secret: false,
                    });
                } else {
                    sink.nested("d", move |sink| down(sink, left - 1));
                }
            }
            down(sink, 31);
        }
    }

    let schema = Schema::describe::<Deep>(&Terrace::new("T_").dialect());
    assert_eq!(schema.keys.len(), 1);
    assert!(schema.keys[0].path.ends_with(".bottom"));
}

/// Every column renders for every key, including the ones that have nothing to show. A column
/// that panicked or produced an empty cell on a `None` would break the table it is in.
#[test]
fn every_column_renders_for_every_key() {
    const ALL: &[Column] = &[
        Column::Path,
        Column::Env,
        Column::EnvFile,
        Column::SecretsFile,
        Column::Default,
        Column::DefaultValue,
        Column::Note,
        Column::Flags,
        Column::Required,
        Column::Secret,
        Column::Docs,
    ];

    let markdown = schema()
        .with_defaults_from(&Config::default())
        .unwrap()
        .to_markdown_with(ALL);

    for line in markdown.lines().filter(|line| !line.is_empty()) {
        assert!(line.starts_with('|') && line.ends_with('|'), "{line}");
    }
    let key_table = markdown.split("\n\n").nth(1).expect("two tables");
    for line in key_table.lines().filter(|line| !line.is_empty()) {
        assert_eq!(cell_boundaries(line), ALL.len() + 1, "{line}");
    }
}

/// A `Describe` with no keys at all is a real shape — an empty configuration section — and has
/// to render as a table with a header and no rows rather than as anything malformed.
#[test]
fn a_schema_with_no_keys_still_renders_and_round_trips() {
    struct Empty;

    impl Describe for Empty {
        fn describe(_sink: &mut terrace_config::schema::Sink) {}
    }

    let schema = Terrace::new("T_").schema::<Empty>();
    assert!(schema.keys.is_empty());

    let markdown = schema.to_markdown();
    assert!(markdown.contains("| TOML |"));

    let parsed: Schema = serde_json::from_str(&schema.to_json().unwrap()).unwrap();
    assert!(parsed.keys.is_empty());
    assert_eq!(parsed.loader.len(), 2);
}

/// The gap this closes: without a type, a required key shows an em dash for its default and the
/// reader has no way to tell whether to supply a string, a number or a list.
#[test]
fn every_key_reports_the_type_it_takes() {
    #[derive(Serialize, Describe)]
    struct Typed {
        name: String,
        port: u16,
        ratio: f64,
        repos: Vec<String>,
        path: std::path::PathBuf,
        table: std::collections::BTreeMap<String, u8>,
        nested_generic: Vec<Vec<u8>>,
    }

    let schema = Schema::describe::<Typed>(&Terrace::new("T_").dialect());
    let ty = |path: &str| {
        key(&schema, path)
            .ty
            .clone()
            .expect("every leaf has a type")
    };
    assert_eq!(ty("name"), "String");
    assert_eq!(ty("port"), "u16");
    assert_eq!(ty("ratio"), "f64");
    assert_eq!(ty("repos"), "Vec<String>");
    assert_eq!(ty("path"), "std::path::PathBuf");
    assert_eq!(ty("table"), "std::collections::BTreeMap<String, u8>");
    assert_eq!(ty("nested_generic"), "Vec<Vec<u8>>");

    assert!(schema.to_markdown().contains("| `Vec<String>` |"));
}

/// `required` already says whether a key may be left out, so `Option<String>` in the type column
/// would say it a second time and less clearly. What an operator supplies is a `String`.
#[test]
fn an_optional_key_reports_the_type_inside_the_option() {
    #[derive(Serialize, Describe)]
    struct Maybe {
        token: Option<String>,
    }

    let schema = Schema::describe::<Maybe>(&Terrace::new("T_").dialect());
    assert_eq!(key(&schema, "token").ty.as_deref(), Some("String"));
    assert!(!key(&schema, "token").required);
}

/// An enum of unit variants *is* the set of values one key accepts, so `Describe` on it reports
/// those values rather than refusing the shape.
#[test]
fn an_enum_key_reports_the_values_it_accepts() {
    #[derive(Serialize, Describe)]
    #[serde(rename_all = "lowercase")]
    enum LogLevel {
        Trace,
        Debug,
        Info,
    }

    #[derive(Serialize, Describe)]
    struct Observability {
        /// How much the service says.
        #[config(values)]
        log_level: LogLevel,
        /// Only when it is turned on at all.
        #[config(values)]
        fallback: Option<LogLevel>,
    }

    let schema = Schema::describe::<Observability>(&Terrace::new("T_").dialect());
    assert_eq!(key(&schema, "log_level").values, ["trace", "debug", "info"]);
    assert_eq!(key(&schema, "fallback").values, ["trace", "debug", "info"]);

    // The choices are what an operator can act on, so they are what the table prints — with the
    // separating pipes escaped, or they would end the cell.
    let markdown = schema.to_markdown();
    assert!(
        markdown.contains(r"`LogLevel`: `trace` \| `debug` \| `info`"),
        "{markdown}"
    );
}

/// Variant names are `PascalCase` by convention, so `rename_all` means something different for a
/// variant than for a field — `snake_case` has to insert the underscores rather than keep them.
#[test]
fn variant_renaming_follows_serdes_variant_rules() {
    macro_rules! variants {
        ($rule:literal) => {{
            #[derive(Describe)]
            #[serde(rename_all = $rule)]
            enum Rule {
                PlainOld,
                Second,
            }
            <Rule as terrace_config::schema::Values>::VARIANTS
        }};
    }

    assert_eq!(variants!("snake_case"), ["plain_old", "second"]);
    assert_eq!(variants!("SCREAMING_SNAKE_CASE"), ["PLAIN_OLD", "SECOND"]);
    assert_eq!(variants!("kebab-case"), ["plain-old", "second"]);
    assert_eq!(variants!("SCREAMING-KEBAB-CASE"), ["PLAIN-OLD", "SECOND"]);
    assert_eq!(variants!("camelCase"), ["plainOld", "second"]);
    assert_eq!(variants!("PascalCase"), ["PlainOld", "Second"]);
    assert_eq!(variants!("lowercase"), ["plainold", "second"]);
    assert_eq!(variants!("UPPERCASE"), ["PLAINOLD", "SECOND"]);
}

#[test]
fn a_renamed_or_skipped_variant_is_honoured() {
    #[derive(Describe)]
    enum Mixed {
        #[serde(rename = "spelled-out")]
        Renamed,
        #[serde(skip)]
        Hidden,
        Kept,
    }

    assert_eq!(
        <Mixed as terrace_config::schema::Values>::VARIANTS,
        ["spelled-out", "Kept"]
    );
}

/// An alias is a spelling that works. Left out of the schema it is documented nowhere, which is
/// the same class of silent gap as a wrong environment variable pointing the other way.
#[test]
fn serde_aliases_are_reported_as_the_key_paths_they_are() {
    #[derive(Serialize, Describe)]
    struct Aliased {
        #[config(nested)]
        github: Inner,
    }

    #[derive(Serialize, Describe)]
    struct Inner {
        /// The account.
        #[serde(alias = "user", alias = "login")]
        username: String,
    }

    let schema = Schema::describe::<Aliased>(&Terrace::new("T_").dialect());
    // Full paths, so each one's spellings derive exactly as the canonical path's do.
    assert_eq!(
        key(&schema, "github.username").aliases,
        ["github.user", "github.login"]
    );

    let markdown = schema.to_markdown_with(&[Column::Path, Column::Aliases]);
    assert!(markdown.contains("| `github.username` | `github.user`, `github.login` |"));
}

#[test]
fn a_key_with_no_aliases_reports_none() {
    assert!(key(&schema(), "dist_dir").aliases.is_empty());
}

/// The whole point of the type column: a required key with no default still tells the reader
/// what shape to supply.
#[test]
fn a_required_key_says_what_to_supply_even_with_no_default() {
    let markdown = schema()
        .with_defaults_from(&Config::default())
        .unwrap()
        .to_markdown();
    let row = markdown
        .lines()
        .find(|line| line.starts_with("| `github.username`"))
        .expect("the required key is in the table");

    assert!(row.contains("| `String` |"), "{row}");
    assert!(row.contains("| required |"), "{row}");
}

/// Found by the `schema` fuzz target on the first libFuzzer campaign that ran it: a key path
/// carrying a NUL produced an environment spelling the schema advertised and `std::env::set_var`
/// then refused. POSIX and Windows both forbid `=` in a variable name, and a NUL ends the string
/// that carries it, so a name containing either is one no operator can create — the same false
/// claim as printing the wrong name.
#[test]
fn a_spelling_no_operating_system_could_hold_is_not_advertised() {
    #[derive(Describe)]
    struct One {
        #[serde(rename = "a\0b")]
        nul: bool,
    }

    let schema = Schema::describe::<One>(&Terrace::new("T_").dialect());
    let key = key(&schema, "a\0b");
    assert_eq!(key.env, None);
    assert_eq!(key.env_file, None);
    assert_eq!(key.secrets_file, None);
}

/// The separator and the suffix are parameters too, so an unusable name can come from the
/// dialect rather than from the key.
#[test]
fn an_unusable_dialect_produces_no_spellings() {
    #[derive(Describe)]
    struct Nested {
        #[config(nested)]
        outer: Inner,
    }

    #[derive(Describe)]
    struct Inner {
        leaf: bool,
    }

    // A separator containing `=` lands in the middle of every nested variable name.
    let equals = Terrace::new("T_").nesting_separator("=").schema::<Nested>();
    assert_eq!(key(&equals, "outer.leaf").env, None);

    // A usable `env` does not make the indirection variable usable: the suffix is its own knob.
    let suffix = Terrace::new("T_").file_suffix("=F").schema::<Nested>();
    assert_eq!(
        key(&suffix, "outer.leaf").env.as_deref(),
        Some("T_OUTER__LEAF")
    );
    assert_eq!(key(&suffix, "outer.leaf").env_file, None);
}

/// A secrets-directory key is one entry *in* a directory. A name carrying a path separator names
/// a path instead, and `SecretsDir` reads `file_name()`, which can never contain one.
#[test]
fn a_secrets_file_name_that_would_be_a_path_is_not_advertised() {
    #[derive(Describe)]
    struct Nested {
        #[config(nested)]
        outer: Inner,
    }

    #[derive(Describe)]
    struct Inner {
        leaf: bool,
    }

    for separator in ["/", "\\"] {
        let schema = Terrace::new("T_")
            .nesting_separator(separator)
            .schema::<Nested>();
        assert_eq!(
            key(&schema, "outer.leaf").secrets_file,
            None,
            "separator {separator:?}"
        );
    }
}
