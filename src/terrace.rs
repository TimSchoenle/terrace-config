//! The layered loader.

use std::path::PathBuf;

use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::de::DeserializeOwned;

use crate::dialect::Dialect;
use crate::error::Error;
use crate::layers::{FileLayers, ShadowPolicy};
use crate::loaded::{Loaded, Sources};
use crate::provider::TomlLayers;

/// The path the TOML layer falls back to when no configuration variable is set.
const DEFAULT_CONFIG_PATH: &str = "config.toml";

/// The layered loader.
///
/// Layers, lowest precedence first: struct defaults, TOML at `$<PREFIX>CONFIG` (a file, or
/// every `*.toml` in it when it names a directory), `<PREFIX>`-prefixed `__`-nested environment
/// variables, `$<PREFIX>SECRETS_DIR`, and `<PREFIX><KEY>_FILE` indirection.
///
/// Every environment name is derived from one prefix unless overridden, which is the whole of
/// the parameterisation this crate needs.
///
/// ```no_run
/// use serde::Deserialize;
/// use terrace_config::Terrace;
///
/// #[derive(Deserialize)]
/// struct Config {
///     database: Database,
/// }
/// # #[derive(Deserialize)]
/// # struct Database { url: String }
///
/// let config: Config = Terrace::new("MYAPP_").reserve("MYAPP_PROFILE").load()?;
/// # Ok::<(), terrace_config::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Terrace {
    /// The prefix every environment name derives from.
    prefix: String,
    /// Overrides the derived `<PREFIX>CONFIG`.
    config_var: Option<String>,
    /// Overrides the derived `<PREFIX>SECRETS_DIR`.
    secrets_dir_var: Option<String>,
    /// Where the TOML layer looks when the configuration variable is unset.
    default_config_path: PathBuf,
    /// The suffix marking an indirection variable.
    file_suffix: String,
    /// What separates nesting levels in an environment key.
    separator: String,
    /// Keys a file may not supply, in full environment spelling. The configuration and
    /// secrets-directory variables are added on top of these; see [`Self::dialect`].
    reserved: Vec<String>,
    /// What to do when one key is supplied by two mechanisms.
    shadow_policy: ShadowPolicy,
}

