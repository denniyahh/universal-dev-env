use super::SyncEngine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Record of a file's state at the last successful synchronization or initialization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStateRecord {
    pub ancestor_hash: String,
    pub last_synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ancestor_content: Option<String>,
}

/// Project synchronization state persisted in `.ude/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectState {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_type: Option<String>,
    pub files: BTreeMap<String, FileStateRecord>,
}

/// Newest `.ude/state.json` schema version this build reads and writes.
///
/// Version 2 records ancestors as [`SyncEngine::fingerprint`] plus LF-normalized
/// content. Version 1 files are migrated in memory on load (D-07).
pub const CURRENT_STATE_VERSION: u32 = 2;

/// Schema whose ancestor hashes are raw-byte SHA-256 ([`SyncEngine::compute_hash`])
/// and whose ancestor content is stored verbatim.
const RAW_HASH_STATE_VERSION: u32 = 1;

/// Location of the state ledger relative to the project root.
pub(crate) const STATE_FILE_REL: &str = ".ude/state.json";

/// Mode for a newly created `state.json` (an existing file keeps its own mode).
#[cfg(unix)]
const STATE_FILE_MODE: u32 = 0o644;

/// A serialized state written to a temp file in `.ude/`, not yet visible as `state.json`.
///
/// [`StagedState::commit`] renames it over `state.json`; dropping it without a commit
/// deletes the temp file and leaves the existing `state.json` untouched.
pub struct StagedState {
    temp: tempfile::NamedTempFile,
    project_dir: PathBuf,
    target: PathBuf,
}

impl StagedState {
    /// Re-validates `.ude/state.json` against symlinks and escapes, then atomically
    /// replaces it with the staged content.
    pub fn commit(self) -> Result<(), String> {
        super::validate_safe_write_target(&self.project_dir, Path::new(STATE_FILE_REL))?;
        self.temp.persist(&self.target).map_err(|e| {
            format!(
                "Failed to replace state file {}: {}",
                self.target.display(),
                e.error
            )
        })?;
        Ok(())
    }
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION,
            project_type: None,
            files: BTreeMap::new(),
        }
    }
}

