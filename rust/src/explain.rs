//! Which layer supplied each configuration key.
//!
//! The loader's worst failure is not the one that raises an error — it is the one that raises
//! none. A value arrives from the layer nobody expected, the service boots, and the first sign
//! of it is a credential that was rotated a week ago still being presented.
//! [`ShadowPolicy::Reject`](crate::ShadowPolicy::Reject) catches the three file-and-environment
//! spellings of that; it deliberately says nothing about the TOML layer, where a checked-in
//! `config.toml` and a `MYAPP_` variable naming the same key is an ordinary, intended override.
//! Ordinary until it is the one you did not know about.
//!
//! [`Terrace::explain`](crate::Terrace::explain) answers it for every key at once, at boot and
//! on every reload, without a debugger and without a rebuild:
//!
//! ```text
//! terrace-config: prefix `MYAPP_`, 4 keys, 1 supplied by more than one layer
//! layers, lowest precedence first:
//!   TOML          MYAPP_CONFIG=/etc/myapp/conf.d
//!                   /etc/myapp/conf.d/10-base.toml (2 keys)
//!                   /etc/myapp/conf.d/20-tuning.toml (1 key)
//!   environment   MYAPP_* (1 key)
//!   secrets dir   MYAPP_SECRETS_DIR=/run/secrets (1 key)
//!   indirection   MYAPP_*_FILE (none)
//! keys:
//!   auth.jwt_secret  <- secrets file /run/secrets/auth__jwt_secret
//!   database.url     <- environment MYAPP_DATABASE__URL
//!                       shadowing TOML /etc/myapp/conf.d/10-base.toml
//!   server.port      <- TOML /etc/myapp/conf.d/20-tuning.toml
//!   server.workers   <- TOML /etc/myapp/conf.d/10-base.toml
//! ```
//!
//! # Nothing here holds a value
//!
//! An [`Explanation`] records *where* each key came from and never *what* it was. That is a
//! property of the type rather than of how it happens to print: there is no field to leak, so
//! [`Debug`], [`Display`](std::fmt::Display) and anything rendered from the accessors are redacted by construction
//! rather than by remembering to redact. It is what makes printing one at boot — into a log that
//! is shipped, indexed and retained for a year — safe by default rather than safe if reviewed.
//!
//! The same rule costs the report one thing, and it is worth naming: a TOML fragment that will
//! not parse is reported as [`Fragment::Unreadable`] with no reason attached, because a parse
//! error quotes the line it failed on and that line can be the credential.
//! [`Terrace::load`](crate::Terrace::load) fails with figment's own message, which is where the
//! detail belongs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use figment::Provider as _;
use figment::providers::{Format as _, Toml};
use figment::value::{Dict, Value};

use crate::terrace::Layers;

/// How deep a TOML fragment is walked before the rest of a branch is reported as one path.
///
/// The `toml` parser has a recursion limit of its own, so this is a belt to its braces rather
/// than the thing standing between a fuzz input and a blown stack — but a report is exactly the
/// code that runs while something is already wrong, and it does not get to be the part that
/// crashes. A truncated path is still a real prefix of the key it stands for.
const MAX_DEPTH: usize = 32;

/// The longest key a rendered line pads to, so one absurd path does not indent every other.
const MAX_KEY_WIDTH: usize = 44;

/// One layer, and the file or variable inside it that carried a value.
///
/// Exhaustive on purpose, unlike [`Error`](crate::Error): these are the layers the crate
/// documents, a fifth one would be a feature nobody could miss, and a caller filtering a report
/// down to "everything that came out of a file" should get a compile error when that day comes
/// rather than a silently incomplete filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layer {
    /// A TOML fragment: the file `$<PREFIX>CONFIG` names, or one of the `*.toml` files in the
    /// directory it names.
    Toml(PathBuf),
    /// A `<PREFIX>`-prefixed environment variable, in the spelling it was set in.
    Env(String),
    /// A key-named file directly inside `$<PREFIX>SECRETS_DIR`.
    SecretsFile(PathBuf),
    /// `<PREFIX><KEY><SUFFIX>` indirection.
    Indirection {
        /// The variable that was set, e.g. `MYAPP_AUTH__JWT_SECRET_FILE`.
        var: String,
        /// The file it named.
        path: PathBuf,
    },
}

