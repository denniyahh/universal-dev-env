pub mod merge;
pub mod state;
use state::STATE_FILE_REL;
pub use state::{FileStateRecord, ProjectState};

use crate::process::{self, RunOutput};
use crate::sanitizer::{SanitizationResult, Sanitizer};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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
    pub upstream_content: Option<String>,
}

/// One present version of a managed file in a three-way comparison.
#[derive(Debug, Clone, Copy)]
struct FileVersion<'a> {
    /// [`SyncEngine::fingerprint`] of the version (for an ancestor, as recorded).
    fingerprint: &'a str,
    /// The text, when the version is valid UTF-8 (and, for an ancestor, recorded).
    text: Option<&'a str>,
}

impl<'a> FileVersion<'a> {
    /// The version with `fingerprint`, or `None` when the file is absent.
    fn new(fingerprint: Option<&'a str>, text: Option<&'a str>) -> Option<Self> {
        fingerprint.map(|fingerprint| Self { fingerprint, text })
    }
}

/// Line-ending style of a text file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolStyle {
    /// `\n` line endings.
    Lf,
    /// `\r\n` line endings.
    Crlf,
}

/// Dominant line-ending style of `bytes`: [`EolStyle::Crlf`] only when CRLF pairs
/// outnumber bare LF bytes. Ties and content without any LF are [`EolStyle::Lf`].
pub fn detect_eol_style(bytes: &[u8]) -> EolStyle {
    let crlf = bytes.windows(2).filter(|pair| *pair == b"\r\n").count();
    let bare_lf = bytes.iter().filter(|&&byte| byte == b'\n').count() - crlf;
    if crlf > bare_lf {
        EolStyle::Crlf
    } else {
        EolStyle::Lf
    }
}

/// Renders LF-normalized `text_lf` with `style` line endings.
pub fn apply_eol_style(text_lf: &str, style: EolStyle) -> String {
    match style {
        EolStyle::Lf => text_lf.to_string(),
        EolStyle::Crlf => text_lf.replace('\n', "\r\n"),
    }
}

/// Line-ending style of the existing file at `path`, or `None` when it is absent,
/// unreadable or not valid UTF-8 (such a destination is written as the source is).
fn existing_text_eol_style(path: &Path) -> Option<EolStyle> {
    let bytes = fs::read(path).ok()?;
    std::str::from_utf8(&bytes).ok()?;
    Some(detect_eol_style(&bytes))
}

#[derive(Debug, PartialEq, Eq)]
pub enum PushResult {
    Success {
        synced_files: usize,
        staged_files: Vec<PathBuf>,
        updated_records: Vec<(PathBuf, String, Option<String>)>,
    },
    SecretBlocked {
        path: PathBuf,
        reason: String,
    },
    ConflictBlocked {
        path: PathBuf,
    },
    BoundaryBlocked {
        path: PathBuf,
    },
    IoFailed {
        path: PathBuf,
        reason: String,
    },
    GitRemoteFailed {
        reason: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PullResult {
    Success { applied_files: usize },
    ConflictBlocked { path: PathBuf },
    IoFailed { path: PathBuf, reason: String },
}

/// Interval between attempts to take a contended remote cache lock.
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Exclusive advisory lock on a remote cache checkout (std file locking: flock(2) on
/// Unix, LockFileEx on Windows). The kernel releases it when the holding process
/// dies, and spawned children do not inherit it (descriptors are close-on-exec).
#[derive(Debug)]
pub struct RemoteLockGuard {
    file: std::fs::File,
    pub lock_path: PathBuf,
}

impl RemoteLockGuard {
    /// Opens (creating if needed) `lock_path` and takes its exclusive lock, polling
    /// every [`LOCK_POLL_INTERVAL`] for up to `timeout_secs` while another holder has it.
    pub fn acquire(lock_path: &Path, timeout_secs: u64) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|e| format!("Failed to open lockfile {}: {e}", lock_path.display()))?;

        let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        file,
                        lock_path: lock_path.to_path_buf(),
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "Timed out after {}s waiting for remote cache lock on {}",
                            timeout_secs,
                            lock_path.display()
                        ));
                    }
                    std::thread::sleep(LOCK_POLL_INTERVAL);
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(format!("Failed to lock {}: {e}", lock_path.display()));
                }
            }
        }
    }
}

impl Drop for RemoteLockGuard {
    fn drop(&mut self) {
        // Best effort: closing the file releases the lock as well.
        let _ = self.file.unlock();
    }
}

/// Time limits for the engine's git and gh spawns.
///
/// Internal parameterisation so tests can use sub-second limits. It is not a
/// user-facing override: there is no config or environment setting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessLimits {
    /// Limit for local git commands (status, add, commit, checkout, reset, show).
    pub local: Duration,
    /// Limit for network commands (clone, fetch, push) and every `gh` call.
    pub network: Duration,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            local: process::LOCAL_TIMEOUT,
            network: process::NETWORK_TIMEOUT,
        }
    }
}

