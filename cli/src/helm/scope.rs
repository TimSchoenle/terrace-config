//! Which charts a change touched, so a gate checks those and no others.
//!
//! A gate over the whole tree fails a pull request for a chart it never touched. Narrowing it to the
//! changed charts fixes that, but only when the narrowing is safe: a change outside one chart's own
//! directory can move every chart's verdict at once. The pinned CLI, `terrace.yaml`, a workflow, a
//! library chart the others depend on: none of these sit under the chart they break. So the rule is
//! one-sided. Every change that is not provably confined to application charts widens the scope to
//! the whole tree, and so does every failure to read the change. This module can therefore widen a
//! gate and never narrow one by mistake.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// The charts a change touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every chart in the tree, and why the change could not be narrowed.
    Every(Widened),
    /// These application charts, by directory name, sorted. Never empty.
    Charts(Vec<String>),
}

impl Scope {
    /// The chart names to select, empty for every chart.
    #[must_use]
    pub fn names(&self) -> &[String] {
        match self {
            Self::Every(_) => &[],
            Self::Charts(names) => names,
        }
    }
}

/// Why a change was not narrowed to the charts it touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widened {
    /// The change could not be read: no such revision, a shallow clone, no Git.
    Unreadable(String),
    /// Nothing changed between the base and `HEAD`.
    NoChanges,
    /// A changed path lies outside every chart's own directory.
    Outside(String),
    /// A changed path lies inside a library chart, which other charts depend on.
    Library(String),
    /// A changed path names a chart directory that is not a valid chart name.
    InvalidName(String),
}

impl std::fmt::Display for Widened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(reason) => write!(f, "the change could not be read: {reason}"),
            Self::NoChanges => f.write_str("the change touches no file"),
            Self::Outside(path) => write!(f, "{path} is outside every chart's own directory"),
            Self::Library(chart) => write!(f, "{chart} is a library chart"),
            Self::InvalidName(path) => write!(f, "{path} is not under a valid chart name"),
        }
    }
}

/// What a chart directory holds, as far as scoping is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An application chart: a change to it affects that chart alone.
    Application,
    /// A library chart: a change to it can affect every chart.
    Library,
    /// No chart: the change deleted it, so nothing is left to check.
    Absent,
}

/// The scope of a change, from the paths it touched.
///
/// `paths` are relative to the repository root, as `git diff --name-only` prints them, and
/// `prefix` is the chart tree's path relative to the same root, without a trailing slash.
/// `kind` says what a chart directory, named by its directory name, holds now.
#[must_use]
pub fn classify<I, S>(paths: I, prefix: &str, kind: impl Fn(&str) -> Kind) -> Scope
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut touched = BTreeSet::new();
    let mut seen_any = false;
    for path in paths {
        let path = path.as_ref();
        if path.is_empty() {
            continue;
        }
        seen_any = true;

        // A chart's own file is `<prefix>/<name>/<something>`. A file directly under the tree,
        // such as its README, belongs to no chart, so it widens like any other outside path.
        let inside = if prefix.is_empty() {
            Some(path)
        } else {
            path.strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix('/'))
        };
        let Some((name, rest)) = inside.and_then(|rest| rest.split_once('/')) else {
            return Scope::Every(Widened::Outside(path.to_owned()));
        };
        if rest.is_empty() {
            return Scope::Every(Widened::Outside(path.to_owned()));
        }
        if !is_chart_name(name) {
            return Scope::Every(Widened::InvalidName(path.to_owned()));
        }
        match kind(name) {
            Kind::Application => {
                touched.insert(name.to_owned());
            }
            Kind::Library => return Scope::Every(Widened::Library(name.to_owned())),
            Kind::Absent => {}
        }
    }

    if !seen_any {
        return Scope::Every(Widened::NoChanges);
    }
    if touched.is_empty() {
        // Every change was to a chart that no longer exists. Nothing is left to narrow to, and an
        // empty selection reads as "every chart" everywhere it is passed, so say so.
        return Scope::Every(Widened::NoChanges);
    }
    Scope::Charts(touched.into_iter().collect())
}

/// Whether `name` is a chart name Helm accepts: lowercase letters, digits and dashes, starting with
/// a letter or a digit.
fn is_chart_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The charts under `charts` that the commits since `base` touched.
///
/// Compares `HEAD` with its merge base with `base`, so a base branch that moved on since the branch
/// was cut does not count its own changes as this one's. Only committed changes count: this answers
/// what a pull request changes, and an uncommitted edit is not part of one.
#[must_use]
pub fn since(charts: &Path, base: &str) -> Scope {
    match changed_paths(charts, base) {
        Ok((paths, prefix)) => classify(&paths, &prefix, |name| kind_of(&charts.join(name))),
        Err(error) => Scope::Every(Widened::Unreadable(error.to_string())),
    }
}