impl ProjectState {
    /// Loads the project state from `<project_dir>/.ude/state.json`.
    ///
    /// A missing file yields the default empty state (first sync of a fresh project).
    /// An unreadable or corrupt file, version 0, or a version newer than
    /// [`CURRENT_STATE_VERSION`] is an error: the file is never reset or rewritten here.
    /// A version 1 file is migrated in memory; the next save persists the result.
    pub fn load_from_dir(project_dir: &Path) -> Result<Self, String> {
        let state_path = project_dir.join(STATE_FILE_REL);
        let content = match fs::read_to_string(&state_path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(format!(
                    "Failed to read state file {}: {e}",
                    state_path.display()
                ));
            }
        };
        let state = serde_json::from_str::<ProjectState>(&content).map_err(|e| {
            format!(
                "State file {} is corrupt ({e}); refusing to reset it. Fix or remove the file, then re-run.",
                state_path.display()
            )
        })?;
        match state.version {
            0 => Err(format!(
                "State file {} has unsupported version 0",
                state_path.display()
            )),
            v if v > CURRENT_STATE_VERSION => Err(format!(
                "State file {} has version {v}, written by a newer ude (this build supports up to {CURRENT_STATE_VERSION}); upgrade ude",
                state_path.display()
            )),
            RAW_HASH_STATE_VERSION => Ok(state.migrated_from_raw_hashes(project_dir)),
            _ => Ok(state),
        }
    }

    /// Converts a version 1 state to fingerprint semantics (D-07).
    ///
    /// A record with content is re-hashed from it and its content LF-normalized. A
    /// record without content adopts the fingerprint of the local file only when the
    /// file's raw hash equals the recorded one, so nothing genuinely modified reads as
    /// in sync. Keys that are not safe project-relative paths are never read.
    fn migrated_from_raw_hashes(mut self, project_dir: &Path) -> Self {
        for (key, record) in &mut self.files {
            match record.ancestor_content.take() {
                Some(text) => {
                    record.ancestor_hash = SyncEngine::fingerprint(text.as_bytes());
                    record.ancestor_content = Some(SyncEngine::normalize_eol(&text));
                }
                None => {
                    if let Ok(path) = super::validate_safe_write_target(project_dir, Path::new(key))
                        && let Ok(bytes) = fs::read(path)
                        && SyncEngine::compute_hash(&bytes) == record.ancestor_hash
                    {
                        record.ancestor_hash = SyncEngine::fingerprint(&bytes);
                    }
                }
            }
        }
        self.version = CURRENT_STATE_VERSION;
        self
    }

    /// Atomically saves the project state to `<project_dir>/.ude/state.json`
    /// ([`Self::stage_to_dir`] followed by [`StagedState::commit`]).
    pub fn save_to_dir(&self, project_dir: &Path) -> Result<(), String> {
        self.stage_to_dir(project_dir)?.commit()
    }

    /// Writes the serialized state to a temp file in `<project_dir>/.ude/` without
    /// touching `state.json`. Refuses a symlinked or escaping `.ude` / `state.json`
    /// before creating anything. The temp file gets the existing `state.json` mode,
    /// or [`STATE_FILE_MODE`] when there is none.
    pub fn stage_to_dir(&self, project_dir: &Path) -> Result<StagedState, String> {
        let target = super::validate_safe_write_target(project_dir, Path::new(STATE_FILE_REL))?;
        let ude_dir = project_dir.join(".ude");
        fs::create_dir_all(&ude_dir)
            .map_err(|e| format!("Failed to create .ude directory: {e}"))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize state: {e}"))?;
        let write_err =
            |e: std::io::Error| format!("Failed to stage state file {}: {e}", target.display());
        let mut temp = tempfile::NamedTempFile::new_in(&ude_dir).map_err(write_err)?;
        temp.write_all(json.as_bytes()).map_err(write_err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = match fs::symlink_metadata(&target) {
                Ok(meta) => meta.permissions().mode() & 0o7777,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => STATE_FILE_MODE,
                Err(e) => return Err(write_err(e)),
            };
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(mode))
                .map_err(write_err)?;
        }
        Ok(StagedState {
            temp,
            project_dir: project_dir.to_path_buf(),
            target,
        })
    }

    /// Retrieves the ancestor hash for a given relative path.
    pub fn get_ancestor_hash(&self, rel_path: &Path) -> Option<&str> {
        let key = rel_path.to_string_lossy().to_string();
        self.files.get(&key).map(|r| r.ancestor_hash.as_str())
    }

    /// Retrieves the ancestor content for a given relative path if preserved.
    pub fn get_ancestor_content(&self, rel_path: &Path) -> Option<&str> {
        let key = rel_path.to_string_lossy().to_string();
        self.files
            .get(&key)
            .and_then(|r| r.ancestor_content.as_deref())
    }

    /// Records or updates the ancestor state of a managed file.
    pub fn record_file(
        &mut self,
        rel_path: &Path,
        ancestor_hash: String,
        ancestor_content: Option<String>,
    ) {
        let key = rel_path.to_string_lossy().to_string();
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .ok();

        self.files.insert(
            key,
            FileStateRecord {
                ancestor_hash,
                last_synced_at: timestamp,
                ancestor_content,
            },
        );
    }

    /// Removes a file record from tracked state.
    pub fn remove_file(&mut self, rel_path: &Path) {
        let key = rel_path.to_string_lossy().to_string();
        self.files.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_project_state_persistence_roundtrip() {
        let dir = tempdir().unwrap();
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            "abc123hash".to_string(),
            Some("[core]\n".to_string()),
        );
        state.record_file(Path::new("mise.toml"), "def456hash".to_string(), None);

        state.save_to_dir(dir.path()).unwrap();

        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        assert_eq!(loaded.version, CURRENT_STATE_VERSION);
        assert_eq!(
            loaded.get_ancestor_hash(Path::new(".gitconfig")),
            Some("abc123hash")
        );
        assert_eq!(
            loaded.get_ancestor_content(Path::new(".gitconfig")),
            Some("[core]\n")
        );
        assert_eq!(
            loaded.get_ancestor_hash(Path::new("mise.toml")),
            Some("def456hash")
        );
        assert_eq!(loaded.get_ancestor_content(Path::new("mise.toml")), None);
        assert_eq!(loaded.get_ancestor_hash(Path::new("nonexistent")), None);
    }

    #[test]
    fn test_project_state_load_nonexistent_returns_default() {
        let dir = tempdir().unwrap();
        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        assert_eq!(loaded.version, CURRENT_STATE_VERSION);
        assert!(loaded.files.is_empty());
    }

    /// Writes `content` as `<dir>/.ude/state.json` and returns its path.
    fn write_state_file(dir: &Path, content: &[u8]) -> std::path::PathBuf {
        let ude_dir = dir.join(".ude");
        fs::create_dir_all(&ude_dir).unwrap();
        let path = ude_dir.join("state.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_load_corrupt_state_errors() {
        let dir = tempdir().unwrap();
        let garbage = b"{not json";
        let path = write_state_file(dir.path(), garbage);

        let err = ProjectState::load_from_dir(dir.path())
            .expect_err("a corrupt state.json must not load as empty state");
        assert!(err.contains("corrupt"), "error was: {err}");
        assert!(err.contains("state.json"), "error was: {err}");
        assert_eq!(fs::read(&path).unwrap(), garbage, "state.json was modified");
    }

    #[test]
    fn test_load_unreadable_state_errors() {
        let dir = tempdir().unwrap();
        // A directory in place of the file: the read fails with something other than NotFound.
        fs::create_dir_all(dir.path().join(".ude/state.json")).unwrap();

        let err = ProjectState::load_from_dir(dir.path())
            .expect_err("an unreadable state.json must not load as empty state");
        assert!(
            err.contains("Failed to read state file"),
            "error was: {err}"
        );
    }

    #[test]
    fn test_load_future_version_errors() {
        let dir = tempdir().unwrap();
        for version in [CURRENT_STATE_VERSION + 1, 99] {
            let content = format!(r#"{{"version": {version}, "files": {{}}}}"#);
            let path = write_state_file(dir.path(), content.as_bytes());
            let err = ProjectState::load_from_dir(dir.path())
                .expect_err("a state.json from a newer ude must be refused");
            assert!(err.contains("newer"), "version {version}: {err}");
            assert!(
                err.contains(&version.to_string()),
                "version {version}: {err}"
            );
            assert_eq!(fs::read(&path).unwrap(), content.as_bytes());
        }

        write_state_file(dir.path(), br#"{"version": 0, "files": {}}"#);
        let err = ProjectState::load_from_dir(dir.path()).expect_err("version 0 must be refused");
        assert!(err.contains("unsupported"), "version 0: {err}");

        // Negative control: the current version loads.
        let current = format!(r#"{{"version": {CURRENT_STATE_VERSION}, "files": {{}}}}"#);
        write_state_file(dir.path(), current.as_bytes());
        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        assert_eq!(loaded.version, CURRENT_STATE_VERSION);
    }

    /// Writes a version 1 state.json holding `(key, ancestor_hash, ancestor_content)` records.
    fn write_v1_state(dir: &Path, records: &[(&str, &str, Option<&str>)]) -> PathBuf {
        let mut files = serde_json::Map::new();
        for (key, hash, content) in records {
            let mut record = serde_json::json!({ "ancestor_hash": hash, "last_synced_at": null });
            if let Some(content) = content {
                record["ancestor_content"] = (*content).into();
            }
            files.insert((*key).to_string(), record);
        }
        let state = serde_json::json!({ "version": 1, "files": files });
        write_state_file(dir, state.to_string().as_bytes())
    }

    #[test]
    fn test_migrate_v1_content_record() {
        let dir = tempdir().unwrap();
        let crlf = "a\r\nb\r\n";
        let old_hash = SyncEngine::compute_hash(crlf.as_bytes());
        let path = write_v1_state(dir.path(), &[(".gitconfig", &old_hash, Some(crlf))]);
        let before = fs::read(&path).unwrap();

        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        let rel = Path::new(".gitconfig");
        assert_eq!(
            loaded.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(b"a\nb\n").as_str()),
            "a content record is re-hashed from its stored content"
        );
        assert_eq!(loaded.get_ancestor_content(rel), Some("a\nb\n"));
        assert_eq!(loaded.version, 2);
        assert_eq!(
            fs::read(&path).unwrap(),
            before,
            "a load rewrote state.json"
        );
    }

    #[test]
    fn test_migrate_v1_hash_only_record() {
        let dir = tempdir().unwrap();
        let local = b"a\r\nb\r\n";
        fs::write(dir.path().join(".gitconfig"), local).unwrap();
        let old_hash = SyncEngine::compute_hash(local);
        assert_ne!(old_hash, SyncEngine::fingerprint(local), "precondition");
        write_v1_state(dir.path(), &[(".gitconfig", &old_hash, None)]);

        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        let rel = Path::new(".gitconfig");
        assert_eq!(
            loaded.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(local).as_str()),
            "a hash-only record matching the local file adopts its fingerprint"
        );
        assert_eq!(loaded.get_ancestor_content(rel), None);
        assert_eq!(loaded.version, 2);
    }

    #[test]
    fn test_migrate_v1_mismatch_unchanged() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitconfig"), b"a\r\nb\r\n").unwrap();
        let old_hash = SyncEngine::compute_hash(b"a\r\nedited\r\n");
        write_v1_state(dir.path(), &[(".gitconfig", &old_hash, None)]);

        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        assert_eq!(
            loaded.get_ancestor_hash(Path::new(".gitconfig")),
            Some(old_hash.as_str()),
            "a record that does not match the local file must keep its old hash"
        );
        assert_eq!(loaded.version, 2);
    }

    #[test]
    fn test_migrate_v1_unsafe_key_ignored() {
        let root = tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        // Outside files whose raw hash matches their records: adopting them would mean
        // the migration read outside the project.
        let outside = b"outside\r\n";
        fs::write(root.path().join("outside.txt"), outside).unwrap();
        let outside_hash = SyncEngine::compute_hash(outside);
        let absolute_key = root
            .path()
            .join("outside.txt")
            .to_string_lossy()
            .to_string();
        // Positive control in the same state: a safe key matching its local file migrates.
        let local = b"local\r\n";
        fs::write(project.join("mise.toml"), local).unwrap();
        let local_hash = SyncEngine::compute_hash(local);
        write_v1_state(
            &project,
            &[
                ("../outside.txt", &outside_hash, None),
                (&absolute_key, &outside_hash, None),
                ("mise.toml", &local_hash, None),
            ],
        );

        let loaded = ProjectState::load_from_dir(&project).unwrap();
        assert_eq!(loaded.version, 2);
        for key in ["../outside.txt", absolute_key.as_str()] {
            assert_eq!(
                loaded.get_ancestor_hash(Path::new(key)),
                Some(outside_hash.as_str()),
                "unsafe key {key} must be left unchanged"
            );
        }
        assert_eq!(
            loaded.get_ancestor_hash(Path::new("mise.toml")),
            Some(SyncEngine::fingerprint(local).as_str())
        );
    }

    /// A state with one recorded file whose hash is `hash`.
    fn state_with(hash: &str) -> ProjectState {
        let mut state = ProjectState::default();
        state.record_file(Path::new("mise.toml"), hash.to_string(), None);
        state
    }

    /// Names in `dir` that start with `.tmp` (NamedTempFile's prefix).
    fn tmp_entries(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".tmp"))
            .collect()
    }

    #[test]
    fn test_state_save_atomic() {
        let dir = tempdir().unwrap();
        state_with("old").save_to_dir(dir.path()).unwrap();
        let state_path = dir.path().join(".ude/state.json");
        let before = fs::read(&state_path).unwrap();

        // Staged but never committed (a crash before commit): the old file is untouched.
        let staged = state_with("new").stage_to_dir(dir.path()).unwrap();
        drop(staged);
        assert_eq!(
            fs::read(&state_path).unwrap(),
            before,
            "an uncommitted staged state replaced state.json"
        );
        assert!(
            tmp_entries(&dir.path().join(".ude")).is_empty(),
            "a dropped staged state left a temp file"
        );

        // Negative control: committing replaces the file with the new state.
        state_with("new")
            .stage_to_dir(dir.path())
            .unwrap()
            .commit()
            .unwrap();
        let loaded = ProjectState::load_from_dir(dir.path()).unwrap();
        assert_eq!(
            loaded.get_ancestor_hash(Path::new("mise.toml")),
            Some("new")
        );
        assert!(tmp_entries(&dir.path().join(".ude")).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_state_save_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let state_path = dir.path().join(".ude/state.json");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;

        state_with("a").save_to_dir(dir.path()).unwrap();
        assert_ne!(
            mode(&state_path),
            0o600,
            "new state.json kept the tempfile mode"
        );
        assert_eq!(mode(&state_path), 0o644);

        fs::set_permissions(&state_path, fs::Permissions::from_mode(0o640)).unwrap();
        state_with("b").save_to_dir(dir.path()).unwrap();
        assert_eq!(
            mode(&state_path),
            0o640,
            "existing state.json mode not kept"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_state_save_refuses_symlinked_ude_dir() {
        use std::os::unix::fs::symlink;

        // Case A, stage time: `.ude` is a symlink to a directory outside the project.
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), project.path().join(".ude")).unwrap();

        let err = state_with("a")
            .save_to_dir(project.path())
            .expect_err("save_to_dir wrote through a symlinked .ude");
        assert!(err.contains(".ude"), "error does not name the path: {err}");
        let err = state_with("a")
            .stage_to_dir(project.path())
            .err()
            .expect("stage_to_dir accepted a symlinked .ude");
        assert!(err.contains(".ude"), "error does not name the path: {err}");
        assert!(!outside.path().join("state.json").exists());
        assert!(tmp_entries(outside.path()).is_empty());

        // Case B, commit time: state.json becomes a symlink after staging.
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        state_with("old").save_to_dir(project.path()).unwrap();
        let victim = outside.path().join("victim.json");
        fs::write(&victim, b"outside").unwrap();
        let staged = state_with("new").stage_to_dir(project.path()).unwrap();
        let state_path = project.path().join(".ude/state.json");
        fs::remove_file(&state_path).unwrap();
        symlink(&victim, &state_path).unwrap();

        let err = staged
            .commit()
            .expect_err("commit replaced a symlinked state.json");
        assert!(
            err.contains("state.json"),
            "error does not name the symlink: {err}"
        );
        assert!(
            fs::symlink_metadata(&state_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&victim).unwrap(), b"outside");

        // Negative control: a regular `.ude` saves and holds the new state.
        let project = tempdir().unwrap();
        state_with("new").save_to_dir(project.path()).unwrap();
        let loaded = ProjectState::load_from_dir(project.path()).unwrap();
        assert_eq!(
            loaded.get_ancestor_hash(Path::new("mise.toml")),
            Some("new")
        );
    }
}
