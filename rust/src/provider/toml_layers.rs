//! The TOML layer: one file, or every `*.toml` in a directory.

use std::path::{Path, PathBuf};

use figment::providers::{Format, Toml};
use figment::value::{Dict, Map};
use figment::{Figment, Metadata, Profile, Provider};

use crate::error::Error;

/// The TOML files one configured path denotes, in merge order.
#[derive(Debug, Clone)]
pub struct TomlLayers {
    /// The configured path itself, file or directory.
    root: PathBuf,
    /// The files it expanded to, in merge order.
    files: Vec<PathBuf>,
}

impl TomlLayers {
    /// Expand `path` into the TOML files it denotes.
    ///
    /// A plain path is returned as-is, missing or not — `Toml::file` skips an absent file, and
    /// "a misspelled path is not an error" is deliberate: a service with no config file at all
    /// is the normal development case. A *directory* is expanded to every `*.toml` directly
    /// inside it, sorted by name so a `10-base.toml` / `20-overrides.toml` pair merges in the
    /// order an operator reading the mount would predict.
    ///
    /// Dot-prefixed entries are skipped for the same reason as in
    /// [`SecretsDir`](super::SecretsDir): a Kubernetes `ConfigMap` volume is a directory of
    /// symlinks beside a `..data` directory. For that same reason the regular-file test goes
    /// through `fs::metadata`, which follows symlinks, and not `DirEntry::metadata()`, which
    /// despite the name does not — under a `ConfigMap` mount the latter rejects every fragment
    /// and yields an empty config layer.
    ///
    /// `origin` names whatever pointed at `path` and is quoted back in any error.
    ///
    /// # Errors
    /// Returns [`Error::Source`] if `path` is a directory that cannot be read.
    pub fn expand(origin: &str, path: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = path.into();
        if !root.is_dir() {
            return Ok(Self {
                files: vec![root.clone()],
                root,
            });
        }

        let entries = std::fs::read_dir(&root).map_err(|e| {
            Error::source(format!(
                "{origin} is {}, which could not be read: {e}",
                root.display()
            ))
        })?;

        let mut files = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|e| Error::source(format!("reading {}: {e}", root.display())))?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let file = entry.path();
            if !std::fs::metadata(&file).is_ok_and(|m| m.is_file()) {
                continue;
            }
            if file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
            {
                files.push(file);
            }
        }
        files.sort();
        Ok(Self { root, files })
    }

    /// The files, in merge order.
    ///
    /// [`Terrace`](crate::Terrace) merges these one at a time rather than through this type's
    /// [`Provider`] impl, so that figment attributes a bad value to the file it is actually in.
    /// A `Provider` carries one [`Metadata`] for the whole layer and cannot say more.
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The directory a reload has to watch, if there is one.
    ///
    /// The directory either way: watching a `config.toml` that does not exist yet registers
    /// nothing, and a file created later would then never be noticed.
    #[must_use]
    pub fn watch_dir(&self) -> Option<&Path> {
        if self.root.is_dir() {
            return Some(&self.root);
        }
        self.root.parent().filter(|p| !p.as_os_str().is_empty())
    }
}

impl Provider for TomlLayers {
    fn metadata(&self) -> Metadata {
        Metadata::named(format!("TOML at {}", self.root.display()))
    }

    fn data(&self) -> Result<Map<Profile, Dict>, figment::Error> {
        let mut figment = Figment::new();
        for file in &self.files {
            figment = figment.merge(Toml::file(file));
        }
        figment.data()
    }
}
