//! Writing a set of chart files all at once, or not at all.
//!
//! The writers (`adopt`, `helm init`) each plan every file before touching one, and both need the
//! same guarantee from the step that follows: a failure leaves the tree as it was rather than half
//! written. That guarantee lives here, once, so the two cannot drift apart on what "atomic" means.
//!
//! Three phases. Every target is resolved first, and refused when it lands outside the chart it
//! was planned for: a `templates/_helpers.tpl` that is a symbolic link to somewhere else is
//! somebody else's file. Each new text is then written to a temporary file beside its target, so
//! the rename that follows stays on one filesystem, and each target's current contents are kept in
//! memory; a failure here removes the temporaries and has changed no file. Only then is each
//! temporary renamed over its target. A rename is atomic, so no reader sees half a file, and the
//! staging has already spent the space and proved the permissions, so a rename rarely fails. When
//! one does, the targets already replaced are put back from what was kept, and a file that could not
//! be put back is named in the error.
//!
//! Two things are not covered. The directories a new file needs are created during resolution, so
//! a failure after that can leave an empty directory behind: removing it again could remove one
//! that somebody created in the meantime, and an empty directory changes no render. And nothing here
//! locks the chart: a file another process changes between the plan and the rename is replaced
//! without notice, so a writer runs on a tree nobody else is writing.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// One file to write: where, the chart directory it must stay inside, and its whole new text.
#[derive(Debug, Clone)]
pub struct Planned {
    /// The chart directory the file was planned for. The write is refused when `path` resolves
    /// outside it.
    pub chart: PathBuf,
    /// The file to replace or create.
    pub path: PathBuf,
    /// Its complete new contents.
    pub text: String,
}

/// Write every planned file, all of them or none.
///
/// A file that exists keeps its permissions. A file that does not is created with the process's
/// defaults, and the directories it needs are created first.
///
/// # Errors
/// [`Error::Io`] when a file or directory cannot be resolved, created or written, and
/// [`Error::Invalid`] when a file resolves outside its chart. In every case no file has changed,
/// unless putting a file back after a failed rename failed too; the error then names each file
/// left changed.
pub fn write_all(planned: &[Planned]) -> Result<(), Error> {
    let mut targets = Vec::with_capacity(planned.len());
    for file in planned {
        targets.push(resolve(file)?);
    }

    let mut staged: Vec<Staged> = Vec::with_capacity(planned.len());
    for (index, (file, target)) in planned.iter().zip(&targets).enumerate() {
        match stage(target, &file.text, index) {
            Ok(held) => staged.push(held),
            Err(error) => {
                discard(&staged);
                return Err(error);
            }
        }
    }

    for (position, (held, target)) in staged.iter().zip(&targets).enumerate() {
        if let Err(error) = std::fs::rename(&held.temporary, target) {
            discard(&staged[position..]);
            let error = Error::io(target.display(), error);
            let unrestored = restore(&staged[..position], &targets[..position]);
            if unrestored.is_empty() {
                return Err(error);
            }
            return Err(Error::Invalid(format!(
                "{error}; these files were replaced and could not be put back: {}",
                unrestored.join(", ")
            )));
        }
    }
    Ok(())
}

/// One file's new text, staged beside its target, and what the target held before.
struct Staged {
    /// The temporary holding the new text.
    temporary: PathBuf,
    /// The target's contents before the write, or `None` when it did not exist.
    original: Option<Vec<u8>>,
}

/// Remove the temporaries not yet renamed.
fn discard(staged: &[Staged]) {
    for held in staged {
        let _ = std::fs::remove_file(&held.temporary);
    }
}

/// Put each replaced target back as it was, and name the ones that could not be.
///
/// A target that did not exist is removed; one that did is rewritten in place, which keeps the
/// permissions its replacement copied from it.
fn restore(replaced: &[Staged], targets: &[PathBuf]) -> Vec<String> {
    let mut unrestored = Vec::new();
    for (held, target) in replaced.iter().zip(targets) {
        let undone = match &held.original {
            Some(original) => std::fs::write(target, original),
            None => std::fs::remove_file(target),
        };
        if undone.is_err() {
            unrestored.push(target.display().to_string());
        }
    }
    unrestored
}

/// The canonical path `file` is written at, once it is known to be inside its chart, with the
/// directories it needs created.
fn resolve(file: &Planned) -> Result<PathBuf, Error> {
    let target = inside(&file.chart, &file.path)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.display(), e))?;
    }
    Ok(target)
}

/// The canonical path `path` names, refused when it resolves outside `chart`.
///
/// The check runs on the deepest ancestor that already exists and creates nothing, so a link
/// pointing out of the chart cannot have a directory made through it either. A link at `path`
/// itself that points nowhere yet is not followed: the writers here replace the link by a rename
/// rather than write through it.
///
/// # Errors
/// [`Error::Io`] when `chart` or an existing ancestor cannot be resolved, and [`Error::Invalid`]
/// when `path` resolves outside `chart` or has no existing ancestor.
pub fn inside(chart: &Path, path: &Path) -> Result<PathBuf, Error> {
    let root = chart
        .canonicalize()
        .map_err(|e| Error::io(chart.display(), e))?;

    let mut existing = path;
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    while !existing.exists() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return Err(Error::Invalid(format!(
                "{}: has no existing ancestor to resolve",
                path.display()
            )));
        };
        missing.push(name);
        existing = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
    }
    let anchor = existing
        .canonicalize()
        .map_err(|e| Error::io(existing.display(), e))?;
    if !anchor.starts_with(&root) {
        return Err(Error::Invalid(format!(
            "{}: resolves to {}, outside the chart {}; nothing was written",
            path.display(),
            anchor.display(),
            root.display()
        )));
    }

    let mut target = anchor;
    for name in missing.iter().rev() {
        target.push(name);
    }
    Ok(target)
}

