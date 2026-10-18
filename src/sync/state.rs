use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
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
pub const CURRENT_STATE_VERSION: u32 = 1;

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
    /// Returns default empty state if the file does not exist or is invalid.
    pub fn load_from_dir(project_dir: &Path) -> Result<Self, String> {
        let state_path = project_dir.join(".ude/state.json");
        if state_path.exists()
            && let Ok(content) = fs::read_to_string(&state_path)
            && let Ok(state) = serde_json::from_str::<ProjectState>(&content)
        {
            return Ok(state);
        }
        Ok(Self::default())
    }

    /// Saves the project state to `<project_dir>/.ude/state.json`.
    pub fn save_to_dir(&self, project_dir: &Path) -> Result<(), String> {
        let ude_dir = project_dir.join(".ude");
        fs::create_dir_all(&ude_dir)
            .map_err(|e| format!("Failed to create .ude directory: {e}"))?;
        let state_path = ude_dir.join("state.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize state: {e}"))?;
        fs::write(&state_path, json)
            .map_err(|e| format!("Failed to write state file {}: {e}", state_path.display()))?;
        Ok(())
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
}