#[derive(Default)]
pub struct SyncEngine {
    sanitizer: Sanitizer,
    /// Extra environment for every spawned git/gh, applied before the runner's
    /// scrub of repo-local git variables (tests inject a fixture gitconfig here).
    extra_env: Vec<(OsString, OsString)>,
    limits: ProcessLimits,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            sanitizer: Sanitizer::new(),
            extra_env: Vec::new(),
            limits: ProcessLimits::default(),
        }
    }

    /// Adds environment variables to every git/gh command the engine spawns. The
    /// runner still removes the repo-local git variables, so this cannot redirect
    /// git to another repository; it can point git at a global config file.
    pub fn with_extra_env(mut self, vars: Vec<(OsString, OsString)>) -> Self {
        self.extra_env = vars;
        self
    }

    /// Replaces the time limits of the engine's git/gh commands (tests only).
    pub fn with_limits(mut self, limits: ProcessLimits) -> Self {
        self.limits = limits;
        self
    }

    /// A `program` command with the engine's extra environment, run in `dir` if given.
    fn tool(&self, program: &str, dir: Option<&Path>) -> Command {
        let mut cmd = Command::new(program);
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
        cmd.envs(self.extra_env.iter().map(|(key, value)| (key, value)));
        cmd
    }

    /// A git command with the engine's extra environment, run in `dir` if given.
    fn git(&self, dir: Option<&Path>) -> Command {
        self.tool("git", dir)
    }

    /// Runs `cmd` on the local tier. `label` must not contain argv (remote URLs).
    fn run_local(&self, cmd: Command, label: &str) -> Result<RunOutput, String> {
        process::run_bounded(cmd, label, self.limits.local)
    }

    /// Runs `cmd` on the network tier. `label` must not contain argv (remote URLs).
    fn run_network(&self, cmd: Command, label: &str) -> Result<RunOutput, String> {
        process::run_bounded(cmd, label, self.limits.network)
    }

    /// Computes the sha256 checksum of a byte slice
    pub fn compute_hash(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    /// Content fingerprint used to classify managed files and record ancestors.
    ///
    /// Valid UTF-8 is hashed after [`Self::normalize_eol`], so CRLF and LF renderings
    /// of the same text share a fingerprint (a lone CR stays content). Other bytes are
    /// hashed raw. Non-content keys (cache directory, branch names) keep using the
    /// byte-exact [`Self::compute_hash`].
    pub fn fingerprint(bytes: &[u8]) -> String {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self::compute_hash(Self::normalize_eol(text).as_bytes()),
            Err(_) => Self::compute_hash(bytes),
        }
    }

    /// Replaces every CRLF pair in `text` with LF. A lone CR is content and is kept.
    pub fn normalize_eol(text: &str) -> String {
        text.replace("\r\n", "\n")
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

    /// Classifies one managed file from its ancestor, local and upstream versions.
    ///
    /// The status comes from fingerprints alone. Contents are used only to merge, and
    /// only when the fingerprints conflict and all three versions are text; the merged
    /// text is LF-normalized. An absent version is never merged as empty text.
    fn classify_file(
        ancestor: Option<FileVersion<'_>>,
        local: Option<FileVersion<'_>>,
        upstream: Option<FileVersion<'_>>,
        has_local_override: bool,
    ) -> (SyncStatus, Option<String>) {
        let status = Self::compare_3way_hashes(
            ancestor.map(|v| v.fingerprint),
            local.map(|v| v.fingerprint),
            upstream.map(|v| v.fingerprint),
            has_local_override,
        );
        if status != SyncStatus::Conflicted {
            return (status, None);
        }
        let (Some(base), Some(local), Some(upstream)) = (
            ancestor.and_then(|v| v.text),
            local.and_then(|v| v.text),
            upstream.and_then(|v| v.text),
        ) else {
            return (status, None);
        };
        match merge::merge_3way_text(
            &Self::normalize_eol(base),
            &Self::normalize_eol(local),
            &Self::normalize_eol(upstream),
        ) {
            merge::MergeResult::Clean(merged) => (SyncStatus::Merged, Some(merged)),
            merge::MergeResult::Conflict => (SyncStatus::Conflicted, None),
        }
    }

    /// Detects 3-way sync status of text contents by fingerprint and attempts an
    /// orthogonal text merge when both sides diverge from the ancestor.
    pub fn compare_3way_with_merge(
        ancestor_content: Option<&str>,
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> (SyncStatus, Option<String>) {
        let fingerprint = |content: Option<&str>| content.map(|c| Self::fingerprint(c.as_bytes()));
        let (ancestor_fp, local_fp, upstream_fp) = (
            fingerprint(ancestor_content),
            fingerprint(local_content),
            fingerprint(upstream_content),
        );
        Self::classify_file(
            FileVersion::new(ancestor_fp.as_deref(), ancestor_content),
            FileVersion::new(local_fp.as_deref(), local_content),
            FileVersion::new(upstream_fp.as_deref(), upstream_content),
            has_local_override,
        )
    }

    /// Detects 3-way sync status between ancestor (base), local, and upstream content string slices.
    pub fn compare_3way(
        ancestor_content: Option<&str>,
        local_content: Option<&str>,
        upstream_content: Option<&str>,
        has_local_override: bool,
    ) -> SyncStatus {
        Self::compare_3way_with_merge(
            ancestor_content,
            local_content,
            upstream_content,
            has_local_override,
        )
        .0
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
    ) -> Result<Vec<FileSyncState>, String> {
        let mut states = Vec::new();
        let project_state = ProjectState::load_from_dir(local_root)?;
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

            let local_bytes = fs::read(&local_path).ok();
            let local_content = local_bytes
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok().map(|s| s.to_string()));
            let local_hash = local_bytes.as_ref().map(|b| Self::fingerprint(b));

            let (upstream_bytes, upstream_content) = match upstream_path.as_ref() {
                Some(p) => {
                    if let Ok(bytes) = fs::read(p) {
                        if let Ok(raw_str) = std::str::from_utf8(&bytes) {
                            let raw_string = raw_str.to_string();
                            let should_render = p.extension().and_then(|e| e.to_str())
                                == Some("jinja")
                                || raw_string.contains("{{");
                            if should_render {
                                if let Some(c) = ctx {
                                    let rendered = renderer
                                        .render_str(&raw_string, c)
                                        .unwrap_or_else(|_| raw_string.clone());
                                    let r_bytes = rendered.as_bytes().to_vec();
                                    (Some(r_bytes), Some(rendered))
                                } else {
                                    (Some(bytes), Some(raw_string))
                                }
                            } else {
                                (Some(bytes), Some(raw_string))
                            }
                        } else {
                            (Some(bytes), None)
                        }
                    } else {
                        (None, None)
                    }
                }
                None => (None, None),
            };
            let upstream_hash = upstream_bytes.as_ref().map(|b| Self::fingerprint(b));

            let has_local_override = check_for_override(local_root, &rel);
            let ancestor_hash = project_state.get_ancestor_hash(&rel).map(String::from);
            let ancestor_content = project_state.get_ancestor_content(&rel);

            let (status, merged_content) = Self::classify_file(
                FileVersion::new(ancestor_hash.as_deref(), ancestor_content),
                FileVersion::new(local_hash.as_deref(), local_content.as_deref()),
                FileVersion::new(upstream_hash.as_deref(), upstream_content.as_deref()),
                has_local_override,
            );

            states.push(FileSyncState {
                relative_path: rel,
                status,
                local_hash,
                upstream_hash,
                ancestor_hash,
                merged_content,
                upstream_content,
            });
        }

        Ok(states)
    }

    /// Ensures a remote git repository is cached locally and updated.
    /// Serializes concurrent access via an advisory lockfile (`.lock`).
    pub fn ensure_remote_repo(
        &self,
        remote_url: &str,
        branch: &str,
        cache_dir: &Path,
    ) -> Result<(PathBuf, RemoteLockGuard), String> {
        fs::create_dir_all(cache_dir)
            .map_err(|e| format!("Failed to create cache dir {}: {e}", cache_dir.display()))?;

        // Hash remote URL and branch to isolate checkouts and prevent refspec collisions.
        // Sanitize branch slug so slashes in branch names don't fail lockfile creation.
        let safe_branch = sanitize_branch_slug(branch);
        let key = format!("{remote_url}#{branch}");
        let repo_hash = &Self::compute_hash(key.as_bytes())[..12];
        let lock_path = cache_dir.join(format!("remote-{repo_hash}-{safe_branch}.lock"));
        let lock_guard = RemoteLockGuard::acquire(&lock_path, 10)?;

        let target_checkout = cache_dir.join(format!("remote-{repo_hash}-{safe_branch}"));

        if !target_checkout.join(".git").exists() {
            println!(
                "  Cloning remote repository into cache: {}...",
                process::redact_userinfo(remote_url)
            );
            let mut clone = self.git(None);
            clone
                .args(["clone", "--depth", "1", "-b", branch, remote_url])
                .arg(&target_checkout);
            let out = self.run_network(clone, "git clone")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    format!("git clone failed with exit status {:?}", out.status.code()),
                    &out,
                ));
            }
        } else {
            // Fetch latest branch
            let mut fetch = self.git(Some(&target_checkout));
            fetch.args(["fetch", "origin", branch]);
            let out = self.run_network(fetch, "git fetch")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    format!("git fetch failed with exit status {:?}", out.status.code()),
                    &out,
                ));
            }

            let mut checkout = self.git(Some(&target_checkout));
            checkout.args(["checkout", "-B", branch, &format!("origin/{branch}")]);
            let out = self.run_local(checkout, "git checkout -B")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    format!(
                        "git checkout -B {branch} failed with exit status {:?}",
                        out.status.code()
                    ),
                    &out,
                ));
            }

            let mut reset = self.git(Some(&target_checkout));
            reset.args(["reset", "--hard", &format!("origin/{branch}")]);
            let out = self.run_local(reset, "git reset --hard")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    format!("git reset failed with exit status {:?}", out.status.code()),
                    &out,
                ));
            }

            let mut clean = self.git(Some(&target_checkout));
            clean.args(["clean", "-fd"]);
            let _ = self.run_local(clean, "git clean");
        }

        // Return the templates directory inside the remote checkout along with the held lock
        let templates_path = target_checkout.join("templates");
        if templates_path.exists() {
            Ok((templates_path, lock_guard))
        } else {
            Ok((target_checkout, lock_guard))
        }
    }

    /// Pulls upstream updates into the local project directory.
    /// Operates with strict safety:
    /// 1. Rejects pull completely if any managed file is Conflicted (conflict gate).
    /// 2. Validates targets defensively against symlink traversal and symlink destinations.
    /// 3. Supports binary and non-UTF8 template files cleanly without data corruption.
    /// 4. Reconciles converged ancestor states in `.ude/state.json` to prevent future false conflicts.
    /// 5. Preserves Unix execute permissions on scripts and hooks.
    pub fn pull_updates(
        &self,
        local_root: &Path,
        upstream_root: &Path,
        ctx: &crate::template::TemplateContext,
    ) -> PullResult {
        let states = match self.scan_project(local_root, upstream_root, Some(ctx)) {
            Ok(states) => states,
            Err(reason) => return state_pull_failure(reason),
        };
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
        for s in &states {
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

                let target_path = match validate_safe_write_target(local_root, &s.relative_path) {
                    Ok(p) => p,
                    Err(reason) => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason,
                        };
                    }
                };

                let upstream_bytes = match fs::read(&upstream_path) {
                    Ok(b) => b,
                    Err(e) => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason: format!(
                                "Failed to read upstream file {}: {e}",
                                upstream_path.display()
                            ),
                        };
                    }
                };

                let (content_bytes, ancestor_content) =
                    if let Ok(raw_str) = std::str::from_utf8(&upstream_bytes) {
                        let should_render = upstream_path.extension().and_then(|e| e.to_str())
                            == Some("jinja")
                            || raw_str.contains("{{");
                        let rendered = if should_render {
                            match renderer.render_str(raw_str, ctx) {
                                Ok(r) => r,
                                Err(e) => {
                                    return PullResult::IoFailed {
                                        path: s.relative_path.clone(),
                                        reason: format!(
                                            "Failed to render template {}: {e}",
                                            upstream_path.display()
                                        ),
                                    };
                                }
                            }
                        } else {
                            raw_str.to_string()
                        };
                        // Keep the local file's line endings; a file absent locally
                        // (or binary) is written exactly as upstream renders it.
                        let rendered_lf = Self::normalize_eol(&rendered);
                        let written = match existing_text_eol_style(&target_path) {
                            Some(style) => apply_eol_style(&rendered_lf, style),
                            None => rendered,
                        };
                        (written.into_bytes(), Some(rendered_lf))
                    } else {
                        (upstream_bytes, None)
                    };

                let new_hash = Self::fingerprint(&content_bytes);
                candidates_to_write.push((
                    s.relative_path.clone(),
                    target_path,
                    content_bytes,
                    new_hash,
                    ancestor_content,
                ));
            } else if s.status == SyncStatus::Merged {
                let target_path = match validate_safe_write_target(local_root, &s.relative_path) {
                    Ok(p) => p,
                    Err(reason) => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason,
                        };
                    }
                };

                let content = match &s.merged_content {
                    Some(m) => m.clone(),
                    None => {
                        return PullResult::IoFailed {
                            path: s.relative_path.clone(),
                            reason: "Missing merged content for clean 3-way merge".to_string(),
                        };
                    }
                };

                // For merged files, record the incoming upstream content as ancestor state
                let up_hash = s
                    .upstream_hash
                    .clone()
                    .unwrap_or_else(|| Self::fingerprint(content.as_bytes()));
                let up_content = s
                    .upstream_content
                    .as_deref()
                    .map(Self::normalize_eol)
                    .or_else(|| Some(content.clone()));
                // The merged text is LF-normalized; write it in the local file's style.
                let written = match existing_text_eol_style(&target_path) {
                    Some(style) => apply_eol_style(&content, style),
                    None => content,
                };
                candidates_to_write.push((
                    s.relative_path.clone(),
                    target_path,
                    written.into_bytes(),
                    up_hash,
                    up_content,
                ));
            }
        }

        // Reconcile converged ancestor states for InSync files whose recorded ancestor is stale
        let mut converged_to_record = Vec::new();
        for s in &states {
            if s.status == SyncStatus::InSync
                && let Some(loc_h) = &s.local_hash
                && s.ancestor_hash.as_ref() != Some(loc_h)
            {
                converged_to_record.push((
                    s.relative_path.clone(),
                    loc_h.clone(),
                    s.upstream_content.as_deref().map(Self::normalize_eol),
                ));
            }
        }

        if candidates_to_write.is_empty() && converged_to_record.is_empty() {
            return PullResult::Success { applied_files: 0 };
        }

        // Pass 2: Write files and update ancestor state
        let mut project_state = match ProjectState::load_from_dir(local_root) {
            Ok(state) => state,
            Err(reason) => return state_pull_failure(reason),
        };
        let mut applied = 0;

        for (rel, target, content_bytes, ancestor_hash, ancestor_content) in candidates_to_write {
            if let Err(reason) = validate_safe_write_target(local_root, &rel) {
                return PullResult::IoFailed { path: rel, reason };
            }

            if let Some(parent) = target.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                return PullResult::IoFailed {
                    path: rel,
                    reason: format!("Failed to create parent directory: {e}"),
                };
            }

            if let Err(e) = fs::write(&target, &content_bytes) {
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

            project_state.record_file(&rel, ancestor_hash, ancestor_content);
            applied += 1;
        }

        let mut reconciled = false;
        for (rel, hash, content) in converged_to_record {
            project_state.record_file(&rel, hash, content);
            reconciled = true;
        }

        if (applied > 0 || reconciled)
            && let Err(e) = project_state.save_to_dir(local_root)
        {
            return PullResult::IoFailed {
                path: PathBuf::from(".ude/state.json"),
                reason: format!("Failed to persist state: {e}"),
            };
        }

        PullResult::Success {
            applied_files: applied,
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
        persist_ancestor_immediately: bool,
    ) -> PushResult {
        let dummy_ctx =
            crate::template::TemplateContext::detect_for_project(local_root, "project", "generic");
        let states = match self.scan_project(local_root, upstream_root, Some(&dummy_ctx)) {
            Ok(states) => states,
            Err(reason) => return state_push_failure(reason),
        };
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

                let raw_bytes = match fs::read(&local_path) {
                    Ok(b) => b,
                    Err(err) => {
                        return PushResult::IoFailed {
                            path: state.relative_path.clone(),
                            reason: format!("Failed to read local file: {err}"),
                        };
                    }
                };

                let upstream_target = map_to_upstream(upstream_root, &state.relative_path);
                let (parameterized_bytes, raw_local_str) =
                    if let Ok(content) = std::str::from_utf8(&raw_bytes) {
                        // Gate 2: Secret check (reject credentials)
                        match self.sanitizer.check_secrets(content) {
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
                            .parameterize_paths(content, user_home, xdg_config);
                        // Keep an existing template's line endings; a new template
                        // is published as the local text is.
                        let published =
                            match upstream_target.as_deref().and_then(existing_text_eol_style) {
                                Some(style) => {
                                    apply_eol_style(&Self::normalize_eol(&parameterized), style)
                                }
                                None => parameterized,
                            };
                        (published.into_bytes(), Some(Self::normalize_eol(content)))
                    } else {
                        (raw_bytes.clone(), None)
                    };

                // Stage into candidates list
                if let Some(target_upstream) = upstream_target {
                    candidates_to_write.push((
                        state.relative_path.clone(),
                        target_upstream,
                        parameterized_bytes,
                        raw_bytes,
                        raw_local_str,
                    ));
                }
            }
        }

        // Pass 2: Transactional materialization of validated files
        let mut synced = 0;
        let mut staged_files = Vec::new();
        let mut updated_records = Vec::new();

        for (rel_path, target_upstream, parameterized_bytes, raw_bytes, raw_local_str) in
            candidates_to_write
        {
            if let Some(parent) = target_upstream.parent()
                && let Err(err) = fs::create_dir_all(parent)
            {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to create parent directory: {err}"),
                };
            }
            if let Err(err) = fs::write(&target_upstream, &parameterized_bytes) {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!("Failed to write upstream template: {err}"),
                };
            }

            let local_hash = Self::fingerprint(&raw_bytes);
            staged_files.push(target_upstream.clone());
            updated_records.push((rel_path, local_hash, raw_local_str));
            synced += 1;
        }

        if persist_ancestor_immediately {
            let mut project_state = match ProjectState::load_from_dir(local_root) {
                Ok(state) => state,
                Err(reason) => return state_push_failure(reason),
            };
            for (rel, hash, content) in &updated_records {
                project_state.record_file(rel, hash.clone(), content.clone());
            }
            if let Err(e) = project_state.save_to_dir(local_root) {
                return state_push_failure(format!("Failed to persist state: {e}"));
            }
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
        let mut status_cmd = self.git(Some(remote_checkout));
        status_cmd.args(["status", "--porcelain"]);
        let status = self
            .run_local(status_cmd, "git status")
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
            let mut add = self.git(Some(remote_checkout));
            add.args(["add", "--"]).arg(rel_p);
            let add_out = self
                .run_local(add, "git add")
                .map_err(|e| format!("Failed to stage {}: {e}", rel_p.display()))?;

            if !add_out.status.success() {
                self.reset_index(remote_checkout);
                return Err(with_excerpt(
                    format!("git add failed in remote checkout for {}", rel_p.display()),
                    &add_out,
                ));
            }
        }

        // Inspect staged index: assert that ONLY allowed files are staged
        let mut diff = self.git(Some(remote_checkout));
        diff.args(["diff", "--cached", "--name-only"]);
        let diff_cached = self
            .run_local(diff, "git diff --cached")
            .map_err(|e| format!("Failed to inspect staged diff in remote checkout: {e}"))?;

        let staged_output = String::from_utf8_lossy(&diff_cached.stdout);
        let expected_set: HashSet<PathBuf> = rel_staged_paths.into_iter().collect();

        for line in staged_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let p = PathBuf::from(trimmed);
            if !expected_set.contains(&p) {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: unvalidated file staged in remote checkout: {}",
                    p.display()
                ));
            }

            // Defense in depth: Check staged git index blob bytes for secrets
            let show_arg = format!(":{}", p.to_string_lossy());
            let mut show = self.git(Some(remote_checkout));
            show.args(["show", &show_arg]);
            let blob_res = self
                .run_local(show, "git show")
                .map_err(|e| format!("Failed to read staged git blob for {}: {e}", p.display()))?;

            if !blob_res.status.success() {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: failed to inspect staged git blob for {}",
                    p.display()
                ));
            }

            let blob_content = String::from_utf8_lossy(&blob_res.stdout);
            if let SanitizationResult::SecretDetected { reason, .. } =
                self.sanitizer.check_secrets(&blob_content)
            {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: secret detected in staged index blob {}: {reason}",
                    p.display()
                ));
            }
        }

        let commit_msg = "chore(sync): synchronize environment refinements from local workstation";

        if open_pr {
            let epoch_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let sync_branch = format!(
                "ude-sync-{}-{}",
                &Self::compute_hash(commit_msg.as_bytes())[..8],
                epoch_secs
            );
            let mut create_branch = self.git(Some(remote_checkout));
            create_branch.args(["checkout", "-b", &sync_branch]);
            let branch_out = self
                .run_local(create_branch, "git checkout -b")
                .map_err(|e| format!("Failed to create sync branch {sync_branch}: {e}"))?;

            if !branch_out.status.success() {
                return Err(with_excerpt(
                    format!("git checkout -b {sync_branch} failed"),
                    &branch_out,
                ));
            }

            let commit_out = self
                .run_local(
                    self.commit_command(remote_checkout, commit_msg),
                    "git commit",
                )
                .map_err(|e| format!("Failed to commit in remote checkout: {e}"))?;

            if !commit_out.status.success() {
                self.abandon_sync_branch(remote_checkout, branch, &sync_branch);
                return Err(with_excerpt(
                    "git commit failed in remote checkout".to_string(),
                    &commit_out,
                ));
            }

            if let Err(err) = self.validate_committed_tree(remote_checkout, &expected_set) {
                self.rollback_commit(remote_checkout);
                self.abandon_sync_branch(remote_checkout, branch, &sync_branch);
                return Err(err);
            }

            let mut push = self.git(Some(remote_checkout));
            push.args(["push", "-u", "origin", &sync_branch]);
            let push_out = self
                .run_network(push, "git push")
                .map_err(|e| format!("Failed to push sync branch: {e}"))?;

            // Return working tree to canonical branch
            self.git_best_effort(remote_checkout, &["checkout", branch], "git checkout");

            if !push_out.status.success() {
                return Err(with_excerpt(
                    format!("git push failed for branch {sync_branch}"),
                    &push_out,
                ));
            }

            // Attempt to create PR via gh CLI if available
            let mut pr = self.tool("gh", Some(remote_checkout));
            pr.args([
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
            ]);

            match self.run_network(pr, "gh pr create") {
                Ok(output) if output.status.success() => {
                    let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    Ok(format!("Opened GitHub Pull Request: {pr_url}"))
                }
                _ => Ok(format!(
                    "Pushed changes to branch '{sync_branch}'. Create a PR on GitHub to merge into '{branch}'."
                )),
            }
        } else {
            let commit_out = self
                .run_local(
                    self.commit_command(remote_checkout, commit_msg),
                    "git commit",
                )
                .map_err(|e| format!("Failed to commit in remote checkout: {e}"))?;

            if !commit_out.status.success() {
                return Err(with_excerpt(
                    "git commit failed in remote checkout".to_string(),
                    &commit_out,
                ));
            }

            if let Err(err) = self.validate_committed_tree(remote_checkout, &expected_set) {
                self.rollback_commit(remote_checkout);
                return Err(err);
            }

            let mut push = self.git(Some(remote_checkout));
            push.args(["push", "origin", &format!("HEAD:refs/heads/{branch}")]);
            let push_out = self
                .run_network(push, "git push")
                .map_err(|e| format!("Failed to push to remote: {e}"))?;

            if !push_out.status.success() {
                return Err(with_excerpt(
                    format!("git push origin {branch} failed"),
                    &push_out,
                ));
            }

            Ok(format!(
                "Directly pushed changes to remote branch '{branch}'."
            ))
        }
    }

    /// The cache-checkout commit command (no hooks, no signing).
    fn commit_command(&self, repo: &Path, message: &str) -> Command {
        let mut commit = self.git(Some(repo));
        commit.args(["commit", "--no-verify", "--no-gpg-sign", "-m", message]);
        commit
    }

    /// Runs a cleanup git command in `repo` on the local tier and ignores the result.
    fn git_best_effort(&self, repo: &Path, args: &[&str], label: &str) {
        let mut cmd = self.git(Some(repo));
        cmd.args(args);
        let _ = self.run_local(cmd, label);
    }

    /// Best-effort unstaging of everything in `repo`'s index.
    fn reset_index(&self, repo: &Path) {
        self.git_best_effort(repo, &["reset", "HEAD", "--"], "git reset");
    }

    /// Best-effort return to `branch` and deletion of the abandoned `sync_branch`.
    fn abandon_sync_branch(&self, repo: &Path, branch: &str, sync_branch: &str) {
        self.git_best_effort(repo, &["checkout", branch], "git checkout");
        self.git_best_effort(repo, &["branch", "-D", sync_branch], "git branch -D");
    }

    /// Rolls back a committed HEAD revision in the remote checkout repository cleanly.
    fn rollback_commit(&self, repo: &Path) {
        let mut verify = self.git(Some(repo));
        verify.args(["rev-parse", "--verify", "HEAD~1"]);
        let has_parent = self
            .run_local(verify, "git rev-parse")
            .map(|out| out.status.success())
            .unwrap_or(false);

        if has_parent {
            self.git_best_effort(repo, &["reset", "--hard", "HEAD~1"], "git reset --hard");
        } else {
            self.git_best_effort(repo, &["update-ref", "-d", "HEAD"], "git update-ref");
            self.git_best_effort(repo, &["reset", "--hard"], "git reset --hard");
            self.git_best_effort(repo, &["clean", "-fd"], "git clean");
        }
    }

    /// Validates the committed tree in HEAD before pushing to remote repository,
    /// asserting that no unexpected files or secret credentials exist in the committed
    /// tree. `paths` is the validated allowlist, relative to `repo`.
    fn validate_committed_tree(&self, repo: &Path, paths: &HashSet<PathBuf>) -> Result<(), String> {
        let mut diff_tree = self.git(Some(repo));
        diff_tree.args([
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            "HEAD",
        ]);
        let diff_tree = self.run_local(diff_tree, "git diff-tree").map_err(|e| {
            format!("Failed to inspect committed diff-tree in remote checkout: {e}")
        })?;

        if !diff_tree.status.success() {
            return Err("git diff-tree failed in remote checkout".to_string());
        }

        let committed_output = String::from_utf8_lossy(&diff_tree.stdout);
        for line in committed_output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let p = PathBuf::from(trimmed);
            if !paths.contains(&p) {
                return Err(format!(
                    "Post-commit publication gate violation: unvalidated file committed in HEAD: {}",
                    p.display()
                ));
            }

            let show_arg = format!("HEAD:{}", p.to_string_lossy());
            let mut show = self.git(Some(repo));
            show.args(["show", &show_arg]);
            let blob_res = self
                .run_local(show, "git show")
                .map_err(|e| format!("Failed to read committed blob for {}: {e}", p.display()))?;

            if !blob_res.status.success() {
                return Err(format!(
                    "Post-commit publication gate violation: failed to inspect committed blob for {}",
                    p.display()
                ));
            }

            let blob_content = String::from_utf8_lossy(&blob_res.stdout);
            if let SanitizationResult::SecretDetected { reason, .. } =
                self.sanitizer.check_secrets(&blob_content)
            {
                return Err(format!(
                    "Post-commit publication gate violation: secret detected in committed HEAD blob {}: {reason}",
                    p.display()
                ));
            }
        }

        Ok(())
    }
}