impl std::fmt::Display for Layer {
    /// One layer as a phrase that can be pasted into a `grep`, `ls` or `printenv`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toml(path) => write!(f, "TOML {}", path.display()),
            Self::Env(var) => write!(f, "environment {var}"),
            Self::SecretsFile(path) => write!(f, "secrets file {}", path.display()),
            Self::Indirection { var, path } => {
                write!(f, "indirection {var} -> {}", path.display())
            }
        }
    }
}

/// One key, and every layer that supplied it.
///
/// The fields are private because there is an invariant to keep: a key is here *because* some
/// layer supplied it, so there is always exactly one [`effective`](Self::effective) layer.
/// Splitting it out of the list rather than asserting over one is what makes that invariant a
/// property of the type instead of a `# Panics` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The figment key path, e.g. `auth.jwt_secret`.
    key: String,
    /// The layer whose value is in effect: the last one merged.
    effective: Layer,
    /// Layers that also supplied the key and were overridden, lowest precedence first.
    shadowed: Vec<Layer>,
}

impl Origin {
    /// The figment key path, e.g. `auth.jwt_secret`.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The layer whose value is the one the service is running on.
    #[must_use]
    pub fn effective(&self) -> &Layer {
        &self.effective
    }

    /// The layers that also supplied this key and lost, lowest precedence first.
    ///
    /// Empty for almost every key. When it is not, it is the answer to "why is my mounted secret
    /// not being picked up" — the mount is right there in the list, underneath whatever beat it.
    #[must_use]
    pub fn shadowed(&self) -> &[Layer] {
        &self.shadowed
    }

    /// Every layer that supplied this key, lowest precedence first, ending with
    /// [`Self::effective`].
    pub fn sources(&self) -> impl Iterator<Item = &Layer> {
        self.shadowed.iter().chain(std::iter::once(&self.effective))
    }

    /// Whether more than one layer supplied this key.
    #[must_use]
    pub fn is_contested(&self) -> bool {
        !self.shadowed.is_empty()
    }
}

/// What became of one file in the TOML layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fragment {
    /// Read, and supplied this many keys. Zero is a real answer: an empty file, or one holding
    /// only section headers.
    Read(usize),
    /// Named but not there.
    ///
    /// Not an error — `Toml::file` skips an absent file, and a service with no configuration
    /// file at all is the normal development case. It is, however, the most common reason a
    /// configured path supplies nothing, which is why it is reported rather than passed over.
    Missing,
    /// There, but not parseable as TOML.
    ///
    /// **The reason is not reported here.** A TOML parse error quotes the line it failed on and
    /// this type never carries a value; [`Terrace::load`](crate::Terrace::load) fails with
    /// figment's own message, which names the file, the line and the column.
    Unreadable,
}

impl std::fmt::Display for Fragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(0) => f.write_str("no keys"),
            Self::Read(1) => f.write_str("1 key"),
            Self::Read(keys) => write!(f, "{keys} keys"),
            Self::Missing => f.write_str("missing"),
            Self::Unreadable => f.write_str("not valid TOML"),
        }
    }
}

/// Where every value a loader can see would come from.
///
/// Built by [`Terrace::explain`](crate::Terrace::explain). Print it — [`Display`](std::fmt::Display) renders the
/// whole report, and it carries no configuration value, so a log is a safe place for it — or
/// walk [`Self::origins`] and emit whatever your own telemetry wants.
///
/// ```no_run
/// # use terrace_config::Terrace;
/// let explanation = Terrace::new("MYAPP_").explain()?;
///
/// // At boot, or from inside a reload closure.
/// println!("{explanation}");
///
/// // Or only the part that is ever surprising.
/// for origin in explanation.contested() {
///     println!("{} is set in more than one place: {}", origin.key(), origin.effective());
/// }
/// # Ok::<(), terrace_config::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Explanation {
    /// The prefix every variable in this report derives from.
    prefix: String,
    /// What marks an indirection variable. Stored rather than inferred from the variables that
    /// were found: it is a parameter, and the header has to name it even when the layer supplied
    /// nothing — which is precisely when an operator is reading the header.
    indirection_suffix: String,
    /// The variable naming the TOML layer, and whether it was actually set.
    config_var: String,
    config_path: PathBuf,
    config_from_env: bool,
    /// Every file the TOML layer expanded to, in merge order, and what became of it.
    fragments: Vec<(PathBuf, Fragment)>,
    /// The variable naming the secrets directory, and what it resolved to.
    secrets_var: String,
    secrets_dir: Option<PathBuf>,
    /// How many keys each of the three non-TOML layers supplied.
    env_keys: usize,
    secrets_keys: usize,
    indirection_keys: usize,
    /// Every key some layer supplied, by key path.
    origins: Vec<Origin>,
}

