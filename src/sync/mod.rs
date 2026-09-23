use crate::sanitizer::{SanitizationResult, Sanitizer};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SyncStatus {
    InSync,
    LocallyModified,
    UpstreamModified,
    Conflicted,
    Overridden,
}

#[derive(Debug, Clone)]
pub struct FileSyncState {
    pub relative_path: PathBuf,
    pub status: SyncStatus,
    pub local_hash: Option<String>,
    pub upstream_hash: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushResult {
    Success { synced_files: usize },
    SecretBlocked { path: PathBuf, reason: String },
    ConflictBlocked { path: PathBuf },
    BoundaryBlocked { path: PathBuf },
}

#[derive(Default)]
pub struct SyncEngine {
    sanitizer: Sanitizer,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            sanitizer: Sanitizer::new(),
        }
    }

    /// Computes the sha256 checksum of a byte slice
    pub fn compute_hash(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    /// Detects sync status between a local file and upstream content
    pub fn compare(
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> SyncStatus {
        if has_local_override {
            return SyncStatus::Overridden;
        }

        match (local_content, upstream_content) {
            (Some(local), Some(upstream)) => {
                if local == upstream {
                    SyncStatus::InSync
                } else {
                    SyncStatus::LocallyModified
                }
            }
            (Some(_), None) => SyncStatus::LocallyModified,
            (None, Some(_)) => SyncStatus::UpstreamModified,
            (None, None) => SyncStatus::InSync,
        }
    }

    /// Compares a local directory against an upstream template directory
    pub fn scan_project(&self, local_root: &Path, upstream_root: &Path) -> Vec<FileSyncState> {
        let mut states = Vec::new();

        // Inspect known managed files
        let managed_files = vec![
            PathBuf::from(".gitconfig"),
            PathBuf::from(".gitignore"),
            PathBuf::from("scripts/hooks/commit-msg"),
            PathBuf::from("scripts/hooks/pre-commit"),
            PathBuf::from("scripts/hooks/pre-push"),
            PathBuf::from("scripts/check-in-container.sh"),
            PathBuf::from("mise.toml"),
            PathBuf::from(".agents/AGENTS.md"),
            PathBuf::from("scratch/sync-personal-env.sh"),
            PathBuf::from("scratch/new-worktree.sh"),
            PathBuf::from("scratch/check-publish-boundary.sh"),
        ];

        for rel in managed_files {
            let local_path = local_root.join(&rel);
            let upstream_path = map_to_upstream(upstream_root, &rel);

            let local_content = fs::read_to_string(&local_path).ok();
            let upstream_content = upstream_path
                .as_ref()
                .and_then(|p| fs::read_to_string(p).ok());

            let has_local_override = check_for_override(local_root, &rel);

            let status = Self::compare(
                local_content.as_deref(),
                upstream_content.as_deref(),
                has_local_override,
            );

            let local_hash = local_content.as_ref().map(|c| Self::compute_hash(c.as_bytes()));
            let upstream_hash =
                upstream_content.as_ref().map(|c| Self::compute_hash(c.as_bytes()));

            states.push(FileSyncState {
                relative_path: rel,
                status,
                local_hash,
                upstream_hash,
            });
        }

        states
    }

    /// Pre-validates and pushes locally modified files to upstream
    pub fn push_updates(
        &self,
        local_root: &Path,
        upstream_root: &Path,
        user_home: &str,
        xdg_config: &str,
    ) -> PushResult {
        let states = self.scan_project(local_root, upstream_root);
        let mut synced = 0;

        for state in states {
            // Overridden files are never pushed upstream (Option B)
            if state.status == SyncStatus::Overridden {
                continue;
            }

            if state.status == SyncStatus::LocallyModified {
                let local_path = local_root.join(&state.relative_path);

                // Gate 1: Check publish boundary
                if self.sanitizer.is_local_only_path(&state.relative_path) {
                    return PushResult::BoundaryBlocked {
                        path: state.relative_path,
                    };
                }

                let content = match fs::read_to_string(&local_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                // Gate 2: Secret check (reject credentials)
                match self.sanitizer.check_secrets(&content) {
                    SanitizationResult::SecretDetected { reason, .. } => {
                        return PushResult::SecretBlocked {
                            path: state.relative_path,
                            reason,
                        };
                    }
                    SanitizationResult::Clean(_) => {}
                }

                // Gate 3: Path parameterization
                let parameterized = self
                    .sanitizer
                    .parameterize_paths(&content, user_home, xdg_config);

                // Materialize into upstream template
                if let Some(target_upstream) = map_to_upstream(upstream_root, &state.relative_path)
                {
                    if let Some(parent) = target_upstream.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::write(target_upstream, parameterized);
                    synced += 1;
                }
            }
        }

        PushResult::Success {
            synced_files: synced,
        }
    }
}

fn map_to_upstream(upstream_root: &Path, rel: &Path) -> Option<PathBuf> {
    let rel_str = rel.to_string_lossy();
    if rel_str == ".gitconfig" {
        Some(upstream_root.join("projects/generic/gitconfig.repo"))
    } else if rel_str == ".gitignore" {
        Some(upstream_root.join("projects/generic/gitignore.repo"))
    } else if rel_str.starts_with("scripts/hooks/") {
        let hook_name = rel_str.trim_start_matches("scripts/hooks/");
        Some(upstream_root.join(format!("projects/generic/hooks/{hook_name}")))
    } else if rel_str == "scripts/check-in-container.sh" {
        Some(upstream_root.join("projects/generic/container/check-in-container.sh"))
    } else if rel_str.starts_with("scratch/") {
        let scratch_name = rel_str.trim_start_matches("scratch/");
        Some(upstream_root.join(format!("projects/agents/scratch/{scratch_name}")))
    } else if rel_str == "mise.toml" {
        Some(upstream_root.join("projects/agents/mise.toml.jinja"))
    } else if rel_str == ".agents/AGENTS.md" {
        Some(upstream_root.join("projects/agents/agents.md.jinja"))
    } else {
        None
    }
}

fn check_for_override(local_root: &Path, rel: &Path) -> bool {
    // Check .local extension (e.g. mise.local.toml, AGENTS.local.md)
    let file_name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));

    let local_variant = if let Some(ext) = rel.extension().and_then(|e| e.to_str()) {
        let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        local_root.join(parent).join(format!("{stem}.local.{ext}"))
    } else {
        local_root.join(parent).join(format!("{file_name}.local"))
    };

    if local_variant.exists() {
        return true;
    }

    // Check .ude/overrides/ folder
    let override_dir_variant = local_root.join(".ude/overrides").join(rel);
    override_dir_variant.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sync_status_in_sync() {
        let content = "same content";
        assert_eq!(
            SyncEngine::compare(Some(content), Some(content), false),
            SyncStatus::InSync
        );
    }

    #[test]
    fn test_sync_status_locally_modified() {
        assert_eq!(
            SyncEngine::compare(Some("local change"), Some("upstream original"), false),
            SyncStatus::LocallyModified
        );
    }

    #[test]
    fn test_sync_status_overridden() {
        assert_eq!(
            SyncEngine::compare(Some("local change"), Some("upstream original"), true),
            SyncStatus::Overridden
        );
    }

    #[test]
    fn test_push_updates_clean_with_parameterization() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        let local_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(
            &local_gitconfig,
            "# Local config with user path /home/denniyahh\n[core]\n",
        )
        .unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
        );

        assert_eq!(res, PushResult::Success { synced_files: 1 });

        let written_upstream = upstream_dir
            .path()
            .join("projects/generic/gitconfig.repo");
        let content = fs::read_to_string(written_upstream).unwrap();
        assert!(content.contains("{{ user_home }}"));
        assert!(!content.contains("/home/denniyahh"));
    }

    #[test]
    fn test_push_updates_blocked_on_secret() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        let local_gitconfig = local_dir.path().join(".gitconfig");
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        fs::write(
            &local_gitconfig,
            format!("export TOKEN={prefix}{body}\n"),
        )
        .unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
        );

        match res {
            PushResult::SecretBlocked { path, .. } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
            }
            _ => panic!("Expected SecretBlocked, got {:?}", res),
        }
    }

    #[test]
    fn test_push_updates_blocked_on_publish_boundary() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        let scratch_script = local_dir.path().join("scratch/sync-personal-env.sh");
        fs::create_dir_all(scratch_script.parent().unwrap()).unwrap();
        fs::write(&scratch_script, "# Private scratch script").unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
        );

        match res {
            PushResult::BoundaryBlocked { path } => {
                assert_eq!(path, PathBuf::from("scratch/sync-personal-env.sh"));
            }
            _ => panic!("Expected BoundaryBlocked, got {:?}", res),
        }
    }

    #[test]
    fn test_push_updates_skipped_on_local_override() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Base file
        let local_mise = local_dir.path().join("mise.toml");
        fs::write(&local_mise, "[tools]\nrust = '1.98'\n").unwrap();

        // Local override file
        let override_mise = local_dir.path().join("mise.local.toml");
        fs::write(&override_mise, "[tools]\nrust = 'nightly'\n").unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
        );

        assert_eq!(res, PushResult::Success { synced_files: 0 });
    }
}