/// Appends the redacted stderr excerpt of a failed git/gh run to `message`.
fn with_excerpt(message: String, out: &RunOutput) -> String {
    let excerpt = process::stderr_excerpt(out);
    if excerpt.is_empty() {
        message
    } else {
        format!("{message}: {excerpt}")
    }
}

/// Sanitizes a git branch name into a filesystem-safe slug, preventing directory traversal
/// or uncreated parent folder failures when branch names contain slashes (e.g. `feature/my-branch`).
pub fn sanitize_branch_slug(branch: &str) -> String {
    let slug: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Pull outcome for an error loading or saving `.ude/state.json`.
fn state_pull_failure(reason: String) -> PullResult {
    PullResult::IoFailed {
        path: PathBuf::from(STATE_FILE_REL),
        reason,
    }
}

/// Push outcome for an error loading or saving `.ude/state.json`.
fn state_push_failure(reason: String) -> PushResult {
    PushResult::IoFailed {
        path: PathBuf::from(STATE_FILE_REL),
        reason,
    }
}

/// Validates that writing to a relative path within local_root is safe and does not escape or write through symlinks.
fn validate_safe_write_target(local_root: &Path, rel_path: &Path) -> Result<PathBuf, String> {
    for comp in rel_path.components() {
        match comp {
            std::path::Component::Normal(_) => {}
            _ => {
                return Err(format!(
                    "Unsafe path component in relative path: {}",
                    rel_path.display()
                ));
            }
        }
    }

    let target = local_root.join(rel_path);

    // Reject if target already exists as a symlink
    if let Ok(meta) = fs::symlink_metadata(&target)
        && meta.file_type().is_symlink()
    {
        return Err(format!("Target path is a symlink: {}", target.display()));
    }

    // Reject if any existing parent directory is a symlink or escapes local_root
    let canonical_root = fs::canonicalize(local_root)
        .map_err(|e| format!("Failed to canonicalize local root: {e}"))?;

    let mut curr = target.parent();
    while let Some(dir) = curr {
        if dir.exists() {
            let canonical_dir = fs::canonicalize(dir)
                .map_err(|e| format!("Failed to canonicalize parent {}: {e}", dir.display()))?;
            if !canonical_dir.starts_with(&canonical_root) {
                return Err(format!(
                    "Target parent directory escapes project root: {}",
                    dir.display()
                ));
            }
            if let Ok(meta) = fs::symlink_metadata(dir)
                && meta.file_type().is_symlink()
            {
                return Err(format!("Parent directory is a symlink: {}", dir.display()));
            }
            break;
        }
        curr = dir.parent();
    }

    Ok(target)
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
    use std::ffi::{OsStr, OsString};
    use std::process::{Output, Stdio};
    use tempfile::tempdir;

    const FIXTURE_GITCONFIG: &str =
        "[user]\n\tname = UDE Tester\n\temail = tester@example.com\n[commit]\n\tgpgsign = false\n";

    /// Hermetic git runner for test fixtures. Commands ignore inherited repo-local
    /// variables, read only the fixture gitconfig (no host identity, no system
    /// config), never prompt, and `run` / `output` panic when a step fails.
    struct GitFixture {
        _home: tempfile::TempDir,
        gitconfig: PathBuf,
    }

    impl GitFixture {
        fn new() -> Self {
            let home = tempdir().expect("create fixture gitconfig dir");
            let gitconfig = home.path().join("gitconfig");
            fs::write(&gitconfig, FIXTURE_GITCONFIG).expect("write fixture gitconfig");
            Self {
                _home: home,
                gitconfig,
            }
        }

        fn cmd(&self, dir: &Path) -> Command {
            let mut cmd = Command::new("git");
            cmd.current_dir(dir);
            for name in process::GIT_LOCAL_ENV_VARS {
                cmd.env_remove(name);
            }
            cmd.env("GIT_CONFIG_GLOBAL", &self.gitconfig)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null());
            cmd
        }

        fn run(&self, dir: &Path, args: &[&str]) {
            self.checked(dir, args);
        }

        fn output(&self, dir: &Path, args: &[&str]) -> String {
            let out = self.checked(dir, args);
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        fn checked(&self, dir: &Path, args: &[&str]) -> Output {
            let out = self.cmd(dir).args(args).output().unwrap_or_else(|e| {
                panic!("fixture step `git {}` did not start: {e}", args.join(" "))
            });
            assert!(
                out.status.success(),
                "fixture step `git {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            out
        }

        /// Global-config variables for code under test that spawns its own git.
        fn gitconfig_env(&self) -> Vec<(OsString, OsString)> {
            vec![
                ("GIT_CONFIG_GLOBAL".into(), self.gitconfig.clone().into()),
                ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ]
        }
    }

    #[test]
    fn test_git_fixture_scrubs_local_env() {
        let fixture = GitFixture::new();
        let dir = tempdir().unwrap();
        let cmd = fixture.cmd(dir.path());
        let envs: std::collections::HashMap<_, _> = cmd.get_envs().collect();
        for name in process::GIT_LOCAL_ENV_VARS {
            assert_eq!(
                envs.get(OsStr::new(name)),
                Some(&None),
                "{name} must be removed from fixture git commands"
            );
        }
        let gitconfig = fixture.gitconfig.as_os_str();
        assert_eq!(
            envs.get(OsStr::new("GIT_CONFIG_NOSYSTEM")),
            Some(&Some(OsStr::new("1")))
        );
        assert_eq!(
            envs.get(OsStr::new("GIT_CONFIG_GLOBAL")),
            Some(&Some(gitconfig))
        );
        for (key, value) in fixture.gitconfig_env() {
            assert_eq!(
                envs.get(key.as_os_str()),
                Some(&Some(value.as_os_str())),
                "gitconfig_env disagrees with cmd"
            );
        }

        // The fixture gitconfig is what git actually reads: the identity comes from it.
        fixture.run(dir.path(), &["init", "-b", "main"]);
        assert_eq!(
            fixture.output(dir.path(), &["config", "user.name"]),
            "UDE Tester"
        );

        // Negative control: a plain git Command carries none of these entries.
        let plain = Command::new("git");
        let plain_envs: Vec<_> = plain.get_envs().collect();
        for name in process::GIT_LOCAL_ENV_VARS
            .iter()
            .chain(&["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM"])
        {
            assert!(
                plain_envs.iter().all(|(key, _)| *key != OsStr::new(name)),
                "plain git Command unexpectedly configures {name}"
            );
        }
    }

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
            SyncEngine::compare_3way(None, Some("local edit"), Some("upstream edit"), false,),
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
            SyncEngine::compare_3way(Some("base version"), None, Some("base version"), false,),
            SyncStatus::LocallyModified
        );

        // Upstream deleted, local untouched
        assert_eq!(
            SyncEngine::compare_3way(Some("base version"), Some("base version"), None, false,),
            SyncStatus::UpstreamModified
        );

        // Conflicting deletion: local deleted, upstream modified
        assert_eq!(
            SyncEngine::compare_3way(Some("base version"), None, Some("upstream modified"), false,),
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
    fn test_fingerprint() {
        let fp = SyncEngine::fingerprint;
        assert_eq!(
            fp(b"a\r\nb\r\n"),
            fp(b"a\nb\n"),
            "CRLF and LF renderings of the same text must share a fingerprint"
        );

        // Negative controls: real differences keep distinct fingerprints.
        assert_ne!(fp(b"a\nb\n"), fp(b"a\nc\n"));
        assert_ne!(fp(b"a\rb"), fp(b"a\nb"), "a lone CR is content");
        let invalid_crlf = [0xff, b'\r', b'\n'];
        let invalid_lf = [0xff, b'\n'];
        assert_ne!(
            fp(&invalid_crlf),
            fp(&invalid_lf),
            "non-UTF-8 bytes must not be normalized"
        );
        assert_eq!(fp(&invalid_crlf), SyncEngine::compute_hash(&invalid_crlf));
    }

    proptest::proptest! {
        #[test]
        fn test_fingerprint_crlf_lf_equivalence_prop(
            lines in proptest::collection::vec(r"[^\r\n]{0,16}", 0..=8)
        ) {
            let lf = lines.join("\n");
            let crlf = lines.join("\r\n");
            proptest::prop_assert_eq!(
                SyncEngine::fingerprint(crlf.as_bytes()),
                SyncEngine::fingerprint(lf.as_bytes())
            );
            // CR-free text is already canonical: its fingerprint is the raw hash.
            proptest::prop_assert_eq!(
                SyncEngine::fingerprint(lf.as_bytes()),
                SyncEngine::compute_hash(lf.as_bytes())
            );
            // Negative control: a real content change moves the fingerprint.
            let extended = format!("{lf}x");
            proptest::prop_assert_ne!(
                SyncEngine::fingerprint(extended.as_bytes()),
                SyncEngine::fingerprint(lf.as_bytes())
            );
        }
    }

    /// Status that `scan_project` reports for `rel`.
    fn scan_status(engine: &SyncEngine, local: &Path, upstream: &Path, rel: &str) -> SyncStatus {
        engine
            .scan_project(local, upstream, None)
            .unwrap()
            .into_iter()
            .find(|s| s.relative_path == Path::new(rel))
            .unwrap_or_else(|| panic!("{rel} is not a managed file"))
            .status
    }

    /// Writes `content` as the upstream template for `.gitconfig`.
    fn write_upstream_gitconfig(upstream: &Path, content: &[u8]) {
        let path = upstream.join("projects/generic/gitconfig.repo");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_scan_crlf_only_difference_in_sync() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let lf = "[core]\n  autocrlf = input\n";

        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            SyncEngine::fingerprint(lf.as_bytes()),
            Some(lf.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();
        write_upstream_gitconfig(upstream_dir.path(), lf.as_bytes());

        let local = local_dir.path().join(".gitconfig");
        fs::write(&local, lf.replace('\n', "\r\n")).unwrap();
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::InSync,
            "a CRLF-only difference must not register as drift"
        );

        // Negative control: a real edit in the CRLF file is still a local modification.
        fs::write(&local, lf.replace("input", "false").replace('\n', "\r\n")).unwrap();
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::LocallyModified
        );
    }

    #[test]
    fn test_scan_after_v1_migration_in_sync() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let lf = "[core]\n  autocrlf = input\n";
        let crlf = lf.replace('\n', "\r\n");

        // A version 1 ledger: raw-byte hash and verbatim CRLF content.
        let v1 = serde_json::json!({
            "version": 1,
            "files": { ".gitconfig": {
                "ancestor_hash": SyncEngine::compute_hash(crlf.as_bytes()),
                "last_synced_at": null,
                "ancestor_content": crlf,
            }},
        });
        let state_path = local_dir.path().join(STATE_FILE_REL);
        fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        fs::write(&state_path, v1.to_string()).unwrap();
        fs::write(local_dir.path().join(".gitconfig"), &crlf).unwrap();
        write_upstream_gitconfig(upstream_dir.path(), lf.as_bytes());

        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::InSync
        );

        // The migrated ancestor is what the local copy is compared against: with only
        // upstream edited, the unchanged CRLF local copy must not count as a local edit.
        let edited = lf.replace("input", "false");
        write_upstream_gitconfig(upstream_dir.path(), edited.as_bytes());
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::UpstreamModified
        );

        // The next save persists version 2 with fingerprint semantics.
        ProjectState::load_from_dir(local_dir.path())
            .unwrap()
            .save_to_dir(local_dir.path())
            .unwrap();
        let saved = fs::read_to_string(&state_path).unwrap();
        assert!(saved.contains("\"version\": 2"), "saved state was: {saved}");
        assert!(
            saved.contains(&SyncEngine::fingerprint(lf.as_bytes())),
            "saved state was: {saved}"
        );
    }

    #[test]
    fn test_eol_detection() {
        assert_eq!(detect_eol_style(b"a\r\nb\r\n"), EolStyle::Crlf);
        assert_eq!(detect_eol_style(b"\r\n"), EolStyle::Crlf);
        assert_eq!(
            detect_eol_style(b"a\r\nb\r\nc\n"),
            EolStyle::Crlf,
            "CRLF pairs outnumber bare LF"
        );

        assert_eq!(detect_eol_style(b"a\nb\n"), EolStyle::Lf);
        assert_eq!(detect_eol_style(b"a\r\nb\n"), EolStyle::Lf, "a tie is LF");
        assert_eq!(detect_eol_style(b"no newline"), EolStyle::Lf);
        assert_eq!(detect_eol_style(b""), EolStyle::Lf);
        assert_eq!(detect_eol_style(b"\n"), EolStyle::Lf);
        assert_eq!(
            detect_eol_style(b"a\rb\r"),
            EolStyle::Lf,
            "lone CRs are not line endings"
        );
    }

    proptest::proptest! {
        #[test]
        fn test_eol_style_roundtrip_prop(
            lines in proptest::collection::vec(r"[^\r\n]{0,16}", 0..=8)
        ) {
            let text = lines.join("\n");
            let crlf = apply_eol_style(&text, EolStyle::Crlf);
            let lf = apply_eol_style(&text, EolStyle::Lf);
            proptest::prop_assert_eq!(SyncEngine::normalize_eol(&crlf), text.clone());
            proptest::prop_assert_eq!(&lf, &text);
            proptest::prop_assert!(!lf.contains('\r'), "an LF file gained a CR");
            if text.contains('\n') {
                proptest::prop_assert_eq!(detect_eol_style(crlf.as_bytes()), EolStyle::Crlf);
                // Negative control: the LF rendering is detected as LF.
                proptest::prop_assert_eq!(detect_eol_style(text.as_bytes()), EolStyle::Lf);
            }
        }
    }

    /// `text` with every LF turned into CRLF.
    fn crlf(text: &str) -> String {
        text.replace('\n', "\r\n")
    }

    /// Pulls `.gitconfig` with the given ancestor (recorded LF), local copy and upstream
    /// template, asserts the status that follows, and returns the local file's bytes.
    fn pull_gitconfig(
        ancestor: Option<&str>,
        local: Option<&str>,
        upstream: &str,
        status_after: SyncStatus,
    ) -> Vec<u8> {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        if let Some(base) = ancestor {
            let mut state = ProjectState::default();
            state.record_file(
                Path::new(".gitconfig"),
                SyncEngine::fingerprint(base.as_bytes()),
                Some(base.to_string()),
            );
            state.save_to_dir(local_dir.path()).unwrap();
        }
        let local_path = local_dir.path().join(".gitconfig");
        if let Some(text) = local {
            fs::write(&local_path, text).unwrap();
        }
        write_upstream_gitconfig(upstream_dir.path(), upstream.as_bytes());
        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        assert_eq!(
            engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx),
            PullResult::Success { applied_files: 1 }
        );
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            status_after
        );
        fs::read(&local_path).unwrap()
    }

    #[test]
    fn test_pull_preserves_eol_style() {
        let base = "[core]\n  autocrlf = input\n  editor = vim\n";
        let new = "[core]\n  autocrlf = input\n  editor = nano\n";

        let written = pull_gitconfig(Some(base), Some(&crlf(base)), new, SyncStatus::InSync);
        assert_eq!(
            String::from_utf8(written).unwrap(),
            crlf(new),
            "pull into a CRLF file must write CRLF"
        );

        // Negative control: an LF local file never gains a CR.
        let written = pull_gitconfig(Some(base), Some(base), new, SyncStatus::InSync);
        assert!(!written.contains(&b'\r'), "an LF file gained a CR");
        assert_eq!(written, new.as_bytes());

        // A file absent locally is written exactly as upstream renders it.
        let written = pull_gitconfig(None, None, &crlf(new), SyncStatus::InSync);
        assert_eq!(written, crlf(new).as_bytes());
    }

    #[test]
    fn test_pull_merged_crlf_keeps_crlf() {
        let base = "line1\nline2\nline3\nline4\nline5\n";
        let local = "LOCAL1\nline2\nline3\nline4\nline5\n";
        let upstream = "line1\nline2\nline3\nUPSTREAM4\nline5\n";
        let merged = "LOCAL1\nline2\nline3\nUPSTREAM4\nline5\n";

        // A merged file keeps the local edit, so it reads as locally modified afterwards.
        let written = pull_gitconfig(
            Some(base),
            Some(&crlf(local)),
            upstream,
            SyncStatus::LocallyModified,
        );
        assert_eq!(
            String::from_utf8(written).unwrap(),
            crlf(merged),
            "a merge into a CRLF file must write CRLF"
        );

        // Negative control: an LF local file gets the merge with LF only.
        let written = pull_gitconfig(
            Some(base),
            Some(local),
            upstream,
            SyncStatus::LocallyModified,
        );
        assert!(!written.contains(&b'\r'), "an LF file gained a CR");
        assert_eq!(written, merged.as_bytes());
    }

    /// Pushes a locally edited `.gitconfig` (`ancestor` recorded LF when given) over
    /// the upstream template `template` (absent when `None`) and returns the template.
    fn push_gitconfig(ancestor: Option<&str>, template: Option<&str>, local: &str) -> Vec<u8> {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        if let Some(base) = ancestor {
            let mut state = ProjectState::default();
            state.record_file(
                Path::new(".gitconfig"),
                SyncEngine::fingerprint(base.as_bytes()),
                Some(base.to_string()),
            );
            state.save_to_dir(local_dir.path()).unwrap();
        }
        if let Some(text) = template {
            write_upstream_gitconfig(upstream_dir.path(), text.as_bytes());
        }
        fs::write(local_dir.path().join(".gitconfig"), local).unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
            true,
        );
        match res {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 1),
            other => panic!("Expected Success on push, got {other:?}"),
        }
        fs::read(upstream_dir.path().join("projects/generic/gitconfig.repo")).unwrap()
    }

    #[test]
    fn test_push_preserves_upstream_eol_style() {
        let base = "[core]\n  autocrlf = input\n  editor = vim\n";
        let edited = "[core]\n  autocrlf = input\n  editor = nano\n";

        let template = push_gitconfig(Some(base), Some(base), &crlf(edited));
        assert!(!template.contains(&b'\r'), "an LF template gained a CR");
        assert_eq!(template, edited.as_bytes());

        // Negative control: a CRLF template stays CRLF when an LF local edit is pushed.
        let template = push_gitconfig(Some(base), Some(&crlf(base)), edited);
        assert_eq!(
            String::from_utf8(template).unwrap(),
            crlf(edited),
            "a CRLF template must stay CRLF"
        );

        // A template absent upstream is published as the local text is.
        let template = push_gitconfig(None, None, &crlf(edited));
        assert_eq!(template, crlf(edited).as_bytes());
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

        let up_hook = upstream_dir
            .path()
            .join("projects/generic/hooks/pre-commit");
        fs::create_dir_all(up_hook.parent().unwrap()).unwrap();
        fs::write(&up_hook, "#!/bin/sh\nexit 0\n").unwrap();

        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(
            &up_mise,
            "[tools]\nrust = \"1.85.0\"\nhome = \"{{ user.home }}\"\n",
        )
        .unwrap();

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
        assert_eq!(
            fs::read_to_string(&loc_gitconfig).unwrap(),
            "[core]\n  autocrlf = input\n"
        );

        let loc_hook = local_dir.path().join("scripts/hooks/pre-commit");
        assert!(loc_hook.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&loc_hook).unwrap();
            assert_eq!(
                meta.permissions().mode() & 0o111,
                0o111,
                "Hook must be executable"
            );
        }

        let loc_mise = local_dir.path().join("mise.toml");
        assert!(loc_mise.exists());
        let mise_content = fs::read_to_string(&loc_mise).unwrap();
        assert!(mise_content.contains(&ctx.user.home));

        // Verify state persistence
        let state = ProjectState::load_from_dir(local_dir.path()).unwrap();
        assert!(state.get_ancestor_hash(Path::new(".gitconfig")).is_some());
        assert!(
            state
                .get_ancestor_hash(Path::new("scripts/hooks/pre-commit"))
                .is_some()
        );
        assert!(state.get_ancestor_hash(Path::new("mise.toml")).is_some());

        // Negative control: Re-scanning now must report InSync for all 3 files
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        for s in states {
            if s.relative_path == Path::new(".gitconfig")
                || s.relative_path == Path::new("scripts/hooks/pre-commit")
                || s.relative_path == Path::new("mise.toml")
            {
                assert_eq!(
                    s.status,
                    SyncStatus::InSync,
                    "File {:?} must be InSync after pull",
                    s.relative_path
                );
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
        state.record_file(
            Path::new(".gitconfig"),
            base_hash,
            Some(base_content.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();

        // Local has modified .gitconfig
        let loc_gitconfig = local_dir.path().join(".gitconfig");
        fs::write(&loc_gitconfig, "[core]\n  autocrlf = local_divergence\n").unwrap();

        // Upstream has ALSO modified .gitconfig
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, "[core]\n  autocrlf = upstream_divergence\n").unwrap();

        // Upstream also has a clean file that should NOT be written due to the conflict
        let up_hook = upstream_dir
            .path()
            .join("projects/generic/hooks/pre-commit");
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
        state.record_file(
            Path::new(".gitconfig"),
            base_hash,
            Some(base_content.to_string()),
        );
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

        let written_upstream = upstream_dir.path().join("projects/generic/gitconfig.repo");
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
        fs::write(&local_gitconfig, format!("export TOKEN={prefix}{body}\n")).unwrap();

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
        let upstream_gitconfig =
            map_to_upstream(upstream_dir.path(), Path::new(".gitconfig")).unwrap();
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
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let cache_dir = tempdir().unwrap();
        let fake_remote = tempdir().unwrap();

        // Initialize a fake remote git repository with a templates/ directory
        git.run(fake_remote.path(), &["init", "-b", "main"]);

        let fake_templates = fake_remote.path().join("templates");
        fs::create_dir_all(&fake_templates).unwrap();
        fs::write(fake_templates.join("test.txt"), "remote template content").unwrap();

        git.run(fake_remote.path(), &["add", "-A"]);
        git.run(fake_remote.path(), &["commit", "-m", "init fake remote"]);

        let remote_url = fake_remote.path().to_string_lossy().to_string();
        let (resolved, _lock1) = engine
            .ensure_remote_repo(&remote_url, "main", cache_dir.path())
            .expect("remote cache resolution");

        assert!(resolved.exists());
        assert!(resolved.join("test.txt").exists());

        // Test branch switching isolation: create develop branch with distinct file
        git.run(fake_remote.path(), &["checkout", "-b", "develop"]);
        fs::write(
            fake_templates.join("develop.txt"),
            "develop branch template",
        )
        .unwrap();
        git.run(fake_remote.path(), &["add", "-A"]);
        git.run(fake_remote.path(), &["commit", "-m", "develop commit"]);
        git.run(fake_remote.path(), &["checkout", "main"]);

        let (resolved_develop, _lock2) = engine
            .ensure_remote_repo(&remote_url, "develop", cache_dir.path())
            .expect("remote cache resolution on develop branch");
        assert!(resolved_develop.exists());
        assert!(resolved_develop.join("develop.txt").exists());

        drop(_lock1);
        // Switching back to main must still work
        let (resolved_main, _lock3) = engine
            .ensure_remote_repo(&remote_url, "main", cache_dir.path())
            .expect("remote cache resolution on main branch");
        assert!(resolved_main.exists());
        assert!(resolved_main.join("test.txt").exists());
    }

    #[test]
    fn test_remote_lock_concurrency_serialization_and_timeout() {
        let temp = tempdir().unwrap();
        let lock_path = temp.path().join("concurrency.lock");

        // 1. Initial lock acquisition
        let guard1 = RemoteLockGuard::acquire(&lock_path, 1).expect("initial lock acquisition");

        // 2. Contended lock acquisition with zero-second / fast timeout -> MUST fail with timeout!
        let start = std::time::Instant::now();
        let err2 = RemoteLockGuard::acquire(&lock_path, 0);
        assert!(
            err2.is_err(),
            "Contended lock acquisition must fail when locked by another thread!"
        );
        let err_msg = err2.unwrap_err();
        assert!(
            err_msg.contains("Timed out") || err_msg.contains("waiting for remote cache lock"),
            "Error was: {err_msg}"
        );
        assert!(start.elapsed() < std::time::Duration::from_millis(500));

        // 3. Drop guard1, then acquire again -> MUST succeed
        drop(guard1);
        let guard3 = RemoteLockGuard::acquire(&lock_path, 1);
        assert!(
            guard3.is_ok(),
            "Lock acquisition must succeed after prior guard is dropped!"
        );
    }

    /// A child spawned as the leader of its own process group. Drop kills the whole
    /// group with the `kill` binary and reaps the child, so a failed assertion still
    /// cleans up (including grandchildren such as flock's `sleep`).
    #[cfg(target_os = "linux")]
    struct GroupGuard {
        child: std::process::Child,
        killed: bool,
    }

    #[cfg(target_os = "linux")]
    impl GroupGuard {
        fn spawn(mut cmd: Command) -> Self {
            use std::os::unix::process::CommandExt;
            let child = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .expect("spawn process group leader");
            Self {
                child,
                killed: false,
            }
        }

        /// SIGKILLs the group (its PGID is the leader's PID) and reaps the leader.
        fn kill(&mut self) -> std::process::ExitStatus {
            let pgid = self.child.id();
            let status = Command::new("kill")
                .args(["-KILL", "--", &format!("-{pgid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("run the kill binary");
            self.killed = true;
            let _ = self.child.wait();
            status
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for GroupGuard {
        fn drop(&mut self) {
            if !self.killed {
                let _ = self.kill();
            }
        }
    }

    /// A direct child; Drop stops it with `Child::kill` and reaps it.
    #[cfg(target_os = "linux")]
    struct ChildGuard(std::process::Child);

    #[cfg(target_os = "linux")]
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Link targets of every open descriptor of `pid`, from /proc/<pid>/fd.
    #[cfg(target_os = "linux")]
    fn open_fd_targets(pid: u32) -> Vec<PathBuf> {
        fs::read_dir(format!("/proc/{pid}/fd"))
            .expect("list /proc/<pid>/fd")
            .map(|entry| fs::read_link(entry.expect("fd entry").path()).expect("read fd link"))
            .collect()
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_remote_lock_released_when_holder_killed() {
        let flock = which::which("flock")
            .expect("util-linux `flock` not found on PATH; this test needs it as the lock holder");
        let temp = tempdir().unwrap();
        let lock_path = temp.path().join("holder.lock");

        // flock(1) takes the lock, forks `sleep 30` with the lock fd closed (-o) and
        // holds the lock itself until it is killed.
        let mut cmd = Command::new(flock);
        cmd.args(["-o", "-x"]).arg(&lock_path).args(["sleep", "30"]);
        let mut holder = GroupGuard::spawn(cmd);

        // Cross-process contention through flock(2), observed within 2 s.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let err = loop {
            match RemoteLockGuard::acquire(&lock_path, 0) {
                Err(e) => break e,
                Ok(guard) => {
                    drop(guard);
                    assert!(
                        std::time::Instant::now() < deadline,
                        "flock holder never took the lock"
                    );
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        assert!(
            err.contains("waiting for remote cache lock"),
            "unexpected contention error: {err}"
        );

        let status = holder.kill();
        assert!(
            status.success(),
            "kill of the holder's process group failed: {status}"
        );
        let reacquired = RemoteLockGuard::acquire(&lock_path, 2);
        assert!(
            reacquired.is_ok(),
            "lock still held after its holder was killed: {:?}",
            reacquired.err()
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_remote_lock_not_inherited_by_children() {
        use std::os::unix::process::CommandExt;
        let spawn_sleep = |stdin: Stdio| {
            let child = Command::new("sleep")
                .arg("5")
                .stdin(stdin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .expect("spawn sleep");
            ChildGuard(child)
        };
        let temp = tempdir().unwrap();
        let lock_path = temp.path().join("inherit.lock");
        let _guard = RemoteLockGuard::acquire(&lock_path, 1).expect("acquire remote lock");
        let lock_file = fs::canonicalize(&lock_path).unwrap();

        // std's spawn returns after exec, so the child's descriptor table is final.
        let plain = spawn_sleep(Stdio::null());
        assert!(
            !open_fd_targets(plain.0.id()).contains(&lock_file),
            "a spawned child inherited the lock file descriptor"
        );

        // Negative control: a child handed the lock file as stdin is seen holding it.
        let inheriting = spawn_sleep(Stdio::from(fs::File::open(&lock_path).unwrap()));
        assert!(
            open_fd_targets(inheriting.0.id()).contains(&lock_file),
            "the descriptor scan missed an inherited lock file"
        );
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
        state.record_file(
            Path::new(".gitconfig"),
            base_hash.clone(),
            Some(base_content.to_string()),
        );
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
        let current_state = ProjectState::load_from_dir(local_dir.path()).unwrap();
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
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();

        // Initialize fake remote repository
        git.run(remote_repo.path(), &["init", "-b", "main"]);
        let templates_dir = remote_repo.path().join("templates/projects/generic");
        fs::create_dir_all(&templates_dir).unwrap();
        let valid_file = templates_dir.join("gitconfig.repo");
        fs::write(&valid_file, "[core]\n  autocrlf = true\n").unwrap();

        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);

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
        let status_out = git.output(remote_repo.path(), &["status", "--porcelain"]);

        // The dirty file must be strictly untracked ('??') and never committed into HEAD
        assert!(
            status_out.contains("?? templates/unvalidated-secret.txt"),
            "Dirty secret file must remain strictly untracked! Status: {status_out}"
        );

        let ls_tree_out = git.output(
            remote_repo.path(),
            &["ls-tree", "-r", "--name-only", "HEAD"],
        );
        assert!(
            !ls_tree_out.contains("unvalidated-secret.txt"),
            "Dirty secret file was committed into HEAD tree! Trees:\n{ls_tree_out}"
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
        assert!(matches!(
            push_res,
            PushResult::Success {
                synced_files: 1,
                ..
            }
        ));

        // Verify upstream gitconfig.repo has {{ user.home }}
        let up_gitconfig = upstream_dir.path().join("projects/generic/gitconfig.repo");
        let up_content = fs::read_to_string(&up_gitconfig).unwrap();
        assert_eq!(
            up_content,
            "[core]\n  excludesfile = {{ user.home }}/.gitignore\n"
        );

        // 2. Scan: must render {{ user.home }} back to /home/fixture even though extension is .repo, reporting InSync!
        let mut ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );
        ctx.user.home = user_home.to_string();
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let gitconfig_state = states
            .iter()
            .find(|s| s.relative_path == Path::new(".gitconfig"))
            .unwrap();
        assert_eq!(
            gitconfig_state.status,
            SyncStatus::InSync,
            "Static template with {{ user.home }} must render to InSync"
        );
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
        state.record_file(
            Path::new("mise.toml"),
            base_hash,
            Some(base_mise.to_string()),
        );
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
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let mise_state = states
            .iter()
            .find(|s| s.relative_path == Path::new("mise.toml"))
            .unwrap();
        assert_eq!(
            mise_state.status,
            SyncStatus::UpstreamModified,
            "Layered override must NOT block upstream updates to base template"
        );

        // Pull updates: must update mise.toml to 1.86.0 while preserving mise.local.toml
        let pull_res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull_res, PullResult::Success { applied_files: 1 });
        assert_eq!(fs::read_to_string(&loc_mise).unwrap(), updated_upstream);
        assert_eq!(
            fs::read_to_string(&loc_override).unwrap(),
            "[tools]\nrust = \"nightly\"\n"
        );
    }

    #[test]
    fn test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();

        // Initialize fake remote repository
        git.run(remote_repo.path(), &["init", "-b", "main"]);
        let templates_dir = remote_repo.path().join("templates/projects/generic");
        fs::create_dir_all(&templates_dir).unwrap();
        let valid_file = templates_dir.join("gitconfig.repo");
        fs::write(&valid_file, "[core]\n  autocrlf = true\n").unwrap();

        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);

        // Now modify valid_file on disk with clean content
        fs::write(&valid_file, "[core]\n  autocrlf = false\n").unwrap();

        // Simulate a Git clean filter injecting a secret into the staged index blob during git add:
        let prefix = "ghp_";
        let body = "999988887777666655554444333322221111";
        let script = remote_repo.path().join("inject_secret.sh");
        fs::write(
            &script,
            format!("#!/bin/sh\ncat\necho \"secret = {prefix}{body}\"\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let filter_cmd = format!("{}", script.display());
        git.run(
            remote_repo.path(),
            &["config", "filter.injectsecret.clean", &filter_cmd],
        );

        let info_dir = remote_repo.path().join(".git/info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(
            info_dir.join("attributes"),
            "templates/projects/generic/gitconfig.repo filter=injectsecret\n",
        )
        .unwrap();

        // Working tree file has benign content, but staged index blob will have secret via clean filter!
        let staged_allowlist = vec![valid_file.clone()];
        let res = engine.push_to_remote(remote_repo.path(), "main", false, &staged_allowlist);

        // Publication gate must detect secret in the staged blob and reject!
        assert!(
            res.is_err(),
            "Publication gate must fail when staged blob contains secret!"
        );
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("Publication gate violation: secret detected in staged index blob"),
            "Error was: {err_msg}"
        );

        // Verify negative control: git reset HEAD was performed, staging index must be empty
        let diff_cached = git.output(remote_repo.path(), &["diff", "--cached", "--name-only"]);
        assert!(
            diff_cached.is_empty(),
            "Staging area must be reset clean after secret detection violation!"
        );
    }

    #[test]
    fn test_multi_pull_merge_idempotence() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // 1. Base ancestor setup: a=1, b=1
        let base_content = "a=1\nb=1\n";
        let base_hash = SyncEngine::compute_hash(base_content.as_bytes());
        let mut state = ProjectState::default();
        state.record_file(
            Path::new("mise.toml"),
            base_hash,
            Some(base_content.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();

        // 2. Local has line 1 edited: a=2, b=1
        let loc_mise = local_dir.path().join("mise.toml");
        fs::write(&loc_mise, "a=2\nb=1\n").unwrap();

        // 3. Upstream has line 2 edited: a=1, b=2
        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(&up_mise, "a=1\nb=2\n").unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // 4. First pull: orthogonal merge produces a=2, b=2
        let pull1 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull1, PullResult::Success { applied_files: 1 });
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nb=2\n",
            "First pull must merge orthogonal changes cleanly"
        );

        // 5. Second pull: with upstream UNCHANGED, local edit must NOT be erased!
        let pull2 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull2, PullResult::Success { applied_files: 0 });
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nb=2\n",
            "Second pull must preserve local edit and remain idempotent!"
        );

        // 6. Upstream now updates line 2 again to b=3
        fs::write(&up_mise, "a=1\nb=3\n").unwrap();

        // 7. Third pull: merges new upstream change with preserved local edit to produce a=2, b=3
        let pull3 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull3, PullResult::Success { applied_files: 1 });
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nb=3\n",
            "Subsequent pull must merge future upstream updates with preserved local edit!"
        );
    }

    #[test]
    fn test_slash_branch_name_lock_and_cache() {
        assert_eq!(
            sanitize_branch_slug("feature/custom-task"),
            "feature-custom-task"
        );
        assert_eq!(
            sanitize_branch_slug("release/v1.0.0-rc1"),
            "release-v1-0-0-rc1"
        );
        assert_eq!(sanitize_branch_slug("///"), "default");
        assert_eq!(
            sanitize_branch_slug("fix/issue#42/sub_dir"),
            "fix-issue-42-sub_dir"
        );

        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_dir = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();

        // Initialize remote git repo with slash-delimited branch
        git.run(remote_dir.path(), &["init", "-b", "feature/custom-task"]);

        let tmpl_file = remote_dir.path().join("templates/test.txt");
        fs::create_dir_all(tmpl_file.parent().unwrap()).unwrap();
        fs::write(&tmpl_file, "branch content\n").unwrap();

        git.run(remote_dir.path(), &["add", "-A"]);
        git.run(remote_dir.path(), &["commit", "-m", "init slash branch"]);

        // Call ensure_remote_repo: must acquire lock and clone without path nesting failure!
        let res = engine.ensure_remote_repo(
            remote_dir.path().to_str().unwrap(),
            "feature/custom-task",
            cache_dir.path(),
        );
        assert!(
            res.is_ok(),
            "ensure_remote_repo must succeed for slash branch: {:?}",
            res.err()
        );
        let (checkout_tmpl, _lock) = res.unwrap();
        assert!(checkout_tmpl.exists());
    }

    #[test]
    fn test_direct_push_after_pr_mode_resets_branch_and_pushes_target() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare_remote = tempdir().unwrap();
        let cache_dir = tempdir().unwrap();

        // Initialize bare remote
        git.run(bare_remote.path(), &["init", "--bare", "-b", "main"]);

        // Seed initial commit into bare remote
        let seed_dir = tempdir().unwrap();
        git.run(seed_dir.path(), &["init", "-b", "main"]);
        let seed_tmpl = seed_dir
            .path()
            .join("templates/projects/generic/gitconfig.repo");
        fs::create_dir_all(seed_tmpl.parent().unwrap()).unwrap();
        fs::write(&seed_tmpl, "[core]\n  autocrlf = true\n").unwrap();
        git.run(seed_dir.path(), &["add", "-A"]);
        git.run(seed_dir.path(), &["commit", "-m", "initial seed"]);
        let bare_url = bare_remote.path().to_str().unwrap();
        git.run(seed_dir.path(), &["remote", "add", "origin", bare_url]);
        git.run(seed_dir.path(), &["push", "-u", "origin", "main"]);

        // Step 1: Ensure cache checkout of main
        let (tmpl_dir, lock) = engine
            .ensure_remote_repo(
                bare_remote.path().to_str().unwrap(),
                "main",
                cache_dir.path(),
            )
            .unwrap();
        let remote_checkout = tmpl_dir.parent().unwrap();

        // Step 2: Push changes in PR mode (open_pr: true)
        let managed_gitconfig = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig,
            "[core]\n  autocrlf = false\n# pr edit\n",
        )
        .unwrap();
        let staged = vec![managed_gitconfig.clone()];
        let pr_res = engine.push_to_remote(remote_checkout, "main", true, &staged);
        assert!(pr_res.is_ok(), "PR push must succeed: {:?}", pr_res.err());

        // Explicitly release first lock before acquiring lock in Step 3
        drop(lock);

        // Verify remote has a ude-sync branch
        let branch_out = git.output(bare_remote.path(), &["branch", "--list"]);
        assert!(
            branch_out.contains("ude-sync-"),
            "PR branch must be pushed to bare remote: {branch_out}"
        );

        // Step 3: Now run direct push on main (open_pr: false)
        // ensure_remote_repo must cleanly reset the working tree back to main
        let (tmpl_dir_2, _lock_2) = engine
            .ensure_remote_repo(
                bare_remote.path().to_str().unwrap(),
                "main",
                cache_dir.path(),
            )
            .unwrap();
        let remote_checkout_2 = tmpl_dir_2.parent().unwrap();
        let managed_gitconfig_2 = tmpl_dir_2.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig_2,
            "[core]\n  autocrlf = input\n# direct edit\n",
        )
        .unwrap();
        let staged_2 = vec![managed_gitconfig_2.clone()];
        let direct_res = engine.push_to_remote(remote_checkout_2, "main", false, &staged_2);
        assert!(
            direct_res.is_ok(),
            "Direct push must succeed: {:?}",
            direct_res.err()
        );

        // Step 4: Verify bare remote's `main` branch HEAD has the direct edit!
        let main_content = git.output(
            bare_remote.path(),
            &["show", "main:templates/projects/generic/gitconfig.repo"],
        );
        assert!(
            main_content.contains("autocrlf = input"),
            "Bare remote main branch must contain direct commit! Got: {main_content}"
        );
    }

    /// A bare remote seeded with one template on `main` whose pre-receive hook is the
    /// POSIX sh script `hook` (installed after seeding, mode 0o755).
    #[cfg(unix)]
    fn bare_remote_with_pre_receive(git: &GitFixture, hook: &str) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let bare = tempdir().unwrap();
        git.run(bare.path(), &["init", "--bare", "-b", "main"]);
        let seed = tempdir().unwrap();
        git.run(seed.path(), &["init", "-b", "main"]);
        let tmpl = seed
            .path()
            .join("templates/projects/generic/gitconfig.repo");
        fs::create_dir_all(tmpl.parent().unwrap()).unwrap();
        fs::write(&tmpl, "[core]\n  autocrlf = true\n").unwrap();
        git.run(seed.path(), &["add", "-A"]);
        git.run(seed.path(), &["commit", "-m", "seed"]);
        git.run(
            seed.path(),
            &["push", bare.path().to_str().unwrap(), "main"],
        );
        let hook_path = bare.path().join("hooks/pre-receive");
        fs::write(&hook_path, hook).unwrap();
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
        bare
    }

    /// Clones `bare` into a fresh cache, edits the managed template and pushes directly.
    /// Returns the push result and how long push_to_remote took.
    #[cfg(unix)]
    fn direct_push_through(
        engine: &SyncEngine,
        bare: &Path,
    ) -> (Result<String, String>, std::time::Duration) {
        let cache = tempdir().unwrap();
        let (tmpl_dir, _lock) = engine
            .ensure_remote_repo(bare.to_str().unwrap(), "main", cache.path())
            .expect("cache checkout of the bare remote");
        let checkout = tmpl_dir.parent().unwrap();
        let managed = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, "[core]\n  autocrlf = false\n").unwrap();
        let start = std::time::Instant::now();
        let res = engine.push_to_remote(checkout, "main", false, &[managed]);
        (res, start.elapsed())
    }

    #[test]
    #[cfg(unix)]
    fn test_push_timeout_hanging_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new()
            .with_extra_env(git.gitconfig_env())
            .with_limits(ProcessLimits {
                local: Duration::from_secs(10),
                network: Duration::from_millis(1500),
            });

        let hanging = bare_remote_with_pre_receive(&git, "#!/bin/sh\nsleep 20\n");
        let main_before = git.output(hanging.path(), &["rev-parse", "refs/heads/main"]);
        let (res, elapsed) = direct_push_through(&engine, hanging.path());
        let err = match res {
            Ok(msg) => panic!("push through a hanging hook returned Ok after {elapsed:?}: {msg}"),
            Err(e) => e,
        };
        assert!(
            err.contains("git push"),
            "error does not name git push: {err}"
        );
        assert!(
            err.contains("timed out after 1.5s"),
            "error does not name the limit: {err}"
        );
        assert!(
            elapsed < Duration::from_secs(8),
            "push_to_remote took {elapsed:?}"
        );
        assert_eq!(
            git.output(hanging.path(), &["rev-parse", "refs/heads/main"]),
            main_before,
            "remote main moved although the push was cancelled"
        );

        // Negative control: an accepting hook under the same limits lets the push through.
        let accepting = bare_remote_with_pre_receive(&git, "#!/bin/sh\nexit 0\n");
        let main_before = git.output(accepting.path(), &["rev-parse", "refs/heads/main"]);
        let (res, _) = direct_push_through(&engine, accepting.path());
        assert!(
            res.is_ok(),
            "push through an accepting hook failed: {res:?}"
        );
        assert_ne!(
            git.output(accepting.path(), &["rev-parse", "refs/heads/main"]),
            main_before,
            "remote main did not advance after a successful push"
        );
    }

    #[test]
    fn test_post_commit_verification_catches_hook_injected_secrets_and_rolls_back() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();

        // Initialize git repo with main
        git.run(remote_repo.path(), &["init", "-b", "main"]);

        let base_file = remote_repo.path().join("base.txt");
        fs::write(&base_file, "initial").unwrap();
        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);

        let base_head_sha = git.output(remote_repo.path(), &["rev-parse", "HEAD"]);

        // Staged allowlist has only base.txt
        let expected_set: std::collections::HashSet<PathBuf> =
            [PathBuf::from("base.txt")].into_iter().collect();

        // 1. Clean commit validation
        fs::write(&base_file, "updated benign content").unwrap();
        git.run(remote_repo.path(), &["add", "base.txt"]);
        git.run(remote_repo.path(), &["commit", "-m", "benign update"]);
        let clean_check = engine.validate_committed_tree(remote_repo.path(), &expected_set);
        assert!(
            clean_check.is_ok(),
            "Clean commit must pass validate_committed_tree"
        );

        // 2. Secret in committed tree: simulate secret injected into HEAD
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let secret_val = format!("{prefix}{body}");
        fs::write(&base_file, format!("token = {secret_val}\n")).unwrap();
        git.run(remote_repo.path(), &["add", "base.txt"]);
        git.run(remote_repo.path(), &["commit", "-m", "injected secret"]);

        let secret_check = engine.validate_committed_tree(remote_repo.path(), &expected_set);
        assert!(
            secret_check.is_err(),
            "Committed secret must fail validate_committed_tree"
        );
        assert!(
            secret_check
                .unwrap_err()
                .contains("secret detected in committed HEAD blob")
        );

        // Rollback commit and assert HEAD was restored to the parent
        engine.rollback_commit(remote_repo.path());
        let head_after_sha = git.output(remote_repo.path(), &["rev-parse", "HEAD"]);
        assert_ne!(
            head_after_sha, base_head_sha,
            "HEAD should be at benign commit, not base"
        );

        let head_content = git.output(remote_repo.path(), &["show", "HEAD:base.txt"]);
        assert!(
            !head_content.contains("ghp_"),
            "HEAD must not contain secret after rollback: {head_content}"
        );
    }

    #[test]
    fn test_pull_updates_refuses_to_write_through_symlink() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();

        // Baseline ancestor state for mise.toml
        let outside_file = outside_dir.path().join("sensitive_outside.txt");
        let initial_content = "original outside content\n";
        fs::write(&outside_file, initial_content).unwrap();

        let mut state = ProjectState::default();
        let anc_hash = SyncEngine::compute_hash(initial_content.as_bytes());
        state.record_file(
            Path::new("mise.toml"),
            anc_hash,
            Some(initial_content.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();

        // Upstream has mise.toml.jinja update
        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(&up_mise, "[tools]\nrust = \"1.86.0\"\n").unwrap();

        // Local target file is a symlink pointing to outside file
        let loc_mise = local_dir.path().join("mise.toml");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_file, &loc_mise).unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        let res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);

        // Must reject write through symlink!
        #[cfg(unix)]
        {
            assert!(
                matches!(res, PullResult::IoFailed { ref reason, .. } if reason.contains("symlink")),
                "pull_updates must fail when target is a symlink: {:?}",
                res
            );

            // Negative control: Assert outside file content was NOT overwritten
            let outside_content = fs::read_to_string(&outside_file).unwrap();
            assert_eq!(
                outside_content, "original outside content\n",
                "Outside file must remain untouched when pull is blocked on symlink!"
            );
        }
    }

    #[test]
    fn test_converged_files_reconcile_ancestor_preventing_false_conflicts() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // 1. Ancestor was version 1
        let v1_content = "[tools]\nrust = \"1.80.0\"\n";
        let v1_hash = SyncEngine::compute_hash(v1_content.as_bytes());
        let mut state = ProjectState::default();
        state.record_file(
            Path::new("mise.toml"),
            v1_hash,
            Some(v1_content.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();

        // 2. Both local and upstream converged to version 2
        let v2_content = "[tools]\nrust = \"1.85.0\"\n";
        let loc_mise = local_dir.path().join("mise.toml");
        fs::write(&loc_mise, v2_content).unwrap();

        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(&up_mise, v2_content).unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // 3. Scan: reports InSync
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let mise_st = states
            .iter()
            .find(|s| s.relative_path == Path::new("mise.toml"))
            .unwrap();
        assert_eq!(mise_st.status, SyncStatus::InSync);

        // 4. Pull updates: reconciles ancestor state in .ude/state.json
        let pull_res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(pull_res, PullResult::Success { applied_files: 0 });

        let updated_state = ProjectState::load_from_dir(local_dir.path()).unwrap();
        let v2_hash = SyncEngine::compute_hash(v2_content.as_bytes());
        assert_eq!(
            updated_state.get_ancestor_hash(Path::new("mise.toml")),
            Some(v2_hash.as_str()),
            "Ancestor hash in state.json must reconcile to converged v2 hash"
        );

        // 5. Upstream now advances to version 3
        let v3_content = "[tools]\nrust = \"1.86.0\"\n";
        fs::write(&up_mise, v3_content).unwrap();

        // 6. Scan: must report UpstreamModified, NOT Conflicted!
        let states2 = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let mise_st2 = states2
            .iter()
            .find(|s| s.relative_path == Path::new("mise.toml"))
            .unwrap();
        assert_eq!(
            mise_st2.status,
            SyncStatus::UpstreamModified,
            "After converged reconciliation, new upstream change must be UpstreamModified, not Conflicted!"
        );
    }

    #[test]
    fn test_binary_non_utf8_files_scanned_and_tracked_by_hash() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // Managed binary hook with invalid UTF-8 bytes
        let bin_bytes = vec![0xFF, 0xFE, 0xFD, 0x80, 0x00, 0x01];
        let loc_hook = local_dir.path().join("scripts/hooks/pre-push");
        fs::create_dir_all(loc_hook.parent().unwrap()).unwrap();
        fs::write(&loc_hook, &bin_bytes).unwrap();

        let up_hook = upstream_dir.path().join("projects/generic/hooks/pre-push");
        fs::create_dir_all(up_hook.parent().unwrap()).unwrap();
        fs::write(&up_hook, &bin_bytes).unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // Scan: must detect identical hashes and report InSync without failing UTF-8 decode
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let hook_st = states
            .iter()
            .find(|s| s.relative_path == Path::new("scripts/hooks/pre-push"))
            .unwrap();
        assert_eq!(hook_st.status, SyncStatus::InSync);
        assert!(hook_st.local_hash.is_some());
        assert_eq!(hook_st.local_hash, hook_st.upstream_hash);

        // Modify local binary file by 1 byte
        let mut modified_bytes = bin_bytes.clone();
        modified_bytes[0] = 0xAA;
        fs::write(&loc_hook, &modified_bytes).unwrap();

        let states2 = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let hook_st2 = states2
            .iter()
            .find(|s| s.relative_path == Path::new("scripts/hooks/pre-push"))
            .unwrap();
        assert_eq!(hook_st2.status, SyncStatus::Conflicted); // no ancestor recorded, so divergent hashes = Conflicted
    }
}