impl Explanation {
    /// Report on already-collected layers.
    ///
    /// Crate-private, and takes the loader's own assembly product rather than re-reading the
    /// environment: a report that did its own reading could disagree with the load it describes.
    pub(crate) fn of(layers: &Layers) -> Self {
        // Appended in merge order, so the last source of a key is the one in effect. The order
        // below is `Terrace::assemble`'s, and has to stay it.
        let mut sources: BTreeMap<String, Vec<Layer>> = BTreeMap::new();

        let mut fragments = Vec::with_capacity(layers.toml.files().len());
        for path in layers.toml.files() {
            let fragment = if !path.is_file() {
                Fragment::Missing
            } else if let Some(keys) = fragment_keys(path) {
                let read = Fragment::Read(keys.len());
                for key in keys {
                    sources
                        .entry(key)
                        .or_default()
                        .push(Layer::Toml(path.clone()));
                }
                read
            } else {
                Fragment::Unreadable
            };
            fragments.push((path.clone(), fragment));
        }

        let env = layers.dialect.plain_env_entries();
        let env_keys = env.len();
        for (key, vars) in env {
            for var in vars {
                sources
                    .entry(key.clone())
                    .or_default()
                    .push(Layer::Env(var));
            }
        }

        let secrets = layers.files.secrets();
        let secrets_keys = secrets.map_or(0, |secrets| secrets.values().len());
        if let Some(secrets) = secrets {
            for (key, value) in secrets.values() {
                sources
                    .entry(key.clone())
                    .or_default()
                    .push(Layer::SecretsFile(value.path().to_path_buf()));
            }
        }

        let indirections = layers.files.indirections();
        let indirection_keys = indirections.map_or(0, |files| files.values().len());
        if let Some(files) = indirections {
            for (key, value) in files.values() {
                sources
                    .entry(key.clone())
                    .or_default()
                    .push(Layer::Indirection {
                        // Recorded alongside the value, so this is the spelling the operator
                        // used. The fallback reconstructs the documented one: it cannot be
                        // reached, and a report is the last code that should panic to say so.
                        var: files.origin(key).map_or_else(
                            || {
                                format!(
                                    "{}{}",
                                    layers.dialect.env_spelling(key),
                                    layers.dialect.indirection_suffix()
                                )
                            },
                            str::to_owned,
                        ),
                        path: value.path().to_path_buf(),
                    });
            }
        }

        Self {
            prefix: layers.dialect.prefix().to_owned(),
            indirection_suffix: layers.dialect.indirection_suffix().to_owned(),
            config_var: layers.config_var.clone(),
            config_path: layers.config_path.clone(),
            config_from_env: layers.config_from_env,
            fragments,
            secrets_var: layers.secrets_var.clone(),
            secrets_dir: layers.secrets_dir.clone(),
            env_keys,
            secrets_keys,
            indirection_keys,
            origins: sources
                .into_iter()
                .filter_map(Origin::from_sources)
                .collect(),
        }
    }

    /// Every key some layer supplied, in key-path order.
    ///
    /// Keys nothing supplied are absent: this reports what the *environment* did, not what the
    /// configuration type can carry. The `schema` module answers the other half, and answers it
    /// without reading anything — named in prose rather than linked, because this module compiles
    /// without that feature and `broken_intra_doc_links` is denied.
    #[must_use]
    pub fn origins(&self) -> &[Origin] {
        &self.origins
    }

    /// One key's origin, by figment key path (`auth.jwt_secret`).
    #[must_use]
    pub fn origin(&self, key: &str) -> Option<&Origin> {
        // Binary search rather than a scan: `origins` is built from a `BTreeMap`, so it is
        // sorted by key, and a boot-time report over a few hundred keys is called per key by
        // anything checking a list of them.
        self.origins
            .binary_search_by(|origin| origin.key.as_str().cmp(key))
            .ok()
            .map(|at| &self.origins[at])
    }

    /// The keys more than one layer supplied, in key-path order.
    pub fn contested(&self) -> impl Iterator<Item = &Origin> {
        self.origins.iter().filter(|origin| origin.is_contested())
    }

    /// Every file the TOML layer expanded to, in merge order, and what became of it.
    #[must_use]
    pub fn fragments(&self) -> &[(PathBuf, Fragment)] {
        &self.fragments
    }