/// Write one file's new text to a temporary beside it, with the target's permissions if it has
/// any, and keep what the target holds now.
///
/// The temporary is created exclusively: a file or a symbolic link already at its path is refused
/// rather than written through.
fn stage(target: &Path, text: &str, index: usize) -> Result<Staged, Error> {
    let original = match std::fs::read(target) {
        Ok(held) => Some(held),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Error::io(target.display(), error)),
    };
    let name = target
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let temporary = target.with_file_name(format!(
        ".{name}.terrace-{}-{index}.tmp",
        std::process::id()
    ));
    let mut created = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
    {
        Ok(created) => created,
        Err(error) => return Err(Error::io(temporary.display(), error)),
    };
    let written = created
        .write_all(text.as_bytes())
        .and_then(|()| created.sync_all())
        .and_then(|()| match std::fs::metadata(target) {
            Ok(metadata) => created.set_permissions(metadata.permissions()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        });
    drop(created);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::io(temporary.display(), error));
    }
    Ok(Staged {
        temporary,
        original,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(tag: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("terrace-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("chart")).expect("a chart directory");
        Scratch(root)
    }

    fn planned(chart: &Path, relative: &str, text: &str) -> Planned {
        Planned {
            chart: chart.to_path_buf(),
            path: chart.join(relative),
            text: text.to_owned(),
        }
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(leftovers(&path));
            } else if path.to_string_lossy().ends_with(".tmp") {
                found.push(path.display().to_string());
            }
        }
        found
    }

    #[test]
    fn replaces_existing_files_and_creates_new_ones_with_their_directories() {
        let root = scratch("create");
        let chart = root.0.join("chart");
        std::fs::write(chart.join("values.yaml"), "old: 1\n").expect("a file");

        write_all(&[
            planned(&chart, "values.yaml", "new: 2\n"),
            planned(&chart, "templates/tests/case.yaml", "case\n"),
        ])
        .expect("it writes");

        assert_eq!(
            std::fs::read_to_string(chart.join("values.yaml")).expect("readable"),
            "new: 2\n"
        );
        assert_eq!(
            std::fs::read_to_string(chart.join("templates/tests/case.yaml")).expect("readable"),
            "case\n"
        );
        assert_eq!(leftovers(&root.0), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_outside_its_chart_fails_the_whole_write_before_anything_lands() {
        let root = scratch("escape");
        let chart = root.0.join("chart");
        let outside = root.0.join("elsewhere");
        std::fs::create_dir_all(&outside).expect("a directory");
        std::fs::write(chart.join("values.yaml"), "old: 1\n").expect("a file");
        std::os::unix::fs::symlink(&outside, chart.join("templates")).expect("a link");

        let error = write_all(&[
            planned(&chart, "values.yaml", "new: 2\n"),
            planned(&chart, "templates/sub/_helpers.tpl", "x\n"),
        ])
        .expect_err("the link escapes its chart");

        assert!(error.to_string().contains("outside the chart"), "{error}");
        assert_eq!(
            std::fs::read_to_string(chart.join("values.yaml")).expect("readable"),
            "old: 1\n"
        );
        assert!(
            !outside.join("sub").exists(),
            "no directory made through the link"
        );
        assert_eq!(leftovers(&root.0), Vec::<String>::new());
    }

    #[test]
    fn a_failed_rename_puts_back_the_files_already_replaced() {
        let root = scratch("restore");
        let chart = root.0.join("chart");
        let existing = chart.join("values.yaml");
        let created = chart.join("new.yaml");
        std::fs::write(&existing, "old: 1\n").expect("a file");

        let staged = [
            stage(&existing, "new: 2\n", 0).expect("staged"),
            stage(&created, "made\n", 1).expect("staged"),
        ];
        let targets = [existing.clone(), created.clone()];
        for (held, target) in staged.iter().zip(&targets) {
            std::fs::rename(&held.temporary, target).expect("renamed");
        }

        assert_eq!(restore(&staged, &targets), Vec::<String>::new());
        assert_eq!(
            std::fs::read_to_string(&existing).expect("readable"),
            "old: 1\n"
        );
        assert!(
            !created.exists(),
            "a file the write created is removed again"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_planted_at_the_temporary_path_is_refused_rather_than_written_through() {
        let root = scratch("planted");
        let chart = root.0.join("chart");
        let victim = root.0.join("victim");
        std::fs::write(&victim, "untouched\n").expect("a file");
        std::os::unix::fs::symlink(
            &victim,
            chart.join(format!(".values.yaml.terrace-{}-0.tmp", std::process::id())),
        )
        .expect("a link");

        write_all(&[planned(&chart, "values.yaml", "new: 2\n")])
            .expect_err("the temporary path is taken");

        assert_eq!(
            std::fs::read_to_string(&victim).expect("readable"),
            "untouched\n"
        );
        assert!(!chart.join("values.yaml").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("mode");
        let chart = root.0.join("chart");
        let path = chart.join("run.sh");
        std::fs::write(&path, "old\n").expect("a file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).expect("a mode");

        write_all(&[planned(&chart, "run.sh", "new\n")]).expect("it writes");

        let mode = std::fs::metadata(&path)
            .expect("present")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750);
    }
}
