//! Stage-then-commit transaction for multi-file writes (SYNC-02, D-05).
//!
//! Every file is first written to a temp file in its target's own directory and
//! validated. Only after all staging succeeded are the temps renamed over their
//! targets. No fsync and no on-disk journal (D-01): a failure during the rename phase
//! can leave a partial update, and the error says how many files were replaced.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Mode for a new hook or script file.
pub const EXECUTABLE_MODE: u32 = 0o755;

/// Mode for a new file that is not a hook or script (fixed, not umask-derived).
pub const REGULAR_MODE: u32 = 0o644;

/// Owner execute bit: an existing hook or script always keeps at least this.
const OWNER_EXEC: u32 = 0o100;

/// Read bits of the owner, group and other classes.
const READ_BITS: u32 = 0o444;

/// Shift that turns a class's read bit into the same class's execute bit.
const READ_TO_EXEC_SHIFT: u32 = 2;

/// Permission and special bits taken over from an existing target.
#[cfg(unix)]
const MODE_BITS: u32 = 0o7777;

/// Whether `rel` is a hook or script path that must stay executable.
fn is_executable_path(rel: &Path) -> bool {
    rel.starts_with("scripts/hooks")
        || rel.starts_with("scratch")
        || rel == Path::new("scripts/check-in-container.sh")
}

/// Mode bits of `target` when it exists as a regular file (a symlink is never followed).
fn existing_file_mode(target: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(target)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.permissions().mode() & MODE_BITS)
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        None
    }
}

/// Mode for staging `rel` over `target`.
///
/// An existing file keeps its mode, so a file the user restricted (0o600) stays
/// restricted. An existing hook or script additionally gets an execute bit for every
/// class that can read it plus the owner (0o644 becomes 0o755, 0o700 stays 0o700), so
/// it is never left non-executable and its access is never widened. A new file gets
/// [`EXECUTABLE_MODE`] for a hook or script path and [`REGULAR_MODE`] otherwise.
pub fn file_mode_for(rel: &Path, target: &Path) -> u32 {
    let executable = is_executable_path(rel);
    match existing_file_mode(target) {
        Some(existing) if executable => {
            existing | ((existing & READ_BITS) >> READ_TO_EXEC_SHIFT) | OWNER_EXEC
        }
        Some(existing) => existing,
        None if executable => EXECUTABLE_MODE,
        None => REGULAR_MODE,
    }
}

/// One staged file: its project-relative path, absolute target and temp file.
struct StagedWrite {
    rel: PathBuf,
    target: PathBuf,
    temp: tempfile::NamedTempFile,
}

/// A set of staged file writes under one root, applied together by [`Self::commit`].
///
/// Dropping the transaction without a successful commit deletes every staged temp
/// file and removes the directories staging created (when they are empty), so a
/// staging failure leaves the tree as it was.
pub struct StagedTransaction {
    root: PathBuf,
    writes: Vec<StagedWrite>,
    deletes: Vec<PathBuf>,
    created_dirs: Vec<PathBuf>,
}