    /// The secrets directory in use, if one was configured.
    #[must_use]
    pub fn secrets_dir(&self) -> Option<&Path> {
        self.secrets_dir.as_deref()
    }

    /// The prefix every variable in this report derives from.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

impl std::fmt::Display for Explanation {
    /// The whole report, as the module documentation shows it.
    ///
    /// No trailing newline, so `tracing::info!("{explanation}")` and `println!` both do the
    /// right thing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let contested = self.contested().count();
        write!(
            f,
            "terrace-config: prefix `{}`, {}",
            self.prefix,
            plural(self.origins.len(), "key")
        )?;
        if contested > 0 {
            write!(f, ", {contested} supplied by more than one layer")?;
        }

        f.write_str("\nlayers, lowest precedence first:\n  TOML          ")?;
        if self.config_from_env {
            write!(f, "{}={}", self.config_var, self.config_path.display())?;
        } else {
            // The path alone would read as though the variable were set to it, and "the
            // variable is not set" is a different thing to check than "the file is not there".
            write!(
                f,
                "{} unset, default {}",
                self.config_var,
                self.config_path.display()
            )?;
        }
        for (path, fragment) in &self.fragments {
            write!(f, "\n                  {} ({fragment})", path.display())?;
        }

        write!(
            f,
            "\n  environment   {}* ({})",
            self.prefix,
            count(self.env_keys)
        )?;

        f.write_str("\n  secrets dir   ")?;
        match &self.secrets_dir {
            Some(dir) => write!(
                f,
                "{}={} ({})",
                self.secrets_var,
                dir.display(),
                count(self.secrets_keys)
            )?,
            None => write!(f, "{} unset", self.secrets_var)?,
        }

        write!(
            f,
            "\n  indirection   {}*{} ({})",
            self.prefix,
            self.indirection_suffix,
            count(self.indirection_keys)
        )?;

        f.write_str("\nkeys:")?;
        if self.origins.is_empty() {
            // An empty section reads as a rendering bug; this reads as the finding it is.
            return f.write_str("\n  none — every value in this configuration is a default");
        }

        let width = self
            .origins
            .iter()
            .map(|origin| origin.key.len())
            .max()
            .unwrap_or(0)
            .min(MAX_KEY_WIDTH);
        for origin in &self.origins {
            write!(
                f,
                "\n  {:width$}  <- {}",
                origin.key,
                origin.effective,
                width = width
            )?;
            for layer in &origin.shadowed {
                write!(f, "\n  {:width$}     shadowing {layer}", "", width = width)?;
            }
        }
        Ok(())
    }
}

impl Origin {
    /// One key's sources, in merge order, split into the one in effect and the rest.
    ///
    /// [`None`] for an empty list, which cannot happen — an entry exists because a layer wrote
    /// into it — and which is filtered out rather than asserted away for the reason the whole
    /// module avoids panicking.
    fn from_sources((key, mut sources): (String, Vec<Layer>)) -> Option<Self> {
        let effective = sources.pop()?;
        Some(Self {
            key,
            effective,
            shadowed: sources,
        })
    }
}

/// Every key path one TOML fragment supplies, or [`None`] if it will not parse.
fn fragment_keys(path: &Path) -> Option<Vec<String>> {
    // The provider directly rather than through a `Figment`: this needs the fragment's own keys,
    // and a figment would have already coalesced them with something.
    let data = Toml::file(path).data().ok()?;
    let mut keys = Vec::new();
    for dict in data.values() {
        push_leaves(dict, &mut String::new(), &mut keys, 0);
    }
    Some(keys)
}

/// Push the dotted path of every leaf in `dict` into `keys`.
fn push_leaves(dict: &Dict, prefix: &mut String, keys: &mut Vec<String>, depth: usize) {
    for (segment, value) in dict {
        let restore = prefix.len();
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(segment);
        match value {
            // An empty table is a leaf: it is a path the file really does mention, and a
            // fragment holding nothing else would otherwise report as supplying nothing.
            Value::Dict(_, inner) if !inner.is_empty() && depth < MAX_DEPTH => {
                push_leaves(inner, prefix, keys, depth + 1);
            }
            _ => keys.push(prefix.clone()),
        }
        prefix.truncate(restore);
    }
}

/// `1 key` / `4 keys`, so a report reads as English rather than as a template.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// A layer's key count, where zero is worth spelling out: it is the finding, not the absence of
/// one.
fn count(keys: usize) -> String {
    if keys == 0 {
        "none".to_owned()
    } else {
        plural(keys, "key")
    }
}
