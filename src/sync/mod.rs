pub mod merge;
pub mod state;
pub use state::{FileStateRecord, ProjectState};

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
    Merged,
    Conflicted,
    Overridden,
}

#[derive(Debug, Clone)]
pub struct FileSyncState {
    pub relative_path: PathBuf,
    pub status: SyncStatus,
    pub local_hash: Option<String>,
    pub upstream_hash: Option<String>,
    pub ancestor_hash: Option<String>,
    pub merged_content: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushResult {
    Success {
        synced_files: usize,
        staged_files: Vec<PathBuf>,
        updated_records: Vec<(PathBuf, String, Option<String>)>,
    },
    SecretBlocked { path: PathBuf, reason: String },
    ConflictBlocked { path: PathBuf },
    BoundaryBlocked { path: PathBuf },
    IoFailed { path: PathBuf, reason: String },
    GitRemoteFailed { reason: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PullResult {
    Success { applied_files: usize },
    ConflictBlocked { path: PathBuf },
    IoFailed { path: PathBuf, reason: String },
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

    /// Detects 3-way sync status using SHA-256 hashes of ancestor (O), local (A), and upstream (B).
    pub fn compare_3way_hashes(
        ancestor_hash: Option<&str>,
        local_hash: Option<&str>,
        upstream_hash: Option<&str>,
        has_local_override: bool,
    ) -> SyncStatus {
        // Clean identical match
        if local_hash == upstream_hash && local_hash.is_some() {
            return if has_local_override {
                SyncStatus::Overridden
            } else {
                SyncStatus::InSync
            };
        }

        match ancestor_hash {
            Some(o) => {
                let local_matches_base = local_hash == Some(o);
                let upstream_matches_base = upstream_hash == Some(o);

                match (local_matches_base, upstream_matches_base) {
                    (true, false) => SyncStatus::UpstreamModified,
                    (false, true) => {
                        if has_local_override {
                            SyncStatus::Overridden
                        } else {
                            SyncStatus::LocallyModified
                        }
                    }
                    (false, false) => SyncStatus::Conflicted,
                    (true, true) => {
                        if has_local_override {
                            SyncStatus::Overridden
                        } else {
                            SyncStatus::InSync
                        }
                    }
                }
            }
            None => match (local_hash, upstream_hash) {
                (Some(_), None) => {
                    if has_local_override {
                        SyncStatus::Overridden
                    } else {
                        SyncStatus::LocallyModified
                    }
                }
                (None, Some(_)) => SyncStatus::UpstreamModified,
                (Some(_), Some(_)) => SyncStatus::Conflicted,
                (None, None) => {
                    if has_local_override {
                        SyncStatus::Overridden
                    } else {
                        SyncStatus::InSync
                    }
                }
            },
        }
    }

    /// Detects 3-way sync status and attempts orthogonal text merge when both sides diverge from ancestor.
    pub fn compare_3way_with_merge(
        ancestor_content: Option<&str>,
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> (SyncStatus, Option<String>) {
        if local_content == upstream_content && local_content.is_some() {
            return if has_local_override {
                (SyncStatus::Overridden, None)
            } else {
                (SyncStatus::InSync, None)
            };
        }

        match ancestor_content {
            Some(o) => {
                let local_matches_base = local_content == Some(o);
                let upstream_matches_base = upstream_content == Some(o);

                match (local_matches_base, upstream_matches_base) {
                    (true, false) => (SyncStatus::UpstreamModified, None),
                    (false, true) => {
                        if has_local_override {
                            (SyncStatus::Overridden, None)
                        } else {
                            (SyncStatus::LocallyModified, None)
                        }
                    }
                    (false, false) => {
                        let local_str = local_content.unwrap_or("");
                        let upstream_str = upstream_content.unwrap_or("");
                        match merge::merge_3way_text(o, local_str, upstream_str) {
                            merge::MergeResult::Clean(merged) => (SyncStatus::Merged, Some(merged)),
                            merge::MergeResult::Conflict => (SyncStatus::Conflicted, None),
                        }
                    }
                    (true, true) => {
                        if has_local_override {
                            (SyncStatus::Overridden, None)
                        } else {
                            (SyncStatus::InSync, None)
                        }
                    }
                }
            }
            None => {
                let a_hash = None;
                let l_hash = local_content.map(|c| Self::compute_hash(c.as_bytes()));
                let u_hash = upstream_content.map(|c| Self::compute_hash(c.as_bytes()));
                let status = Self::compare_3way_hashes(
                    a_hash,
                    l_hash.as_deref(),
                    u_hash.as_deref(),
                    has_local_override,
                );
                (status, None)
            }
        }
    }

    /// Detects 3-way sync status between ancestor (base), local, and upstream content string slices.
    pub fn compare_3way(
        ancestor_content: Option<&str>,
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> SyncStatus {
        Self::compare_3way_with_merge(ancestor_content, local_content, upstream_content, has_local_override).0
    }

    /// Detects sync status between a local file and upstream content (delegates to 3-way comparison with no ancestor).
    pub fn compare(
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> SyncStatus {
        Self::compare_3way(None, local_content, upstream_content, has_local_override)
    }

    /// Compares a local directory against an upstream template directory using 3-way ancestor state.
    pub fn scan_project(
        &self,
        local_root: &Path,
        upstream_root: &Path,
        ctx: Option<&crate::template::TemplateContext>,
    ) -> Vec<FileSyncState> {
        let mut states = Vec::new();
        let project_state = ProjectState::load_from_dir(local_root);
        let renderer = crate::template::TemplateRenderer::new();

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
            let upstream_content = upstream_path.as_ref().and_then(|p| {
                fs::read_to_string(p).ok().and_then(|raw| {
                    let should_render = p.extension().and_then(|e| e.to_str()) == Some("jinja")
                        || raw.contains("{{");
                    if should_render {
                        if let Some(c) = ctx {
                            renderer.render_str(&raw, c).ok().or(Some(raw))
                        } else {
                            Some(raw)
                        }
                    } else {
                        Some(raw)
                    }
                })
            });

            let has_local_override = check_for_override(local_root, &rel);
            let ancestor_hash = project_state.get_ancestor_hash(&rel).map(String::from);
            let ancestor_content = project_state.get_ancestor_content(&rel);
            let local_hash = local_content.as_ref().map(|c| Self::compute_hash(c.as_bytes()));
            let upstream_hash = upstream_content.as_ref().map(|c| Self::compute_hash(c.as_bytes()));

            let (status, merged_content) = Self::compare_3way_with_merge(
                ancestor_content,
                local_content.as_deref(),
                upstream_content.as_deref(),
                has_local_override,
            );

            states.push(FileSyncState {
                relative_path: rel,
                status,
                local_hash,
                upstream_hash,
                ancestor_hash,
                merged_content,
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

    /// Pulls upstream updates into the local project directory.
    /// Operates with strict safety:
    /// 1. Rejects pull completely if any managed file is Conflicted (conflict gate).
    /// 2. For every UpstreamModified file, materializes the rendered/copied content.
    /// 3. Preserves Unix execute permissions on scripts and hooks.
    /// 4. Updates ancestor checksums in `.ude/state.json`.
    pub fn pull_updates(
        &self,
        local_root: &Path,
        upstream_root: &Path,
        ctx: &crate::template::TemplateContext,
    ) -> PullResult {
        let states = self.scan_project(local_root, upstream_root, Some(ctx));
        let renderer = crate::template::TemplateRenderer::new();

        // Gate 1: Check for conflicts across the entire project
        for s in &states {
            if s.status == SyncStatus::Conflicted {
                return PullResult::ConflictBlocked {
                    path: s.relative_path.clone(),
                };
            }
        }

        let mut candidates_to_write = Vec::new();

        // Pass 1: Resolve all upstream content
        for s in states {
            if s.status == SyncStatus::UpstreamModified {
                let upstream_path = match map_to_upstream(upstream_root, &s.relative_path) {
                    Some(p) => p,
                    None => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason: "No upstream mapping found for file".to_string(),
                        };
                    }
                };

                let raw = match fs::read_to_string(&upstream_path) {
                    Ok(r) => r,
                    Err(e) => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason: format!("Failed to read upstream file {}: {e}", upstream_path.display()),
                        };
                    }
                };

                let should_render = upstream_path.extension().and_then(|e| e.to_str()) == Some("jinja")
                    || raw.contains("{{");
                let content = if should_render {
                    match renderer.render_str(&raw, ctx) {
                        Ok(rendered) => rendered,
                        Err(e) => {
                            return PullResult::IoFailed {
                                path: s.relative_path.clone(),
                                reason: format!("Failed to render template {}: {e}", upstream_path.display()),
                            };
                        }
                    }
                } else {
                    raw
                };

                let target_path = local_root.join(&s.relative_path);
                let new_hash = Self::compute_hash(content.as_bytes());
                candidates_to_write.push((s.relative_path, target_path, content, new_hash));
            } else if s.status == SyncStatus::Merged {
                let content = match s.merged_content {
                    Some(m) => m,
                    None => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason: "Missing merged content for clean 3-way merge".to_string(),
                        };
                    }
                };
                let target_path = local_root.join(&s.relative_path);
                let new_hash = Self::compute_hash(content.as_bytes());
                candidates_to_write.push((s.relative_path, target_path, content, new_hash));
            }
        }

        if candidates_to_write.is_empty() {
            return PullResult::Success { applied_files: 0 };
        }

        // Pass 2: Write files and update ancestor state
        let mut project_state = ProjectState::load_from_dir(local_root);
        let mut applied = 0;

        for (rel, target, content, new_hash) in candidates_to_write {
            if let Some(parent) = target.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                return PullResult::IoFailed {
                    path: rel,
                    reason: format!("Failed to create parent directory: {e}"),
                };
            }

            if let Err(e) = fs::write(&target, &content) {
                return PullResult::IoFailed {
                    path: rel,
                    reason: format!("Failed to write local file: {e}"),
                };
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let rel_str = rel.to_string_lossy();
                if rel_str.starts_with("scripts/hooks/")
                    || rel_str.starts_with("scratch/")
                    || rel_str == "scripts/check-in-container.sh"
                {
                    let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
                }
            }

            project_state.record_file(&rel, new_hash, Some(content));
            applied += 1;
        }

        if let Err(e) = project_state.save_to_dir(local_root) {
            return PullResult::IoFailed {
                path: PathBuf::from(".ude/state.json"),
                reason: format!("Failed to persist state: {e}"),
            };
        }

        PullResult::Success { applied_files: applied }
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
        persist_ancestor_immediately: bool,
    ) -> PushResult {
        let dummy_ctx = crate::template::TemplateContext::detect_for_project(
            local_root,
            "project",
            "generic",
        );
        let states = self.scan_project(local_root, upstream_root, Some(&dummy_ctx));
        let mut candidates_to_write = Vec::new();

        // Pass 1: Strict atomic pre-validation across ALL modified candidates
        for state in &states {
            // Conflict gate: Abort push if any file is conflicted
            if state.status == SyncStatus::Conflicted {
                return PushResult::ConflictBlocked {
                    path: state.relative_path.clone(),
                };
            }

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
                        path: state.relative_path.clone(),
                    };
                }

                let content = match fs::read_to_string(&local_path) {
                    Ok(c) => c,
                    Err(err) => {
                        return PushResult::IoFailed {
                            path: state.relative_path.clone(),
                            reason: format!("Failed to read local file: {err}"),
                        };
                    }
                };

                // Gate 2: Secret check (reject credentials)
                match self.sanitizer.check_secrets(&content) {
                    SanitizationResult::SecretDetected { reason, .. } => {
                        return PushResult::SecretBlocked {
                            path: state.relative_path.clone(),
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
                    candidates_to_write.push((state.relative_path.clone(), target_upstream, parameterized, content));
                }
            }
        }

        // Pass 2: Transactional materialization of validated files
        let mut synced = 0;
        let mut staged_files = Vec::new();
        let mut updated_records = Vec::new();

        for (rel_path, target_upstream, parameterized, raw_local_content) in candidates_to_write {
            if let Some(parent) = target_upstream.parent()
                && let Err(err) = fs::create_dir_all(parent)
            {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to create parent directory: {err}"),
                };
            }
            if let Err(err) = fs::write(&target_upstream, &parameterized) {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to write upstream template: {err}"),
                };
            }

            let local_hash = Self::compute_hash(raw_local_content.as_bytes());
            staged_files.push(target_upstream.clone());
            updated_records.push((rel_path, local_hash, Some(raw_local_content)));
            synced += 1;
        }

