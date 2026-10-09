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
}

/// Project synchronization state persisted in `.ude/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectState {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_type: Option<String>,
    pub files: BTreeMap<String, FileStateRecord>,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            version: 1,
            project_type: None,
            files: BTreeMap::new(),
        }
    }
}

impl ProjectState {
    /// Loads the project state from `<project_dir>/.ude/state.json`.
    /// Returns default empty state if the file does not exist or is invalid.
    pub fn load_from_dir(project_dir: &Path) -> Self {
        let state_path = project_dir.join(".ude/state.json");
        if state_path.exists()
            && let Ok(content) = fs::read_to_string(&state_path)
            && let Ok(state) = serde_json::from_str::<ProjectState>(&content)
        {
            return state;
        }
        Self::default()
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

    /// Records or updates the ancestor state of a managed file.
    pub fn record_file(&mut self, rel_path: &Path, ancestor_hash: String) {
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
        state.record_file(Path::new(".gitconfig"), "abc123hash".to_string());
        state.record_file(Path::new("mise.toml"), "def456hash".to_string());

        state.save_to_dir(dir.path()).unwrap();

        let loaded = ProjectState::load_from_dir(dir.path());
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.get_ancestor_hash(Path::new(".gitconfig")), Some("abc123hash"));
        assert_eq!(loaded.get_ancestor_hash(Path::new("mise.toml")), Some("def456hash"));
        assert_eq!(loaded.get_ancestor_hash(Path::new("nonexistent")), None);
    }

    #[test]
    fn test_project_state_load_nonexistent_returns_default() {
        let dir = tempdir().unwrap();
        let loaded = ProjectState::load_from_dir(dir.path());
        assert_eq!(loaded.version, 1);
        assert!(loaded.files.is_empty());
    }
}
