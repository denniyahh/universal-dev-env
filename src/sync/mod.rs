pub mod merge;
pub mod staging;
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
    /// Tracked, deleted locally, upstream unchanged: push deletes the template (D-14, D-15).
    LocalDeleted,
    /// Tracked, deleted upstream, local copy unchanged: pull deletes it (D-12).
    UpstreamDeleted,
    /// Tracked, template missing from a remote whose history never deleted it (the
    /// remote never had it): pull keeps the file, edited or not, and its record.
    MissingUpstream,
    /// Tracked, deleted on both sides: pull drops the record (D-16).
    BothDeleted,
    /// Untracked, tombstoned, present locally only: never republished (SYNC-01).
    Unmanaged,
    /// Not present locally or upstream and not tracked.
    Absent,
    /// An open conflict record whose local and upstream versions have not converged:
    /// the file is left untouched until `ude sync resolve` or convergence (D-08, D-09).
    ConflictPending,
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
    /// LF-normalized local text (`None` when the local file is absent or not UTF-8).
    pub local_content: Option<String>,
    /// Whether the path has an open conflict record (`ProjectState::conflicts`).
    pub conflict_open: bool,
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
        /// Local paths whose upstream template was deleted (D-14).
        deleted: Vec<PathBuf>,
        /// Files with an unresolved conflict (Conflicted or ConflictPending), not
        /// published (D-10).
        skipped_conflicts: Vec<PathBuf>,
        /// Files whose local and upstream edits merge cleanly, not published until the
        /// merge is pulled (D-17).
        skipped_merged: Vec<PathBuf>,
        /// Locally deleted or edited files inside the private publish boundary, never
        /// published; the rest of the push goes ahead (CR-01, D-10 pattern).
        skipped_boundary: Vec<PathBuf>,
        /// Locally edited files whose upstream `.jinja` template has logic a rendered
        /// push would erase; the template must be edited upstream (CR-02).
        skipped_templates: Vec<PathBuf>,
        /// Locally edited files whose template the remote never had: never published.
        skipped_missing_upstream: Vec<PathBuf>,
    },
    /// The project could not be compared with upstream; nothing was changed.
    ScanFailed {
        reason: String,
    },
    SecretBlocked {
        path: PathBuf,
        reason: String,
    },
    IoFailed {
        path: PathBuf,
        reason: String,
    },
    GitRemoteFailed {
        reason: String,
    },
}

/// What a successful pull changed, path by path (project-relative paths).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PullReport {
    /// Files written from upstream (updated or cleanly merged).
    pub applied: Vec<PathBuf>,
    /// Files whose tracking ended because they were deleted upstream (D-12, D-16).
    pub deleted: Vec<PathBuf>,
    /// Files deleted locally but not yet pushed; pull keeps them deleted (D-15).
    pub locally_deleted_pending: Vec<PathBuf>,
    /// Conflicted files left untouched, each with a `<file>.conflict` sidecar (D-08).
    pub conflicts: Vec<PathBuf>,
    /// Open conflicts cleared because local and upstream converged (D-09 amendment).
    pub converged: Vec<PathBuf>,
    /// Files kept because their template is missing from a remote that never deleted it.
    pub missing_upstream: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PullResult {
    Success(PullReport),
    IoFailed {
        path: PathBuf,
        reason: String,
    },
    /// The project could not be compared with upstream (state, a refused path, or the
    /// remote's history); nothing was changed.
    ScanFailed {
        reason: String,
    },
}

/// Permission bits a conflict sidecar may take from its local file: execute bits are
/// cleared (a sidecar is data), read/write bits are kept so it is never wider (T-01-24).
const SIDECAR_EXEC_MASK: u32 = 0o666;

/// `<rel>.conflict`, the sidecar path beside the managed file `rel` (D-08).
fn sidecar_rel(rel: &Path) -> PathBuf {
    let mut name = rel.as_os_str().to_os_string();
    name.push(".conflict");
    PathBuf::from(name)
}

/// One version of a conflicted file for its sidecar: the text when known, else the
/// fingerprint (binary, or an ancestor recorded without content), else absent.
fn conflict_side<'a>(
    text: Option<&'a str>,
    fingerprint: Option<&'a str>,
) -> merge::ConflictSide<'a> {
    match (text, fingerprint) {
        (Some(text), _) => merge::ConflictSide::Text(text),
        (None, Some(fingerprint)) => merge::ConflictSide::Binary { fingerprint },
        (None, None) => merge::ConflictSide::Absent,
    }
}

/// Project-relative paths UDE manages, in scan order.
pub const MANAGED_FILES: [&str; 11] = [
    ".gitconfig",
    ".gitignore",
    "scripts/hooks/commit-msg",
    "scripts/hooks/pre-commit",
    "scripts/hooks/pre-push",
    "scripts/check-in-container.sh",
    "mise.toml",
    ".agents/AGENTS.md",
    "scratch/sync-personal-env.sh",
    "scratch/new-worktree.sh",
    "scratch/check-publish-boundary.sh",
];

/// What `ude sync resolve` recorded for a conflicted file (state table R1-R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// R1: the local file is kept against the recorded upstream; push publishes it.
    KeptLocal,
    /// R2: the local deletion is kept against the recorded upstream; push deletes the
    /// upstream template (D-14).
    AcceptedLocalDeletion,
    /// R3: upstream deleted the file and the local copy is kept as an unmanaged file
    /// (tombstone with `kept_unmanaged`, D-13); it is never republished.
    KeptUnmanaged,
    /// R4: upstream deleted the file and so did the user; tracking ends.
    DroppedDeleted,
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

/// A synchronized remote cache checkout, as returned by
/// [`SyncEngine::ensure_remote_repo`].
#[derive(Debug)]
pub struct RemoteCache {
    /// The checkout's `templates/` directory, or the checkout root when it has none.
    pub templates_dir: PathBuf,
    /// The cache checkout's root (its git toplevel), where publication runs git. Never
    /// derive it from `templates_dir`: for a remote without `templates/` the two are
    /// the same directory.
    pub checkout_dir: PathBuf,
    /// The remote branch head UDE fetched (`refs/remotes/origin/<branch>`). A direct
    /// push must build on exactly this commit (branch continuity, D-18).
    pub fetched_sha: String,
    /// The cache lock; hold it for the whole sync action.
    pub lock: RemoteLockGuard,
}

/// Why [`SyncEngine::push_to_remote`] published nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRemoteError {
    /// The remote branch moved after UDE fetched it, so its fast-forward check rejected
    /// the push. The cache commit was rolled back; the user should pull first (D-18).
    UpstreamMoved {
        /// The remote branch that moved.
        branch: String,
    },
    /// Any other failure (staging, publication gate, commit, continuity, network,
    /// remote hook rejection). A hard failure.
    Failed(String),
}

/// What `git push --porcelain` reported for the pushed ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPorcelain {
    /// No ref was flagged `!`.
    Accepted,
    /// A ref was flagged `!` with a `[rejected]` summary: the remote's fast-forward
    /// check refused it because the branch moved (`fetch first` / `non-fast-forward`).
    RejectedNonFastForward,
    /// A ref was flagged `!` for another reason, for example `[remote rejected]` from a
    /// remote hook.
    Other,
}

/// Classifies the stdout of `git push --porcelain` (lines `<flag>\t<from:to>\t<summary>`).
///
/// Only the flag field and the summary prefix are read, never the parenthetical
/// reason: a line flagged exactly `!` whose summary starts with `[rejected]` is
/// [`PushPorcelain::RejectedNonFastForward`], any other `!` line is
/// [`PushPorcelain::Other`], and no `!` line at all is [`PushPorcelain::Accepted`].
pub fn classify_push_porcelain(stdout: &str) -> PushPorcelain {
    let mut result = PushPorcelain::Accepted;
    for line in stdout.lines() {
        let mut fields = line.split('\t');
        if fields.next() != Some("!") {
            continue;
        }
        if fields
            .nth(1)
            .is_some_and(|summary| summary.starts_with("[rejected]"))
        {
            return PushPorcelain::RejectedNonFastForward;
        }
        result = PushPorcelain::Other;
    }
    result
}