impl Terrace {
    /// A loader over `prefix`.
    ///
    /// `Terrace::new("MYAPP_")` reads `MYAPP_CONFIG`, `MYAPP_SECRETS_DIR`, `MYAPP_*` and
    /// `MYAPP_<KEY>_FILE`. The prefix is taken verbatim, trailing underscore included.
    #[must_use]
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            config_var: None,
            secrets_dir_var: None,
            default_config_path: PathBuf::from(DEFAULT_CONFIG_PATH),
            file_suffix: "_FILE".to_owned(),
            separator: "__".to_owned(),
            reserved: Vec::new(),
            shadow_policy: ShadowPolicy::Reject,
        }
    }

    /// Override the variable naming the TOML layer. Defaults to `<PREFIX>CONFIG`.
    #[must_use]
    pub fn config_var(mut self, name: impl Into<String>) -> Self {
        self.config_var = Some(name.into());
        self
    }

    /// Override the variable naming the secrets directory. Defaults to `<PREFIX>SECRETS_DIR`.
    #[must_use]
    pub fn secrets_dir_var(mut self, name: impl Into<String>) -> Self {
        self.secrets_dir_var = Some(name.into());
        self
    }

    /// Where the TOML layer looks when the configuration variable is unset. Defaults to
    /// `config.toml`, relative to the working directory.
    #[must_use]
    pub fn default_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.default_config_path = path.into();
        self
    }

    /// Override the indirection suffix. Defaults to `_FILE`.
    #[must_use]
    pub fn file_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.file_suffix = suffix.into();
        self
    }

    /// Override the nesting separator. Defaults to `__`.
    #[must_use]
    pub fn nesting_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Reserve a key, in its **full environment spelling**, e.g. `MYAPP_PROFILE`.
    ///
    /// A reserved key is read from the environment before the layers exist, which a file
    /// therefore may not supply. The configuration and secrets-directory variables are reserved
    /// automatically — including when [`Self::config_var`] or [`Self::secrets_dir_var`] renamed
    /// them after this call, because the set is resolved at load time rather than here.
    #[must_use]
    pub fn reserve(mut self, key: impl Into<String>) -> Self {
        self.reserved.push(key.into());
        self
    }

    /// What to do when one key is supplied by two mechanisms. Defaults to
    /// [`ShadowPolicy::Reject`].
    #[must_use]
    pub fn shadow_policy(mut self, policy: ShadowPolicy) -> Self {
        self.shadow_policy = policy;
        self
    }

    /// The variable naming the TOML layer.
    #[must_use]
    pub fn config_var_name(&self) -> String {
        self.config_var
            .clone()
            .unwrap_or_else(|| format!("{}CONFIG", self.prefix))
    }

    /// The variable naming the secrets directory.
    #[must_use]
    pub fn secrets_dir_var_name(&self) -> String {
        self.secrets_dir_var
            .clone()
            .unwrap_or_else(|| format!("{}SECRETS_DIR", self.prefix))
    }

    /// The environment spelling this loader reads, reserved keys included.
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        let dialect = Dialect::new(self.prefix.clone())
            .nesting_separator(self.separator.clone())
            .file_suffix(self.file_suffix.clone())
            // Both are read to decide what the layers *are*, so neither can come from a layer.
            .reserve(self.config_var_name())
            .reserve(self.secrets_dir_var_name());
        self.reserved
            .iter()
            .fold(dialect, |dialect, key| dialect.reserve(key.clone()))
    }

    /// The assembled figment.
    ///
    /// # Errors
    /// As [`Self::load`], minus the extraction.
    pub fn figment(&self) -> Result<Figment, Error> {
        Ok(self.assemble()?.0)
    }

    /// Load a typed config.
    ///
    /// # Errors
    /// Returns [`Error`] if a required value is missing, a value fails to parse, a file-backed
    /// source cannot be read, or — under [`ShadowPolicy::Reject`] — one key is supplied by more
    /// than one of the last three layers.
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, Error> {
        self.figment()?.extract().map_err(|e| Box::new(e).into())
    }

    /// Load a typed config and everything needed to load it again later.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn load_watched<T: DeserializeOwned>(&self) -> Result<Loaded<T>, Error> {
        let (figment, toml, files) = self.assemble()?;

        // Extracted before the typed value and kept whole: it is the only comparable
        // representation of the config, because the typed struct may hold non-`PartialEq`
        // secret types.
        let fingerprint = figment
            .extract::<figment::value::Value>()
            .map_err(Box::new)?;
        let value = figment.extract().map_err(Box::new)?;

        let mut watch: Vec<PathBuf> = files.watch_paths().into_iter().collect();
        watch.extend(toml.watch_dir().map(std::path::Path::to_path_buf));
        watch.sort();
        watch.dedup();

        Ok(Loaded {
            value,
            sources: Sources { watch, fingerprint },
        })
    }

    /// The assembled figment, plus the layers that went into it.
    ///
    /// Split out because a reload has to re-run exactly this assembly, and needs the layers to
    /// know which paths to watch.
    fn assemble(&self) -> Result<(Figment, TomlLayers, FileLayers), Error> {
        let dialect = self.dialect();

        let config_var = self.config_var_name();
        let config_path = std::env::var(&config_var)
            .map_or_else(|_| self.default_config_path.clone(), PathBuf::from);
        let toml = TomlLayers::expand(&config_var, config_path)?;

        let mut figment = Figment::new();
        // File by file rather than through `TomlLayers`'s own `Provider` impl, so figment
        // attributes a bad value to the fragment it is actually in.
        for file in toml.files() {
            figment = figment.merge(Toml::file(file));
        }
        figment = figment.merge(Env::prefixed(&self.prefix).split(&self.separator));

        // Collected after the environment layer is in place but merged on top of it: the file
        // layers are the more deliberate way to supply a value, and `FileLayers::collect` has
        // already refused any key that two mechanisms define.
        let secrets_var = self.secrets_dir_var_name();
        let dir = std::env::var(&secrets_var)
            .ok()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from);
        let files = FileLayers::collect(dir, &secrets_var, &dialect, self.shadow_policy)?;
        if !files.is_empty() {
            figment = figment.merge(files.clone());
        }

        Ok((figment, toml, files))
    }
}