impl StagedTransaction {
    /// Starts an empty transaction whose paths are relative to `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            writes: Vec::new(),
            deletes: Vec::new(),
            created_dirs: Vec::new(),
        }
    }

    /// Stages `bytes` for `rel` with permission bits `mode` (Unix only) without
    /// touching the target. Refuses unsafe paths, symlinked targets or parents, and
    /// targets that are directories. Missing parent directories are created and
    /// recorded so an abort can remove them.
    pub fn stage_write(&mut self, rel: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
        let target = super::validate_safe_write_target(&self.root, rel)?;
        if fs::symlink_metadata(&target).is_ok_and(|meta| meta.is_dir()) {
            return Err(format!("Target path is a directory: {}", target.display()));
        }
        let parent = target
            .parent()
            .ok_or_else(|| format!("Target path has no parent: {}", target.display()))?
            .to_path_buf();
        self.create_missing_dirs(&parent)?;
        let stage_err = |e: std::io::Error| format!("Failed to stage {}: {e}", rel.display());
        let mut temp = tempfile::NamedTempFile::new_in(&parent).map_err(stage_err)?;
        temp.write_all(bytes).map_err(stage_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(mode))
                .map_err(stage_err)?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        self.writes.push(StagedWrite {
            rel: rel.to_path_buf(),
            target,
            temp,
        });
        Ok(())
    }

    /// Stages the deletion of `rel` without touching it. Refuses unsafe paths, symlinked
    /// parents, and anything that is not a regular file (directories, symlinks, missing
    /// paths). [`Self::commit`] removes staged deletions after all staged writes.
    pub fn stage_delete(&mut self, rel: &Path) -> Result<(), String> {
        let target = super::validate_safe_write_target(&self.root, rel)?;
        let meta = fs::symlink_metadata(&target)
            .map_err(|e| format!("Failed to stage deletion of {}: {e}", rel.display()))?;
        if !meta.is_file() {
            return Err(format!(
                "Refusing to delete {}: not a regular file",
                target.display()
            ));
        }
        self.deletes.push(rel.to_path_buf());
        Ok(())
    }

    /// Creates `dir` and any missing ancestors one level at a time, recording each
    /// directory it created. A path that cannot be inspected is treated as missing;
    /// creating it then reports the real error (for example ENOTDIR under a file).
    fn create_missing_dirs(&mut self, dir: &Path) -> Result<(), String> {
        let mut missing = Vec::new();
        let mut current = Some(dir);
        while let Some(path) = current {
            if fs::symlink_metadata(path).is_ok() {
                break;
            }
            missing.push(path);
            current = path.parent();
        }
        for path in missing.into_iter().rev() {
            fs::create_dir(path)
                .map_err(|e| format!("Failed to create directory {}: {e}", path.display()))?;
            self.created_dirs.push(path.to_path_buf());
        }
        Ok(())
    }

    /// Renames every staged temp over its target, in staging order, re-validating each
    /// target immediately before its rename, then removes every staged deletion the
    /// same way (a deletion target that is already gone counts as removed). Returns the
    /// applied (written) relative paths.
    ///
    /// On an error the temps not yet renamed are deleted; files already renamed or
    /// removed stay so (D-01) and the error names how many writes there were.
    pub fn commit(mut self) -> Result<Vec<PathBuf>, String> {
        let writes = std::mem::take(&mut self.writes);
        let total = writes.len();
        let mut applied = Vec::with_capacity(total);
        for StagedWrite { rel, target, temp } in writes {
            let replaced = applied.len();
            super::validate_safe_write_target(&self.root, &rel).map_err(|reason| {
                format!(
                    "Refusing to replace {}: {reason} ({replaced} of {total} files were already replaced)",
                    rel.display()
                )
            })?;
            temp.persist(&target).map_err(|e| {
                format!(
                    "Failed to replace {}: {} ({replaced} of {total} files were already replaced)",
                    rel.display(),
                    e.error
                )
            })?;
            applied.push(rel);
        }
        for rel in std::mem::take(&mut self.deletes) {
            let target = super::validate_safe_write_target(&self.root, &rel).map_err(|reason| {
                format!(
                    "Refusing to delete {}: {reason} (all {total} writes were applied)",
                    rel.display()
                )
            })?;
            match fs::remove_file(&target) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(format!(
                        "Failed to delete {}: {e} (all {total} writes were applied)",
                        rel.display()
                    ));
                }
            }
        }
        self.created_dirs.clear();
        Ok(applied)
    }
}

impl Drop for StagedTransaction {
    /// Deletes staged temps first, then removes the directories this transaction
    /// created, deepest first. A directory that still holds a file stays.
    fn drop(&mut self) {
        self.writes.clear();
        for dir in self.created_dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}