/// The paths changed since the merge base, and the chart tree's path, both relative to the root.
fn changed_paths(charts: &Path, base: &str) -> Result<(Vec<String>, String), Error> {
    let here = if charts.is_dir() {
        charts.to_path_buf()
    } else {
        PathBuf::from(".")
    };
    let root = PathBuf::from(git(&["rev-parse", "--show-toplevel"], &here)?.trim());
    let prefix = relative_to(&root, charts)?;
    let fork = git(&["merge-base", base, "HEAD"], &root)?;
    let listed = git(
        &["diff", "--name-only", "--no-renames", fork.trim(), "HEAD"],
        &root,
    )?;
    Ok((listed.lines().map(str::to_owned).collect(), prefix))
}

/// `charts` relative to `root`, with forward slashes, as Git prints paths.
fn relative_to(root: &Path, charts: &Path) -> Result<String, Error> {
    let canonical = |path: &Path| {
        path.canonicalize()
            .map_err(|e| Error::io(path.display(), e))
    };
    let root = canonical(root)?;
    let charts = canonical(charts)?;
    let relative = charts.strip_prefix(&root).map_err(|_| {
        Error::Invalid(format!(
            "{} is outside the Git repository at {}",
            charts.display(),
            root.display()
        ))
    })?;
    Ok(relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

/// What the directory holds now.
fn kind_of(chart_dir: &Path) -> Kind {
    let Ok(text) = std::fs::read_to_string(chart_dir.join("Chart.yaml")) else {
        return Kind::Absent;
    };
    let library = text.lines().any(|line| {
        line.strip_prefix("type:")
            .is_some_and(|value| value.trim().trim_matches(['"', '\'']) == "library")
    });
    if library {
        Kind::Library
    } else {
        Kind::Application
    }
}

/// One Git command, with its output.
fn git(arguments: &[&str], directory: &Path) -> Result<String, Error> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .map_err(|failure| Error::Invalid(format!("git is not runnable: {failure}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Invalid(format!(
            "git {}: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, Scope, Widened, classify};

    fn apps(name: &str) -> Kind {
        match name {
            "common" => Kind::Library,
            "gone" => Kind::Absent,
            _ => Kind::Application,
        }
    }

    fn scope(paths: &[&str]) -> Scope {
        classify(paths, "charts", apps)
    }

    #[test]
    fn narrows_to_the_application_charts_touched_sorted_and_deduplicated() {
        assert_eq!(
            scope(&[
                "charts/web/values.yaml",
                "charts/api/Chart.yaml",
                "charts/web/templates/a.yaml",
            ]),
            Scope::Charts(vec!["api".to_owned(), "web".to_owned()])
        );
    }

    #[test]
    fn widens_on_any_path_outside_a_chart() {
        for path in [
            "terrace.yaml",
            ".github/workflows/ci.yaml",
            "charts/README.md",
            "chartsx/web/values.yaml",
        ] {
            assert_eq!(
                scope(&["charts/web/values.yaml", path]),
                Scope::Every(Widened::Outside(path.to_owned())),
                "{path}"
            );
        }
    }

    #[test]
    fn widens_on_a_library_chart() {
        assert_eq!(
            scope(&["charts/web/values.yaml", "charts/common/templates/_x.tpl"]),
            Scope::Every(Widened::Library("common".to_owned()))
        );
    }

    #[test]
    fn widens_on_a_directory_that_is_not_a_chart_name() {
        assert_eq!(
            scope(&["charts/Web/values.yaml"]),
            Scope::Every(Widened::InvalidName("charts/Web/values.yaml".to_owned()))
        );
        assert_eq!(
            scope(&["charts/-web/values.yaml"]),
            Scope::Every(Widened::InvalidName("charts/-web/values.yaml".to_owned()))
        );
    }

    #[test]
    fn leaves_out_a_deleted_chart() {
        assert_eq!(
            scope(&["charts/gone/Chart.yaml", "charts/web/values.yaml"]),
            Scope::Charts(vec!["web".to_owned()])
        );
    }

    #[test]
    fn widens_when_nothing_is_left_to_narrow_to() {
        assert_eq!(scope(&[]), Scope::Every(Widened::NoChanges));
        assert_eq!(
            scope(&["charts/gone/Chart.yaml"]),
            Scope::Every(Widened::NoChanges)
        );
    }

    #[test]
    fn reads_a_tree_at_the_repository_root_or_nested() {
        assert_eq!(
            classify(["web/values.yaml"], "", apps),
            Scope::Charts(vec!["web".to_owned()])
        );
        assert_eq!(
            classify(["deploy/charts/web/values.yaml"], "deploy/charts", apps),
            Scope::Charts(vec!["web".to_owned()])
        );
        assert_eq!(
            classify(["charts/web/values.yaml"], "deploy/charts", apps),
            Scope::Every(Widened::Outside("charts/web/values.yaml".to_owned()))
        );
    }

    #[test]
    fn names_are_empty_for_every_chart() {
        assert_eq!(Scope::Every(Widened::NoChanges).names(), [] as [String; 0]);
        assert_eq!(Scope::Charts(vec!["a".to_owned()]).names(), ["a"]);
    }
}