/// Commit message of every sync commit in the remote cache checkout.
const SYNC_COMMIT_MESSAGE: &str =
    "chore(sync): synchronize environment refinements from local workstation";

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
    /// Remote checkout and branch whose history must show a template's deletion before
    /// a scan reports it as deleted upstream (set for remote status and pull).
    deletion_history: Option<(PathBuf, String)>,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            sanitizer: Sanitizer::new(),
            extra_env: Vec::new(),
            limits: ProcessLimits::default(),
            deletion_history: None,
        }
    }

    /// Confirms each upstream deletion against the history of `checkout` (a remote
    /// cache checkout of `branch`): a template the remote never deleted is reported as
    /// [`SyncStatus::MissingUpstream`] and pull keeps the file.
    pub fn with_deletion_history(mut self, checkout: PathBuf, branch: &str) -> Self {
        self.deletion_history = Some((checkout, branch.to_string()));
        self
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

    /// Detects 3-way sync status from the fingerprints of the ancestor record, the local
    /// file and the upstream template (`None` = absent; an empty file is present).
    ///
    /// Implements the C-less rows of the phase sync state table: deletion rows 1-5,
    /// the all-present rows (6), and the no-ancestor rows 7-11. `tombstoned` (the path
    /// has a tombstone) matters only without an ancestor record (invariant I1). A local
    /// override only changes rows where the local file is present, as before (I2).
    pub fn compare_3way_hashes(
        ancestor_hash: Option<&str>,
        local_hash: Option<&str>,
        upstream_hash: Option<&str>,
        has_local_override: bool,
        tombstoned: bool,
    ) -> SyncStatus {
        let local_change = if has_local_override {
            SyncStatus::Overridden
        } else {
            SyncStatus::LocallyModified
        };
        let in_sync = if has_local_override {
            SyncStatus::Overridden
        } else {
            SyncStatus::InSync
        };
        match (ancestor_hash, local_hash, upstream_hash) {
            // Rows 5 and 11: present nowhere.
            (Some(_), None, None) => SyncStatus::BothDeleted,
            (None, None, None) => SyncStatus::Absent,
            // Rows 3 and 4: deleted locally.
            (Some(o), None, Some(u)) if u == o => SyncStatus::LocalDeleted,
            (Some(_), None, Some(_)) => SyncStatus::Conflicted,
            // Rows 1 and 2: deleted upstream.
            (Some(o), Some(l), None) if l == o => SyncStatus::UpstreamDeleted,
            (Some(_), Some(_), None) => SyncStatus::Conflicted,
            // Row 6: present everywhere.
            (Some(_), Some(l), Some(u)) if l == u => in_sync,
            (Some(o), Some(l), Some(_)) if l == o => SyncStatus::UpstreamModified,
            (Some(o), Some(_), Some(u)) if u == o => local_change,
            (Some(_), Some(_), Some(_)) => SyncStatus::Conflicted,
            // Rows 7-10: untracked.
            (None, Some(_), None) if tombstoned => SyncStatus::Unmanaged,
            (None, Some(_), None) => local_change,
            (None, None, Some(_)) => SyncStatus::UpstreamModified,
            (None, Some(l), Some(u)) if l == u => in_sync,
            (None, Some(_), Some(_)) => SyncStatus::Conflicted,
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
        tombstoned: bool,
    ) -> (SyncStatus, Option<String>) {
        let status = Self::compare_3way_hashes(
            ancestor.map(|v| v.fingerprint),
            local.map(|v| v.fingerprint),
            upstream.map(|v| v.fingerprint),
            has_local_override,
            tombstoned,
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
        match merge::merge_texts(
            &Self::normalize_eol(base),
            &Self::normalize_eol(local),
            &Self::normalize_eol(upstream),
        ) {
            merge::MergeOutcome::Clean(merged) => (SyncStatus::Merged, Some(merged)),
            merge::MergeOutcome::Conflict(_) => (SyncStatus::Conflicted, None),
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
            false,
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

        for rel in MANAGED_FILES.iter().map(PathBuf::from) {
            let local_path = local_root.join(&rel);
            let upstream_path = map_to_upstream(upstream_root, &rel);

            let local_bytes = read_if_present(local_root, &local_path)?;
            let local_content = local_bytes
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok().map(|s| s.to_string()));
            let local_hash = local_bytes.as_ref().map(|b| Self::fingerprint(b));

            let (upstream_bytes, upstream_content) = match upstream_path.as_ref() {
                Some(p) => {
                    if let Some(bytes) = read_if_present(upstream_root, p)? {
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
                project_state.tombstone(&rel).is_some(),
            );
            // An open conflict wins over every record-less row until local and upstream
            // converge (state table P1); once converged the record-less status stands and
            // `conflict_open` tells pull to clear the record (P2, rows 13-16).
            let conflict_open = project_state.conflict(&rel).is_some();
            let converged = local_hash == upstream_hash;
            let (status, merged_content) = if conflict_open && !converged {
                (SyncStatus::ConflictPending, None)
            } else {
                (status, merged_content)
            };

            states.push(FileSyncState {
                relative_path: rel,
                status,
                local_hash,
                upstream_hash,
                ancestor_hash,
                merged_content,
                upstream_content,
                local_content: local_content.as_deref().map(Self::normalize_eol),
                conflict_open,
            });
        }

        if let Some((checkout, branch)) = &self.deletion_history {
            // Upstream deletions, and deletion conflicts (edited locally, template gone).
            let deleted: Vec<PathBuf> = states
                .iter()
                .filter(|s| {
                    s.status == SyncStatus::UpstreamDeleted
                        || (s.status == SyncStatus::Conflicted
                            && s.upstream_hash.is_none()
                            && s.local_hash.is_some())
                })
                .map(|s| s.relative_path.clone())
                .collect();
            let never_deleted =
                self.never_deleted_upstream(checkout, branch, upstream_root, &deleted)?;
            for state in &mut states {
                if never_deleted.contains(&state.relative_path) {
                    state.status = SyncStatus::MissingUpstream;
                }
            }
        }

        Ok(states)
    }

    /// Paths in `deleted` whose template no commit in `checkout`'s history deletes. A
    /// shallow checkout is deepened first, so the answer comes from the full history.
    fn never_deleted_upstream(
        &self,
        checkout: &Path,
        branch: &str,
        upstream_root: &Path,
        deleted: &[PathBuf],
    ) -> Result<HashSet<PathBuf>, String> {
        let mut never_deleted = HashSet::new();
        if deleted.is_empty() {
            return Ok(never_deleted);
        }
        let mut probe = self.git(Some(checkout));
        probe.args(["rev-parse", "--is-shallow-repository"]);
        let out = self.run_local(probe, "git rev-parse --is-shallow-repository")?;
        if !out.status.success() {
            return Err(with_excerpt(
                "git rev-parse failed in the remote cache".to_string(),
                &out,
            ));
        }
        if String::from_utf8_lossy(&out.stdout).trim() == "true" {
            let mut unshallow = self.git(Some(checkout));
            unshallow.args(["fetch", "--unshallow", "origin", branch]);
            let out = self.run_network(unshallow, "git fetch --unshallow")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    "git fetch --unshallow failed; cannot confirm upstream deletions".to_string(),
                    &out,
                ));
            }
        }
        for rel in deleted {
            let in_repo = map_to_upstream(upstream_root, rel)
                .and_then(|template| template.strip_prefix(checkout).ok().map(Path::to_path_buf));
            let Some(in_repo) = in_repo else {
                never_deleted.insert(rel.clone());
                continue;
            };
            let mut log = self.git(Some(checkout));
            // first-parent diffs for merges, so a deletion made while resolving a merge
            // conflict counts too.
            log.args([
                "log",
                "-1",
                "--format=%H",
                "--diff-filter=D",
                "--no-renames",
                "--diff-merges=first-parent",
                "--no-patch",
                "HEAD",
                "--",
            ])
            .arg(&in_repo);
            let out = self.run_local(log, "git log --diff-filter=D")?;
            if !out.status.success() {
                return Err(with_excerpt(
                    "git log failed in the remote cache".to_string(),
                    &out,
                ));
            }
            if out.stdout.iter().all(u8::is_ascii_whitespace) {
                never_deleted.insert(rel.clone());
            }
        }
        Ok(never_deleted)
    }

    /// Marks the open conflict on `rel` as resolved with the file's current local state
    /// (D-09), against the upstream version recorded when the conflict was detected.
    /// Offline: reads only `state.json` and the local file.
    ///
    /// The outcome follows state table rows R1-R4. The new state is staged first, the
    /// `<rel>.conflict` sidecar is removed next and the state is committed last, so a
    /// failure never leaves the conflict cleared while its sidecar remains: until the
    /// commit, `state.json` is untouched and the conflict stays open.
    pub fn resolve_conflict(
        &self,
        local_root: &Path,
        rel: &Path,
    ) -> Result<ResolveOutcome, String> {
        let rel = managed_resolve_target(rel)?;
        let mut state = ProjectState::load_from_dir(local_root)?;
        let Some(record) = state.conflict(rel).cloned() else {
            return Err(format!(
                "No open conflict for {}; nothing to resolve",
                rel.display()
            ));
        };
        let local_bytes = read_resolve_local(local_root, rel)?;
        let ancestor_hash = state.get_ancestor_hash(rel).map(String::from);

        let outcome = match (record.upstream_hash, local_bytes) {
            (Some(upstream_hash), local) => {
                state.record_file(rel, upstream_hash, record.upstream_content);
                if local.is_some() {
                    ResolveOutcome::KeptLocal
                } else {
                    ResolveOutcome::AcceptedLocalDeletion
                }
            }
            (None, Some(bytes)) => {
                let deleted_hash = ancestor_hash.unwrap_or_else(|| Self::fingerprint(&bytes));
                state.remove_file(rel);
                state.record_tombstone(rel, deleted_hash, true);
                ResolveOutcome::KeptUnmanaged
            }
            (None, None) => {
                // With no files record there is no fingerprint to tombstone (same end
                // state as convergence row 16).
                if let Some(deleted_hash) = ancestor_hash {
                    state.remove_file(rel);
                    state.record_tombstone(rel, deleted_hash, false);
                }
                ResolveOutcome::DroppedDeleted
            }
        };
        state.clear_conflict(rel);

        let staged = state.stage_to_dir(local_root)?;
        remove_conflict_sidecar(local_root, rel)?;
        let sidecar = sidecar_rel(rel);
        staged.commit().map_err(|e| {
            format!(
                "Removed {} but failed to save state: {e}; {} is still unresolved (the next ude sync pull regenerates the sidecar)",
                sidecar.display(),
                rel.display()
            )
        })?;
        Ok(outcome)
    }

    /// Ensures a remote git repository is cached locally and updated.
    /// Serializes concurrent access via an advisory lockfile (`.lock`).
    pub fn ensure_remote_repo(
        &self,
        remote_url: &str,
        branch: &str,
        cache_dir: &Path,
    ) -> Result<RemoteCache, String> {
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

        let fetched_sha = self.fetched_head(&target_checkout, branch)?;

        // Return the templates directory inside the remote checkout along with the held lock
        let templates_path = target_checkout.join("templates");
        let templates_dir = if templates_path.exists() {
            templates_path
        } else {
            target_checkout.clone()
        };
        // Any other repository would make every managed file read as deleted upstream, so
        // the remote must hold at least one template for a managed file.
        let has_template = MANAGED_FILES.iter().any(|rel| {
            map_to_upstream(&templates_dir, Path::new(rel)).is_some_and(|src| src.is_file())
        });
        if !has_template {
            return Err(format!(
                "{} is not a ude template repository (no template for any managed file)",
                process::redact_userinfo(remote_url)
            ));
        }
        Ok(RemoteCache {
            templates_dir,
            checkout_dir: target_checkout,
            fetched_sha,
            lock: lock_guard,
        })
    }

    /// The commit `refs/remotes/origin/<branch>` points at in `checkout`.
    fn fetched_head(&self, checkout: &Path, branch: &str) -> Result<String, String> {
        let mut rev_parse = self.git(Some(checkout));
        rev_parse.args([
            "rev-parse",
            "--verify",
            &format!("refs/remotes/origin/{branch}"),
        ]);
        let out = self.run_local(rev_parse, "git rev-parse")?;
        if !out.status.success() {
            return Err(with_excerpt(
                format!("Failed to read the fetched head of origin/{branch}"),
                &out,
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Pulls upstream updates into the local project directory.
    /// Operates with strict safety:
    /// 1. Applies clean files and leaves each Conflicted or ConflictPending file
    ///    untouched, (re)writing a `<file>.conflict` sidecar and recording the current
    ///    upstream version (D-08, D-09). An open conflict whose local and upstream
    ///    versions converged is cleared and its sidecar removed (D-09 amendment).
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
            Err(reason) => return PullResult::ScanFailed { reason },
        };
        let renderer = crate::template::TemplateRenderer::new();

        // Conflicted and still-pending files are left untouched; each gets a sidecar and
        // a conflict record in the same transaction as the clean files (D-08, row 12).
        let conflicted: Vec<&FileSyncState> = states
            .iter()
            .filter(|s| {
                matches!(
                    s.status,
                    SyncStatus::Conflicted | SyncStatus::ConflictPending
                )
            })
            .collect();
        // An open record whose file is no longer pending has converged (rows 13-16).
        let converged: Vec<&FileSyncState> = states
            .iter()
            .filter(|s| s.conflict_open && s.status != SyncStatus::ConflictPending)
            .collect();

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

                let upstream_bytes = match refuse_symlink_under(upstream_root, &upstream_path)
                    .and_then(|()| fs::read(&upstream_path).map_err(|e| e.to_string()))
                {
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
                && !s.conflict_open
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

        // Deletions. UpstreamDeleted (D-12) deletes the unchanged local copy; BothDeleted
        // (D-16) has no file to touch. Both end tracking with a tombstone. LocalDeleted
        // (D-15) stays deleted until a push propagates it.
        let mut report = PullReport::default();
        let mut upstream_deleted = Vec::new();
        let mut both_deleted = Vec::new();
        for s in &states {
            match (&s.status, &s.ancestor_hash) {
                (SyncStatus::UpstreamDeleted, Some(ancestor)) => {
                    upstream_deleted.push((s.relative_path.clone(), ancestor.clone()));
                }
                (SyncStatus::BothDeleted, Some(ancestor)) => {
                    both_deleted.push((s.relative_path.clone(), ancestor.clone()));
                }
                (SyncStatus::LocalDeleted, _) => {
                    report.locally_deleted_pending.push(s.relative_path.clone());
                }
                (SyncStatus::MissingUpstream, _) => {
                    report.missing_upstream.push(s.relative_path.clone());
                }
                _ => {}
            }
        }

        // Nothing to write, delete or record: skip staging but keep the report built so far.
        if candidates_to_write.is_empty()
            && converged_to_record.is_empty()
            && upstream_deleted.is_empty()
            && both_deleted.is_empty()
            && conflicted.is_empty()
            && converged.is_empty()
        {
            return PullResult::Success(report);
        }

        // Pass 2: stage every file and the new state before replacing anything (D-05).
        // Any staging error returns here; dropping `transaction` deletes the temps and
        // the directories staging created, so nothing in the project changes.
        let mut project_state = match ProjectState::load_from_dir(local_root) {
            Ok(state) => state,
            Err(reason) => return state_pull_failure(reason),
        };
        let mut transaction = staging::StagedTransaction::new(local_root);

        for (rel, target, content_bytes, ancestor_hash, ancestor_content) in candidates_to_write {
            let mode = staging::file_mode_for(&rel, &target);
            if let Err(reason) = transaction.stage_write(&rel, &content_bytes, mode) {
                return PullResult::IoFailed { path: rel, reason };
            }
            project_state.record_file(&rel, ancestor_hash, ancestor_content);
        }

        let mut sidecars = HashSet::new();
        for s in conflicted {
            let rel = &s.relative_path;
            let upstream_lf = s.upstream_content.as_deref().map(Self::normalize_eol);
            let body = merge::render_conflict_sidecar(
                rel,
                conflict_side(
                    project_state.get_ancestor_content(rel),
                    project_state.get_ancestor_hash(rel),
                ),
                conflict_side(s.local_content.as_deref(), s.local_hash.as_deref()),
                conflict_side(upstream_lf.as_deref(), s.upstream_hash.as_deref()),
            );
            let sidecar = sidecar_rel(rel);
            // A missing local file yields REGULAR_MODE (or EXECUTABLE_MODE, masked to it).
            let mode = staging::file_mode_for(rel, &local_root.join(rel)) & SIDECAR_EXEC_MASK;
            if let Err(reason) = transaction.stage_write(&sidecar, body.as_bytes(), mode) {
                return PullResult::IoFailed {
                    path: sidecar,
                    reason,
                };
            }
            project_state.record_conflict(rel, s.upstream_hash.clone(), upstream_lf);
            sidecars.insert(sidecar);
            report.conflicts.push(rel.clone());
        }

        // Convergence clears the conflict and its sidecar. Both present (rows 13/15): the
        // upstream version becomes the ancestor. Both absent: BothDeleted (row 14) is
        // handled with the other deletions below; Absent (row 16) records nothing.
        for s in converged {
            let rel = &s.relative_path;
            project_state.clear_conflict(rel);
            let sidecar = sidecar_rel(rel);
            if fs::symlink_metadata(local_root.join(&sidecar)).is_ok_and(|meta| meta.is_file())
                && let Err(reason) = transaction.stage_delete(&sidecar)
            {
                return PullResult::IoFailed {
                    path: sidecar,
                    reason,
                };
            }
            if s.local_hash.is_some()
                && let Some(upstream_hash) = &s.upstream_hash
            {
                project_state.record_file(
                    rel,
                    upstream_hash.clone(),
                    s.upstream_content.as_deref().map(Self::normalize_eol),
                );
            }
            report.converged.push(rel.clone());
        }

        for (rel, hash, content) in converged_to_record {
            project_state.record_file(&rel, hash, content);
        }

        for (rel, ancestor) in upstream_deleted {
            if let Err(reason) = transaction.stage_delete(&rel) {
                return PullResult::IoFailed { path: rel, reason };
            }
            project_state.remove_file(&rel);
            project_state.record_tombstone(&rel, ancestor, false);
            report.deleted.push(rel);
        }

        for (rel, ancestor) in both_deleted {
            project_state.remove_file(&rel);
            project_state.record_tombstone(&rel, ancestor, false);
            report.deleted.push(rel);
        }

        let staged_state = match project_state.stage_to_dir(local_root) {
            Ok(staged) => staged,
            Err(reason) => return state_pull_failure(format!("Failed to stage state: {reason}")),
        };

        // Commit phase: files first, state.json last.
        let applied = match transaction.commit() {
            Ok(applied) => applied,
            Err(reason) => {
                return PullResult::IoFailed {
                    path: local_root.to_path_buf(),
                    reason: format!("rename phase failed, state not updated: {reason}"),
                };
            }
        };
        if let Err(e) = staged_state.commit() {
            return state_pull_failure(format!("Failed to persist state: {e}"));
        }

        report.applied = applied
            .into_iter()
            .filter(|rel| !sidecars.contains(rel))
            .collect();
        PullResult::Success(report)
    }

    /// Pre-validates and pushes locally modified files to upstream template directory.
    /// Operates atomically: verifies that ALL candidate files pass read integrity and
    /// secret scanning before performing any upstream modifications. Files inside the
    /// publish boundary are skipped and reported, not published (CR-01).
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
            Err(reason) => return PushResult::ScanFailed { reason },
        };
        let mut candidates_to_write = Vec::new();
        let mut deletions = Vec::new();
        let mut skipped_conflicts = Vec::new();
        let mut skipped_merged = Vec::new();
        let mut skipped_boundary = Vec::new();
        let mut skipped_templates = Vec::new();
        let mut skipped_missing_upstream = Vec::new();

        // Pass 1: Strict atomic pre-validation across ALL modified candidates
        for state in &states {
            // Never publish content the developer has not had in the working tree: an
            // unresolved conflict (D-10) or a clean merge not yet pulled (D-17) is
            // skipped and reported; the other files are still published.
            match state.status {
                SyncStatus::Conflicted | SyncStatus::ConflictPending => {
                    skipped_conflicts.push(state.relative_path.clone());
                    continue;
                }
                SyncStatus::Merged => {
                    skipped_merged.push(state.relative_path.clone());
                    continue;
                }
                SyncStatus::MissingUpstream => {
                    if state.local_hash != state.ancestor_hash {
                        skipped_missing_upstream.push(state.relative_path.clone());
                    }
                    continue;
                }
                _ => {}
            }

            // Overridden files are never pushed upstream (Option B)
            if state.status == SyncStatus::Overridden {
                continue;
            }

            // Gate 1: publish boundary. A local deletion or edit of a private path is
            // skipped and reported, like a conflict (D-10); the rest is still published.
            // It is checked in two halves around Gate 2, the order the status hint
            // reports: a local-only path first, then template logic, which --force
            // cannot clear, then the --force requirement for agent rules.
            let boundary_change = matches!(
                state.status,
                SyncStatus::LocalDeleted | SyncStatus::LocallyModified
            );
            if boundary_change
                && self
                    .sanitizer
                    .is_local_only_path(&state.relative_path, true)
            {
                skipped_boundary.push(state.relative_path.clone());
                continue;
            }

            let upstream_target = map_to_upstream(upstream_root, &state.relative_path);

            // Gate 2: never replace template logic with rendered text (CR-02), and never
            // delete it: one project's deletion would remove it for every project.
            if matches!(
                state.status,
                SyncStatus::LocallyModified | SyncStatus::LocalDeleted
            ) {
                match upstream_target.as_deref().map(template_has_logic) {
                    Some(Ok(true)) => {
                        skipped_templates.push(state.relative_path.clone());
                        continue;
                    }
                    Some(Err(reason)) => {
                        return PushResult::IoFailed {
                            path: state.relative_path.clone(),
                            reason,
                        };
                    }
                    Some(Ok(false)) | None => {}
                }
            }

            if boundary_change
                && self
                    .sanitizer
                    .is_local_only_path(&state.relative_path, allow_agent_rules)
            {
                skipped_boundary.push(state.relative_path.clone());
                continue;
            }

            // Deleted locally, unchanged upstream (D-14): delete the upstream template.
            // Unmanaged, UpstreamDeleted, BothDeleted and Absent are never published.
            if state.status == SyncStatus::LocalDeleted {
                if let Some(target_upstream) = upstream_target {
                    deletions.push((state.relative_path.clone(), target_upstream));
                }
                continue;
            }

            if state.status == SyncStatus::LocallyModified {
                let local_path = local_root.join(&state.relative_path);

                let raw_bytes = match refuse_symlink_under(local_root, &local_path)
                    .and_then(|()| fs::read(&local_path).map_err(|e| e.to_string()))
                {
                    Ok(b) => b,
                    Err(err) => {
                        return PushResult::IoFailed {
                            path: state.relative_path.clone(),
                            reason: format!("Failed to read local file: {err}"),
                        };
                    }
                };

                let (parameterized_bytes, raw_local_str) =
                    if let Ok(content) = std::str::from_utf8(&raw_bytes) {
                        // Gate 3: Secret check (reject credentials)
                        match self.sanitizer.check_secrets(content) {
                            SanitizationResult::SecretDetected { reason, .. } => {
                                return PushResult::SecretBlocked {
                                    path: state.relative_path.clone(),
                                    reason,
                                };
                            }
                            SanitizationResult::Clean(_) => {}
                        }

                        // Gate 4: Path parameterization
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

        // Pass 2: stage every template (and, when persisting immediately, the new state)
        // before replacing anything (D-05). A staging error returns here; dropping
        // `transaction` deletes the temps and any directories staging created.
        let mut transaction = staging::StagedTransaction::new(upstream_root);
        let mut updated_records = Vec::new();

        for (rel_path, target_upstream, parameterized_bytes, raw_bytes, raw_local_str) in
            candidates_to_write
        {
            let Ok(upstream_rel) = target_upstream.strip_prefix(upstream_root) else {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!(
                        "Upstream target {} is outside the upstream root",
                        target_upstream.display()
                    ),
                };
            };
            let mode = staging::file_mode_for(&rel_path, &target_upstream);
            if let Err(reason) = transaction.stage_write(upstream_rel, &parameterized_bytes, mode) {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason,
                };
            }
            updated_records.push((rel_path, Self::fingerprint(&raw_bytes), raw_local_str));
        }

        let mut deleted = Vec::new();
        let mut deleted_templates = Vec::new();
        for (rel_path, target_upstream) in deletions {
            let Ok(upstream_rel) = target_upstream.strip_prefix(upstream_root) else {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason: format!(
                        "Upstream target {} is outside the upstream root",
                        target_upstream.display()
                    ),
                };
            };
            if let Err(reason) = transaction.stage_delete(upstream_rel) {
                return PushResult::IoFailed {
                    path: rel_path,
                    reason,
                };
            }
            deleted.push(rel_path);
            deleted_templates.push(target_upstream);
        }

        let staged_state = if persist_ancestor_immediately {
            let mut project_state = match ProjectState::load_from_dir(local_root) {
                Ok(state) => state,
                Err(reason) => return state_push_failure(reason),
            };
            for (rel, hash, content) in &updated_records {
                project_state.record_file(rel, hash.clone(), content.clone());
            }
            for rel in &deleted {
                project_state.remove_file(rel);
            }
            match project_state.stage_to_dir(local_root) {
                Ok(staged) => Some(staged),
                Err(reason) => {
                    return state_push_failure(format!("Failed to stage state: {reason}"));
                }
            }
        } else {
            None
        };

        // Commit phase: templates first, state.json last.
        let applied = match transaction.commit() {
            Ok(applied) => applied,
            Err(reason) => {
                return PushResult::IoFailed {
                    path: upstream_root.to_path_buf(),
                    reason: format!("rename phase failed, state not updated: {reason}"),
                };
            }
        };
        if let Some(staged) = staged_state
            && let Err(e) = staged.commit()
        {
            return state_push_failure(format!("Failed to persist state: {e}"));
        }

        let mut staged_files: Vec<PathBuf> =
            applied.iter().map(|rel| upstream_root.join(rel)).collect();
        staged_files.extend(deleted_templates);
        PushResult::Success {
            synced_files: staged_files.len(),
            staged_files,
            updated_records,
            deleted,
            skipped_conflicts,
            skipped_merged,
            skipped_boundary,
            skipped_templates,
            skipped_missing_upstream,
        }
    }

    /// Publishes the staged template updates: commits them in the cache checkout and
    /// pushes the commit directly to `branch`, or to a fresh `ude-sync-*` branch with a
    /// PR when `open_pr` is set. `fetched_sha` is the head UDE fetched
    /// ([`RemoteCache::fetched_sha`]); a direct push must build on it.
    pub fn push_to_remote(
        &self,
        remote_checkout: &Path,
        branch: &str,
        open_pr: bool,
        staged_files: &[PathBuf],
        fetched_sha: &str,
    ) -> Result<String, PushRemoteError> {
        let Some(expected_set) = self
            .stage_publication(remote_checkout, staged_files)
            .map_err(PushRemoteError::Failed)?
        else {
            return Ok("No changes to push to remote repository.".to_string());
        };
        if open_pr {
            self.push_pr_branch(remote_checkout, branch, &expected_set)
                .map_err(PushRemoteError::Failed)
        } else {
            self.push_direct(remote_checkout, branch, &expected_set, fetched_sha)
        }
    }

    /// Stages exactly `staged_files` in the cache checkout and runs the publication gate
    /// on the index (allowlist, secret scan of every staged blob). Returns the validated
    /// allowlist relative to `remote_checkout`, or `None` when there is nothing to push.
    fn stage_publication(
        &self,
        remote_checkout: &Path,
        staged_files: &[PathBuf],
    ) -> Result<Option<HashSet<PathBuf>>, String> {
        let mut status_cmd = self.git(Some(remote_checkout));
        status_cmd.args(["status", "--porcelain"]);
        let status = self
            .run_local(status_cmd, "git status")
            .map_err(|e| format!("Failed to check git status in remote checkout: {e}"))?;
        // Empty output means "nothing to push" only when git status succeeded.
        if !status.status.success() {
            return Err(with_excerpt(
                format!(
                    "git status failed in remote checkout {} with exit status {:?}",
                    remote_checkout.display(),
                    status.status.code()
                ),
                &status,
            ));
        }

        if status.stdout.is_empty() {
            return Ok(None);
        }

        if staged_files.is_empty() {
            return Ok(None);
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

        // Inspect staged index: assert that ONLY allowed files are staged. Renames are
        // off so a deletion plus a similar addition is listed as two entries.
        let mut diff = self.git(Some(remote_checkout));
        diff.args(["diff", "--cached", "--name-status", "--no-renames"]);
        let diff_cached = self
            .run_local(diff, "git diff --cached")
            .map_err(|e| format!("Failed to inspect staged diff in remote checkout: {e}"))?;
        // The gate below checks only what this lists; a failed listing checks nothing.
        if !diff_cached.status.success() {
            self.reset_index(remote_checkout);
            return Err(with_excerpt(
                format!(
                    "git diff --cached failed in remote checkout with exit status {:?}",
                    diff_cached.status.code()
                ),
                &diff_cached,
            ));
        }

        let staged_output = String::from_utf8_lossy(&diff_cached.stdout);
        let expected_set: HashSet<PathBuf> = rel_staged_paths.into_iter().collect();

        for line in staged_output.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Some((status, p)) = parse_name_status(line) else {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: unparseable staged entry: {line}"
                ));
            };
            if !expected_set.contains(&p) {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: unvalidated file staged in remote checkout: {}",
                    p.display()
                ));
            }
            // A deletion publishes no blob; every other entry is scanned.
            if status.starts_with('D') {
                continue;
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

        Ok(Some(expected_set))
    }

    /// PR mode: commits on a fresh `ude-sync-<hash>-<epoch>` branch, pushes it and opens
    /// a PR against `branch`. Not subject to the upstream-moved abort (D-18 scope note).
    fn push_pr_branch(
        &self,
        remote_checkout: &Path,
        branch: &str,
        expected_set: &HashSet<PathBuf>,
    ) -> Result<String, String> {
        let epoch_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sync_branch = format!(
            "ude-sync-{}-{}",
            &Self::compute_hash(SYNC_COMMIT_MESSAGE.as_bytes())[..8],
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
                self.commit_command(remote_checkout, SYNC_COMMIT_MESSAGE),
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

        if let Err(err) = self.validate_committed_tree(remote_checkout, expected_set) {
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
            SYNC_COMMIT_MESSAGE,
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
    }

    /// Direct mode: commits in the cache checkout and pushes the commit to `branch`.
    ///
    /// The commit must build on `fetched_sha` (branch continuity). The push is a plain
    /// fast-forward push, never forced: when upstream moved after the fetch, the remote's
    /// fast-forward check rejects it, the cache commit is rolled back and the result is
    /// [`PushRemoteError::UpstreamMoved`] (D-18).
    fn push_direct(
        &self,
        remote_checkout: &Path,
        branch: &str,
        expected_set: &HashSet<PathBuf>,
        fetched_sha: &str,
    ) -> Result<String, PushRemoteError> {
        use PushRemoteError::Failed;
        let commit_out = self
            .run_local(
                self.commit_command(remote_checkout, SYNC_COMMIT_MESSAGE),
                "git commit",
            )
            .map_err(|e| Failed(format!("Failed to commit in remote checkout: {e}")))?;

        if !commit_out.status.success() {
            return Err(Failed(with_excerpt(
                "git commit failed in remote checkout".to_string(),
                &commit_out,
            )));
        }

        if let Err(err) = self.validate_committed_tree(remote_checkout, expected_set) {
            self.rollback_commit(remote_checkout);
            return Err(Failed(err));
        }

        if let Err(err) = self.check_continuity(remote_checkout, branch, fetched_sha) {
            self.rollback_commit(remote_checkout);
            return Err(Failed(err));
        }

        // No force or lease option, ever: the server's fast-forward check is the
        // compare-and-swap that detects a concurrent publisher (D-18).
        let refspec = format!("HEAD:refs/heads/{branch}");
        let mut push = self.git(Some(remote_checkout));
        push.args(["push", "--porcelain", "origin", &refspec]);
        let push_out = self
            .run_network(push, "git push")
            .map_err(|e| Failed(format!("Failed to push to remote: {e}")))?;

        if !push_out.status.success() {
            // Porcelain stdout starts with a `To <url>` line; it is never printed.
            let stdout = String::from_utf8_lossy(&push_out.stdout);
            if classify_push_porcelain(&stdout) == PushPorcelain::RejectedNonFastForward {
                self.rollback_commit(remote_checkout);
                return Err(PushRemoteError::UpstreamMoved {
                    branch: branch.to_string(),
                });
            }
            return Err(Failed(with_excerpt(
                format!("git push origin {branch} failed"),
                &push_out,
            )));
        }

        Ok(format!(
            "Directly pushed changes to remote branch '{branch}'."
        ))
    }

    /// Checks that the cache commit's parent is the fetched head of `branch`.
    fn check_continuity(&self, repo: &Path, branch: &str, fetched_sha: &str) -> Result<(), String> {
        let mut rev_parse = self.git(Some(repo));
        rev_parse.args(["rev-parse", "--verify", "HEAD~1"]);
        let out = self
            .run_local(rev_parse, "git rev-parse")
            .map_err(|e| format!("Branch continuity check failed: {e}"))?;
        if !out.status.success() {
            return Err(with_excerpt(
                "Branch continuity check failed: cache commit has no parent".to_string(),
                &out,
            ));
        }
        let parent = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if parent != fetched_sha {
            return Err(format!(
                "Branch continuity check failed: cache commit parent {parent} is not the fetched {branch} head {fetched_sha}"
            ));
        }
        Ok(())
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
            "--name-status",
            "--no-renames",
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
            if line.trim().is_empty() {
                continue;
            }
            let Some((status, p)) = parse_name_status(line) else {
                return Err(format!(
                    "Post-commit publication gate violation: unparseable committed entry: {line}"
                ));
            };
            if !paths.contains(&p) {
                return Err(format!(
                    "Post-commit publication gate violation: unvalidated file committed in HEAD: {}",
                    p.display()
                ));
            }
            // A deletion publishes no blob; every other entry is scanned.
            if status.starts_with('D') {
                continue;
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

/// Splits one `--name-status --no-renames` line into its status letter(s) and path.
/// `None` when the line has no tab or an empty status or path (callers fail closed).
fn parse_name_status(line: &str) -> Option<(&str, PathBuf)> {
    let (status, path) = line.split_once('\t')?;
    let status = status.trim();
    if status.is_empty() || path.is_empty() {
        return None;
    }
    Some((status, PathBuf::from(path)))
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

/// Refuses a symlink at `path` or at any of its parents below `root` (`root` itself
/// may be a link). Managed files and templates are synced by content: following a
/// link would copy or publish whatever it points at, and a dangling link would read
/// as a deletion.
pub fn refuse_symlink_under(root: &Path, path: &Path) -> Result<(), String> {
    let mut candidates: Vec<&Path> = vec![path];
    if path.starts_with(root) {
        candidates = path.ancestors().take_while(|p| *p != root).collect();
    }
    for candidate in candidates {
        if fs::symlink_metadata(candidate).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!(
                "Refusing to follow symlink {}",
                candidate.display()
            ));
        }
    }
    Ok(())
}

/// Bytes of the file at `path`, `None` when nothing exists there (the path or one of
/// its parents is missing, or a parent is a regular file). Any other read error
/// (permissions, a directory at `path`) is an error: an unreadable file must never be
/// classified as deleted, because deletions are propagated. A symlink at `path` or
/// below `root` is an error too (see [`refuse_symlink_under`]).
fn read_if_present(root: &Path, path: &Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::ErrorKind;
    refuse_symlink_under(root, path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
        Err(e) => Err(format!("Failed to read {}: {e}", path.display())),
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

/// The [`MANAGED_FILES`] entry `rel` names. Refuses absolute paths, `..` and every
/// other non-normal component before the managed-list check.
fn managed_resolve_target(rel: &Path) -> Result<&'static Path, String> {
    let all_normal = rel
        .components()
        .all(|comp| matches!(comp, std::path::Component::Normal(_)));
    if rel.as_os_str().is_empty() || !all_normal {
        return Err(format!("Refusing to resolve unsafe path {}", rel.display()));
    }
    MANAGED_FILES
        .iter()
        .map(Path::new)
        .find(|managed| *managed == rel)
        .ok_or_else(|| format!("{} is not a UDE-managed file", rel.display()))
}

/// The local bytes of the managed file `rel`, or `None` when it is absent. A symlink,
/// a symlinked parent or a non-regular file is an error.
fn read_resolve_local(local_root: &Path, rel: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = validate_safe_write_target(local_root, rel)
        .map_err(|e| format!("Cannot resolve {}: {e}", rel.display()))?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => fs::read(&path)
            .map(Some)
            .map_err(|e| format!("Failed to read {}: {e}", path.display())),
        Ok(_) => Err(format!(
            "Cannot resolve {}: not a regular file",
            rel.display()
        )),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(format!("Failed to inspect {}: {e}", path.display())),
    }
}

/// Removes the regular file `<rel>.conflict` if it exists. Refuses a symlinked,
/// escaping or non-regular sidecar, which leaves the conflict unresolved.
fn remove_conflict_sidecar(local_root: &Path, rel: &Path) -> Result<(), String> {
    let sidecar = sidecar_rel(rel);
    let unresolved = |reason: String| {
        format!(
            "Cannot remove {}: {reason}; {} is still unresolved",
            sidecar.display(),
            rel.display()
        )
    };
    let path = validate_safe_write_target(local_root, &sidecar).map_err(unresolved)?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(unresolved("not a regular file".to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(unresolved(e.to_string())),
    }
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!(
            "Failed to remove {}: {e}; {} is still unresolved",
            sidecar.display(),
            rel.display()
        )),
    }
}

/// Placeholders `parameterize_paths` writes back on push, so a template holding only
/// these survives a push of its rendered text.
const PARAMETERIZED_PLACEHOLDERS: [&str; 2] = ["{{ user.home }}", "{{ user.xdg_config }}"];

/// Whether `template` is a `.jinja` file with template constructs (other than the
/// parameterized path placeholders) that publishing rendered text would erase (CR-02).
/// An absent template has none.
pub fn template_has_logic(template: &Path) -> Result<bool, String> {
    if template.extension().is_none_or(|ext| ext != "jinja") {
        return Ok(false);
    }
    let bytes = match fs::read(template) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(format!(
                "Failed to read upstream template {}: {err}",
                template.display()
            ));
        }
    };
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    for placeholder in PARAMETERIZED_PLACEHOLDERS {
        text = text.replace(placeholder, "");
    }
    Ok(["{{", "{%", "{#"]
        .iter()
        .any(|marker| text.contains(marker)))
}

/// Upstream template path for the managed project path `rel`, or `None` when `rel`
/// has no template.
pub fn map_to_upstream(upstream_root: &Path, rel: &Path) -> Option<PathBuf> {
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

/// The `.local` override of the managed path `rel`, relative to the project root
/// (e.g. `mise.local.toml`, `.agents/AGENTS.local.md`, `.gitignore.local`).
pub fn local_override_path(rel: &Path) -> PathBuf {
    let file_name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    if let Some(ext) = rel.extension().and_then(|e| e.to_str()) {
        let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        parent.join(format!("{stem}.local.{ext}"))
    } else {
        parent.join(format!("{file_name}.local"))
    }
}

fn check_for_override(local_root: &Path, rel: &Path) -> bool {
    // Check .local extension (e.g. mise.local.toml, AGENTS.local.md)
    if local_root.join(local_override_path(rel)).exists() {
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

    #[cfg(unix)]
    #[test]
    fn test_refuse_symlink_under_checks_parents_below_root_only() {
        use std::os::unix::fs::symlink;
        let temp = tempdir().unwrap();
        let real = temp.path().join("real");
        fs::create_dir_all(real.join("hooks")).unwrap();
        fs::write(real.join("hooks/pre-push"), "hook").unwrap();

        // Negative control: a regular path is accepted.
        refuse_symlink_under(&real, &real.join("hooks/pre-push")).unwrap();
        // The root itself may be a link (a symlinked templates checkout).
        let linked_root = temp.path().join("linked_root");
        symlink(&real, &linked_root).unwrap();
        refuse_symlink_under(&linked_root, &linked_root.join("hooks/pre-push")).unwrap();

        // A linked directory below the root is refused, naming the link.
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("pre-push"), "host").unwrap();
        fs::remove_dir_all(real.join("hooks")).unwrap();
        symlink(&outside, real.join("hooks")).unwrap();
        let err = refuse_symlink_under(&real, &real.join("hooks/pre-push")).unwrap_err();
        assert!(
            err.contains(&real.join("hooks").display().to_string()),
            "{err}"
        );
        let err = read_if_present(&real, &real.join("hooks/pre-push")).unwrap_err();
        assert!(err.contains("symlink"), "{err}");
    }

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
                // Discovery stops at the temp dir, so a TMPDIR inside a checkout never
                // puts a fixture inside that repository.
                .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
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
                (
                    "GIT_CEILING_DIRECTORIES".into(),
                    std::env::temp_dir().into(),
                ),
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
        let ceiling = std::env::temp_dir();
        assert_eq!(
            envs.get(OsStr::new("GIT_CEILING_DIRECTORIES")),
            Some(&Some(ceiling.as_os_str())),
            "fixture git discovery must stop at the temp dir"
        );
        assert!(
            fixture.gitconfig_env().iter().any(
                |(key, value)| key == "GIT_CEILING_DIRECTORIES" && value == ceiling.as_os_str()
            ),
            "engine git spawns in tests must stop discovery at the temp dir too"
        );
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

    /// Applied-file count of a successful pull that ended no tracking; panics otherwise.
    fn applied_only(result: PullResult) -> usize {
        match result {
            PullResult::Success(report) => {
                assert!(
                    report.deleted.is_empty(),
                    "unexpected deletions: {report:?}"
                );
                assert!(
                    report.locally_deleted_pending.is_empty(),
                    "unexpected pending local deletions: {report:?}"
                );
                report.applied.len()
            }
            other => panic!("expected a successful pull, got {other:?}"),
        }
    }

    #[test]
    fn test_sync_status_deletions() {
        let base = Some("base version");
        let cases = [
            // (ancestor, local, upstream, expected): state table rows 1-5 and 11.
            (base, None, base, SyncStatus::LocalDeleted),
            (base, base, None, SyncStatus::UpstreamDeleted),
            (
                base,
                None,
                Some("upstream modified"),
                SyncStatus::Conflicted,
            ),
            (base, Some("local edit"), None, SyncStatus::Conflicted),
            (base, None, None, SyncStatus::BothDeleted),
            (None, None, None, SyncStatus::Absent),
            // Negative control: an empty local file is present and edited, not deleted.
            (base, Some(""), None, SyncStatus::Conflicted),
        ];
        for (ancestor, local, upstream, expected) in cases {
            assert_eq!(
                SyncEngine::compare_3way(ancestor, local, upstream, false),
                expected,
                "ancestor {ancestor:?}, local {local:?}, upstream {upstream:?}"
            );
        }
        // A file that exists nowhere is Absent even with a local override (invariant I2).
        assert_eq!(
            SyncEngine::compare_3way(None, None, None, true),
            SyncStatus::Absent
        );
    }

    /// A project whose `.gitconfig` is recorded at `base` and whose local copy is
    /// `local`, plus an upstream tree with no `.gitconfig` template.
    fn upstream_deletion_fixture(
        base: &str,
        local: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir) {
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            SyncEngine::fingerprint(base.as_bytes()),
            Some(base.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();
        fs::write(local_dir.path().join(".gitconfig"), local).unwrap();
        (local_dir, upstream_dir)
    }

    #[test]
    fn test_pull_upstream_deletion() {
        let engine = SyncEngine::new();
        let base = "[core]\n\tautocrlf = input\n";
        let (local, upstream) = upstream_deletion_fixture(base, base);
        let ctx = crate::template::TemplateContext::detect_for_project(local.path(), "p", "rust");

        let result = engine.pull_updates(local.path(), upstream.path(), &ctx);

        assert_eq!(
            result,
            PullResult::Success(PullReport {
                deleted: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            !local.path().join(".gitconfig").exists(),
            "local copy must be deleted"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.get_ancestor_hash(Path::new(".gitconfig")), None);
        let tombstone = state
            .tombstones
            .get(".gitconfig")
            .expect("upstream deletion must leave a tombstone");
        assert_eq!(
            tombstone.deleted_hash,
            SyncEngine::fingerprint(base.as_bytes())
        );
        assert!(!tombstone.kept_unmanaged);

        // Negative control (D-13): an edited local copy is a conflict: kept byte-identical,
        // sidecar written, recorded against a deleted upstream, tracking unchanged.
        let edited = "[core]\n\tautocrlf = false\n";
        let (local, upstream) = upstream_deletion_fixture(base, edited);
        assert_eq!(
            engine.pull_updates(local.path(), upstream.path(), &ctx),
            PullResult::Success(PullReport {
                conflicts: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            edited
        );
        assert!(local.path().join(".gitconfig.conflict").is_file());
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        let record = state
            .conflict(Path::new(".gitconfig"))
            .expect("conflict record");
        assert_eq!(record.upstream_hash, None);
        assert_eq!(state.tombstone(Path::new(".gitconfig")), None);
        assert_eq!(
            state.get_ancestor_hash(Path::new(".gitconfig")),
            Some(SyncEngine::fingerprint(base.as_bytes()).as_str())
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

    /// Records `ancestor` for `.gitconfig`, writes `local` and the upstream template.
    fn gitconfig_three_way(local_dir: &Path, upstream_dir: &Path, versions: [&str; 3]) {
        let [ancestor, local, upstream] = versions;
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            SyncEngine::fingerprint(ancestor.as_bytes()),
            Some(ancestor.to_string()),
        );
        state.save_to_dir(local_dir).unwrap();
        fs::write(local_dir.join(".gitconfig"), local).unwrap();
        write_upstream_gitconfig(upstream_dir, upstream.as_bytes());
    }

    #[test]
    fn test_pull_adjacent_edits_conflict() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();

        // D-02: edits on adjacent lines are a conflict, as in git.
        gitconfig_three_way(
            local_dir.path(),
            upstream_dir.path(),
            ["a=1\nb=1\n", "a=2\nb=1\n", "a=1\nb=2\n"],
        );
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::Conflicted,
            "adjacent-line edits must not auto-merge"
        );

        // Negative control: one unchanged line between the edits merges cleanly.
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        gitconfig_three_way(
            local_dir.path(),
            upstream_dir.path(),
            ["a=1\nx\nb=1\n", "a=2\nx\nb=1\n", "a=1\nx\nb=2\n"],
        );
        assert_eq!(
            scan_status(&engine, local_dir.path(), upstream_dir.path(), ".gitconfig"),
            SyncStatus::Merged
        );
        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );
        let pull = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(applied_only(pull), 1);
        assert_eq!(
            fs::read_to_string(local_dir.path().join(".gitconfig")).unwrap(),
            "a=2\nx\nb=2\n"
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
            applied_only(engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx)),
            1
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

    /// Asserts that the recorded `.gitconfig` ancestor is the fingerprint and text of
    /// `text_lf`, not a raw hash or CRLF content.
    fn assert_gitconfig_ancestor(local: &Path, text_lf: &str, case: &str) {
        let state = ProjectState::load_from_dir(local).unwrap();
        let rel = Path::new(".gitconfig");
        assert_eq!(
            state.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(text_lf.as_bytes()).as_str()),
            "{case}: ancestor hash"
        );
        assert_eq!(
            state.get_ancestor_content(rel),
            Some(text_lf),
            "{case}: ancestor content"
        );
    }

    #[test]
    fn test_ancestor_writers_record_fingerprint_and_lf_content() {
        let engine = SyncEngine::new();
        let base = "line1\nline2\nline3\nline4\nline5\n";
        let local_edit = "LOCAL1\nline2\nline3\nline4\nline5\n";
        let upstream_edit = "line1\nline2\nline3\nUPSTREAM4\nline5\n";
        // Every case writes CRLF bytes, so a raw hash or unnormalized content differs.
        let setup = |ancestor: &str, local: &str, upstream: &str| {
            let local_dir = tempdir().unwrap();
            let upstream_dir = tempdir().unwrap();
            let mut state = ProjectState::default();
            state.record_file(
                Path::new(".gitconfig"),
                SyncEngine::fingerprint(ancestor.as_bytes()),
                Some(ancestor.to_string()),
            );
            state.save_to_dir(local_dir.path()).unwrap();
            fs::write(local_dir.path().join(".gitconfig"), local).unwrap();
            write_upstream_gitconfig(upstream_dir.path(), upstream.as_bytes());
            (local_dir, upstream_dir)
        };
        let pull = |local: &Path, upstream: &Path| {
            let ctx = crate::template::TemplateContext::detect_for_project(local, "p", "rust");
            engine.pull_updates(local, upstream, &ctx)
        };

        // Pull, applied upstream change.
        let (local, upstream) = setup(base, &crlf(base), &crlf(upstream_edit));
        assert_eq!(applied_only(pull(local.path(), upstream.path())), 1);
        assert_gitconfig_ancestor(local.path(), upstream_edit, "pull applied");

        // Pull, clean merge: the upstream version becomes the ancestor.
        let (local, upstream) = setup(base, &crlf(local_edit), &crlf(upstream_edit));
        assert_eq!(applied_only(pull(local.path(), upstream.path())), 1);
        assert_gitconfig_ancestor(local.path(), upstream_edit, "pull merged");

        // Pull, converged copies with a stale ancestor are reconciled.
        let (local, upstream) = setup(base, &crlf(upstream_edit), &crlf(upstream_edit));
        assert_eq!(applied_only(pull(local.path(), upstream.path())), 0);
        assert_gitconfig_ancestor(local.path(), upstream_edit, "pull converged");

        // Push with immediate persistence records the published local version.
        let (local, upstream) = setup(base, &crlf(local_edit), base);
        let res = engine.push_updates(
            local.path(),
            upstream.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
            true,
        );
        assert!(
            matches!(
                res,
                PushResult::Success {
                    synced_files: 1,
                    ..
                }
            ),
            "push: {res:?}"
        );
        assert_gitconfig_ancestor(local.path(), local_edit, "push");
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
        assert_eq!(applied_only(pull_res), 3);

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

    /// Writes `bytes` as the upstream template `up_rel` (relative to the template root).
    fn write_upstream(upstream: &Path, up_rel: &str, bytes: &[u8]) {
        let path = upstream.join(up_rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Every file under `root` (relative to it) whose name ends in `.conflict`.
    fn conflict_files(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(entry.path());
                } else if entry.file_name().to_string_lossy().ends_with(".conflict") {
                    found.push(entry.path().strip_prefix(root).unwrap().to_path_buf());
                }
            }
        }
        found.sort();
        found
    }

    /// Asserts the sidecar of `rel` exists and carries the resolve hint and all four
    /// diff3 markers, each starting a line.
    fn assert_text_sidecar(local: &Path, rel: &str) -> String {
        let sidecar = fs::read_to_string(local.join(format!("{rel}.conflict")))
            .unwrap_or_else(|e| panic!("{rel}.conflict must exist: {e}"));
        for marker in [
            "\n<<<<<<< LOCAL\n",
            "\n||||||| ANCESTOR\n",
            "\n=======\n",
            "\n>>>>>>> UPSTREAM\n",
        ] {
            assert!(
                sidecar.contains(marker),
                "{rel}.conflict lacks {marker:?}: {sidecar}"
            );
        }
        assert!(
            sidecar.contains(&format!("ude sync resolve {rel}")),
            "{rel}.conflict lacks the resolve command: {sidecar}"
        );
        sidecar
    }

    #[test]
    fn test_pull_multiple_conflicts_sidecars() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let ctx =
            crate::template::TemplateContext::detect_for_project(local_dir.path(), "p", "rust");

        // (local path, upstream template, ancestor, local edit, upstream edit): both
        // sides change the same line, so each file conflicts.
        let conflicted = [
            (
                ".gitconfig",
                "projects/generic/gitconfig.repo",
                "[core]\n  autocrlf = false\n",
                "[core]\n  autocrlf = local_divergence\n",
                "[core]\n  autocrlf = upstream_divergence\n",
            ),
            (
                ".gitignore",
                "projects/generic/gitignore.repo",
                "target/\n",
                "local/\n",
                "upstream/\n",
            ),
        ];
        let mut state = ProjectState::default();
        for (rel, up_rel, base, local, upstream) in conflicted {
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(base.as_bytes()),
                Some(base.to_string()),
            );
            fs::write(local_dir.path().join(rel), local).unwrap();
            write_upstream(upstream_dir.path(), up_rel, upstream.as_bytes());
        }
        state.save_to_dir(local_dir.path()).unwrap();
        // A clean upstream addition in the same pull.
        write_upstream(
            upstream_dir.path(),
            "projects/generic/hooks/pre-commit",
            b"#!/bin/sh\nexit 0\n",
        );

        let res = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);

        assert_eq!(
            res,
            PullResult::Success(PullReport {
                applied: vec![PathBuf::from("scripts/hooks/pre-commit")],
                conflicts: vec![PathBuf::from(".gitconfig"), PathBuf::from(".gitignore")],
                ..PullReport::default()
            }),
            "every conflict is listed and the clean file is applied"
        );
        assert_eq!(
            fs::read_to_string(local_dir.path().join("scripts/hooks/pre-commit")).unwrap(),
            "#!/bin/sh\nexit 0\n"
        );
        let state = ProjectState::load_from_dir(local_dir.path()).unwrap();
        for (rel, _, base, local, upstream) in conflicted {
            assert_text_sidecar(local_dir.path(), rel);
            assert_eq!(
                fs::read_to_string(local_dir.path().join(rel)).unwrap(),
                local,
                "{rel}: a conflicted local file must stay byte-identical"
            );
            let record = state
                .conflict(Path::new(rel))
                .unwrap_or_else(|| panic!("{rel}: no conflict record"));
            assert_eq!(
                record.upstream_hash.as_deref(),
                Some(SyncEngine::fingerprint(upstream.as_bytes()).as_str()),
                "{rel}: conflict recorded against the wrong upstream"
            );
            assert_eq!(record.upstream_content.as_deref(), Some(upstream));
            assert_eq!(
                state.get_ancestor_hash(Path::new(rel)),
                Some(SyncEngine::fingerprint(base.as_bytes()).as_str()),
                "{rel}: the ancestor must not advance on a conflict"
            );
        }
        assert_eq!(
            conflict_files(local_dir.path()),
            vec![
                PathBuf::from(".gitconfig.conflict"),
                PathBuf::from(".gitignore.conflict")
            ]
        );

        // Negative control: a clean pull leaves no `.conflict` file anywhere.
        let (clean_local, clean_upstream) = staging_failure_fixture(false);
        assert_eq!(
            applied_only(engine.pull_updates(clean_local.path(), clean_upstream.path(), &ctx)),
            3
        );
        assert_eq!(conflict_files(clean_local.path()), Vec::<PathBuf>::new());
        // Positive control for the walker: a planted nested sidecar is found.
        fs::write(clean_local.path().join("scripts/hooks/x.conflict"), "x").unwrap();
        assert_eq!(
            conflict_files(clean_local.path()),
            vec![PathBuf::from("scripts/hooks/x.conflict")]
        );
    }

    /// Upstream versions written by [`staging_failure_fixture`], as (upstream path,
    /// local path, content).
    const STAGING_UPSTREAM: [(&str, &str, &str); 3] = [
        (
            "projects/generic/gitconfig.repo",
            ".gitconfig",
            "[core]\n  autocrlf = input\n",
        ),
        (
            "projects/generic/gitignore.repo",
            ".gitignore",
            "target/\n*.log\n",
        ),
        (
            "projects/generic/hooks/pre-commit",
            "scripts/hooks/pre-commit",
            "#!/bin/sh\nexit 0\n",
        ),
    ];

    /// Local project whose `.gitconfig` and `.gitignore` sit at their recorded ancestor
    /// while upstream changed both and adds `scripts/hooks/pre-commit`. With `collide`,
    /// `scripts` is a regular file, so staging the hook fails with ENOTDIR (whatever the
    /// uid) after `.gitconfig` and `.gitignore` were already staged.
    fn staging_failure_fixture(collide: bool) -> (tempfile::TempDir, tempfile::TempDir) {
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let mut state = ProjectState::default();
        for (rel, text) in [
            (".gitconfig", "[core]\n  autocrlf = false\n"),
            (".gitignore", "target/\n"),
        ] {
            fs::write(local.path().join(rel), text).unwrap();
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(text.as_bytes()),
                Some(text.to_string()),
            );
        }
        state.save_to_dir(local.path()).unwrap();
        if collide {
            fs::write(local.path().join("scripts"), "not a directory\n").unwrap();
        }
        for (up_rel, _, text) in STAGING_UPSTREAM {
            let path = upstream.path().join(up_rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        (local, upstream)
    }

    /// Every entry under `root` whose name starts with `.tmp` (tempfile's staging prefix).
    fn tmp_leftovers(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name().to_string_lossy().starts_with(".tmp") {
                    found.push(entry.path());
                }
                if entry.file_type().unwrap().is_dir() {
                    pending.push(entry.path());
                }
            }
        }
        found
    }

    #[test]
    fn test_pull_staging_failure_no_partial_writes() {
        let engine = SyncEngine::new();
        let (local, upstream) = staging_failure_fixture(true);
        let ctx =
            crate::template::TemplateContext::detect_for_project(local.path(), "test-proj", "rust");
        let watched = [".gitconfig", ".gitignore", STATE_FILE_REL];
        let before: Vec<Vec<u8>> = watched
            .iter()
            .map(|rel| fs::read(local.path().join(rel)).unwrap())
            .collect();

        let res = engine.pull_updates(local.path(), upstream.path(), &ctx);
        assert!(
            matches!(&res, PullResult::IoFailed { path, .. } if path == Path::new("scripts/hooks/pre-commit")),
            "staging failure must name the hook: {res:?}"
        );
        for (rel, bytes) in watched.iter().zip(&before) {
            assert_eq!(
                &fs::read(local.path().join(rel)).unwrap(),
                bytes,
                "{rel} was changed by a pull whose staging failed"
            );
        }
        assert_eq!(tmp_leftovers(local.path()), Vec::<PathBuf>::new());
        // Positive control for the leftover walker: a planted nested temp is found.
        fs::create_dir_all(local.path().join(".ude/probe")).unwrap();
        fs::write(local.path().join(".ude/probe/.tmpPROBE"), "x").unwrap();
        assert_eq!(tmp_leftovers(local.path()).len(), 1);

        // Negative control: without the collision all three files and the state apply.
        let (local, upstream) = staging_failure_fixture(false);
        let res = engine.pull_updates(local.path(), upstream.path(), &ctx);
        assert_eq!(applied_only(res), 3);
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        for (_, rel, text) in STAGING_UPSTREAM {
            assert_eq!(fs::read_to_string(local.path().join(rel)).unwrap(), text);
            assert_eq!(
                state.get_ancestor_hash(Path::new(rel)),
                Some(SyncEngine::fingerprint(text.as_bytes()).as_str()),
                "{rel} ancestor not updated"
            );
        }
        assert_eq!(tmp_leftovers(local.path()), Vec::<PathBuf>::new());
    }

    #[cfg(unix)]
    fn file_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    #[test]
    fn test_staged_write_modes() {
        use std::os::unix::fs::PermissionsExt;
        let engine = SyncEngine::new();

        // New files: a hook gets 0o755, a config gets 0o644 (never the 0o600 temp default).
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        for (up_rel, text) in [
            (
                "projects/generic/gitconfig.repo",
                "[core]\n  autocrlf = input\n",
            ),
            ("projects/generic/hooks/pre-commit", "#!/bin/sh\nexit 0\n"),
        ] {
            let path = upstream.path().join(up_rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let ctx =
            crate::template::TemplateContext::detect_for_project(local.path(), "test-proj", "rust");
        let res = engine.pull_updates(local.path(), upstream.path(), &ctx);
        assert_eq!(applied_only(res), 2);
        assert_eq!(
            file_mode(&local.path().join("scripts/hooks/pre-commit")),
            0o755
        );
        assert_eq!(file_mode(&local.path().join(".gitconfig")), 0o644);

        // Existing files rewritten by pull (local == ancestor, upstream changed).
        // (local path, upstream path, mode before, expected mode after)
        let cases = [
            (
                ".gitignore",
                "projects/generic/gitignore.repo",
                0o640,
                0o640,
            ),
            (
                ".gitconfig",
                "projects/generic/gitconfig.repo",
                0o600,
                0o600,
            ),
            (
                "scripts/hooks/pre-push",
                "projects/generic/hooks/pre-push",
                0o644,
                0o755,
            ),
            (
                "scripts/hooks/commit-msg",
                "projects/generic/hooks/commit-msg",
                0o700,
                0o700,
            ),
        ];
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let mut state = ProjectState::default();
        for (rel, up_rel, before, _) in cases {
            let old = format!("old {rel}\n");
            let path = local.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &old).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(before)).unwrap();
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(old.as_bytes()),
                Some(old.clone()),
            );
            let up_path = upstream.path().join(up_rel);
            fs::create_dir_all(up_path.parent().unwrap()).unwrap();
            fs::write(up_path, format!("new {rel}\n")).unwrap();
        }
        state.save_to_dir(local.path()).unwrap();
        let res = engine.pull_updates(local.path(), upstream.path(), &ctx);
        assert_eq!(applied_only(res), 4);
        for (rel, _, before, expected) in cases {
            let path = local.path().join(rel);
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                format!("new {rel}\n"),
                "{rel} was not rewritten"
            );
            let after = file_mode(&path);
            assert_eq!(after, expected, "{rel}: {before:o} became {after:o}");
            // Negative control: the 0o600 temp default never leaks onto a pulled file.
            assert!(
                after != 0o600 || before == 0o600,
                "{rel} got the temp default 0o600"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_file_mode_for() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempdir().unwrap();
        let mode_for = |rel: &str| staging::file_mode_for(Path::new(rel), &root.path().join(rel));

        // New files: every executable path family gets 0o755, anything else 0o644.
        for rel in [
            "scripts/hooks/pre-commit",
            "scratch/new-worktree.sh",
            "scripts/check-in-container.sh",
        ] {
            assert_eq!(mode_for(rel), staging::EXECUTABLE_MODE, "new {rel}");
        }
        for rel in [".gitconfig", "scripts/other.sh", "mise.toml"] {
            assert_eq!(mode_for(rel), staging::REGULAR_MODE, "new {rel}");
        }

        // Existing files: (path, mode before, expected mode).
        let cases = [
            ("scripts/hooks/pre-commit", 0o644, 0o755),
            ("scripts/hooks/pre-push", 0o755, 0o755),
            ("scripts/hooks/commit-msg", 0o640, 0o750),
            ("scratch/new-worktree.sh", 0o600, 0o700),
            ("scripts/check-in-container.sh", 0o000, 0o100),
            (".gitconfig", 0o600, 0o600),
            (".gitignore", 0o755, 0o755),
        ];
        for (rel, before, expected) in cases {
            let path = root.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(before)).unwrap();
            assert_eq!(mode_for(rel), expected, "{rel} at {before:o}");
        }

        // A symlinked target is never followed: its mode reads as absent.
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "x\n").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&victim, root.path().join("mise.toml")).unwrap();
        assert_eq!(mode_for("mise.toml"), staging::REGULAR_MODE);
    }

    #[test]
    fn test_staging_abort_removes_created_dirs() {
        let root = tempdir().unwrap();
        // A pre-existing empty directory is not ours to remove.
        fs::create_dir(root.path().join("scratch")).unwrap();
        // A directory where a file is expected makes the last staging step fail.
        fs::create_dir(root.path().join("mise.toml")).unwrap();

        let mut transaction = staging::StagedTransaction::new(root.path());
        for (rel, mode) in [
            ("scripts/hooks/pre-commit", 0o755),
            (".agents/AGENTS.md", 0o644),
            ("scratch/new-worktree.sh", 0o755),
        ] {
            transaction
                .stage_write(Path::new(rel), b"content\n", mode)
                .unwrap();
        }
        assert!(root.path().join("scripts/hooks").is_dir());
        assert!(root.path().join(".agents").is_dir());
        let err = transaction
            .stage_write(Path::new("mise.toml"), b"x\n", 0o644)
            .unwrap_err();
        assert!(err.contains("directory"), "{err}");
        drop(transaction);

        assert!(!root.path().join("scripts").exists());
        assert!(!root.path().join(".agents").exists());
        assert!(root.path().join("scratch").is_dir());
        assert_eq!(tmp_leftovers(root.path()), Vec::<PathBuf>::new());

        // Negative control: a committed transaction keeps the directories it created.
        let root = tempdir().unwrap();
        let mut transaction = staging::StagedTransaction::new(root.path());
        transaction
            .stage_write(Path::new("scripts/hooks/pre-commit"), b"hook\n", 0o755)
            .unwrap();
        transaction
            .stage_write(Path::new(".agents/AGENTS.md"), b"agents\n", 0o644)
            .unwrap();
        let applied = transaction.commit().unwrap();
        assert_eq!(
            applied,
            vec![
                PathBuf::from("scripts/hooks/pre-commit"),
                PathBuf::from(".agents/AGENTS.md")
            ]
        );
        assert_eq!(
            fs::read_to_string(root.path().join("scripts/hooks/pre-commit")).unwrap(),
            "hook\n"
        );
        assert!(root.path().join(".agents/AGENTS.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn test_commit_revalidates_symlink_before_rename() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "outside\n").unwrap();
        let target = root.path().join(".gitconfig");
        fs::write(&target, "old\n").unwrap();

        let mut transaction = staging::StagedTransaction::new(root.path());
        transaction
            .stage_write(Path::new(".gitconfig"), b"new\n", 0o644)
            .unwrap();
        fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(&victim, &target).unwrap();

        let err = transaction.commit().unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert!(err.contains("0 of 1"), "{err}");
        assert!(
            fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the planted symlink must not be replaced"
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "outside\n");
        assert_eq!(tmp_leftovers(root.path()), Vec::<PathBuf>::new());

        // Negative control: without the swap the same commit succeeds.
        let root = tempdir().unwrap();
        let target = root.path().join(".gitconfig");
        fs::write(&target, "old\n").unwrap();
        let mut transaction = staging::StagedTransaction::new(root.path());
        transaction
            .stage_write(Path::new(".gitconfig"), b"new\n", 0o644)
            .unwrap();
        assert_eq!(
            transaction.commit().unwrap(),
            vec![PathBuf::from(".gitconfig")]
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    }

    /// A project and upstream tree for `.gitconfig`: `ancestor` is recorded when given,
    /// `tombstone` writes a tombstone instead (no record), and `local` / `upstream` are
    /// written when given (absent otherwise).
    fn deletion_fixture(
        ancestor: Option<&str>,
        tombstone: Option<&str>,
        local: Option<&str>,
        upstream: Option<&str>,
    ) -> (tempfile::TempDir, tempfile::TempDir) {
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let mut state = ProjectState::default();
        let rel = Path::new(".gitconfig");
        if let Some(base) = ancestor {
            state.record_file(
                rel,
                SyncEngine::fingerprint(base.as_bytes()),
                Some(base.into()),
            );
        }
        if let Some(deleted) = tombstone {
            state.record_tombstone(rel, SyncEngine::fingerprint(deleted.as_bytes()), false);
        }
        state.save_to_dir(local_dir.path()).unwrap();
        if let Some(text) = local {
            fs::write(local_dir.path().join(rel), text).unwrap();
        }
        if let Some(text) = upstream {
            write_upstream_gitconfig(upstream_dir.path(), text.as_bytes());
        }
        (local_dir, upstream_dir)
    }

    fn pull_dirs(engine: &SyncEngine, local: &Path, upstream: &Path) -> PullResult {
        let ctx = crate::template::TemplateContext::detect_for_project(local, "p", "rust");
        engine.pull_updates(local, upstream, &ctx)
    }

    #[test]
    fn test_pull_keeps_local_deletion() {
        let engine = SyncEngine::new();
        let base = "[core]\n\tautocrlf = input\n";
        let (local, upstream) = deletion_fixture(Some(base), None, None, Some(base));

        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                locally_deleted_pending: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            !local.path().join(".gitconfig").exists(),
            "pull must not recreate it"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert!(state.get_ancestor_hash(Path::new(".gitconfig")).is_some());

        // Negative control (D-15 second clause): upstream changed meanwhile.
        let (local, upstream) =
            deletion_fixture(Some(base), None, None, Some("[core]\n\tautocrlf = false\n"));
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::Conflicted
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                conflicts: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            !local.path().join(".gitconfig").exists(),
            "pull must not recreate it"
        );
        assert!(local.path().join(".gitconfig.conflict").is_file());
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(
            state
                .conflict(Path::new(".gitconfig"))
                .expect("conflict record")
                .upstream_hash,
            Some(SyncEngine::fingerprint(b"[core]\n\tautocrlf = false\n"))
        );
    }

    #[test]
    fn test_both_deleted() {
        let engine = SyncEngine::new();
        let base = "[core]\n";
        let (local, upstream) = deletion_fixture(Some(base), None, None, None);
        let gitconfig = local.path().join(".gitconfig");
        let state_path = local.path().join(STATE_FILE_REL);

        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                deleted: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            !gitconfig.exists(),
            "a double deletion must not create a file"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.get_ancestor_hash(Path::new(".gitconfig")), None);
        let tombstone = state.tombstone(Path::new(".gitconfig")).expect("tombstone");
        assert_eq!(
            tombstone.deleted_hash,
            SyncEngine::fingerprint(base.as_bytes())
        );
        assert!(!tombstone.kept_unmanaged);

        // Row 11 afterwards: untracked, tombstoned, absent on both sides.
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::Absent
        );
        let state_before = fs::read(&state_path).unwrap();
        assert_eq!(
            applied_only(pull_dirs(&engine, local.path(), upstream.path())),
            0
        );
        assert_eq!(fs::read(&state_path).unwrap(), state_before);
        assert!(!gitconfig.exists());

        // Negative control: an empty local file is present and edited, not deleted.
        let (local, upstream) = deletion_fixture(Some(base), None, Some(""), None);
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::Conflicted
        );
        let result = pull_dirs(&engine, local.path(), upstream.path());
        assert_eq!(
            result,
            PullResult::Success(PullReport {
                conflicts: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            matches!(&result, PullResult::Success(r) if !r.deleted.contains(&PathBuf::from(".gitconfig")))
        );
        assert_eq!(fs::read(local.path().join(".gitconfig")).unwrap(), b"");
    }

    /// The scanned state of the managed file `rel`.
    fn scan_state(engine: &SyncEngine, local: &Path, upstream: &Path, rel: &str) -> FileSyncState {
        engine
            .scan_project(local, upstream, None)
            .unwrap()
            .into_iter()
            .find(|s| s.relative_path == Path::new(rel))
            .unwrap_or_else(|| panic!("{rel} is not a managed file"))
    }

    /// A pull report listing only `conflicts`.
    fn conflicts_only(paths: &[&str]) -> PullResult {
        PullResult::Success(PullReport {
            conflicts: paths.iter().map(PathBuf::from).collect(),
            ..PullReport::default()
        })
    }

    const CONFLICT_BASE: &str = "[core]\n\tautocrlf = input\n";
    const CONFLICT_LOCAL: &str = "[core]\n\tautocrlf = local\n";
    const CONFLICT_UPSTREAM: &str = "[core]\n\tautocrlf = upstream\n";

    /// A project whose `.gitconfig` text-conflicts, after the first conflicted pull.
    fn conflicted_gitconfig(engine: &SyncEngine) -> (tempfile::TempDir, tempfile::TempDir) {
        let (local, upstream) = deletion_fixture(
            Some(CONFLICT_BASE),
            None,
            Some(CONFLICT_LOCAL),
            Some(CONFLICT_UPSTREAM),
        );
        assert_eq!(
            pull_dirs(engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        (local, upstream)
    }

    fn upstream_gitconfig_path(upstream: &Path) -> PathBuf {
        upstream.join("projects/generic/gitconfig.repo")
    }

    #[test]
    fn test_pull_conflict_regenerates_sidecar() {
        let engine = SyncEngine::new();
        let (local, upstream) = conflicted_gitconfig(&engine);
        let sidecar_path = local.path().join(".gitconfig.conflict");
        let first = fs::read_to_string(&sidecar_path).unwrap();
        assert!(first.contains("autocrlf = upstream\n"), "{first}");

        // Upstream changes again while the conflict is open.
        let newer = "[core]\n\tautocrlf = newer\n";
        write_upstream_gitconfig(upstream.path(), newer.as_bytes());
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::ConflictPending
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let second = fs::read_to_string(&sidecar_path).unwrap();
        assert!(second.contains("autocrlf = newer\n"), "{second}");
        assert!(!second.contains("autocrlf = upstream\n"), "{second}");
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            CONFLICT_LOCAL
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        let record = state.conflict(Path::new(".gitconfig")).expect("record");
        assert_eq!(
            record.upstream_hash,
            Some(SyncEngine::fingerprint(newer.as_bytes()))
        );
        assert_eq!(record.upstream_content.as_deref(), Some(newer));
        assert_eq!(
            state.get_ancestor_hash(Path::new(".gitconfig")),
            Some(SyncEngine::fingerprint(CONFLICT_BASE.as_bytes()).as_str())
        );

        // Hand-deleting the sidecar does not resolve: the next pull recreates it.
        fs::remove_file(&sidecar_path).unwrap();
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::ConflictPending
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), second);

        // Negative control: no upstream change, byte-identical sidecar.
        pull_dirs(&engine, local.path(), upstream.path());
        assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), second);
    }

    #[test]
    fn test_conflict_auto_clears_on_convergence() {
        let engine = SyncEngine::new();
        let (local, upstream) = conflicted_gitconfig(&engine);
        let sidecar_path = local.path().join(".gitconfig.conflict");

        // Negative control: local still differs from upstream, so nothing clears.
        let st = scan_state(&engine, local.path(), upstream.path(), ".gitconfig");
        assert_eq!(
            (st.status, st.conflict_open),
            (SyncStatus::ConflictPending, true)
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        assert!(sidecar_path.is_file());
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert!(state.conflict(Path::new(".gitconfig")).is_some());

        // The user makes the local file identical to upstream (CRLF: same fingerprint).
        fs::write(
            local.path().join(".gitconfig"),
            CONFLICT_UPSTREAM.replace('\n', "\r\n"),
        )
        .unwrap();
        let st = scan_state(&engine, local.path(), upstream.path(), ".gitconfig");
        assert_eq!((st.status, st.conflict_open), (SyncStatus::InSync, true));

        // Nothing else changes in this pull: the convergence clear alone is the work.
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                converged: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(
            !sidecar_path.exists(),
            "sidecar must be removed on convergence"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.conflict(Path::new(".gitconfig")), None);
        assert_eq!(
            state.get_ancestor_hash(Path::new(".gitconfig")),
            Some(SyncEngine::fingerprint(CONFLICT_UPSTREAM.as_bytes()).as_str())
        );
        assert_eq!(
            state.get_ancestor_content(Path::new(".gitconfig")),
            Some(CONFLICT_UPSTREAM)
        );
        let st = scan_state(&engine, local.path(), upstream.path(), ".gitconfig");
        assert_eq!((st.status, st.conflict_open), (SyncStatus::InSync, false));
    }

    #[test]
    fn test_conflict_both_absent_converges() {
        let engine = SyncEngine::new();
        let rel = Path::new(".gitconfig");

        // Case A (row 14): D-13 conflict (upstream deleted, local edited), files record.
        let (local, upstream) =
            deletion_fixture(Some(CONFLICT_BASE), None, Some(CONFLICT_LOCAL), None);
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let sidecar_path = local.path().join(".gitconfig.conflict");
        // Negative control: the edited local file is still present, nothing clears.
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::ConflictPending
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert!(state.conflict(rel).is_some() && sidecar_path.is_file());
        assert!(state.get_ancestor_hash(rel).is_some());

        fs::remove_file(local.path().join(".gitconfig")).unwrap();
        let st = scan_state(&engine, local.path(), upstream.path(), ".gitconfig");
        assert_eq!(
            (st.status, st.conflict_open),
            (SyncStatus::BothDeleted, true)
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                deleted: vec![PathBuf::from(".gitconfig")],
                converged: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.conflict(rel), None);
        assert_eq!(state.get_ancestor_hash(rel), None);
        let tombstone = state.tombstone(rel).expect("D-16 tombstone");
        assert_eq!(
            tombstone.deleted_hash,
            SyncEngine::fingerprint(CONFLICT_BASE.as_bytes())
        );
        assert!(!tombstone.kept_unmanaged);
        assert!(!sidecar_path.exists());
        assert!(!local.path().join(".gitconfig").exists(), "no 0-byte file");

        // Case B (row 16): no files record; local and upstream added different content.
        let (local, upstream) =
            deletion_fixture(None, None, Some("local add\n"), Some("upstream add\n"));
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let sidecar_path = local.path().join(".gitconfig.conflict");
        assert!(sidecar_path.is_file());
        fs::remove_file(local.path().join(".gitconfig")).unwrap();
        fs::remove_file(upstream_gitconfig_path(upstream.path())).unwrap();
        let st = scan_state(&engine, local.path(), upstream.path(), ".gitconfig");
        assert_eq!((st.status, st.conflict_open), (SyncStatus::Absent, true));
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                converged: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.conflict(rel), None);
        assert_eq!(state.get_ancestor_hash(rel), None);
        assert_eq!(state.tombstone(rel), None, "no fingerprint, no tombstone");
        assert!(!sidecar_path.exists());
        assert!(!local.path().join(".gitconfig").exists());
    }

    #[test]
    fn test_pull_deletion_conflicts_write_sidecars() {
        let engine = SyncEngine::new();
        let rel = Path::new(".gitconfig");

        // D-13: upstream deleted, local edited.
        let (local, upstream) =
            deletion_fixture(Some(CONFLICT_BASE), None, Some(CONFLICT_LOCAL), None);
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let sidecar = fs::read_to_string(local.path().join(".gitconfig.conflict")).unwrap();
        assert!(
            sidecar.contains("# UPSTREAM: deleted upstream\n"),
            "{sidecar}"
        );
        assert!(!sidecar.contains("deleted locally"), "{sidecar}");
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            CONFLICT_LOCAL
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.conflict(rel).expect("record").upstream_hash, None);

        // D-15 second clause: local deleted, upstream changed.
        let (local, upstream) =
            deletion_fixture(Some(CONFLICT_BASE), None, None, Some(CONFLICT_UPSTREAM));
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let sidecar = fs::read_to_string(local.path().join(".gitconfig.conflict")).unwrap();
        assert!(sidecar.contains("# LOCAL: deleted locally\n"), "{sidecar}");
        assert!(!sidecar.contains("deleted upstream"), "{sidecar}");
        assert!(!local.path().join(".gitconfig").exists());

        // Open-record precedence (P1, row 12): a text conflict is open, the user restores
        // the ancestor locally and upstream deletes the template.
        let (local, upstream) = conflicted_gitconfig(&engine);
        fs::write(local.path().join(".gitconfig"), CONFLICT_BASE).unwrap();
        fs::remove_file(upstream_gitconfig_path(upstream.path())).unwrap();
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::ConflictPending
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            CONFLICT_BASE
        );
        let sidecar = fs::read_to_string(local.path().join(".gitconfig.conflict")).unwrap();
        assert!(
            sidecar.contains("# UPSTREAM: deleted upstream\n"),
            "{sidecar}"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.conflict(rel).expect("record").upstream_hash, None);
        assert_eq!(state.tombstone(rel), None);
        assert_eq!(
            state.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(CONFLICT_BASE.as_bytes()).as_str())
        );

        // Control (row 1): same files, no conflict record: UpstreamDeleted, pull deletes.
        let (local, upstream) =
            deletion_fixture(Some(CONFLICT_BASE), None, Some(CONFLICT_BASE), None);
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::UpstreamDeleted
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                deleted: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(!local.path().join(".gitconfig").exists());
    }

    #[test]
    fn test_pull_binary_conflict_sidecar() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let (base, local, upstream): (&[u8], &[u8], &[u8]) =
            (b"\xff\xfebase\n", b"\xff\xfelocal\n", b"\xff\xfeupstream\n");
        let mut state = ProjectState::default();
        state.record_file(Path::new(".gitconfig"), SyncEngine::fingerprint(base), None);
        state.save_to_dir(local_dir.path()).unwrap();
        fs::write(local_dir.path().join(".gitconfig"), local).unwrap();
        write_upstream_gitconfig(upstream_dir.path(), upstream);

        assert_eq!(
            pull_dirs(&engine, local_dir.path(), upstream_dir.path()),
            conflicts_only(&[".gitconfig"])
        );
        let sidecar = fs::read_to_string(local_dir.path().join(".gitconfig.conflict")).unwrap();
        for (label, bytes) in [("LOCAL", local), ("ANCESTOR", base), ("UPSTREAM", upstream)] {
            let line = format!(
                "# {label} fingerprint: {}\n",
                SyncEngine::fingerprint(bytes)
            );
            assert!(sidecar.contains(&line), "missing {line:?}: {sidecar}");
        }
        assert!(
            !sidecar.contains("<<<<<<<"),
            "binary sidecar has markers: {sidecar}"
        );
        assert_eq!(
            fs::read(local_dir.path().join(".gitconfig")).unwrap(),
            local
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_pull_refuses_symlinked_conflict_sidecar() {
        use std::os::unix::fs::symlink;
        let engine = SyncEngine::new();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "outside\n").unwrap();
        let fixture = |plant: bool| {
            let (local, upstream) = deletion_fixture(
                Some(CONFLICT_BASE),
                None,
                Some(CONFLICT_LOCAL),
                Some(CONFLICT_UPSTREAM),
            );
            write_upstream(
                upstream.path(),
                "projects/generic/hooks/pre-commit",
                b"#!/bin/sh\nexit 0\n",
            );
            if plant {
                symlink(&victim, local.path().join(".gitconfig.conflict")).unwrap();
            }
            (local, upstream)
        };

        let (local, upstream) = fixture(true);
        let state_before = fs::read(local.path().join(STATE_FILE_REL)).unwrap();
        let res = pull_dirs(&engine, local.path(), upstream.path());
        assert!(
            matches!(&res, PullResult::IoFailed { path, .. } if path == Path::new(".gitconfig.conflict")),
            "a symlinked sidecar must be refused: {res:?}"
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "outside\n");
        assert!(
            fs::symlink_metadata(local.path().join(".gitconfig.conflict"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            !local.path().join("scripts/hooks/pre-commit").exists(),
            "staging aborted, so no clean file may be applied"
        );
        assert_eq!(
            fs::read(local.path().join(STATE_FILE_REL)).unwrap(),
            state_before
        );

        // Negative control: without the symlink the sidecar is written and the hook applied.
        let (local, upstream) = fixture(false);
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            PullResult::Success(PullReport {
                applied: vec![PathBuf::from("scripts/hooks/pre-commit")],
                conflicts: vec![PathBuf::from(".gitconfig")],
                ..PullReport::default()
            })
        );
        assert!(local.path().join(".gitconfig.conflict").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn test_sidecar_mode_follows_local() {
        use std::os::unix::fs::PermissionsExt;
        let engine = SyncEngine::new();

        // A 0o600 local file gives a 0o600 sidecar.
        let (local, upstream) = deletion_fixture(
            Some(CONFLICT_BASE),
            None,
            Some(CONFLICT_LOCAL),
            Some(CONFLICT_UPSTREAM),
        );
        fs::set_permissions(
            local.path().join(".gitconfig"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[".gitconfig"])
        );
        let mode = file_mode(&local.path().join(".gitconfig.conflict"));
        assert_ne!(
            mode, 0o644,
            "a 0o600 file's sidecar must not be world-readable"
        );
        assert_eq!(mode, 0o600);

        // A 0o755 hook gives a 0o644 sidecar (execute bits cleared).
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let hook = "scripts/hooks/pre-commit";
        let base = "#!/bin/sh\nexit 0\n";
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(hook),
            SyncEngine::fingerprint(base.as_bytes()),
            Some(base.to_string()),
        );
        state.save_to_dir(local.path()).unwrap();
        fs::create_dir_all(local.path().join("scripts/hooks")).unwrap();
        fs::write(local.path().join(hook), "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(local.path().join(hook), fs::Permissions::from_mode(0o755)).unwrap();
        write_upstream(
            upstream.path(),
            "projects/generic/hooks/pre-commit",
            b"#!/bin/sh\nexit 2\n",
        );
        assert_eq!(
            pull_dirs(&engine, local.path(), upstream.path()),
            conflicts_only(&[hook])
        );
        assert_eq!(
            file_mode(&local.path().join(format!("{hook}.conflict"))),
            0o644
        );
        assert_eq!(
            file_mode(&local.path().join(hook)),
            0o755,
            "hook mode untouched"
        );
    }

    #[test]
    fn test_unmanaged_and_tombstone_clear() {
        let engine = SyncEngine::new();
        let rel = Path::new(".gitconfig");
        // Row 7: tombstone, no record, present locally only.
        let (local, upstream) = deletion_fixture(None, Some("old\n"), Some("recreated\n"), None);
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::Unmanaged
        );
        let state_path = local.path().join(STATE_FILE_REL);
        let state_before = fs::read(&state_path).unwrap();
        assert_eq!(
            applied_only(pull_dirs(&engine, local.path(), upstream.path())),
            0
        );
        assert_eq!(fs::read(&state_path).unwrap(), state_before);
        assert_eq!(
            fs::read_to_string(local.path().join(rel)).unwrap(),
            "recreated\n"
        );

        // Row 9: upstream re-adds the file; pull writes and records it (I1 clears T).
        fs::remove_file(local.path().join(rel)).unwrap();
        write_upstream_gitconfig(upstream.path(), b"re-added\n");
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::UpstreamModified
        );
        assert_eq!(
            applied_only(pull_dirs(&engine, local.path(), upstream.path())),
            1
        );
        assert_eq!(
            fs::read_to_string(local.path().join(rel)).unwrap(),
            "re-added\n"
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(
            state.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(b"re-added\n").as_str())
        );
        assert_eq!(state.tombstone(rel), None);

        // Row 10: tombstone, equal local and upstream; pull reconciles the ancestor.
        let (local, upstream) =
            deletion_fixture(None, Some("old\n"), Some("same\n"), Some("same\n"));
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::InSync
        );
        assert_eq!(
            applied_only(pull_dirs(&engine, local.path(), upstream.path())),
            0
        );
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(
            state.get_ancestor_hash(rel),
            Some(SyncEngine::fingerprint(b"same\n").as_str())
        );
        assert_eq!(state.tombstone(rel), None);
    }

    /// The C-less rows (1-11) of the phase sync state table, restated row by row.
    /// A tombstone is consulted only without a files record (invariant I1).
    fn state_table_oracle(
        ancestor: Option<&str>,
        local: Option<&str>,
        upstream: Option<&str>,
        has_override: bool,
        tombstoned: bool,
    ) -> SyncStatus {
        use SyncStatus::*;
        let with_override = |status| if has_override { Overridden } else { status };
        match ancestor {
            Some(a) => match (local, upstream) {
                (Some(l), None) if l == a => UpstreamDeleted, // row 1
                (Some(_), None) => Conflicted,                // row 2
                (None, Some(u)) if u == a => LocalDeleted,    // row 3
                (None, Some(_)) => Conflicted,                // row 4
                (None, None) => BothDeleted,                  // row 5
                (Some(l), Some(u)) => {
                    // row 6: the existing three-way rules
                    if l == u {
                        with_override(InSync)
                    } else if l == a {
                        UpstreamModified
                    } else if u == a {
                        with_override(LocallyModified)
                    } else {
                        Conflicted
                    }
                }
            },
            None => match (local, upstream) {
                (Some(_), None) if tombstoned => Unmanaged, // row 7
                (Some(_), None) => with_override(LocallyModified), // row 8
                (None, Some(_)) => UpstreamModified,        // row 9
                (Some(l), Some(u)) if l == u => with_override(InSync), // row 10
                (Some(_), Some(_)) => Conflicted,           // row 10
                (None, None) => Absent,                     // row 11
            },
        }
    }

    #[test]
    fn test_compare_3way_hashes_exhaustive() {
        // Negative controls: the oracle agrees with hand-written rows.
        assert_eq!(
            state_table_oracle(Some("a"), Some("a"), None, false, false),
            SyncStatus::UpstreamDeleted
        );
        assert_eq!(
            state_table_oracle(Some("a"), None, None, true, false),
            SyncStatus::BothDeleted
        );
        assert_eq!(
            state_table_oracle(Some("a"), Some("b"), None, false, false),
            SyncStatus::Conflicted
        );

        let values = [None, Some("a"), Some("b"), Some("c")];
        let mut visited = 0;
        for ancestor in values {
            for local in values {
                for upstream in values {
                    for has_override in [false, true] {
                        for tombstoned in [false, true] {
                            visited += 1;
                            assert_eq!(
                                SyncEngine::compare_3way_hashes(
                                    ancestor,
                                    local,
                                    upstream,
                                    has_override,
                                    tombstoned
                                ),
                                state_table_oracle(
                                    ancestor,
                                    local,
                                    upstream,
                                    has_override,
                                    tombstoned
                                ),
                                "ancestor {ancestor:?}, local {local:?}, upstream {upstream:?}, \
                                 override {has_override}, tombstone {tombstoned}"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(visited, 256);
    }

    fn push_dirs(engine: &SyncEngine, local: &Path, upstream: &Path) -> PushResult {
        engine.push_updates(
            local,
            upstream,
            "/home/alice",
            "/home/alice/.config",
            false,
            true,
        )
    }

    #[test]
    fn test_push_local_deletion() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let gitconfig = "[core]\n\tautocrlf = input\n";
        let gitignore = "target/\n";
        let mut state = ProjectState::default();
        for (rel, text) in [(".gitconfig", gitconfig), (".gitignore", gitignore)] {
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(text.as_bytes()),
                Some(text.to_string()),
            );
        }
        state.save_to_dir(local.path()).unwrap();
        fs::write(local.path().join(".gitignore"), gitignore).unwrap();
        write_upstream_gitconfig(upstream.path(), gitconfig.as_bytes());
        let template = upstream.path().join("projects/generic/gitconfig.repo");
        let other = upstream.path().join("projects/generic/gitignore.repo");
        fs::write(&other, gitignore).unwrap();

        let result = push_dirs(&engine, local.path(), upstream.path());

        let PushResult::Success {
            staged_files,
            deleted,
            ..
        } = &result
        else {
            panic!("push: {result:?}");
        };
        assert_eq!(deleted, &vec![PathBuf::from(".gitconfig")]);
        assert_eq!(staged_files, &vec![template.clone()]);
        assert!(!template.exists(), "the upstream template must be deleted");
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert_eq!(state.get_ancestor_hash(Path::new(".gitconfig")), None);
        assert_eq!(state.tombstone(Path::new(".gitconfig")), None);
        // Negative control: the untouched template stays byte-identical and tracked.
        assert_eq!(fs::read_to_string(&other).unwrap(), gitignore);
        assert!(state.get_ancestor_hash(Path::new(".gitignore")).is_some());
    }

    #[test]
    fn test_push_to_remote_deletion() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = tempdir().unwrap();
        let cache = tempdir().unwrap();
        git.run(bare.path(), &["init", "--bare", "-b", "main"]);
        let seed = tempdir().unwrap();
        git.run(seed.path(), &["init", "-b", "main"]);
        let generic = seed.path().join("templates/projects/generic");
        fs::create_dir_all(generic.join("hooks")).unwrap();
        fs::write(generic.join("gitconfig.repo"), "[core]\n").unwrap();
        fs::write(generic.join("gitignore.repo"), "target/\n").unwrap();
        fs::write(generic.join("hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        git.run(seed.path(), &["add", "-A"]);
        git.run(seed.path(), &["commit", "-m", "seed"]);
        git.run(
            seed.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git.run(seed.path(), &["push", "-u", "origin", "main"]);

        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let tmpl_dir = &cached.templates_dir;
        let checkout = cached.checkout_dir.as_path();
        let remote_tree = || git.output(bare.path(), &["ls-tree", "-r", "--name-only", "main"]);

        let gitconfig = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::remove_file(&gitconfig).unwrap();
        let pushed =
            engine.push_to_remote(checkout, "main", false, &[gitconfig], &cached.fetched_sha);
        assert!(pushed.is_ok(), "deletion push: {pushed:?}");
        let tree = remote_tree();
        assert!(!tree.contains("gitconfig.repo"), "{tree}");
        assert!(
            tree.contains("templates/projects/generic/gitignore.repo"),
            "{tree}"
        );

        // Negative control: a deletion does not exempt the other staged blobs from the
        // secret scan, and nothing is published.
        let gitignore = tmpl_dir.join("projects/generic/gitignore.repo");
        let hook = tmpl_dir.join("projects/generic/hooks/pre-commit");
        fs::remove_file(&gitignore).unwrap();
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        fs::write(&hook, format!("#!/bin/sh\n# {prefix}{body}\n")).unwrap();
        let err = push_failure(engine.push_to_remote(
            checkout,
            "main",
            false,
            &[gitignore, hook],
            &cached.fetched_sha,
        ));
        assert!(
            err.starts_with("Publication gate violation: secret detected"),
            "{err}"
        );
        assert_eq!(remote_tree(), tree);
    }

    #[test]
    fn test_parse_name_status_fails_closed() {
        assert_eq!(
            parse_name_status("D\ttemplates/a.repo"),
            Some(("D", PathBuf::from("templates/a.repo")))
        );
        assert_eq!(
            parse_name_status("M\ttemplates/a b.repo"),
            Some(("M", PathBuf::from("templates/a b.repo")))
        );
        for malformed in [
            "templates/a.repo",
            "\ttemplates/a.repo",
            "M\t",
            "M templates/a.repo",
        ] {
            assert_eq!(parse_name_status(malformed), None, "{malformed:?}");
        }
    }

    #[test]
    fn test_template_has_logic() {
        let dir = tempdir().unwrap();
        let check = |name: &str, text: Option<&str>| {
            let path = dir.path().join(name);
            if let Some(text) = text {
                fs::write(&path, text).unwrap();
            }
            template_has_logic(&path).unwrap()
        };
        for logic in [
            "{% if x %}a{% endif %}",
            "a = {{ project_name }}",
            "{# note #}",
        ] {
            assert!(check("logic.jinja", Some(logic)), "{logic}");
        }
        // Only the placeholders push writes back: a rendered push loses nothing.
        assert!(!check(
            "params.jinja",
            Some("home = \"{{ user.home }}\"\ncfg = \"{{ user.xdg_config }}\"\n")
        ));
        assert!(!check("plain.jinja", Some("[tools]\nrust = \"latest\"\n")));
        assert!(!check("absent.jinja", None));
        // Not a .jinja template: never treated as template logic.
        assert!(!check("gitconfig.repo", Some("{% if x %}a{% endif %}")));
    }

    #[test]
    fn test_push_never_overwrites_template_logic() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let template_text = "[tools]\n{% if true %}rust = \"latest\"\n{% endif %}\n";
        let template = upstream.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(&template, template_text).unwrap();
        // The ancestor is the template as push's scan renders it.
        let ctx = crate::template::TemplateContext::detect_for_project(
            local.path(),
            "project",
            "generic",
        );
        let rendered = crate::template::TemplateRenderer::new()
            .render_str(template_text, &ctx)
            .unwrap();
        let gitconfig = "[core]\n\tautocrlf = input\n";
        write_upstream_gitconfig(upstream.path(), gitconfig.as_bytes());
        let mut state = ProjectState::default();
        for (rel, text) in [("mise.toml", rendered.as_str()), (".gitconfig", gitconfig)] {
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(text.as_bytes()),
                Some(text.into()),
            );
        }
        state.save_to_dir(local.path()).unwrap();
        fs::write(
            local.path().join("mise.toml"),
            format!("{rendered}[env]\nFOO = \"1\"\n"),
        )
        .unwrap();
        let edited_gitconfig = format!("{gitconfig}\tfsmonitor = false\n");
        fs::write(local.path().join(".gitconfig"), &edited_gitconfig).unwrap();
        let mise_status = engine
            .scan_project(local.path(), upstream.path(), Some(&ctx))
            .unwrap()
            .into_iter()
            .find(|s| s.relative_path == Path::new("mise.toml"))
            .unwrap()
            .status;
        assert_eq!(mise_status, SyncStatus::LocallyModified);

        let result = push_dirs(&engine, local.path(), upstream.path());
        let PushResult::Success {
            skipped_templates, ..
        } = &result
        else {
            panic!("push: {result:?}");
        };
        assert_eq!(skipped_templates, &vec![PathBuf::from("mise.toml")]);
        assert_eq!(
            fs::read_to_string(&template).unwrap(),
            template_text,
            "push must not replace template logic with rendered text"
        );
        // Negative control: the non-template file in the same push is published.
        assert_eq!(
            fs::read_to_string(upstream.path().join("projects/generic/gitconfig.repo")).unwrap(),
            edited_gitconfig
        );
    }

    #[test]
    fn test_push_local_deletion_respects_publish_boundary() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let rel = "scratch/sync-personal-env.sh";
        let text = "#!/bin/sh\n";
        let gitconfig = "[core]\n\tautocrlf = input\n";
        let mut state = ProjectState::default();
        for (path, content) in [(rel, text), (".gitconfig", gitconfig)] {
            state.record_file(
                Path::new(path),
                SyncEngine::fingerprint(content.as_bytes()),
                Some(content.into()),
            );
        }
        state.save_to_dir(local.path()).unwrap();
        let template = upstream
            .path()
            .join("projects/agents/scratch/sync-personal-env.sh");
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(&template, text).unwrap();
        write_upstream_gitconfig(upstream.path(), gitconfig.as_bytes());
        let edited = format!("{gitconfig}\tfsmonitor = false\n");
        fs::write(local.path().join(".gitconfig"), &edited).unwrap();
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), rel),
            SyncStatus::LocalDeleted
        );

        // The boundary path is skipped, not published; the rest of the push goes ahead.
        let result = push_dirs(&engine, local.path(), upstream.path());
        let PushResult::Success {
            skipped_boundary,
            deleted,
            ..
        } = &result
        else {
            panic!("a boundary path must not abort the push: {result:?}");
        };
        assert_eq!(skipped_boundary, &vec![PathBuf::from(rel)]);
        assert_eq!(deleted, &Vec::<PathBuf>::new());
        assert_eq!(fs::read_to_string(&template).unwrap(), text);
        assert_eq!(
            fs::read_to_string(upstream.path().join("projects/generic/gitconfig.repo")).unwrap(),
            edited,
            "the eligible .gitconfig edit is published"
        );
        // The skipped deletion stays pending: its ancestor is kept, not dropped.
        let state = ProjectState::load_from_dir(local.path()).unwrap();
        assert!(state.get_ancestor_hash(Path::new(rel)).is_some());
    }

    #[test]
    fn test_tombstone_prevents_republish() {
        let engine = SyncEngine::new();
        let base = "[core]\n";
        let (local, upstream) = upstream_deletion_fixture(base, base);
        let pulled = pull_dirs(&engine, local.path(), upstream.path());
        assert!(
            matches!(&pulled, PullResult::Success(report) if report.deleted.len() == 1),
            "{pulled:?}"
        );
        fs::write(
            local.path().join(".gitconfig"),
            "[core]\n\trecreated = true\n",
        )
        .unwrap();
        let template = upstream.path().join("projects/generic/gitconfig.repo");

        let result = push_dirs(&engine, local.path(), upstream.path());
        let PushResult::Success { staged_files, .. } = &result else {
            panic!("push: {result:?}");
        };
        assert!(!staged_files.contains(&template), "{staged_files:?}");
        assert!(
            !template.exists(),
            "a tombstoned path must not be republished"
        );

        // Negative control: without the tombstone the same file is a new local file.
        fs::remove_file(local.path().join(STATE_FILE_REL)).unwrap();
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::LocallyModified
        );
        let result = push_dirs(&engine, local.path(), upstream.path());
        let PushResult::Success { staged_files, .. } = &result else {
            panic!("push: {result:?}");
        };
        assert_eq!(staged_files, &vec![template.clone()]);
        assert!(template.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_stage_delete_only_removes_regular_files_after_writes() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "outside\n").unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        std::os::unix::fs::symlink(&victim, root.path().join("link")).unwrap();
        let mut transaction = staging::StagedTransaction::new(root.path());
        for (rel, why) in [
            ("dir", "a directory"),
            ("link", "a symlink"),
            ("missing", "a missing path"),
            ("../escape", "an escaping path"),
        ] {
            assert!(
                transaction.stage_delete(Path::new(rel)).is_err(),
                "staging the deletion of {why} must fail"
            );
        }
        assert!(root.path().join("dir").is_dir());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "outside\n");

        // Negative control: a regular file is deleted, after the staged writes.
        fs::write(root.path().join("old"), "old\n").unwrap();
        transaction.stage_delete(Path::new("old")).unwrap();
        transaction
            .stage_write(Path::new("new"), b"new\n", 0o644)
            .unwrap();
        assert_eq!(transaction.commit().unwrap(), vec![PathBuf::from("new")]);
        assert!(!root.path().join("old").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("new")).unwrap(),
            "new\n"
        );

        // A staged deletion swapped for a symlink before commit is refused, and a
        // target that is already gone counts as deleted.
        let mut transaction = staging::StagedTransaction::new(root.path());
        fs::write(root.path().join("swap"), "x\n").unwrap();
        fs::write(root.path().join("gone"), "x\n").unwrap();
        transaction.stage_delete(Path::new("gone")).unwrap();
        transaction.stage_delete(Path::new("swap")).unwrap();
        fs::remove_file(root.path().join("gone")).unwrap();
        fs::remove_file(root.path().join("swap")).unwrap();
        std::os::unix::fs::symlink(&victim, root.path().join("swap")).unwrap();
        let err = transaction.commit().unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert!(fs::symlink_metadata(root.path().join("swap")).is_ok());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "outside\n");
    }

    #[test]
    fn test_unreadable_upstream_is_not_a_deletion() {
        let engine = SyncEngine::new();
        let base = "[core]\n";
        let (local, upstream) = upstream_deletion_fixture(base, base);
        // A directory where the template should be cannot be read as a file.
        fs::create_dir_all(upstream.path().join("projects/generic/gitconfig.repo")).unwrap();
        let ctx = crate::template::TemplateContext::detect_for_project(local.path(), "p", "rust");

        let result = engine.pull_updates(local.path(), upstream.path(), &ctx);
        assert!(
            matches!(&result, PullResult::ScanFailed { reason } if reason.contains("gitconfig.repo")),
            "{result:?}"
        );
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            base
        );

        // Negative control: a missing template is a deletion.
        fs::remove_dir(upstream.path().join("projects/generic/gitconfig.repo")).unwrap();
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::UpstreamDeleted
        );
    }

    /// Bytes of every regular file under `root`, keyed by relative path.
    fn tree_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut found = std::collections::BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let rel = path.strip_prefix(root).unwrap().to_path_buf();
                    found.insert(rel, fs::read(&path).unwrap());
                }
            }
        }
        found
    }

    /// The skip lists of a successful push (panics on any other result).
    fn push_skips(result: &PushResult) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let PushResult::Success {
            skipped_conflicts,
            skipped_merged,
            ..
        } = result
        else {
            panic!("expected a successful push, got {result:?}");
        };
        (skipped_conflicts.clone(), skipped_merged.clone())
    }

    #[test]
    fn test_push_skips_conflicts_and_merged() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let ignore_base = "target/\n*.log\n.env\nbuild/\ndist/\n";
        let hook_base = "#!/bin/sh\nexit 0\n";
        let mut state = ProjectState::default();
        for (rel, base) in [
            (".gitconfig", CONFLICT_BASE),
            (".gitignore", ignore_base),
            ("scripts/hooks/pre-commit", hook_base),
        ] {
            state.record_file(
                Path::new(rel),
                SyncEngine::fingerprint(base.as_bytes()),
                Some(base.to_string()),
            );
        }
        state.save_to_dir(local.path()).unwrap();

        // .gitconfig: both sides edit the same line (Conflicted). .gitignore: local
        // edits line 1, upstream line 4 (Merged). pre-commit: local edit only.
        write_upstream(local.path(), ".gitconfig", CONFLICT_LOCAL.as_bytes());
        write_upstream(
            local.path(),
            ".gitignore",
            b"target-local/\n*.log\n.env\nbuild/\ndist/\n",
        );
        let local_hook = "#!/bin/sh\necho local\nexit 0\n";
        write_upstream(
            local.path(),
            "scripts/hooks/pre-commit",
            local_hook.as_bytes(),
        );
        write_upstream(
            upstream.path(),
            "projects/generic/gitconfig.repo",
            CONFLICT_UPSTREAM.as_bytes(),
        );
        write_upstream(
            upstream.path(),
            "projects/generic/gitignore.repo",
            b"target/\n*.log\n.env\nbuild-up/\ndist/\n",
        );
        write_upstream(
            upstream.path(),
            "projects/generic/hooks/pre-commit",
            hook_base.as_bytes(),
        );
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitignore"),
            SyncStatus::Merged,
            "fixture: .gitignore must merge cleanly"
        );
        let local_before = tree_bytes(local.path());
        let upstream_before = tree_bytes(upstream.path());

        let result = push_dirs(&engine, local.path(), upstream.path());
        assert_eq!(
            push_skips(&result),
            (
                vec![PathBuf::from(".gitconfig")],
                vec![PathBuf::from(".gitignore")]
            )
        );
        let PushResult::Success { staged_files, .. } = &result else {
            unreachable!("push_skips checked the variant");
        };
        let hook_template = upstream.path().join("projects/generic/hooks/pre-commit");
        assert_eq!(staged_files, &vec![hook_template.clone()]);
        assert_eq!(fs::read_to_string(&hook_template).unwrap(), local_hook);

        // Nothing about a skipped file is written upstream or locally.
        let mut upstream_after = tree_bytes(upstream.path());
        let mut upstream_expected = upstream_before.clone();
        upstream_after.remove(Path::new("projects/generic/hooks/pre-commit"));
        upstream_expected.remove(Path::new("projects/generic/hooks/pre-commit"));
        assert_eq!(upstream_after, upstream_expected);
        let mut local_after = tree_bytes(local.path());
        let mut local_expected = local_before.clone();
        local_after.remove(Path::new(STATE_FILE_REL));
        local_expected.remove(Path::new(STATE_FILE_REL));
        assert_eq!(
            local_after, local_expected,
            "push never rewrites local files"
        );

        // A ConflictPending file (open record after a conflicted pull) is skipped too.
        let (pending_local, pending_upstream) = conflicted_gitconfig(&engine);
        assert_eq!(
            scan_status(
                &engine,
                pending_local.path(),
                pending_upstream.path(),
                ".gitconfig"
            ),
            SyncStatus::ConflictPending
        );
        let pending_template = fs::read(upstream_gitconfig_path(pending_upstream.path())).unwrap();
        let pending = push_dirs(&engine, pending_local.path(), pending_upstream.path());
        assert_eq!(
            push_skips(&pending),
            (vec![PathBuf::from(".gitconfig")], Vec::new())
        );
        assert_eq!(
            fs::read(upstream_gitconfig_path(pending_upstream.path())).unwrap(),
            pending_template
        );

        // Negative control: a push with only a modified hook skips nothing.
        let clean_local = tempdir().unwrap();
        let clean_upstream = tempdir().unwrap();
        write_upstream(
            clean_local.path(),
            "scripts/hooks/pre-commit",
            local_hook.as_bytes(),
        );
        let clean = push_dirs(&engine, clean_local.path(), clean_upstream.path());
        assert_eq!(push_skips(&clean), (Vec::new(), Vec::new()));
        assert!(matches!(
            clean,
            PushResult::Success {
                synced_files: 1,
                ..
            }
        ));
    }

    fn load_state(local: &Path) -> ProjectState {
        ProjectState::load_from_dir(local).unwrap()
    }

    fn gitconfig_conflict(local: &Path) -> Option<state::ConflictRecord> {
        load_state(local).conflict(Path::new(".gitconfig")).cloned()
    }

    fn resolve_gitconfig(engine: &SyncEngine, local: &Path) -> Result<ResolveOutcome, String> {
        engine.resolve_conflict(local, Path::new(".gitconfig"))
    }

    #[test]
    fn test_resolve() {
        let engine = SyncEngine::new();
        let (local, upstream) = conflicted_gitconfig(&engine);
        let sidecar = local.path().join(".gitconfig.conflict");
        assert!(sidecar.is_file(), "fixture: pull wrote the sidecar");
        let recorded = gitconfig_conflict(local.path()).expect("fixture: open conflict");

        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::KeptLocal)
        );
        let state = load_state(local.path());
        assert_eq!(
            state.get_ancestor_hash(Path::new(".gitconfig")),
            recorded.upstream_hash.as_deref(),
            "the recorded upstream version becomes the ancestor"
        );
        assert_eq!(
            state.get_ancestor_content(Path::new(".gitconfig")),
            Some(CONFLICT_UPSTREAM)
        );
        assert!(state.conflict(Path::new(".gitconfig")).is_none());
        assert!(!sidecar.exists(), "resolve removes the sidecar");
        assert_eq!(
            fs::read_to_string(local.path().join(".gitconfig")).unwrap(),
            CONFLICT_LOCAL,
            "resolve never rewrites the local file"
        );
        let pushed = push_dirs(&engine, local.path(), upstream.path());
        assert_eq!(push_skips(&pushed), (Vec::new(), Vec::new()));
        assert_eq!(
            fs::read_to_string(upstream_gitconfig_path(upstream.path())).unwrap(),
            CONFLICT_LOCAL,
            "the next push publishes the local content"
        );

        // Negative control: deleting the sidecar by hand does not resolve.
        let (hand_local, hand_upstream) = conflicted_gitconfig(&engine);
        fs::remove_file(hand_local.path().join(".gitconfig.conflict")).unwrap();
        assert!(gitconfig_conflict(hand_local.path()).is_some());
        let skipped = push_dirs(&engine, hand_local.path(), hand_upstream.path());
        assert_eq!(
            push_skips(&skipped),
            (vec![PathBuf::from(".gitconfig")], Vec::new())
        );
        assert_eq!(
            fs::read_to_string(upstream_gitconfig_path(hand_upstream.path())).unwrap(),
            CONFLICT_UPSTREAM
        );
    }

    /// A `.gitconfig` deletion conflict after the first conflicted pull.
    fn deletion_conflict(
        engine: &SyncEngine,
        local: Option<&str>,
        upstream: Option<&str>,
    ) -> (tempfile::TempDir, tempfile::TempDir) {
        let (local_dir, upstream_dir) =
            deletion_fixture(Some(CONFLICT_BASE), None, local, upstream);
        assert_eq!(
            pull_dirs(engine, local_dir.path(), upstream_dir.path()),
            conflicts_only(&[".gitconfig"])
        );
        assert!(local_dir.path().join(".gitconfig.conflict").is_file());
        (local_dir, upstream_dir)
    }

    /// Asserts resolve cleared the conflict and its sidecar and left `tombstone`.
    fn assert_resolved(local: &Path, tombstone: Option<(&str, bool)>) {
        let state = load_state(local);
        let rel = Path::new(".gitconfig");
        assert!(state.conflict(rel).is_none(), "conflict record cleared");
        assert!(
            !local.join(".gitconfig.conflict").exists(),
            "sidecar removed"
        );
        assert!(
            state.get_ancestor_hash(rel).is_none(),
            "files record removed"
        );
        let actual = state
            .tombstone(rel)
            .map(|t| (t.deleted_hash.clone(), t.kept_unmanaged));
        let expected =
            tombstone.map(|(text, kept)| (SyncEngine::fingerprint(text.as_bytes()), kept));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_resolve_deletion_conflicts() {
        let engine = SyncEngine::new();

        // D-13, local copy kept: unmanaged, never republished.
        let (local, upstream) = deletion_conflict(&engine, Some(CONFLICT_LOCAL), None);
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::KeptUnmanaged)
        );
        assert_resolved(local.path(), Some((CONFLICT_BASE, true)));
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::Unmanaged
        );
        let pushed = push_dirs(&engine, local.path(), upstream.path());
        assert_eq!(push_skips(&pushed), (Vec::new(), Vec::new()));
        assert!(matches!(
            pushed,
            PushResult::Success {
                synced_files: 0,
                ..
            }
        ));
        assert!(!upstream_gitconfig_path(upstream.path()).exists());

        // D-13 after the user deleted the local copy: tracking ends.
        let (local, _upstream) = deletion_conflict(&engine, Some(CONFLICT_LOCAL), None);
        fs::remove_file(local.path().join(".gitconfig")).unwrap();
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::DroppedDeleted)
        );
        assert_resolved(local.path(), Some((CONFLICT_BASE, false)));

        // D-15 (deleted locally, changed upstream), resolved with the file absent: the
        // next push deletes the upstream template.
        let (local, upstream) = deletion_conflict(&engine, None, Some(CONFLICT_UPSTREAM));
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::AcceptedLocalDeletion)
        );
        assert!(gitconfig_conflict(local.path()).is_none());
        assert!(!local.path().join(".gitconfig.conflict").exists());
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::LocalDeleted
        );
        let pushed = push_dirs(&engine, local.path(), upstream.path());
        let PushResult::Success { deleted, .. } = &pushed else {
            panic!("expected a successful push, got {pushed:?}");
        };
        assert_eq!(deleted, &vec![PathBuf::from(".gitconfig")]);
        assert!(!upstream_gitconfig_path(upstream.path()).exists());

        // The same conflict after the user restored the upstream content: kept, in sync.
        let (local, upstream) = deletion_conflict(&engine, None, Some(CONFLICT_UPSTREAM));
        fs::write(local.path().join(".gitconfig"), CONFLICT_UPSTREAM).unwrap();
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::KeptLocal)
        );
        assert_eq!(
            scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
            SyncStatus::InSync
        );

        // R4 with no files record: nothing to fingerprint, so no tombstone (row 16).
        for with_record in [false, true] {
            let local = tempdir().unwrap();
            let upstream = tempdir().unwrap();
            let mut state = ProjectState::default();
            if with_record {
                state.record_file(
                    Path::new(".gitconfig"),
                    SyncEngine::fingerprint(CONFLICT_BASE.as_bytes()),
                    Some(CONFLICT_BASE.to_string()),
                );
            }
            state.record_conflict(Path::new(".gitconfig"), None, None);
            state.save_to_dir(local.path()).unwrap();
            fs::write(local.path().join(".gitconfig.conflict"), "sidecar\n").unwrap();
            assert_eq!(
                resolve_gitconfig(&engine, local.path()),
                Ok(ResolveOutcome::DroppedDeleted),
                "with_record = {with_record}"
            );
            // Control: with a files record the ancestor fingerprint is tombstoned.
            let tombstone = with_record.then_some((CONFLICT_BASE, false));
            assert_resolved(local.path(), tombstone);
            assert_eq!(
                scan_status(&engine, local.path(), upstream.path(), ".gitconfig"),
                SyncStatus::Absent
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_sidecar_failure_keeps_conflict() {
        use std::os::unix::fs::symlink;
        let engine = SyncEngine::new();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "outside\n").unwrap();

        for case in ["symlink", "directory"] {
            let (local, _upstream) = conflicted_gitconfig(&engine);
            let sidecar = local.path().join(".gitconfig.conflict");
            fs::remove_file(&sidecar).unwrap();
            if case == "symlink" {
                symlink(&victim, &sidecar).unwrap();
            } else {
                fs::create_dir(&sidecar).unwrap();
            }
            let state_before = fs::read(local.path().join(STATE_FILE_REL)).unwrap();

            let err = resolve_gitconfig(&engine, local.path())
                .expect_err(&format!("a {case} sidecar must be refused"));
            assert!(err.contains(".gitconfig.conflict"), "{case}: {err}");
            assert!(err.contains("still unresolved"), "{case}: {err}");
            assert_eq!(
                fs::read(local.path().join(STATE_FILE_REL)).unwrap(),
                state_before,
                "{case}: state.json must be byte-identical"
            );
            assert!(gitconfig_conflict(local.path()).is_some(), "{case}");
            let meta = fs::symlink_metadata(&sidecar).unwrap();
            if case == "symlink" {
                assert!(meta.file_type().is_symlink(), "the symlink is left alone");
            } else {
                assert!(meta.is_dir(), "the directory is left alone");
            }
        }
        assert_eq!(fs::read_to_string(&victim).unwrap(), "outside\n");

        // Negative control: a regular sidecar resolves, so the errors above came from
        // the sidecar step and not from the setup.
        let (local, _upstream) = conflicted_gitconfig(&engine);
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::KeptLocal)
        );
        assert!(!local.path().join(".gitconfig.conflict").exists());
        assert!(gitconfig_conflict(local.path()).is_none());
    }

    #[test]
    fn test_resolve_rejects_invalid_targets() {
        let engine = SyncEngine::new();
        let outside = tempdir().unwrap();
        let (local, _upstream) = conflicted_gitconfig(&engine);
        let state_path = local.path().join(STATE_FILE_REL);
        let state_before = fs::read(&state_path).unwrap();
        let escape = local.path().join("../x");
        let outside_target = outside.path().join("x");
        let cases: [(PathBuf, &str); 6] = [
            (PathBuf::from("/etc/passwd"), "unsafe path"),
            (outside_target.clone(), "unsafe path"),
            (PathBuf::from("../x"), "unsafe path"),
            (PathBuf::from("./.gitconfig"), "unsafe path"),
            (PathBuf::from("README.md"), "not a UDE-managed file"),
            (PathBuf::from("mise.toml"), "No open conflict"),
        ];
        for (target, expected) in &cases {
            let err = engine
                .resolve_conflict(local.path(), target)
                .expect_err(&format!("{} must be refused", target.display()));
            assert!(err.contains(expected), "{}: {err}", target.display());
            assert_eq!(fs::read(&state_path).unwrap(), state_before);
        }
        assert!(!escape.exists() && !outside_target.exists());

        // Control: the open .gitconfig conflict was untouched by every refusal, and a
        // managed file with no open conflict is refused the same way after resolving.
        assert!(local.path().join(".gitconfig.conflict").is_file());
        assert_eq!(
            resolve_gitconfig(&engine, local.path()),
            Ok(ResolveOutcome::KeptLocal)
        );
        let resolved_state = fs::read(&state_path).unwrap();
        let err = resolve_gitconfig(&engine, local.path()).unwrap_err();
        assert!(err.contains("No open conflict for .gitconfig"), "{err}");
        assert_eq!(fs::read(&state_path).unwrap(), resolved_state);

        // A symlinked managed file is refused, not read through or treated as absent.
        #[cfg(unix)]
        {
            let (linked, _upstream) = conflicted_gitconfig(&engine);
            let victim = outside.path().join("victim");
            fs::write(&victim, CONFLICT_LOCAL).unwrap();
            fs::remove_file(linked.path().join(".gitconfig")).unwrap();
            std::os::unix::fs::symlink(&victim, linked.path().join(".gitconfig")).unwrap();
            let linked_state = linked.path().join(STATE_FILE_REL);
            let before = fs::read(&linked_state).unwrap();
            let err = resolve_gitconfig(&engine, linked.path()).unwrap_err();
            assert!(err.contains("symlink"), "{err}");
            assert_eq!(fs::read(&linked_state).unwrap(), before);
            assert!(linked.path().join(".gitconfig.conflict").is_file());
            assert_eq!(fs::read_to_string(&victim).unwrap(), CONFLICT_LOCAL);
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

    /// Pushes a local `scripts/hooks/pre-push` holding `local_bytes` (no upstream file,
    /// no state) and returns the result plus the upstream root.
    fn push_local_hook(local_bytes: &[u8]) -> (PushResult, tempfile::TempDir) {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let loc_hook = local_dir.path().join("scripts/hooks/pre-push");
        fs::create_dir_all(loc_hook.parent().unwrap()).unwrap();
        fs::write(&loc_hook, local_bytes).unwrap();
        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
            false,
            true,
        );
        (res, upstream_dir)
    }

    /// True when the upstream hooks directory is absent or holds no entry.
    fn upstream_hooks_empty(upstream_root: &Path) -> bool {
        let hooks = upstream_root.join("projects/generic/hooks");
        match fs::read_dir(&hooks) {
            Ok(mut entries) => entries.next().is_none(),
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }

    /// Asserts a staging refusal of the hook for a GitHub PAT on line 2 that names
    /// the rule and line, never the token body, and published nothing.
    fn assert_hook_secret_blocked(res: &PushResult, upstream_root: &Path, body: &str) {
        match res {
            PushResult::SecretBlocked { path, reason } => {
                assert_eq!(path, &PathBuf::from("scripts/hooks/pre-push"));
                assert!(reason.contains("GitHub Personal Access Token"), "{reason}");
                assert!(reason.contains("at line 2"), "{reason}");
                assert!(!reason.contains(body), "refusal leaked the token body");
            }
            other => panic!("Expected SecretBlocked, got {other:?}"),
        }
        assert!(
            upstream_hooks_empty(upstream_root),
            "nothing may be written upstream on a refusal"
        );
    }

    /// Asserts the hook was published byte-for-byte.
    fn assert_hook_published(res: &PushResult, upstream_root: &Path, local_bytes: &[u8]) {
        match res {
            PushResult::Success { synced_files, .. } => assert_eq!(*synced_files, 1),
            other => panic!("Expected Success, got {other:?}"),
        }
        let published = fs::read(upstream_root.join("projects/generic/hooks/pre-push")).unwrap();
        assert_eq!(published, local_bytes);
    }

    #[test]
    fn test_push_blocks_secret_in_non_utf8_file() {
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let mut dirty = vec![0xFF, 0xFE, b'\n'];
        dirty.extend_from_slice(format!("export TOKEN={prefix}{body}\n").as_bytes());
        assert!(
            std::str::from_utf8(&dirty).is_err(),
            "fixture must be non-UTF-8"
        );

        let (res, upstream) = push_local_hook(&dirty);
        assert_hook_secret_blocked(&res, upstream.path(), body);

        // Negative control: the same bytes without the token line are published as-is.
        let clean = vec![0xFF, 0xFE, b'\n'];
        let (res, upstream) = push_local_hook(&clean);
        assert_hook_published(&res, upstream.path(), &clean);
    }

    /// UTF-16LE with the BOM bytes 0xFF 0xFE.
    fn utf16le_bom(text: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }

    #[test]
    fn test_push_blocks_secret_in_utf16_file() {
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let dirty = utf16le_bom(&format!("#!/bin/sh\nexport TOKEN={prefix}{body}\n"));

        let (res, upstream) = push_local_hook(&dirty);
        assert_hook_secret_blocked(&res, upstream.path(), body);

        // Negative control: benign UTF-16 is published byte-for-byte.
        let clean = utf16le_bom("#!/bin/sh\necho ok\n");
        let (res, upstream) = push_local_hook(&clean);
        assert_hook_published(&res, upstream.path(), &clean);
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

    /// Project with a locally modified `.gitconfig` (ancestor = the upstream template)
    /// and a new `scripts/hooks/pre-commit`. With `collide`, the upstream
    /// `projects/generic/hooks` is a regular file, so staging the hook template fails
    /// after `gitconfig.repo` was staged.
    fn push_staging_fixture(collide: bool) -> (tempfile::TempDir, tempfile::TempDir) {
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let old = "[core]\n  autocrlf = false\n";
        let up_gitconfig = upstream.path().join("projects/generic/gitconfig.repo");
        fs::create_dir_all(up_gitconfig.parent().unwrap()).unwrap();
        fs::write(&up_gitconfig, old).unwrap();
        if collide {
            fs::write(
                upstream.path().join("projects/generic/hooks"),
                "not a directory\n",
            )
            .unwrap();
        }
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            SyncEngine::fingerprint(old.as_bytes()),
            Some(old.to_string()),
        );
        state.save_to_dir(local.path()).unwrap();
        fs::write(
            local.path().join(".gitconfig"),
            "[core]\n  autocrlf = input\n",
        )
        .unwrap();
        let hook = local.path().join("scripts/hooks/pre-commit");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        (local, upstream)
    }

    #[test]
    fn test_push_staging_failure_no_partial_writes() {
        let engine = SyncEngine::new();
        let push = |local: &Path, upstream: &Path| {
            engine.push_updates(
                local,
                upstream,
                "/home/alice",
                "/home/alice/.config",
                false,
                true,
            )
        };
        let (local, upstream) = push_staging_fixture(true);
        let up_gitconfig = upstream.path().join("projects/generic/gitconfig.repo");
        let template_before = fs::read(&up_gitconfig).unwrap();
        let state_before = fs::read(local.path().join(STATE_FILE_REL)).unwrap();

        let res = push(local.path(), upstream.path());
        assert!(
            matches!(&res, PushResult::IoFailed { path, .. } if path == Path::new("scripts/hooks/pre-commit")),
            "staging failure must name the hook: {res:?}"
        );
        assert_eq!(
            fs::read(&up_gitconfig).unwrap(),
            template_before,
            "gitconfig.repo was changed by a push whose staging failed"
        );
        assert_eq!(
            fs::read(local.path().join(STATE_FILE_REL)).unwrap(),
            state_before
        );
        assert_eq!(tmp_leftovers(upstream.path()), Vec::<PathBuf>::new());
        assert_eq!(tmp_leftovers(local.path()), Vec::<PathBuf>::new());

        // Negative control: without the collision both templates are written.
        let (local, upstream) = push_staging_fixture(false);
        match push(local.path(), upstream.path()) {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 2),
            other => panic!("Expected Success, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(upstream.path().join("projects/generic/gitconfig.repo")).unwrap(),
            "[core]\n  autocrlf = input\n"
        );
        assert_eq!(
            fs::read_to_string(upstream.path().join("projects/generic/hooks/pre-commit")).unwrap(),
            "#!/bin/sh\nexit 0\n"
        );
        assert_eq!(tmp_leftovers(upstream.path()), Vec::<PathBuf>::new());
    }

    #[test]
    fn test_push_updates_skips_publish_boundary_edit() {
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

        let PushResult::Success {
            skipped_boundary,
            synced_files,
            ..
        } = &res
        else {
            panic!("a boundary edit must be skipped, not abort the push: {res:?}");
        };
        assert_eq!(
            skipped_boundary,
            &vec![PathBuf::from("scratch/sync-personal-env.sh")]
        );
        assert_eq!(*synced_files, 0);
        assert!(
            !upstream_dir
                .path()
                .join("projects/agents/scratch/sync-personal-env.sh")
                .exists(),
            "a boundary path must never be published"
        );
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
        // A ude template tree has projects/; ensure_remote_repo refuses any other repo.
        fs::create_dir_all(fake_templates.join("projects/generic")).unwrap();
        fs::write(
            fake_templates.join("projects/generic/gitconfig.repo"),
            "[core]\n",
        )
        .unwrap();

        git.run(fake_remote.path(), &["add", "-A"]);
        git.run(fake_remote.path(), &["commit", "-m", "init fake remote"]);

        let remote_url = fake_remote.path().to_string_lossy().to_string();
        let RemoteCache {
            templates_dir: resolved,
            lock: _lock1,
            ..
        } = engine
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

        let RemoteCache {
            templates_dir: resolved_develop,
            lock: _lock2,
            ..
        } = engine
            .ensure_remote_repo(&remote_url, "develop", cache_dir.path())
            .expect("remote cache resolution on develop branch");
        assert!(resolved_develop.exists());
        assert!(resolved_develop.join("develop.txt").exists());

        drop(_lock1);
        // Switching back to main must still work
        let RemoteCache {
            templates_dir: resolved_main,
            lock: _lock3,
            ..
        } = engine
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

    /// Scheduler state letter of `pid` (`S` = interruptible sleep), from /proc/<pid>/stat.
    #[cfg(target_os = "linux")]
    fn proc_state(pid: u32) -> Option<char> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name may contain spaces or parentheses; the state follows the
        // last `)`.
        stat[stat.rfind(')')? + 1..].trim_start().chars().next()
    }

    /// How long a spawned child may take to finish exec and its loader.
    #[cfg(target_os = "linux")]
    const CHILD_SETTLE_TIMEOUT: Duration = Duration::from_secs(10);

    /// Link targets of every open descriptor of `pid` once it runs `exe` and sleeps.
    /// Spawn can return before exec closes the close-on-exec descriptors, and the
    /// dynamic loader opens and closes files after exec; /proc/<pid>/fd is unreadable
    /// while exec runs. A process running `exe` in interruptible sleep is past all of
    /// that, so its descriptor table is final. A descriptor that still closes between
    /// listing and reading it is skipped.
    #[cfg(target_os = "linux")]
    fn settled_fd_targets(pid: u32, exe: &Path) -> Vec<PathBuf> {
        let deadline = std::time::Instant::now() + CHILD_SETTLE_TIMEOUT;
        loop {
            if fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|target| target == exe)
                && proc_state(pid) == Some('S')
                && let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd"))
            {
                return entries
                    .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
                    .collect();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "process {pid} never settled into sleeping as {}",
                exe.display()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
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
        let sleep_exe =
            fs::canonicalize(which::which("sleep").expect("sleep on PATH")).expect("resolve sleep");
        let temp = tempdir().unwrap();
        let lock_path = temp.path().join("inherit.lock");
        let _guard = RemoteLockGuard::acquire(&lock_path, 1).expect("acquire remote lock");
        let lock_file = fs::canonicalize(&lock_path).unwrap();

        let plain = spawn_sleep(Stdio::null());
        assert!(
            !settled_fd_targets(plain.0.id(), &sleep_exe).contains(&lock_file),
            "a spawned child inherited the lock file descriptor"
        );

        // Negative control: a child handed the lock file as stdin is seen holding it.
        let inheriting = spawn_sleep(Stdio::from(fs::File::open(&lock_path).unwrap()));
        assert!(
            settled_fd_targets(inheriting.0.id(), &sleep_exe).contains(&lock_file),
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
        assert_eq!(applied_only(pull_res), 0);
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
        let head = git.output(remote_repo.path(), &["rev-parse", "HEAD"]);
        let _ = engine.push_to_remote(remote_repo.path(), "main", false, &staged_allowlist, &head);

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
        assert_eq!(applied_only(pull_res), 1);
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
        let head = git.output(remote_repo.path(), &["rev-parse", "HEAD"]);
        let res =
            engine.push_to_remote(remote_repo.path(), "main", false, &staged_allowlist, &head);

        // Publication gate must detect secret in the staged blob and reject!
        assert!(
            res.is_err(),
            "Publication gate must fail when staged blob contains secret!"
        );
        let err_msg = push_failure(res);
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

        // 1. Base ancestor setup: a=1, x, b=1 (the unchanged middle line keeps the two
        // edited lines apart; adjacent-line edits are a conflict per D-02)
        let base_content = "a=1\nx\nb=1\n";
        let base_hash = SyncEngine::compute_hash(base_content.as_bytes());
        let mut state = ProjectState::default();
        state.record_file(
            Path::new("mise.toml"),
            base_hash,
            Some(base_content.to_string()),
        );
        state.save_to_dir(local_dir.path()).unwrap();

        // 2. Local has line 1 edited: a=2, x, b=1
        let loc_mise = local_dir.path().join("mise.toml");
        fs::write(&loc_mise, "a=2\nx\nb=1\n").unwrap();

        // 3. Upstream has line 3 edited: a=1, x, b=2
        let up_mise = upstream_dir.path().join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(up_mise.parent().unwrap()).unwrap();
        fs::write(&up_mise, "a=1\nx\nb=2\n").unwrap();

        let ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(),
            "test-proj",
            "rust",
        );

        // 4. First pull: orthogonal merge produces a=2, x, b=2
        let pull1 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(applied_only(pull1), 1);
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nx\nb=2\n",
            "First pull must merge orthogonal changes cleanly"
        );

        // 5. Second pull: with upstream UNCHANGED, local edit must NOT be erased!
        let pull2 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(applied_only(pull2), 0);
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nx\nb=2\n",
            "Second pull must preserve local edit and remain idempotent!"
        );

        // 6. Upstream now updates line 3 again to b=3
        fs::write(&up_mise, "a=1\nx\nb=3\n").unwrap();

        // 7. Third pull: merges new upstream change with preserved local edit to produce a=2, x, b=3
        let pull3 = engine.pull_updates(local_dir.path(), upstream_dir.path(), &ctx);
        assert_eq!(applied_only(pull3), 1);
        assert_eq!(
            fs::read_to_string(&loc_mise).unwrap(),
            "a=2\nx\nb=3\n",
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
        // A ude template tree has projects/; ensure_remote_repo refuses any other repo.
        let marker = remote_dir
            .path()
            .join("templates/projects/generic/gitconfig.repo");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, "[core]\n").unwrap();

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
        let cached = res.unwrap();
        assert!(cached.templates_dir.exists());
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
        let cached = engine
            .ensure_remote_repo(
                bare_remote.path().to_str().unwrap(),
                "main",
                cache_dir.path(),
            )
            .unwrap();
        let tmpl_dir = &cached.templates_dir;
        let remote_checkout = cached.checkout_dir.as_path();

        // Step 2: Push changes in PR mode (open_pr: true)
        let managed_gitconfig = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig,
            "[core]\n  autocrlf = false\n# pr edit\n",
        )
        .unwrap();
        let staged = vec![managed_gitconfig.clone()];
        let pr_res =
            engine.push_to_remote(remote_checkout, "main", true, &staged, &cached.fetched_sha);
        assert!(pr_res.is_ok(), "PR push must succeed: {:?}", pr_res.err());

        // Explicitly release first lock before acquiring lock in Step 3
        drop(cached);

        // Verify remote has a ude-sync branch
        let branch_out = git.output(bare_remote.path(), &["branch", "--list"]);
        assert!(
            branch_out.contains("ude-sync-"),
            "PR branch must be pushed to bare remote: {branch_out}"
        );

        // Step 3: Now run direct push on main (open_pr: false)
        // ensure_remote_repo must cleanly reset the working tree back to main
        let cached_2 = engine
            .ensure_remote_repo(
                bare_remote.path().to_str().unwrap(),
                "main",
                cache_dir.path(),
            )
            .unwrap();
        let tmpl_dir_2 = &cached_2.templates_dir;
        let remote_checkout_2 = cached_2.checkout_dir.as_path();
        let managed_gitconfig_2 = tmpl_dir_2.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig_2,
            "[core]\n  autocrlf = input\n# direct edit\n",
        )
        .unwrap();
        let staged_2 = vec![managed_gitconfig_2.clone()];
        let direct_res = engine.push_to_remote(
            remote_checkout_2,
            "main",
            false,
            &staged_2,
            &cached_2.fetched_sha,
        );
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

    /// The message of a hard push failure; panics on success or on `UpstreamMoved`.
    fn push_failure(res: Result<String, PushRemoteError>) -> String {
        match res {
            Err(PushRemoteError::Failed(e)) => e,
            other => panic!("expected a hard push failure, got {other:?}"),
        }
    }

    /// A bare remote seeded with one template on `main`
    /// (`templates/projects/generic/gitconfig.repo`).
    fn seeded_bare_remote(git: &GitFixture) -> tempfile::TempDir {
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
        bare
    }

    /// `git status` decides whether anything is published, so its failure (here: the
    /// directory is not a repository) is a hard error, never "nothing to push".
    #[test]
    fn test_stage_publication_fails_when_git_status_fails() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let rel = Path::new("projects/generic/gitconfig.repo");

        let not_repo = tempdir().unwrap();
        let probe = git
            .cmd(not_repo.path())
            .args(["rev-parse", "--git-dir"])
            .output()
            .unwrap();
        assert!(
            !probe.status.success(),
            "fixture dir must not be inside a git repository"
        );
        let edited = not_repo.path().join(rel);
        fs::create_dir_all(edited.parent().unwrap()).unwrap();
        fs::write(&edited, "[core]\n").unwrap();
        let err = engine
            .stage_publication(not_repo.path(), std::slice::from_ref(&edited))
            .expect_err("a failed git status must be an error");
        assert!(err.contains("git status"), "{err}");
        let pushed = engine.push_to_remote(not_repo.path(), "main", false, &[edited], "");
        assert!(
            matches!(pushed, Err(PushRemoteError::Failed(_))),
            "{pushed:?}"
        );

        // Negative control: in a real checkout a clean tree is "nothing to push" and an
        // edit is staged.
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let tracked = repo.path().join(rel);
        fs::create_dir_all(tracked.parent().unwrap()).unwrap();
        fs::write(&tracked, "[core]\n").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "seed"]);
        assert_eq!(
            engine.stage_publication(repo.path(), std::slice::from_ref(&tracked)),
            Ok(None)
        );
        fs::write(&tracked, "[core]\n  autocrlf = false\n").unwrap();
        let staged = engine
            .stage_publication(repo.path(), std::slice::from_ref(&tracked))
            .unwrap()
            .expect("the edit must be staged");
        assert!(staged.contains(rel), "{staged:?}");
    }

    /// `checkout_dir` is the cache checkout's git toplevel whether or not the remote
    /// keeps its templates under `templates/`.
    #[test]
    fn test_remote_cache_checkout_dir_is_git_toplevel() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        for (template, templates_subdir) in [
            ("templates/projects/generic/gitconfig.repo", true),
            ("projects/generic/gitconfig.repo", false),
        ] {
            let bare = tempdir().unwrap();
            git.run(bare.path(), &["init", "--bare", "-b", "main"]);
            let seed = tempdir().unwrap();
            git.run(seed.path(), &["init", "-b", "main"]);
            let tmpl = seed.path().join(template);
            fs::create_dir_all(tmpl.parent().unwrap()).unwrap();
            fs::write(&tmpl, "[core]\n").unwrap();
            git.run(seed.path(), &["add", "-A"]);
            git.run(seed.path(), &["commit", "-m", "seed"]);
            git.run(
                seed.path(),
                &["push", bare.path().to_str().unwrap(), "main"],
            );

            let cache = tempdir().unwrap();
            let cached = engine
                .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
                .unwrap();
            let toplevel = git.output(&cached.checkout_dir, &["rev-parse", "--show-toplevel"]);
            assert_eq!(
                fs::canonicalize(toplevel).unwrap(),
                fs::canonicalize(&cached.checkout_dir).unwrap(),
                "{template}: checkout_dir must be the git toplevel"
            );
            assert!(
                cached
                    .templates_dir
                    .join("projects/generic/gitconfig.repo")
                    .is_file()
            );
            // Negative control: `templates_dir` equals the checkout only without `templates/`.
            assert_eq!(
                cached.templates_dir != cached.checkout_dir,
                templates_subdir,
                "{template}: templates_dir {}",
                cached.templates_dir.display()
            );
        }
    }

    /// Writes an edit to the managed template of `cached` and pushes it directly,
    /// building on the head `cached` fetched.
    fn direct_push_edit(
        engine: &SyncEngine,
        cached: &RemoteCache,
        content: &str,
    ) -> Result<String, PushRemoteError> {
        let checkout = cached.checkout_dir.as_path();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, content).unwrap();
        engine.push_to_remote(checkout, "main", false, &[managed], &cached.fetched_sha)
    }

    #[test]
    fn test_direct_push_aborts_when_upstream_moved() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let bare_url = bare.path().to_str().unwrap();
        let remote_main = || git.output(bare.path(), &["rev-parse", "refs/heads/main"]);

        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare_url, "main", cache.path())
            .unwrap();
        assert_eq!(cached.fetched_sha, remote_main());
        let checkout = cached.checkout_dir.as_path();

        // Another machine publishes after UDE fetched.
        let other = tempdir().unwrap();
        git.run(other.path(), &["clone", bare_url, "."]);
        fs::write(other.path().join("unrelated.txt"), "other machine\n").unwrap();
        git.run(other.path(), &["add", "unrelated.txt"]);
        git.run(other.path(), &["commit", "-m", "concurrent publish"]);
        git.run(other.path(), &["push", "origin", "main"]);
        let moved = remote_main();
        assert_ne!(moved, cached.fetched_sha);

        let res = direct_push_edit(&engine, &cached, "[core]\n  autocrlf = false\n");
        assert_eq!(
            res,
            Err(PushRemoteError::UpstreamMoved {
                branch: "main".to_string()
            })
        );
        assert_eq!(
            remote_main(),
            moved,
            "the remote branch must keep the other push"
        );
        assert_eq!(
            git.output(checkout, &["rev-parse", "HEAD"]),
            cached.fetched_sha,
            "the cache commit was not rolled back"
        );
        drop(cached);

        // Negative control: with no concurrent push the same edit is published on top
        // of the fetched head.
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare_url, "main", cache.path())
            .unwrap();
        assert_eq!(cached.fetched_sha, moved);
        let res = direct_push_edit(&engine, &cached, "[core]\n  autocrlf = false\n");
        assert!(res.is_ok(), "push without a race: {res:?}");
        assert_ne!(remote_main(), moved, "the remote branch did not advance");
        assert_eq!(
            git.output(bare.path(), &["rev-parse", "refs/heads/main~1"]),
            cached.fetched_sha,
            "the published commit must build on the fetched head"
        );
    }

    #[test]
    fn test_classify_push_porcelain() {
        let fetch_first = "To /x\n!\tHEAD:refs/heads/main\t[rejected] (fetch first)\nDone\n";
        let non_ff = "To /x\n!\tHEAD:refs/heads/main\t[rejected] (non-fast-forward)\nDone\n";
        let hook =
            "To /x\n!\tHEAD:refs/heads/main\t[remote rejected] (pre-receive hook declined)\nDone\n";
        let accepted = "To /x\n \tHEAD:refs/heads/main\t5891ce6..6b9fbf5\nDone\n";
        assert_eq!(
            classify_push_porcelain(fetch_first),
            PushPorcelain::RejectedNonFastForward
        );
        assert_eq!(
            classify_push_porcelain(non_ff),
            PushPorcelain::RejectedNonFastForward
        );
        assert_eq!(classify_push_porcelain(hook), PushPorcelain::Other);
        assert_eq!(classify_push_porcelain(accepted), PushPorcelain::Accepted);
        assert_eq!(classify_push_porcelain(""), PushPorcelain::Accepted);
        // Negative controls: `[rejected]` outside a `!`-flagged line is not a rejection.
        for not_flagged in [
            "To /x [rejected]\nDone\n",
            " \tHEAD:refs/heads/main\t[rejected] (fetch first)\n",
            "=\tHEAD:refs/heads/main\t[rejected] (fetch first)\n",
            "!!\tHEAD:refs/heads/main\t[rejected] (fetch first)\n",
            "x!\tHEAD:refs/heads/main\t[rejected] (fetch first)\n",
        ] {
            assert_ne!(
                classify_push_porcelain(not_flagged),
                PushPorcelain::RejectedNonFastForward,
                "{not_flagged:?}"
            );
        }
        // `[rejected]` must be the summary (third field), as a prefix.
        assert_eq!(
            classify_push_porcelain("!\t[rejected]\tother summary\n"),
            PushPorcelain::Other
        );
        assert_eq!(
            classify_push_porcelain("!\tHEAD:refs/heads/main\tnot [rejected]\n"),
            PushPorcelain::Other
        );
        assert_eq!(classify_push_porcelain("!\n"), PushPorcelain::Other);
        // Order does not matter: a `[rejected]` line decides whichever `!` line comes first.
        for mixed in [
            "!\tA\t[rejected] (fetch first)\n!\tB\t[remote rejected] (hook declined)\n",
            "!\tB\t[remote rejected] (hook declined)\n!\tA\t[rejected] (fetch first)\n",
        ] {
            assert_eq!(
                classify_push_porcelain(mixed),
                PushPorcelain::RejectedNonFastForward,
                "{mixed:?}"
            );
        }
    }

    /// One porcelain "noise" line: never `!`-flagged (empty, or a first character that
    /// is neither `!` nor a tab).
    fn porcelain_noise() -> impl proptest::strategy::Strategy<Value = String> {
        proptest::prop_oneof![
            proptest::strategy::Just(String::new()),
            "[^!\t\n][^\n]{0,30}",
        ]
    }

    proptest::proptest! {
        #[test]
        fn test_classify_push_porcelain_prop(
            noise in proptest::collection::vec(porcelain_noise(), 0..=6),
            at in 0usize..=6,
            refname in "[A-Za-z0-9:/_.-]{1,30}",
            text in "[^\n]{0,20}",
        ) {
            // Property 1: noise alone is never a rejection.
            proptest::prop_assert_eq!(
                classify_push_porcelain(&noise.join("\n")),
                PushPorcelain::Accepted
            );
            let with_line = |summary: &str| {
                let mut lines = noise.clone();
                lines.insert(at % (noise.len() + 1), format!("!\t{refname}\t{summary} {text}"));
                lines.join("\n")
            };
            // Properties 2 and 3: same shape, only the summary prefix differs, so each is
            // the other's negative control.
            proptest::prop_assert_eq!(
                classify_push_porcelain(&with_line("[remote rejected]")),
                PushPorcelain::Other
            );
            proptest::prop_assert_eq!(
                classify_push_porcelain(&with_line("[rejected]")),
                PushPorcelain::RejectedNonFastForward
            );
        }
    }

    #[test]
    fn test_push_continuity_violation_is_hard_failure() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let bare_url = bare.path().to_str().unwrap();
        let remote_main = || git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        let main_before = remote_main();

        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare_url, "main", cache.path())
            .unwrap();
        let checkout = cached.checkout_dir.as_path();
        // A stray local commit: the sync commit's parent is no longer the fetched head.
        git.run(checkout, &["commit", "--allow-empty", "-m", "stray"]);
        let stray = git.output(checkout, &["rev-parse", "HEAD"]);
        assert_ne!(stray, cached.fetched_sha);

        let err = push_failure(direct_push_edit(
            &engine,
            &cached,
            "[core]\n  autocrlf = false\n",
        ));
        assert!(err.contains("Branch continuity check failed"), "{err}");
        assert_eq!(remote_main(), main_before, "nothing may be published");
        assert_eq!(
            git.output(checkout, &["rev-parse", "HEAD"]),
            stray,
            "the sync commit was not rolled back"
        );
        drop(cached);

        // Negative control: without the stray commit the same push succeeds.
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare_url, "main", cache.path())
            .unwrap();
        let res = direct_push_edit(&engine, &cached, "[core]\n  autocrlf = false\n");
        assert!(res.is_ok(), "push without a stray commit: {res:?}");
        assert_ne!(
            remote_main(),
            main_before,
            "the remote branch did not advance"
        );
    }

    /// A bare remote seeded with one template on `main` whose pre-receive hook is the
    /// POSIX sh script `hook` (installed after seeding, mode 0o755).
    #[cfg(unix)]
    fn bare_remote_with_pre_receive(git: &GitFixture, hook: &str) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let bare = seeded_bare_remote(git);
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
    ) -> (Result<String, PushRemoteError>, std::time::Duration) {
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.to_str().unwrap(), "main", cache.path())
            .expect("cache checkout of the bare remote");
        let checkout = cached.checkout_dir.as_path();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, "[core]\n  autocrlf = false\n").unwrap();
        let start = std::time::Instant::now();
        let res = engine.push_to_remote(checkout, "main", false, &[managed], &cached.fetched_sha);
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
            Err(PushRemoteError::Failed(e)) => e,
            Err(other) => {
                panic!("push through a hanging hook: expected a hard failure, got {other:?}")
            }
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

    #[cfg(unix)]
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
                matches!(res, PullResult::ScanFailed { ref reason } if reason.contains("symlink")),
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
        assert_eq!(applied_only(pull_res), 0);

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
