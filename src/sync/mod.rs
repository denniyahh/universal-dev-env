use crate::sanitizer::{SanitizationResult, Sanitizer};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    IoFailed { path: PathBuf, reason: String },
    GitRemoteFailed { reason: String },
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

    /// Ensures a remote git repository is cached locally and updated
    pub fn ensure_remote_repo(
        &self,
        remote_url: &str,
        branch: &str,
        cache_dir: &Path,
    ) -> Result<PathBuf, String> {
        fs::create_dir_all(cache_dir)
            .map_err(|e| format!("Failed to create cache dir {}: {e}", cache_dir.display()))?;

        // Hash remote URL to isolate checkouts
        let repo_hash = &Self::compute_hash(remote_url.as_bytes())[..12];
        let target_checkout = cache_dir.join(format!("remote-{repo_hash}"));

        if !target_checkout.join(".git").exists() {
            println!("  Cloning remote repository into cache: {}...", remote_url);
            let status = Command::new("git")
                .args(["clone", "--depth", "1", "-b", branch, remote_url])
                .arg(&target_checkout)
                .status()
                .map_err(|e| format!("Failed to execute git clone: {e}"))?;

            if !status.success() {
                return Err(format!("git clone failed with exit status {:?}", status.code()));
            }
        } else {
            // Fetch latest branch
            let status = Command::new("git")
                .current_dir(&target_checkout)
                .args(["fetch", "origin", branch])
                .status()
                .map_err(|e| format!("Failed to execute git fetch: {e}"))?;

            if !status.success() {
                return Err(format!("git fetch failed with exit status {:?}", status.code()));
            }

            let status = Command::new("git")
                .current_dir(&target_checkout)
                .args(["reset", "--hard", &format!("origin/{branch}")])
                .status()
                .map_err(|e| format!("Failed to execute git reset: {e}"))?;

            if !status.success() {
                return Err(format!("git reset failed with exit status {:?}", status.code()));
            }
        }

        // Return the templates directory inside the remote checkout
        let templates_path = target_checkout.join("templates");
        if templates_path.exists() {
            Ok(templates_path)
        } else {
            Ok(target_checkout)
        }
    }

    /// Pre-validates and pushes locally modified files to upstream template directory.
    /// Operates atomically: verifies that ALL candidate files pass publish boundary,
    /// read integrity, and secret scanning before performing any upstream modifications.
    pub fn push_updates(
        &self,
        local_root: &Path,
        upstream_root: &Path,
        user_home: &str,
        xdg_config: &str,
        allow_agent_rules: bool,
    ) -> PushResult {
        let states = self.scan_project(local_root, upstream_root);
        let mut candidates_to_write = Vec::new();

        // Pass 1: Strict atomic pre-validation across ALL modified candidates
        for state in states {
            // Overridden files are never pushed upstream (Option B)
            if state.status == SyncStatus::Overridden {
                continue;
            }

            if state.status == SyncStatus::LocallyModified {
                let local_path = local_root.join(&state.relative_path);

                // Gate 1: Check publish boundary
                if self
                    .sanitizer
                    .is_local_only_path(&state.relative_path, allow_agent_rules)
                {
                    return PushResult::BoundaryBlocked {
                        path: state.relative_path,
                    };
                }

                let content = match fs::read_to_string(&local_path) {
                    Ok(c) => c,
                    Err(err) => {
                        return PushResult::IoFailed {
                            path: state.relative_path,
                            reason: format!("Failed to read local file: {err}"),
                        };
                    }
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

                // Stage into candidates list
                if let Some(target_upstream) = map_to_upstream(upstream_root, &state.relative_path) {
                    candidates_to_write.push((state.relative_path, target_upstream, parameterized));
                }
            }
        }

        // Pass 2: Transactional materialization of validated files
        let mut synced = 0;
        for (rel_path, target_upstream, parameterized) in candidates_to_write {
            if let Some(parent) = target_upstream.parent()
                && let Err(err) = fs::create_dir_all(parent)
            {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to create parent directory: {err}"),
                };
            }
            if let Err(err) = fs::write(&target_upstream, parameterized) {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to write upstream template: {err}"),
                };
            }
            synced += 1;
        }

        PushResult::Success {
            synced_files: synced,
        }
    }

    /// Pushes committed upstream updates to the remote git origin or creates a PR
    pub fn push_to_remote(
        &self,
        remote_checkout: &Path,
        branch: &str,
        open_pr: bool,
    ) -> Result<String, String> {
        let status = Command::new("git")
            .current_dir(remote_checkout)
            .args(["status", "--porcelain"])
            .output()
            .map_err(|e| format!("Failed to check git status in remote checkout: {e}"))?;

        if status.stdout.is_empty() {
            return Ok("No changes to push to remote repository.".to_string());
        }

        // Stage only the validated templates directory to prevent accidental untracked file leaks
        let add_status = Command::new("git")
            .current_dir(remote_checkout)
            .args(["add", "templates"])
            .status()
            .map_err(|e| format!("Failed to stage remote template changes: {e}"))?;

        if !add_status.success() {
            return Err("git add failed in remote checkout".to_string());
        }

        // Verify that only files under templates/ are staged
        let diff_cached = Command::new("git")
            .current_dir(remote_checkout)
            .args(["diff", "--cached", "--name-only"])
            .output()
            .map_err(|e| format!("Failed to inspect staged diff in remote checkout: {e}"))?;

        let staged_output = String::from_utf8_lossy(&diff_cached.stdout);
        for line in staged_output.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("templates/") {
                return Err(format!(
                    "Security boundary violation: staging detected file outside templates/: {trimmed}"
                ));
            }
        }

        let commit_msg = "chore(sync): synchronize environment refinements from local workstation";

        if open_pr {
            let sync_branch = format!("ude-sync-{}", &Self::compute_hash(commit_msg.as_bytes())[..8]);
            let _ = Command::new("git")
                .current_dir(remote_checkout)
                .args(["checkout", "-b", &sync_branch])
                .status();

            let commit_status = Command::new("git")
                .current_dir(remote_checkout)
                .args(["commit", "-m", commit_msg])
                .status()
                .map_err(|e| format!("Failed to commit in remote checkout: {e}"))?;

            if !commit_status.success() {
                return Err("git commit failed in remote checkout".to_string());
            }

            let push_status = Command::new("git")
                .current_dir(remote_checkout)
                .args(["push", "-u", "origin", &sync_branch])
                .status()
                .map_err(|e| format!("Failed to push sync branch: {e}"))?;

            if !push_status.success() {
                return Err(format!("git push failed for branch {sync_branch}"));
            }

            // Attempt to create PR via gh CLI if available
            let pr_cmd = Command::new("gh")
                .current_dir(remote_checkout)
                .args([
                    "pr",
                    "create",
                    "--title",
                    commit_msg,
                    "--body",
                    "Automated pull request from UDE remote synchronization.",
                    "--base",
                    branch,
                    "--head",
                    &sync_branch,
                ])
                .output();

            match pr_cmd {
                Ok(output) if output.status.success() => {
                    let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    Ok(format!("Opened GitHub Pull Request: {pr_url}"))
                }
                _ => Ok(format!(
                    "Pushed changes to branch '{sync_branch}'. Create a PR on GitHub to merge into '{branch}'."
                )),
            }
        } else {
            let commit_status = Command::new("git")
                .current_dir(remote_checkout)
                .args(["commit", "-m", commit_msg])
                .status()
                .map_err(|e| format!("Failed to commit in remote checkout: {e}"))?;

            if !commit_status.success() {
                return Err("git commit failed in remote checkout".to_string());
            }

            let push_status = Command::new("git")
                .current_dir(remote_checkout)
                .args(["push", "origin", branch])
                .status()
                .map_err(|e| format!("Failed to push to remote: {e}"))?;

            if !push_status.success() {
                return Err(format!("git push origin {branch} failed"));
            }

            Ok(format!("Directly pushed changes to remote branch '{branch}'."))
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
            false,
        );

        assert_eq!(res, PushResult::Success { synced_files: 1 });

        let written_upstream = upstream_dir
            .path()
            .join("projects/generic/gitconfig.repo");
        let content = fs::read_to_string(written_upstream).unwrap();
        assert!(content.contains("{{ user.home }}"));
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
            false,
        );

        match res {
            PushResult::SecretBlocked { path, .. } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
            }
            _ => panic!("Expected SecretBlocked, got {:?}", res),
        }
    }

    #[test]
    fn test_push_updates_atomic_prevalidation_no_partial_writes() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Candidate 1: Clean file (.gitconfig)
        let local_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(&local_gitconfig, "[user]\nname = Alice\n").unwrap();

        // Candidate 2: Dirty file (.gitignore with secret)
        let local_gitignore = local_dir.path().join(".gitignore");
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        fs::write(&local_gitignore, format!("target/\n{prefix}{body}\n")).unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
        );

        // Must reject on the dirty file
        match res {
            PushResult::SecretBlocked { path, .. } => {
                assert_eq!(path, PathBuf::from(".gitignore"));
            }
            _ => panic!("Expected SecretBlocked, got {:?}", res),
        }

        // Negative control: Assert that .gitconfig upstream was NOT created / written
        let upstream_gitconfig = map_to_upstream(upstream_dir.path(), Path::new(".gitconfig")).unwrap();
        assert!(
            !upstream_gitconfig.exists(),
            "Atomicity violation: upstream file was written before pre-validation completed!"
        );
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
            false,
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
            false,
        );

        assert_eq!(res, PushResult::Success { synced_files: 0 });
    }

    #[test]
    fn test_remote_repo_cache_management() {
        let engine = SyncEngine::new();
        let cache_dir = tempdir().unwrap();
        let fake_remote = tempdir().unwrap();

        // Initialize a fake remote git repository with a templates/ directory
        let status = Command::new("git")
            .current_dir(fake_remote.path())
            .args(["init", "-b", "main"])
            .status()
            .unwrap();
        assert!(status.success());

        let fake_templates = fake_remote.path().join("templates");
        fs::create_dir_all(&fake_templates).unwrap();
        fs::write(fake_templates.join("test.txt"), "remote template content").unwrap();

        let _ = Command::new("git")
            .current_dir(fake_remote.path())
            .args(["add", "-A"])
            .status();
        let commit_status = Command::new("git")
            .current_dir(fake_remote.path())
            .args([
                "-c", "user.name=Tester",
                "-c", "user.email=tester@example.com",
                "-c", "commit.gpgsign=false",
                "commit",
                "--no-gpg-sign",
                "-m", "init fake remote",
            ])
            .status()
            .unwrap();
        assert!(commit_status.success());

        let remote_url = fake_remote.path().to_string_lossy().to_string();
        let resolved = engine
            .ensure_remote_repo(&remote_url, "main", cache_dir.path())
            .expect("remote cache resolution");

        assert!(resolved.exists());
        assert!(resolved.join("test.txt").exists());
    }
}