        if persist_ancestor_immediately {
            let mut project_state = ProjectState::load_from_dir(local_root);
            for (rel, hash, content) in &updated_records {
                project_state.record_file(rel, hash.clone(), content.clone());
            }
            let _ = project_state.save_to_dir(local_root);
        }

        PushResult::Success {
            synced_files: synced,
            staged_files,
            updated_records,
        }
    }

    /// Pushes committed upstream updates to the remote git origin or creates a PR
    pub fn push_to_remote(
        &self,
        remote_checkout: &Path,
        branch: &str,
        open_pr: bool,
        staged_files: &[PathBuf],
    ) -> Result<String, String> {
        let status = Command::new("git")
            .current_dir(remote_checkout)
            .args(["status", "--porcelain"])
            .output()
            .map_err(|e| format!("Failed to check git status in remote checkout: {e}"))?;

        if status.stdout.is_empty() {
            return Ok("No changes to push to remote repository.".to_string());
        }

        if staged_files.is_empty() {
            return Ok("No changes to push to remote repository.".to_string());
        }

        // Convert target paths to relative paths from remote checkout
        let mut rel_staged_paths: Vec<PathBuf> = Vec::new();
        for p in staged_files {
            let rel = p.strip_prefix(remote_checkout).unwrap_or(p).to_path_buf();
            rel_staged_paths.push(rel);
        }

        // Stage ONLY the explicit validated allowlist of files
        for rel_p in &rel_staged_paths {
            let add_status = Command::new("git")
                .current_dir(remote_checkout)
                .args(["add", "--"])
                .arg(rel_p)
                .status()
                .map_err(|e| format!("Failed to stage {}: {e}", rel_p.display()))?;

            if !add_status.success() {
                let _ = Command::new("git").current_dir(remote_checkout).args(["reset", "HEAD", "--"]).status();
                return Err(format!("git add failed in remote checkout for {}", rel_p.display()));
            }
        }

        // Inspect staged index: assert that ONLY allowed files are staged
        let diff_cached = Command::new("git")
            .current_dir(remote_checkout)
            .args(["diff", "--cached", "--name-only"])
            .output()
            .map_err(|e| format!("Failed to inspect staged diff in remote checkout: {e}"))?;

        let staged_output = String::from_utf8_lossy(&diff_cached.stdout);
        let expected_set: std::collections::HashSet<PathBuf> = rel_staged_paths.into_iter().collect();

        for line in staged_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let p = PathBuf::from(trimmed);
            if !expected_set.contains(&p) {
                let _ = Command::new("git").current_dir(remote_checkout).args(["reset", "HEAD", "--"]).status();
                return Err(format!(
                    "Publication gate violation: unvalidated file staged in remote checkout: {}",
                    p.display()
                ));
            }

            // Defense in depth: Check staged file bytes for secrets
            let staged_full_path = remote_checkout.join(&p);
            if let Ok(content) = fs::read_to_string(&staged_full_path)
                && let SanitizationResult::SecretDetected { reason, .. } = self.sanitizer.check_secrets(&content)
            {
                let _ = Command::new("git").current_dir(remote_checkout).args(["reset", "HEAD", "--"]).status();
                return Err(format!(
                    "Publication gate violation: secret detected in staged file {}: {reason}",
                    p.display()
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
            SyncEngine::compare_3way(
                Some("base version"),
                Some("local change"),
                Some("base version"),
                false,
            ),
            SyncStatus::LocallyModified
        );
    }

    #[test]
    fn test_sync_status_upstream_modified() {
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                Some("base version"),
                Some("upstream change"),
                false,
            ),
            SyncStatus::UpstreamModified
        );
    }

    #[test]
    fn test_sync_status_conflicted() {
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                Some("local edit"),
                Some("upstream edit"),
                false,
            ),
            SyncStatus::Conflicted
        );
    }

    #[test]
    fn test_sync_status_no_ancestor_divergent() {
        assert_eq!(
            SyncEngine::compare_3way(
                None,
                Some("local edit"),
                Some("upstream edit"),
                false,
            ),
            SyncStatus::Conflicted
        );
    }

    #[test]
    fn test_sync_status_identical_additions() {
        assert_eq!(
            SyncEngine::compare_3way(
                None,
                Some("identical addition"),
                Some("identical addition"),
                false,
            ),
            SyncStatus::InSync
        );
    }

    #[test]
    fn test_sync_status_deletions() {
        // Local deleted, upstream untouched
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                None,
                Some("base version"),
                false,
            ),
            SyncStatus::LocallyModified
        );

        // Upstream deleted, local untouched
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                Some("base version"),
                None,
                false,
            ),
            SyncStatus::UpstreamModified
        );

        // Conflicting deletion: local deleted, upstream modified
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                None,
                Some("upstream modified"),
                false,
            ),
            SyncStatus::Conflicted
        );
    }

    #[test]
    fn test_sync_status_overridden() {
        assert_eq!(
            SyncEngine::compare_3way(
                Some("base version"),
                Some("local change"),
                Some("base version"),
                true,
            ),
            SyncStatus::Overridden
        );
    }

    #[test]
    fn test_pull_updates_materializes_files_and_updates_state() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Setup upstream files: static and jinja
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, "[core]\n  autocrlf = input\n").unwrap();

        let up_hook = upstream_dir.path().join("projects/generic/hooks/pre-commit");
        fs::create_dir_all(up_hook.parent().unwrap()).unwrap();
        fs::write(&up_hook, "#!/bin/sh\nexit 0\n").unwrap();

        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(&up_mise, "[tools]\nrust = \"1.85.0\"\nhome = \"{{ user.home }}\"\n").unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        let pull_res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull_res, PullResult::Success { applied_files: 3 });

        // Verify files materialized locally
        let loc_gitconfig = local_dir.path().join(".gitconfig");
        assert!(loc_gitconfig.exists());
        assert_eq!(fs::read_to_string(&loc_gitconfig).unwrap(), "[core]\n  autocrlf = input\n");

        let loc_hook = local_dir.path().join("scripts/hooks/pre-commit");
        assert!(loc_hook.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&loc_hook).unwrap();
            assert_eq!(meta.permissions().mode() & 0o111, 0o111, "Hook must be executable");
        }

        let loc_mise = local_dir.path().join("mise.toml");
        assert!(loc_mise.exists());
        let mise_content = fs::read_to_string(&loc_mise).unwrap();
        assert!(mise_content.contains(&ctx.user.home));

        // Verify state persistence
        let state = ProjectState::load_from_dir(local_dir.path());
        assert!(state.get_ancestor_hash(Path::new(".gitconfig")).is_some());
        assert!(state.get_ancestor_hash(Path::new("scripts/hooks/pre-commit")).is_some());
        assert!(state.get_ancestor_hash(Path::new("mise.toml")).is_some());

        // Negative control: Re-scanning now must report InSync for all 3 files
        let states = engine.scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx));
        for s in states {
            if s.relative_path == Path::new(".gitconfig")
                || s.relative_path == Path::new("scripts/hooks/pre-commit")
                || s.relative_path == Path::new("mise.toml")
            {
                assert_eq!(s.status, SyncStatus::InSync, "File {:?} must be InSync after pull", s.relative_path);
            }
        }
    }

    #[test]
    fn test_pull_updates_conflict_blocked_no_partial_writes() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // Record ancestor for .gitconfig
        let mut state = ProjectState::default();
        let base_content = "[core]\n  autocrlf = false\n";
        let base_hash = SyncEngine::compute_hash(base_content.as_bytes());
        state.record_file(Path::new(".gitconfig"), base_hash, Some(base_content.to_string()));
        state.save_to_dir(local_dir.path()).unwrap();

        // Local has modified .gitconfig
        let loc_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(&loc_gitconfig, "[core]\n  autocrlf = local_divergence\n").unwrap();

        // Upstream has ALSO modified .gitconfig
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, "[core]\n  autocrlf = upstream_divergence\n").unwrap();

        // Upstream also has a clean file that should NOT be written due to the conflict
        let up_hook = upstream_dir.path().join("projects/generic/hooks/pre-commit");
        fs::create_dir_all(up_hook.parent().unwrap()).unwrap();
        fs::write(&up_hook, "#!/bin/sh\nexit 0\n").unwrap();

        let res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        match res {
            PullResult::ConflictBlocked { path } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
            }
            _ => panic!("Expected ConflictBlocked, got {:?}", res),
        }

        // Negative control: Verify hook was NOT written to disk
        assert!(
            !local_dir.path().join("scripts/hooks/pre-commit").exists(),
            "Atomicity violation: clean file was materialized despite conflict block!"
        );
        // Verify local gitconfig was NOT overwritten
        assert_eq!(
            fs::read_to_string(&loc_gitconfig).unwrap(),
            "[core]\n  autocrlf = local_divergence\n"
        );
    }

    #[test]
    fn test_push_updates_blocked_on_conflict() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Record ancestor for .gitconfig
        let mut state = ProjectState::default();
        let base_content = "[core]\n  autocrlf = false\n";
        let base_hash = SyncEngine::compute_hash(base_content.as_bytes());
        state.record_file(Path::new(".gitconfig"), base_hash, Some(base_content.to_string()));
        state.save_to_dir(local_dir.path()).unwrap();

        // Local edit
        let loc_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(&loc_gitconfig, "[core]\n  autocrlf = local_edit\n").unwrap();

        // Upstream edit
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, "[core]\n  autocrlf = upstream_edit\n").unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
            true,
        );

        match res {
            PushResult::ConflictBlocked { path } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
            }
            _ => panic!("Expected ConflictBlocked on push, got {:?}", res),
        }
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
            true,
        );

        match res {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 1),
            _ => panic!("Expected Success on push, got {:?}", res),
        }

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
            true,
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
            true,
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
            true,
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
            true,
        );

        match res {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 0),
            _ => panic!("Expected Success with 0 synced files, got {:?}", res),
        }
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

    #[test]
    fn test_defect1_failed_push_does_not_commit_ancestor_state() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // 1. Initial base state
        let mut state = ProjectState::default();
        let base_content = "[core]\n  autocrlf = input\n";
        let base_hash = SyncEngine::compute_hash(base_content.as_bytes());
        state.record_file(Path::new(".gitconfig"), base_hash.clone(), Some(base_content.to_string()));
        state.save_to_dir(local_dir.path()).unwrap();

        // 2. Local edit
        let loc_gitconfig = local_dir.path().join(".gitconfig");
        let local_edit = "[core]\n  autocrlf = custom_local\n";
        fs::write(&loc_gitconfig, local_edit).unwrap();

        // 3. Upstream template initialized with base
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, base_content).unwrap();

        // 4. Perform push with persist_ancestor_immediately = false (as happens during remote git sync)
        let push_res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
            false, // Remote push in progress; do not commit ancestor yet!
        );
        assert!(matches!(push_res, PushResult::Success { .. }));

        // Verify ancestor state in .ude/state.json is still the BASE hash, NOT the local edit hash
        let current_state = ProjectState::load_from_dir(local_dir.path());
        assert_eq!(
            current_state.get_ancestor_hash(Path::new(".gitconfig")),
            Some(base_hash.as_str()),
            "Failed push must not commit ancestor state ahead of remote success!"
        );

        // 5. Simulate remote reject / rollback of upstream template store
        fs::write(&up_gitconfig, base_content).unwrap();

        // 6. Next pull must NOT overwrite local edit because local edit was not recorded as ancestor
        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );
        let pull_res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        // It should NOT apply 1 update and wipe local edit; it has 0 pending upstream updates
        assert_eq!(pull_res, PullResult::Success { applied_files: 0 });
        assert_eq!(
            fs::read_to_string(&loc_gitconfig).unwrap(),
            local_edit,
            "Local edit must not be erased after rejected push!"
        );
    }

    #[test]
    fn test_defect2_publication_gate_allowlist_staging_blocks_unvalidated_files() {
        let engine = SyncEngine::new();
        let remote_repo = tempdir().unwrap();

        // Initialize fake remote repository
        let _ = Command::new("git").current_dir(remote_repo.path()).args(["init", "-b", "main"]).status();
        let templates_dir = remote_repo.path().join("templates/projects/generic");
        fs::create_dir_all(&templates_dir).unwrap();
        let valid_file = templates_dir.join("gitconfig.repo");
        fs::write(&valid_file, "[core]\n  autocrlf = true\n").unwrap();

        let _ = Command::new("git").current_dir(remote_repo.path()).args(["add", "-A"]).status();
        let _ = Command::new("git").current_dir(remote_repo.path()).args([
            "-c", "user.name=Tester",
            "-c", "user.email=tester@example.com",
            "-c", "commit.gpgsign=false",
            "commit",
            "--no-gpg-sign",
            "-m", "base commit",
        ]).status();

        // Adversarial setup: Place an unvalidated dirty secret file in templates/
        let dirty_secret_file = remote_repo.path().join("templates/unvalidated-secret.txt");
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        fs::write(&dirty_secret_file, format!("{prefix}{body}\n")).unwrap();

        // Now modify the valid managed file
        fs::write(&valid_file, "[core]\n  autocrlf = false\n").unwrap();

        // Call push_to_remote staging ONLY the valid file
        let staged_allowlist = vec![valid_file.clone()];
        // Note: git push will fail because there is no remote server, but staging and commit will execute
        let _ = engine.push_to_remote(remote_repo.path(), "main", false, &staged_allowlist);

        // Verify git status: the dirty secret file must remain UNTRACKED / UNSTAGED!
        let status = Command::new("git")
            .current_dir(remote_repo.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        let status_out = String::from_utf8_lossy(&status.stdout);

        // The dirty file must be untracked ('??'), never staged in the index!
        assert!(
            status_out.contains("?? templates/unvalidated-secret.txt")
                || !status_out.contains("templates/unvalidated-secret.txt"),
            "Dirty secret file must never be staged! Status: {status_out}"
        );
    }

    #[test]
    fn test_defect3_path_parameterization_roundtrip_static_template() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        let user_home = "/home/fixture";
        let local_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(
            &local_gitconfig,
            "[core]\n  excludesfile = /home/fixture/.gitignore\n",
        )
        .unwrap();

        // 1. Push: parameterizes /home/fixture -> {{ user.home }}
        let push_res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            user_home,
            "/home/fixture/.config",
            false,
            true,
        );
        assert!(matches!(push_res, PushResult::Success { synced_files: 1, .. }));

        // Verify upstream gitconfig.repo has {{ user.home }}
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        let up_content = fs::read_to_string(&up_gitconfig).unwrap();
        assert_eq!(up_content, "[core]\n  excludesfile = {{ user.home }}/.gitignore\n");

        // 2. Scan: must render {{ user.home }} back to /home/fixture even though extension is .repo, reporting InSync!
        let mut ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );
        ctx.user.home = user_home.to_string();
        let states = engine.scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx));
        let gitconfig_state = states.iter().find(|s| s.relative_path == Path::new(".gitconfig")).unwrap();
        assert_eq!(gitconfig_state.status, SyncStatus::InSync, "Static template with {{ user.home }} must render to InSync");
    }

    #[test]
    fn test_defect4_layered_override_allows_upstream_base_updates() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Base ancestor for mise.toml
        let mut state = ProjectState::default();
        let base_mise = "[tools]\nrust = \"1.85.0\"\n";
        let base_hash = SyncEngine::compute_hash(base_mise.as_bytes());
        state.record_file(Path::new("mise.toml"), base_hash, Some(base_mise.to_string()));
        state.save_to_dir(local_dir.path()).unwrap();

        // Local has base mise.toml AND a layered override mise.local.toml
        let loc_mise = local_dir.path().join("mise.toml");
        fs::write(&loc_mise, base_mise).unwrap();
        let loc_override = local_dir.path().join("mise.local.toml");
        fs::write(&loc_override, "[tools]\nrust = \"nightly\"\n").unwrap();

        // Upstream updates mise.toml.jinja
        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        let updated_upstream = "[tools]\nrust = \"1.86.0\"\n";
        fs::write(&up_mise, updated_upstream).unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // Scan project: must report UpstreamModified, NOT frozen in Overridden!
        let states = engine.scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx));
        let mise_state = states.iter().find(|s| s.relative_path == Path::new("mise.toml")).unwrap();
        assert_eq!(
            mise_state.status,
            SyncStatus::UpstreamModified,
            "Layered override must NOT block upstream updates to base template"
        );

        // Pull updates: must update mise.toml to 1.86.0 while preserving mise.local.toml
        let pull_res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull_res, PullResult::Success { applied_files: 1 });
        assert_eq!(fs::read_to_string(&loc_mise).unwrap(), updated_upstream);
        assert_eq!(fs::read_to_string(&loc_override).unwrap(), "[tools]\nrust = \"nightly\"\n");
    }
}
