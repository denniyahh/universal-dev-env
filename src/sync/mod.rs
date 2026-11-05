pub mod merge;
pub mod staging;
pub mod state;
use state::STATE_FILE_REL;
pub use state::{FileStateRecord, ProjectState};

use crate::process::{self, RunOutput};
use crate::sanitizer::Sanitizer;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
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

/// The fingerprint of every file `push_updates` published, keyed by the same path that
/// appears in `staged_files`, so the publication gates can refuse a stored blob that
/// differs from what the secret scan (gate 4) and the reversibility check (gate 5)
/// validated. The record is compared with the fingerprint of each staged index blob, of
/// each blob the commit changes (read by object id), and of each recorded path in the
/// commit's tree, or in the fetched head's tree when there is nothing to push. Which
/// channels that comparison defends, and which it does not, is the closed list in
/// `docs/ARCHITECTURE.md`, section 3 ("What the publication gate promises").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatedBlobs {
    digests: std::collections::BTreeMap<PathBuf, String>,
}

impl ValidatedBlobs {
    /// Records the fingerprint ([`SyncEngine::fingerprint`]: SHA-256 of LF-normalized
    /// UTF-8, raw bytes otherwise) of the exact bytes published at `path`.
    fn record(&mut self, path: PathBuf, published: &[u8]) {
        self.digests
            .insert(path, SyncEngine::fingerprint(published));
    }

    /// The record keyed relative to `repo`, the way the gates key staged paths
    /// (`strip_prefix`, falling back to the key itself).
    fn relative_to(&self, repo: &Path) -> HashMap<PathBuf, &str> {
        self.digests
            .iter()
            .map(|(path, digest)| {
                let rel = path.strip_prefix(repo).unwrap_or(path).to_path_buf();
                (rel, digest.as_str())
            })
            .collect()
    }

    /// The record of each file's current bytes. Tests that call the gates without
    /// `push_updates` use it as the stand-in record; production code never builds a
    /// record from the working tree.
    #[cfg(test)]
    pub(crate) fn from_files(paths: &[PathBuf]) -> Self {
        let mut validated = Self::default();
        for path in paths {
            let bytes = fs::read(path).expect("read a validated file");
            validated
                .digests
                .insert(path.clone(), SyncEngine::fingerprint(&bytes));
        }
        validated
    }
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
        /// See [`ValidatedBlobs`].
        validated_blobs: ValidatedBlobs,
    },
    /// The project could not be compared with upstream; nothing was changed.
    ScanFailed {
        reason: String,
    },
    SecretBlocked {
        path: PathBuf,
        reason: String,
    },
    /// The published text does not render back to the local file (SEC-04); nothing
    /// was changed. `reason` holds fixed text, an error category and line numbers only.
    ReversibilityBlocked {
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
    /// The remote branch head UDE fetched (`refs/remotes/origin/<branch>` in the cache
    /// checkout). Every push, direct or PR, must publish exactly one commit whose only
    /// parent is this commit (branch continuity, D-18).
    ///
    /// Through [`SyncEngine::ensure_remote_repo_for_push`] it is also equal to the head
    /// `git ls-remote` reported for [`RemoteCache::remote_url`] from the filesystem root
    /// before the fetch, so no ref or config in the checkout chose it. Through
    /// [`SyncEngine::ensure_remote_repo`] (status and pull) it is the checkout's own ref,
    /// which a hook or config in the checkout can move.
    pub fetched_sha: String,
    /// The caller's remote URL, with a relative local path made absolute against the
    /// process's working directory (see [`absolute_remote_url`]). Reads and pushes from
    /// outside the checkout use it so they reach the repository the clone used; the cache
    /// key keeps the caller's spelling.
    pub remote_url: String,
    /// The cache lock; hold it for the whole sync action.
    pub lock: RemoteLockGuard,
}

impl RemoteCache {
    /// Where a publication of `branch` from this cache goes: the checkout it is committed
    /// in, the caller's URL it is pushed to and read back from, and the head it builds on.
    pub fn publish_target<'a>(&'a self, branch: &'a str) -> PublishTarget<'a> {
        PublishTarget {
            checkout: &self.checkout_dir,
            remote_url: &self.remote_url,
            branch,
            fetched_sha: &self.fetched_sha,
        }
    }
}

/// Where [`SyncEngine::push_to_remote`] publishes. The push names `remote_url`, never a
/// remote of the checkout, and returns Ok only once `git ls-remote` of `remote_url` from
/// outside the checkout reports the pushed ref at the validated commit.
#[derive(Debug, Clone, Copy)]
pub struct PublishTarget<'a> {
    /// The cache checkout the commit is made in ([`RemoteCache::checkout_dir`]).
    pub checkout: &'a Path,
    /// The caller's remote URL ([`RemoteCache::remote_url`]).
    pub remote_url: &'a str,
    /// The branch a direct push updates and a PR targets.
    pub branch: &'a str,
    /// The head the commit must build on ([`RemoteCache::fetched_sha`]).
    pub fetched_sha: &'a str,
}

/// What the staged gate validated, handed to the commit and to the committed gate.
#[derive(Debug, PartialEq, Eq)]
struct StagedPublication {
    /// The allowlist, relative to the cache checkout.
    paths: HashSet<PathBuf>,
    /// The id of the staged tree, recorded after the staged gate passed.
    tree: String,
}

/// Why [`SyncEngine::push_to_remote`] published nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRemoteError {
    /// The remote branch moved while UDE was fetching or publishing; nothing was published
    /// and the user should pull first (D-18). Either
    /// [`SyncEngine::ensure_remote_repo_for_push`] found the fetched head different from
    /// the head read before the fetch and a second read equal to it (the remote moved
    /// during the fetch), or the push was refused as not a fast-forward and the remote
    /// branch is not confirmed at the commit UDE fetched (the cache commit was rolled
    /// back).
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
    /// A ref was flagged `!` with a `[rejected]` summary (`fetch first` /
    /// `non-fast-forward`): the fast-forward check refused it. Usually the branch moved, but
    /// local state can make git see the commit as not building on the fetched head.
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
    /// Runs right after each publication gate's tree listing is read (tests only), so a
    /// test can swap an object file back between the listing and the checks after it.
    #[cfg(test)]
    after_tree_listing: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    /// The records one publication gate call may list, in place of the production limit
    /// (tests only), so a test can reach the limit with a small tree.
    #[cfg(test)]
    listing_record_limit: Option<usize>,
    /// The listing bytes one publication gate call may list, in place of the production
    /// limit (tests only), so a test can reach the limit with a small tree.
    #[cfg(test)]
    listing_byte_limit: Option<usize>,
    /// The time one publication gate call's walk may take, in place of the local time
    /// limit (tests only). It shortens only the walk, not the git reads, so a test can tell
    /// the walk's deadline from a read that timed out.
    #[cfg(test)]
    listing_time: Option<Duration>,
    /// Counts each blob [`Self::read_blob_by_id`] reads (tests only), so a test can show
    /// that a blob was not read.
    #[cfg(test)]
    blob_reads: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
}

impl SyncEngine {
    pub fn new() -> Self {
        Self {
            sanitizer: Sanitizer::new(),
            extra_env: Vec::new(),
            limits: ProcessLimits::default(),
            deletion_history: None,
            #[cfg(test)]
            after_tree_listing: None,
            #[cfg(test)]
            listing_record_limit: None,
            #[cfg(test)]
            listing_byte_limit: None,
            #[cfg(test)]
            listing_time: None,
            #[cfg(test)]
            blob_reads: None,
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

    /// Sets the hook [`Self::after_tree_listing`] runs (tests only).
    #[cfg(test)]
    fn with_after_tree_listing(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.after_tree_listing = Some(std::sync::Arc::new(hook));
        self
    }

    /// Sets the records one publication gate call may list (tests only).
    #[cfg(test)]
    fn with_listing_record_limit(mut self, records: usize) -> Self {
        self.listing_record_limit = Some(records);
        self
    }

    /// Sets the listing bytes one publication gate call may list (tests only).
    #[cfg(test)]
    fn with_listing_byte_limit(mut self, bytes: usize) -> Self {
        self.listing_byte_limit = Some(bytes);
        self
    }

    /// Sets the time one publication gate call's walk may take (tests only).
    #[cfg(test)]
    fn with_listing_time(mut self, time: Duration) -> Self {
        self.listing_time = Some(time);
        self
    }

    /// Counts each blob [`Self::read_blob_by_id`] reads into `reads` (tests only).
    #[cfg(test)]
    fn with_blob_read_counter(
        mut self,
        reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        self.blob_reads = Some(reads);
        self
    }

    /// The budget one publication gate call's listings and diff draw on:
    /// [`MAX_LISTING_RECORDS`] records, [`MAX_LISTING_BYTES`] bytes and a deadline of the
    /// local time limit from now (the bound each `git ls-tree` and `git diff-tree` run had
    /// before the walk replaced them), or a test's lower limits when set.
    fn listing_budget(&self) -> ListingBudget {
        let time = self.limits.local;
        #[cfg(test)]
        let (records, bytes, time) = (
            self.listing_record_limit.unwrap_or(MAX_LISTING_RECORDS),
            self.listing_byte_limit.unwrap_or(MAX_LISTING_BYTES),
            self.listing_time.unwrap_or(time),
        );
        #[cfg(not(test))]
        let (records, bytes) = (MAX_LISTING_RECORDS, MAX_LISTING_BYTES);
        ListingBudget::new(records, bytes, time)
    }

    /// Runs the test hook set by `with_after_tree_listing`, if any. Called right after a
    /// publication gate's tree listing is read; in a non-test build it does nothing.
    fn after_tree_listing(&self) {
        #[cfg(test)]
        if let Some(hook) = &self.after_tree_listing {
            hook();
        }
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

    /// [`Self::run_local`] with `input` on the child's stdin.
    fn run_local_with_input(
        &self,
        cmd: Command,
        label: &str,
        input: Vec<u8>,
    ) -> Result<RunOutput, String> {
        process::run_bounded_with_input(cmd, label, self.limits.local, input)
    }

    /// Runs `cmd` on the network tier. `label` must not contain argv (remote URLs).
    fn run_network(&self, cmd: Command, label: &str) -> Result<RunOutput, String> {
        process::run_bounded(cmd, label, self.limits.network)
    }

    /// Runs a `git push` on the network tier with the grafts file at the null device.
    /// `label` must not contain argv (remote URLs).
    fn run_push(&self, cmd: Command, label: &str) -> Result<RunOutput, String> {
        process::run_bounded_ignoring_grafts(cmd, label, self.limits.network)
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

    /// Push gate 5 (SEC-04): the text push would publish to `target` must render back,
    /// through the renderer and render rule init, scan and pull use, to exactly the
    /// local text (`local_lf`, LF-normalized). Text the rule does not render passes
    /// unrendered. The error reason holds fixed text, the engine's error category and
    /// line numbers only, never file text or the engine's detail.
    fn check_render_back(
        renderer: &crate::template::TemplateRenderer,
        ctx: &crate::template::TemplateContext,
        target: &Path,
        published: &str,
        local_lf: &str,
    ) -> Result<(), String> {
        if !crate::template::should_render(target, published) {
            return Ok(());
        }
        let rendered = match renderer.render_str_checked(published, ctx) {
            Ok(rendered) => rendered,
            Err(failure) => {
                return Err(format!(
                    "published text does not render back to the local file: {}",
                    failure.describe()
                ));
            }
        };
        let rendered_lf = Self::normalize_eol(&rendered);
        if rendered_lf == local_lf {
            return Ok(());
        }
        // Split on LF so a trailing-newline difference shows as an extra last line.
        let rendered_lines: Vec<&str> = rendered_lf.split('\n').collect();
        let local_lines: Vec<&str> = local_lf.split('\n').collect();
        let first_difference = rendered_lines
            .iter()
            .zip(&local_lines)
            .position(|(rendered_line, local_line)| rendered_line != local_line)
            .unwrap_or_else(|| rendered_lines.len().min(local_lines.len()));
        Err(format!(
            "published text renders back differently from the local file at line {}",
            first_difference + 1
        ))
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
                            if crate::template::should_render(p, &raw_string) {
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
            remote_url: absolute_remote_url(remote_url),
            lock: lock_guard,
        })
    }

    /// [`SyncEngine::ensure_remote_repo`] for a push: the fetched head must be the commit
    /// the remote branch holds, as `git ls-remote` reports it for the caller's URL from
    /// outside the checkout before the fetch (operator decision 2026-11-02, cause 2).
    ///
    /// A hook or config in the cache checkout can move or redirect the tracking ref, which
    /// would make an injected commit the base the push builds on. On a difference a second
    /// read tells the two cases apart: equal to the fetched head means the remote really
    /// moved during the fetch ([`PushRemoteError::UpstreamMoved`]); anything else is a
    /// [`PushRemoteError::Failed`] that names no ref or id from the checkout.
    pub fn ensure_remote_repo_for_push(
        &self,
        remote_url: &str,
        branch: &str,
        cache_dir: &Path,
    ) -> Result<RemoteCache, PushRemoteError> {
        let head_before = self
            .remote_head(&absolute_remote_url(remote_url), branch)
            .map_err(PushRemoteError::Failed)?;
        // The caller's spelling, so the cache key (and the cache directory) is unchanged.
        let cache = self
            .ensure_remote_repo(remote_url, branch, cache_dir)
            .map_err(PushRemoteError::Failed)?;
        if head_before.as_deref() == Some(cache.fetched_sha.as_str()) {
            return Ok(cache);
        }
        let head_again = self
            .remote_head(&cache.remote_url, branch)
            .map_err(PushRemoteError::Failed)?;
        if head_again.as_deref() == Some(cache.fetched_sha.as_str()) {
            return Err(PushRemoteError::UpstreamMoved {
                branch: branch.to_string(),
            });
        }
        Err(PushRemoteError::Failed(
            "the cache checkout's fetched head is not the commit the remote branch holds; \
             a hook or config in the cache checkout most likely moved or redirected it; \
             nothing was published"
                .to_string(),
        ))
    }

    /// The commit `refs/heads/<branch>` is at on `remote_url`, read with `git ls-remote`
    /// from the filesystem root so no repository's config or hooks take part; `None` when
    /// no listed line has exactly that refname (ls-remote matches patterns by tail).
    fn remote_head(&self, remote_url: &str, branch: &str) -> Result<Option<String>, String> {
        let refname = format!("refs/heads/{branch}");
        let mut ls_remote = self.git(Some(Path::new(std::path::MAIN_SEPARATOR_STR)));
        ls_remote.args(["ls-remote", "--refs", "--", remote_url, &refname]);
        let out = self.run_network(ls_remote, "git ls-remote")?;
        if !out.status.success() {
            return Err(with_excerpt(
                format!(
                    "git ls-remote failed with exit status {:?}",
                    out.status.code()
                ),
                &out,
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| {
                let (id, name) = line.split_once('\t')?;
                (name == refname).then(|| id.to_string())
            }))
    }

    /// Ok only when [`Self::remote_head`] of `target.remote_url` reports `refs/heads/<branch>`
    /// at exactly `commit` (whole ids, so a prefix does not pass). What `git push` said is
    /// not enough: config in the checkout (`url.*.pushInsteadOf`, `url.*.insteadOf`,
    /// `remote.<url>.pushurl`) can send a push to the caller's URL somewhere else, and the
    /// read, run from the filesystem root, does not follow it.
    fn confirm_published(
        &self,
        target: &PublishTarget<'_>,
        branch: &str,
        commit: &str,
    ) -> Result<(), String> {
        let detail = match self.remote_head(target.remote_url, branch) {
            Ok(Some(head)) if head == commit => return Ok(()),
            Ok(_) => String::new(),
            Err(e) => format!(": {e}"),
        };
        Err(format!(
            "git push reported success, but the remote branch {branch} is not at the validated commit{detail}; nothing was recorded as published; run ude sync pull, then push again"
        ))
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
                        let rendered = if crate::template::should_render(&upstream_path, raw_str) {
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
        // Gate 3 and gate 5's render context must use one spelling of each root,
        // and callers pass BaseDirs values that may end in a separator (CR-01).
        let user_home = crate::sanitizer::normalize_root(user_home);
        let xdg_config = crate::sanitizer::normalize_root(xdg_config);
        let dummy_ctx =
            crate::template::TemplateContext::detect_for_project(local_root, "project", "generic");
        let states = match self.scan_project(local_root, upstream_root, Some(&dummy_ctx)) {
            Ok(states) => states,
            Err(reason) => return PushResult::ScanFailed { reason },
        };
        // Gate 5 renders with the push's own home and XDG config, the values Gate 3
        // parameterized with, not the host-derived context above.
        let mut render_ctx = dummy_ctx.clone();
        render_ctx.user.home = user_home.to_string();
        render_ctx.user_home = user_home.to_string();
        render_ctx.user.xdg_config = xdg_config.to_string();
        render_ctx.xdg_config = xdg_config.to_string();
        let renderer = crate::template::TemplateRenderer::new();
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

                let (published, raw_local_str) =
                    if let Ok(content) = std::str::from_utf8(&raw_bytes) {
                        // Gate 3: path parameterization and line endings (UTF-8 only)
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

                // Gate 4: secret scan of the exact bytes that will be published, for
                // UTF-8 and non-UTF-8 files alike. A refusal returns before staging.
                let publish_rel = upstream_target
                    .as_deref()
                    .and_then(|target| target.strip_prefix(upstream_root).ok())
                    .map_or_else(|| state.relative_path.clone(), Path::to_path_buf);
                if let Some(finding) = self.sanitizer.scan_bytes(&published, &publish_rel) {
                    return PushResult::SecretBlocked {
                        path: state.relative_path.clone(),
                        reason: finding.reason(),
                    };
                }

                // Gate 5: reversibility, the published text must render back to the
                // local file through the same renderer and rule init, scan and pull
                // use. It runs inside pass 1, so a refusal stages nothing.
                if let (Some(local_lf), Some(target)) =
                    (raw_local_str.as_deref(), upstream_target.as_deref())
                {
                    let published_text = String::from_utf8_lossy(&published);
                    if let Err(reason) = Self::check_render_back(
                        &renderer,
                        &render_ctx,
                        target,
                        &published_text,
                        local_lf,
                    ) {
                        return PushResult::ReversibilityBlocked {
                            path: state.relative_path.clone(),
                            reason,
                        };
                    }
                }

                // Stage into candidates list
                if let Some(target_upstream) = upstream_target {
                    candidates_to_write.push((
                        state.relative_path.clone(),
                        target_upstream,
                        published,
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
        let mut validated_blobs = ValidatedBlobs::default();

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
            // The exact bytes gate 4 scanned and gate 5 validated, keyed as staged_files.
            validated_blobs.record(upstream_root.join(upstream_rel), &parameterized_bytes);
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
            validated_blobs,
        }
    }

    /// Publishes the staged template updates: commits them in `target.checkout` and pushes
    /// the commit to `target.remote_url`, directly to `target.branch`, or to a fresh
    /// `ude-sync-*` branch with a PR when `open_pr` is set. The published commit must have
    /// `target.fetched_sha` as its only parent, and Ok means `git ls-remote` of the URL
    /// reports the pushed ref at that commit. What this push defends is the closed list in
    /// `docs/ARCHITECTURE.md`, section 3, and nothing beyond it.
    pub fn push_to_remote(
        &self,
        target: &PublishTarget<'_>,
        open_pr: bool,
        staged_files: &[PathBuf],
        validated: &ValidatedBlobs,
    ) -> Result<String, PushRemoteError> {
        let Some(staged) = self
            .stage_publication(target, staged_files, validated)
            .map_err(PushRemoteError::Failed)?
        else {
            return Ok("No changes to push to remote repository.".to_string());
        };
        if open_pr {
            self.push_pr_branch(target, &staged, validated)
                .map_err(PushRemoteError::Failed)
        } else {
            self.push_direct(target, &staged, validated)
        }
    }

    /// Stages exactly `staged_files` in the cache checkout (`target.checkout`) and runs the
    /// publication gate on the index (allowlist, secret scan of every staged blob). Returns
    /// the validated allowlist relative to the checkout and the id of the staged tree, or
    /// `None` when there is nothing to push, which needs
    /// [`SyncEngine::require_fetched_head_holds`] to pass. The id is recorded here, in the
    /// one place both modes share and before PR mode's `git checkout -b`, so no hook that
    /// runs after this function can move what the committed gate compares the commit with.
    fn stage_publication(
        &self,
        target: &PublishTarget<'_>,
        staged_files: &[PathBuf],
        validated: &ValidatedBlobs,
    ) -> Result<Option<StagedPublication>, String> {
        let remote_checkout = target.checkout;
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

        // Index flags, excludes, fsmonitor and config can make status print nothing while
        // the working tree holds UDE's edit, so "nothing to push" is checked against what
        // the fetched head holds, whatever emptied status.
        if status.stdout.is_empty() {
            self.require_fetched_head_holds(target, staged_files, validated)?;
            return Ok(None);
        }

        if staged_files.is_empty() {
            self.require_fetched_head_holds(target, staged_files, validated)?;
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
        let validated_rel = validated.relative_to(remote_checkout);

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

            if let Some(finding) = self.sanitizer.scan_bytes(&blob_res.stdout, &p) {
                self.reset_index(remote_checkout);
                return Err(format!(
                    "Publication gate violation: secret detected in staged index blob {}: {}",
                    p.display(),
                    finding.reason()
                ));
            }

            // The blob must be the bytes push_updates validated. Git runs clean filters
            // during `git add`, and a filter or hook can rewrite the working tree too,
            // so the gate compares against the record and never re-reads the file.
            match validated_rel.get(&p) {
                None => {
                    self.reset_index(remote_checkout);
                    return Err(format!(
                        "Publication gate violation: no validated content recorded for {}",
                        p.display()
                    ));
                }
                Some(digest) if *digest != Self::fingerprint(&blob_res.stdout) => {
                    self.reset_index(remote_checkout);
                    return Err(format!(
                        "Publication gate violation: staged index blob {} differs from the validated content",
                        p.display()
                    ));
                }
                Some(_) => {}
            }
        }

        let tree = match self.record_staged_tree(remote_checkout) {
            Ok(tree) => tree,
            Err(err) => {
                self.reset_index(remote_checkout);
                return Err(err);
            }
        };
        Ok(Some(StagedPublication {
            paths: expected_set,
            tree,
        }))
    }

    /// The id of the tree `repo`'s index holds. A refusal carries no stderr excerpt: git
    /// names paths there, and a hook can choose those names.
    fn record_staged_tree(&self, repo: &Path) -> Result<String, String> {
        const REFUSAL: &str = "Publication gate violation: cannot record the staged tree";
        let mut write_tree = self.git(Some(repo));
        write_tree.arg("write-tree");
        let out = self
            .run_local(write_tree, "git write-tree")
            .map_err(|_| REFUSAL.to_string())?;
        if !out.status.success() {
            return Err(REFUSAL.to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// PR mode: commits on a fresh `ude-sync-<hash>-<epoch>` branch, pushes it and opens
    /// a PR against `branch`. Not subject to the upstream-moved abort (D-18 scope note),
    /// but its commit must sit directly on `fetched_sha`, like a direct push's. The push
    /// goes to `target.remote_url`, and `gh pr create` runs only after
    /// [`Self::confirm_published`] found the sync branch there at the commit; otherwise the
    /// local sync branch is removed and nothing is opened. The push reads no commit-graph,
    /// bitmap or grafts file and sees no `GIT_TEST_*` variable.
    fn push_pr_branch(
        &self,
        target: &PublishTarget<'_>,
        staged: &StagedPublication,
        validated: &ValidatedBlobs,
    ) -> Result<String, String> {
        let PublishTarget {
            checkout: remote_checkout,
            branch,
            fetched_sha,
            ..
        } = *target;
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

        let commit = match self.resolve_commit(remote_checkout, "HEAD") {
            Ok(commit) => commit,
            Err(err) => {
                self.rollback_commit(remote_checkout);
                self.abandon_sync_branch(remote_checkout, branch, &sync_branch);
                return Err(err);
            }
        };

        if let Err(err) = self.validate_committed_tree(
            remote_checkout,
            &commit,
            fetched_sha,
            &staged.paths,
            validated,
            &staged.tree,
        ) {
            self.rollback_commit(remote_checkout);
            self.abandon_sync_branch(remote_checkout, branch, &sync_branch);
            return Err(err);
        }

        // The validated id, never the branch name: a hook can leave the branch on another
        // commit. `push.followTags` in the cache repository config (a hook can write it)
        // would also publish annotated tags pointing into the pushed history.
        //
        // The source is `<id>^{commit}`, not the bare id: push resolves a plain id against
        // local ref names before reading it as an object name, so a ref a hook made at
        // refs/tags/<id>, refs/heads/<id> or refs/remotes/<id> would be sent in its place
        // (probed). No ref name can hold `^`, so this source is only ever the object.
        // `--no-replace-objects` keeps a replace ref from changing what the source peels to:
        // a replace ref from the commit to a tag would make `^{commit}` peel to the tag's
        // commit (probed).
        //
        // Push's object walk would follow a hook-written commit-graph (local, split chain,
        // or in an alternate object directory) or a bitmap (pack or multi-pack-index) built
        // under a graft, sending objects outside the validated commit; both are disabled
        // here. Every GIT_TEST_* variable is removed because GIT_TEST_COMMIT_GRAPH makes git
        // load commit-graphs even under core.commitGraph=false (probed), and git's other
        // test-suite variables have no place in a user's push.
        //
        // Push's object walk also reads the grafts file. The gate's refusal runs before the
        // push, and a hook, a process it left or a command it wrote into config (a push to a
        // URL not starting with `/` still runs `remote.<url>.receivepack`) can write the
        // file after that, so run_push makes the walk read the null device.
        //
        // With GIT_GRAFT_FILE naming an existing file git prints a deprecation hint on stderr
        // when a push reads objects, and a refusal's excerpt is the last lines of stderr, so
        // the hint would take lines there. A future git that drops grafts makes the variable
        // and this option no-ops, which needs no action.
        let refspec = format!("{commit}^{{commit}}:refs/heads/{sync_branch}");
        let mut push = self.git(Some(remote_checkout));
        push.args([
            "--no-replace-objects",
            "-c",
            "core.commitGraph=false",
            "-c",
            "pack.useBitmaps=false",
            "-c",
            "advice.graftFileDeprecated=false",
            "push",
            "--no-follow-tags",
            "--",
            target.remote_url,
            &refspec,
        ]);
        remove_git_test_env(&mut push, std::env::vars_os().map(|(key, _)| key));
        let push_out = self
            .run_push(push, "git push")
            .map_err(|e| format!("Failed to push sync branch: {e}"))?;

        // Return working tree to canonical branch
        self.git_best_effort(remote_checkout, &["checkout", branch], "git checkout");

        if !push_out.status.success() {
            return Err(with_excerpt(
                format!("git push failed for branch {sync_branch}"),
                &push_out,
            ));
        }

        // No PR is opened for a branch the caller's remote does not hold at the commit.
        if let Err(err) = self.confirm_published(target, &sync_branch, &commit) {
            self.abandon_sync_branch(remote_checkout, branch, &sync_branch);
            return Err(err);
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

    /// Direct mode: commits in the cache checkout and pushes the commit to `branch` of
    /// `target.remote_url`.
    ///
    /// The commit must build on `fetched_sha` (branch continuity). The push sends the
    /// commit the committed gate validated, named so that no local ref and no replace ref
    /// can stand in for it, and no tags, to the caller's URL rather than to a remote of the
    /// checkout, whose config a hook can write. Ok needs [`Self::confirm_published`] too;
    /// when the remote does not report the commit the cache commit is rolled back and the
    /// result is [`PushRemoteError::Failed`].
    /// It is a plain fast-forward push, never forced: when upstream moved after the fetch,
    /// the remote's fast-forward check rejects it, the cache commit is rolled back and the
    /// result is [`PushRemoteError::UpstreamMoved`] (D-18). When `git ls-remote` instead
    /// reports the branch still at `fetched_sha`, the result is [`PushRemoteError::Failed`].
    /// The push reads no commit-graph, bitmap or grafts file and sees no `GIT_TEST_*`
    /// variable.
    fn push_direct(
        &self,
        target: &PublishTarget<'_>,
        staged: &StagedPublication,
        validated: &ValidatedBlobs,
    ) -> Result<String, PushRemoteError> {
        use PushRemoteError::Failed;
        let PublishTarget {
            checkout: remote_checkout,
            branch,
            fetched_sha,
            ..
        } = *target;
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

        let commit = match self.resolve_commit(remote_checkout, "HEAD") {
            Ok(commit) => commit,
            Err(err) => {
                self.rollback_commit(remote_checkout);
                return Err(Failed(err));
            }
        };

        if let Err(err) = self.validate_committed_tree(
            remote_checkout,
            &commit,
            fetched_sha,
            &staged.paths,
            validated,
            &staged.tree,
        ) {
            self.rollback_commit(remote_checkout);
            return Err(Failed(err));
        }

        // No force or lease option, ever: the server's fast-forward check is the
        // compare-and-swap that detects a concurrent publisher (D-18). `push.followTags`
        // in the cache repository config (a hook can write it) would also publish annotated
        // tags pointing into the pushed history.
        //
        // The source is `<id>^{commit}`, not the bare id: push resolves a plain id against
        // local ref names before reading it as an object name, so a ref a hook made at
        // refs/tags/<id>, refs/heads/<id> or refs/remotes/<id> would be sent in its place
        // (probed). No ref name can hold `^`, so this source is only ever the object.
        // `--no-replace-objects` keeps a replace ref from changing what the source peels to:
        // a replace ref from the commit to a tag would make `^{commit}` peel to the tag's
        // commit (probed).
        //
        // Push's object walk would follow a hook-written commit-graph (local, split chain,
        // or in an alternate object directory) or a bitmap (pack or multi-pack-index) built
        // under a graft, sending objects outside the validated commit; both are disabled
        // here. Every GIT_TEST_* variable is removed because GIT_TEST_COMMIT_GRAPH makes git
        // load commit-graphs even under core.commitGraph=false (probed), and git's other
        // test-suite variables have no place in a user's push.
        //
        // Push's object walk also reads the grafts file. The gate's refusal runs before the
        // push, and a hook, a process it left or a command it wrote into config (a push to a
        // URL not starting with `/` still runs `remote.<url>.receivepack`) can write the
        // file after that, so run_push makes the walk read the null device.
        //
        // With GIT_GRAFT_FILE naming an existing file git prints a deprecation hint on stderr
        // when a push reads objects, and a refusal's excerpt is the last lines of stderr, so
        // the hint would take lines there. A future git that drops grafts makes the variable
        // and this option no-ops, which needs no action.
        let refspec = format!("{commit}^{{commit}}:refs/heads/{branch}");
        let mut push = self.git(Some(remote_checkout));
        push.args([
            "--no-replace-objects",
            "-c",
            "core.commitGraph=false",
            "-c",
            "pack.useBitmaps=false",
            "-c",
            "advice.graftFileDeprecated=false",
            "push",
            "--porcelain",
            "--no-follow-tags",
            "--",
            target.remote_url,
            &refspec,
        ]);
        remove_git_test_env(&mut push, std::env::vars_os().map(|(key, _)| key));
        let push_out = self
            .run_push(push, "git push")
            .map_err(|e| Failed(format!("Failed to push to remote: {e}")))?;

        if !push_out.status.success() {
            // Porcelain stdout starts with a `To <url>` line; it is never printed.
            let stdout = String::from_utf8_lossy(&push_out.stdout);
            if classify_push_porcelain(&stdout) == PushPorcelain::RejectedNonFastForward {
                self.rollback_commit(remote_checkout);
                // A shallow file can make git see the validated commit as not building on
                // the fetched head, so the rejection alone does not mean upstream moved
                // (probed). A replace ref no longer can, because the push reads past replace
                // refs. This read runs only on this failure path.
                let head = self.remote_head(target.remote_url, branch);
                if head.ok().flatten().as_deref() == Some(fetched_sha) {
                    return Err(Failed(format!(
                        "git push to {branch} was refused as not a fast-forward, but git ls-remote reports the remote branch still at the commit UDE fetched; a hook in the cache checkout most likely caused this"
                    )));
                }
                return Err(PushRemoteError::UpstreamMoved {
                    branch: branch.to_string(),
                });
            }
            return Err(Failed(with_excerpt(
                format!("git push to {branch} failed"),
                &push_out,
            )));
        }

        if let Err(err) = self.confirm_published(target, branch, &commit) {
            self.rollback_commit(remote_checkout);
            return Err(Failed(err));
        }

        Ok(format!(
            "Directly pushed changes to remote branch '{branch}'."
        ))
    }

    /// The full object id of the commit `rev` names in `repo`.
    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<String, String> {
        const REFUSAL: &str =
            "Post-commit publication gate violation: cannot resolve the commit to publish";
        let spec = format!("{rev}^{{commit}}");
        // Replace refs are not honoured: the id returned must be the commit itself, which
        // the gate reads raw and the push sends. A replace ref from the commit to a tag
        // would otherwise make `^{commit}` peel through the tag to another commit (probed).
        let mut rev_parse = self.git(Some(repo));
        rev_parse.args(["--no-replace-objects", "rev-parse", "--verify", &spec]);
        let out = self
            .run_local(rev_parse, "git rev-parse")
            .map_err(|e| format!("{REFUSAL}: {e}"))?;
        if !out.status.success() {
            return Err(with_excerpt(REFUSAL.to_string(), &out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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

    /// Refuses while the cache checkout `repo` has a grafts file. A probed channel: with a
    /// graft adding a parent, `git push` sends the grafted commit and its blob while the
    /// remote ref moves to the validated commit, and the push exits 0.
    fn refuse_grafts_file(&self, repo: &Path) -> Result<(), String> {
        const UNLOCATED: &str = "Post-commit publication gate violation: cannot locate the grafts file of the cache checkout";
        let mut rev_parse = self.git(Some(repo));
        rev_parse.args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/grafts",
        ]);
        let located = self
            .run_local(rev_parse, "git rev-parse")
            .map_err(|_| UNLOCATED.to_string())?;
        if !located.status.success() {
            return Err(UNLOCATED.to_string());
        }
        // Strict UTF-8: a lossy path could name another file and read as absent.
        let located = std::str::from_utf8(&located.stdout).map_err(|_| UNLOCATED.to_string())?;
        let path = located.trim_end_matches('\n');
        if path.is_empty() {
            return Err(UNLOCATED.to_string());
        }
        match fs::symlink_metadata(path) {
            Ok(_) => Err(format!(
                "Post-commit publication gate violation: the cache checkout has {path}, which can make git push send objects outside the validated commit; nothing was published"
            )),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(format!(
                "Post-commit publication gate violation: cannot inspect {path}"
            )),
        }
    }

    /// Validates `commit` (a full object id) before pushing it, as the only commit the
    /// push adds on top of `base` (the fetched head). The commit, the fetched head and every
    /// tree of each are read against their ids ([`Self::read_verified_object`]) and both tree
    /// listings are built from those bytes ([`Self::verified_tree_listing`]). The change list
    /// is computed from the two listings ([`listing_diff`]), not by git: a fetched-head tree
    /// whose stored bytes do not match its id could list a path the commit adds, or omit one
    /// it deletes, and a diff that read it would not show that change. In the order it
    /// checks: it has exactly one parent, `base`; its message is the sync commit message and
    /// its header is free of secrets; its listing and the fetched head's together stay
    /// within one budget of [`MAX_LISTING_RECORDS`] records and [`MAX_LISTING_BYTES`]
    /// listing bytes, and one deadline, the local time limit from the start of the walk,
    /// covers both listings and the diff; its tree repeats no path and has no path
    /// component that is empty, `.` or `..`, checked before any blob is read; every blob it
    /// changes, read by object id and checked against it, is on the allowlist `paths`
    /// (relative to `repo`), free of secrets and identical to what `validated` recorded; it
    /// holds no empty directory; every path
    /// `validated` recorded holds its recorded content, each blob read against its id; and
    /// its tree is `staged_tree`, the id [`Self::stage_publication`] recorded. What these
    /// checks defend is the closed list in `docs/ARCHITECTURE.md`, section 3, and nothing
    /// beyond it.
    fn validate_committed_tree(
        &self,
        repo: &Path,
        commit: &str,
        base: &str,
        paths: &HashSet<PathBuf>,
        validated: &ValidatedBlobs,
        staged_tree: &str,
    ) -> Result<(), String> {
        if base.is_empty() {
            return Err(
                "Post-commit publication gate violation: no fetched head to bind the commit to"
                    .to_string(),
            );
        }

        // The refusal on presence stays by operator decision 2026-10-28. It no longer decides
        // what is sent: both pushes run through run_push, so they do not read the file.
        self.refuse_grafts_file(repo)?;

        // A post-commit hook (it runs despite --no-verify) can amend, add commits or build
        // a merge, and a single-commit listing shows nothing for a merge. So the gate
        // requires exactly one new commit on the fetched head and lists everything that
        // commit changes.
        //
        // Push sends the original objects (pack-objects ignores replace refs), so the gate
        // must read them raw. The GIT_NO_REPLACE_OBJECTS variable would be removed by
        // process::prepare_command, hence the option on each read. The commit is read against
        // its id: no later git read checks it, and the parent, message, header and tree line
        // below all come from these bytes.
        let raw_commit = match self.read_verified_object(repo, commit.as_bytes(), "commit") {
            Ok(raw_commit) => raw_commit,
            Err(ObjectDefect::Unreadable) => {
                return Err(format!(
                    "Post-commit publication gate violation: failed to read commit {commit}"
                ));
            }
            Err(ObjectDefect::IdMismatch) => {
                return Err(format!(
                    "Post-commit publication gate violation: the commit to publish {OBJECT_ID_MISMATCH}"
                ));
            }
        };
        match raw_commit_parents(&raw_commit).as_slice() {
            [parent] if parent == base => {}
            // The parent text is whatever a hook wrote into the header (escape sequences,
            // a token), so the refusal names only the fetched head, which ensure read and
            // confirmed against the remote.
            [_] => {
                return Err(format!(
                    "Branch continuity check failed: the cache commit's parent is not the fetched head {base}"
                ));
            }
            parents => {
                return Err(format!(
                    "Post-commit publication gate violation: the commit to publish has {} parents; it must have exactly one parent, the fetched head {base}",
                    parents.len()
                ));
            }
        }

        let Some((header, message)) = raw_commit_parts(&raw_commit) else {
            return Err(format!(
                "Post-commit publication gate violation: unparseable commit {commit}"
            ));
        };
        // prepare-commit-msg runs despite --no-verify and a post-commit hook can amend, so
        // the message is bound byte for byte. Nothing is trimmed: git stored exactly one LF
        // after `-m` under every commit.cleanup mode (probed), so no tolerance is needed.
        if message.strip_suffix(b"\n") != Some(SYNC_COMMIT_MESSAGE.as_bytes()) {
            return Err(
                "Post-commit publication gate violation: the message of the commit to publish is not the sync commit message; a prepare-commit-msg or post-commit hook in the cache checkout most likely rewrote it"
                    .to_string(),
            );
        }
        // The rest of the commit text a hook can write has no independent value to bind to,
        // so it is scanned. A scan, not a bind: text the scanner does not flag still publishes.
        if let Some(finding) = self
            .sanitizer
            .scan_bytes(header, Path::new("commit-header"))
        {
            return Err(format!(
                "Post-commit publication gate violation: secret detected in the header of the commit to publish: {}",
                finding.reason()
            ));
        }

        // The commit's listing comes first, built from tree bytes hashed against their ids, the
        // root included, so no check below reads tree bytes that were not hashed. A tree both
        // commits hold is read here, and refused here if rewritten, before the fetched head's
        // listing reuses it.
        let Some(root) =
            raw_commit_tree(&raw_commit).and_then(|tree| std::str::from_utf8(tree).ok())
        else {
            return Err(format!(
                "Post-commit publication gate violation: unparseable commit {commit}"
            ));
        };
        let validated_rel = validated.relative_to(repo);
        // An id-mismatch refusal is named from the record, then the allowlist, never from the
        // tree.
        let mut recorded: Vec<&Path> = validated_rel.keys().map(PathBuf::as_path).collect();
        recorded.sort();
        let mut allowed: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        allowed.sort();
        let named_under =
            |tree: &[u8]| first_path_under(tree, recorded.iter().chain(&allowed).copied());
        let mut verified = HashMap::new();
        // One budget for both listings of this call, so together they stay within the
        // record and byte limits, and both listings and the diff stop at one deadline.
        let mut budget = self.listing_budget();
        let listing = self.verified_tree_listing(repo, root, &mut verified, &mut budget);
        self.after_tree_listing();
        let listing = match listing {
            Ok(listing) => listing,
            Err(ListingDefect::Unreadable) => {
                return Err(format!(
                    "Post-commit publication gate violation: failed to list the tree of {commit}"
                ));
            }
            Err(ListingDefect::TooLarge) => {
                return Err(format!(
                    "Post-commit publication gate violation: the tree of the commit to publish {LISTING_TOO_LARGE}"
                ));
            }
            Err(ListingDefect::PastDeadline) => {
                return Err(format!(
                    "Post-commit publication gate violation: the tree of the commit to publish {LISTING_PAST_DEADLINE}"
                ));
            }
            Err(ListingDefect::Unparseable) => {
                return Err(format!(
                    "Post-commit publication gate violation: unparseable tree entry in {commit}"
                ));
            }
            Err(ListingDefect::IdMismatch(tree)) => {
                return Err(match named_under(&tree) {
                    Some(path) => format!(
                        "Post-commit publication gate violation: an object read for {} {OBJECT_ID_MISMATCH}",
                        path.display()
                    ),
                    None => format!(
                        "Post-commit publication gate violation: a tree of the commit to publish {OBJECT_ID_MISMATCH}"
                    ),
                });
            }
        };

        // The fetched head and its trees are read the same way, on the same map, so only its
        // trees whose ids the commit does not hold are read again. git's own diff would read
        // them unchecked, and a rewritten one could hide an added path or a deletion.
        const BASE_UNLISTED: &str =
            "Post-commit publication gate violation: failed to list the tree of the fetched head";
        let raw_base = match self.read_verified_object(repo, base.as_bytes(), "commit") {
            Ok(raw_base) => raw_base,
            Err(ObjectDefect::Unreadable) => {
                return Err(format!(
                    "Post-commit publication gate violation: failed to read the fetched head {base}"
                ));
            }
            Err(ObjectDefect::IdMismatch) => {
                return Err(format!(
                    "Post-commit publication gate violation: the fetched head's commit {OBJECT_ID_MISMATCH}"
                ));
            }
        };
        let Some(base_root) =
            raw_commit_tree(&raw_base).and_then(|tree| std::str::from_utf8(tree).ok())
        else {
            return Err(BASE_UNLISTED.to_string());
        };
        let base_listing = match self.verified_tree_listing(
            repo,
            base_root,
            &mut verified,
            &mut budget,
        ) {
            Ok(base_listing) => base_listing,
            Err(ListingDefect::Unreadable) => return Err(BASE_UNLISTED.to_string()),
            Err(ListingDefect::TooLarge) => {
                return Err(format!(
                    "Post-commit publication gate violation: the tree of the fetched head {LISTING_TOO_LARGE}"
                ));
            }
            Err(ListingDefect::PastDeadline) => {
                return Err(format!(
                    "Post-commit publication gate violation: the tree of the fetched head {LISTING_PAST_DEADLINE}"
                ));
            }
            Err(ListingDefect::Unparseable) => {
                return Err(
                    "Post-commit publication gate violation: unparseable tree entry in the fetched head"
                        .to_string(),
                );
            }
            Err(ListingDefect::IdMismatch(tree)) => {
                return Err(match named_under(&tree) {
                    Some(path) => format!(
                        "Post-commit publication gate violation: an object read for {} {OBJECT_ID_MISMATCH}",
                        path.display()
                    ),
                    None => format!(
                        "Post-commit publication gate violation: a tree of the fetched head {OBJECT_ID_MISMATCH}"
                    ),
                });
            }
        };

        // The commit's tree repeats no path and has no bad component, checked before any
        // blob is read: a repeated path would make the change loop below read the same blob
        // once per repeat. The staged tree is read too, not trusted: a post-index-change hook
        // can put a repeated path into it before its id is recorded, and the recorded id then
        // matches (probed). None of these refusals prints a name.
        let listed_blobs = match tree_listing_blobs(&listing) {
            Ok(blobs) => blobs,
            Err(TreeDefect::Unparseable) => {
                return Err(format!(
                    "Post-commit publication gate violation: unparseable tree entry in {commit}"
                ));
            }
            Err(TreeDefect::RepeatedPath) => {
                return Err(
                    "Post-commit publication gate violation: the tree of the commit to publish repeats a path"
                        .to_string(),
                );
            }
            Err(TreeDefect::BadComponent) => {
                return Err(
                    "Post-commit publication gate violation: the tree of the commit to publish has a path component that is empty, `.` or `..`"
                        .to_string(),
                );
            }
        };

        // A tree can list one name twice, and a lookup by name returns one of the entries
        // while the push sends both (probed), so each listed blob is read by its own id.
        // Paths and ids are hook-chosen bytes: a refusal below never prints a listed name
        // that has not passed the allowlist.
        const UNPARSEABLE_ENTRY: &str =
            "Post-commit publication gate violation: unparseable committed entry";
        let changes = match listing_diff(&base_listing, &listing, budget.deadline) {
            Ok(changes) => changes,
            Err(DiffDefect::Unparseable) => return Err(UNPARSEABLE_ENTRY.to_string()),
            Err(DiffDefect::PastDeadline) => {
                return Err(format!(
                    "Post-commit publication gate violation: the changes of the commit to publish {LISTING_PAST_DEADLINE}"
                ));
            }
        };
        let Some(entries) = raw_diff_entries(&changes) else {
            return Err(UNPARSEABLE_ENTRY.to_string());
        };
        for entry in entries {
            // A path that is not valid UTF-8 cannot be on the allowlist.
            let path = std::str::from_utf8(entry.path).ok().map(PathBuf::from);
            let Some(p) = path.filter(|path| paths.contains(path)) else {
                return Err(
                    "Post-commit publication gate violation: unvalidated file committed in HEAD"
                        .to_string(),
                );
            };
            // A deletion publishes no blob; every other entry is scanned.
            if entry.status == b'D' {
                continue;
            }

            let blob = match self.read_blob_by_id(repo, entry.new_oid) {
                Ok(blob) => blob,
                Err(ObjectDefect::IdMismatch) => {
                    return Err(format!(
                        "Post-commit publication gate violation: an object read for {} {OBJECT_ID_MISMATCH}",
                        p.display()
                    ));
                }
                Err(ObjectDefect::Unreadable) => {
                    return Err(format!(
                        "Post-commit publication gate violation: failed to inspect committed blob for {}",
                        p.display()
                    ));
                }
            };

            if let Some(finding) = self.sanitizer.scan_bytes(&blob, &p) {
                return Err(format!(
                    "Post-commit publication gate violation: secret detected in committed HEAD blob {}: {}",
                    p.display(),
                    finding.reason()
                ));
            }

            // Same binding as the staged gate: a post-commit hook (it runs despite
            // --no-verify) may amend the commit after the staged gate passed.
            match validated_rel.get(&p) {
                None => {
                    return Err(format!(
                        "Post-commit publication gate violation: no validated content recorded for {}",
                        p.display()
                    ));
                }
                Some(digest) if *digest != Self::fingerprint(&blob) => {
                    return Err(format!(
                        "Post-commit publication gate violation: committed HEAD blob {} differs from the validated content",
                        p.display()
                    ));
                }
                Some(_) => {}
            }
        }

        // The change list holds blobs only, so an empty directory's name would publish
        // unlisted. The name is hook-chosen text and is never printed.
        match count_empty_directories(&listing) {
            None => {
                return Err(format!(
                    "Post-commit publication gate violation: unparseable tree entry in {commit}"
                ));
            }
            Some(0) => {}
            Some(_) => {
                return Err(
                    "Post-commit publication gate violation: the commit to publish contains an empty directory"
                        .to_string(),
                );
            }
        }

        self.require_validated_content(repo, &listed_blobs, &validated_rel)?;

        // Everything above read what the commit holds, and this binds it: no hook that ran
        // after the tree was recorded can have changed a name, a mode or a blob, and a hook
        // that keeps the content keeps the id and still publishes. It runs last so a refusal
        // names the most specific cause it can.
        if raw_commit_tree(&raw_commit) != Some(staged_tree.as_bytes()) {
            return Err(
                "Post-commit publication gate violation: the tree of the commit to publish is not the tree UDE staged; a hook in the cache checkout most likely changed it"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// Every path in the record (`validated_rel`) must hold its recorded content as a regular
    /// file in `listed_blobs` ([`tree_listing_blobs`]), read as [`Self::first_path_not_held`]
    /// reads it, so a push returning Ok has published it. One check covers a deletion and a
    /// revert, and a deletion nothing validated has no record, so it still publishes.
    fn require_validated_content(
        &self,
        repo: &Path,
        listed_blobs: &HashMap<&[u8], &[u8]>,
        validated_rel: &HashMap<PathBuf, &str>,
    ) -> Result<(), String> {
        match self.first_path_not_held(repo, listed_blobs, validated_rel) {
            Some((path, NotHeld::Content)) => Err(format!(
                "Post-commit publication gate violation: the commit to publish does not hold the validated content of {}",
                path.display()
            )),
            Some((path, NotHeld::IdMismatch)) => Err(format!(
                "Post-commit publication gate violation: an object read for {} {OBJECT_ID_MISMATCH}",
                path.display()
            )),
            None => Ok(()),
        }
    }

    /// The first path of the record (`validated_rel`) that `listed_blobs`
    /// ([`tree_listing_blobs`]) does not hold as a regular file with its recorded content,
    /// and why, or `None` when every recorded path is held. Each blob is read against its id
    /// ([`Self::read_blob_by_id`]), because `cat-file` returns a blob's stored bytes without
    /// checking that they hash to the id; the listing itself was built from hashed tree
    /// bytes ([`Self::verified_tree_listing`]). The path comes from the record, never from
    /// the tree.
    fn first_path_not_held<'r>(
        &self,
        repo: &Path,
        listed_blobs: &HashMap<&[u8], &[u8]>,
        validated_rel: &'r HashMap<PathBuf, &str>,
    ) -> Option<(&'r Path, NotHeld)> {
        validated_rel.iter().find_map(|(path, digest)| {
            let Some(oid) = listed_blobs.get(git_path_bytes(path).as_slice()) else {
                return Some((path.as_path(), NotHeld::Content));
            };
            match self.read_blob_by_id(repo, oid) {
                Ok(blob) if Self::fingerprint(&blob) == *digest => None,
                Err(ObjectDefect::IdMismatch) => Some((path.as_path(), NotHeld::IdMismatch)),
                Ok(_) | Err(ObjectDefect::Unreadable) => Some((path.as_path(), NotHeld::Content)),
            }
        })
    }

    /// What an empty `git status` (or an empty `staged_files`) must satisfy before a push
    /// returns "nothing to push": HEAD is `target.fetched_sha`, its tree, read past replace
    /// refs, holds every recorded path with its recorded content, and every staged path
    /// with no record (a deletion) is gone from it. The listing every decision is taken
    /// from is built ([`Self::verified_tree_listing`]) from tree bytes hashed against their
    /// ids, the root included, starting from the fetched head's commit read the same way
    /// ([`Self::read_verified_object`]); each recorded blob is read against its id too. With
    /// nothing staged and nothing recorded there is nothing to check. The fetched head was
    /// confirmed against the remote by the ensure, so Ok here means the remote's head held
    /// the content when read. A refusal names only a recorded or staged path, or none,
    /// never a name from the tree.
    fn require_fetched_head_holds(
        &self,
        target: &PublishTarget<'_>,
        staged_files: &[PathBuf],
        validated: &ValidatedBlobs,
    ) -> Result<(), String> {
        const PREFIX: &str = "Publication gate violation: git status reports nothing to push, but";
        let repo = target.checkout;
        let validated_rel = validated.relative_to(repo);
        if staged_files.is_empty() && validated_rel.is_empty() {
            return Ok(());
        }

        // A commit holding the content on top of the fetched head would read as held while
        // nothing reached the remote, so HEAD must be the fetched head itself.
        let head = self.resolve_commit(repo, "HEAD")?;
        if head != target.fetched_sha {
            return Err(format!(
                "{PREFIX} HEAD in the cache checkout is not the fetched head; a hook in the cache checkout most likely moved it"
            ));
        }

        // The commit and every tree are read against their ids, past replace refs, and the
        // listing is built from those bytes, so no decision below comes from tree bytes that
        // were not hashed: a tree object swapped between two reads cannot give one answer to
        // the listing and another to the check.
        let cannot_list = || format!("{PREFIX} the fetched head's tree cannot be listed");
        let raw_head = match self.read_verified_object(repo, head.as_bytes(), "commit") {
            Ok(raw_head) => raw_head,
            Err(ObjectDefect::Unreadable) => return Err(cannot_list()),
            Err(ObjectDefect::IdMismatch) => {
                return Err(format!(
                    "{PREFIX} the fetched head's commit {OBJECT_ID_MISMATCH}"
                ));
            }
        };
        let Some(root) = raw_commit_tree(&raw_head).and_then(|tree| std::str::from_utf8(tree).ok())
        else {
            return Err(cannot_list());
        };
        let mut budget = self.listing_budget();
        let listing = self.verified_tree_listing(repo, root, &mut HashMap::new(), &mut budget);
        self.after_tree_listing();
        let unparseable = || {
            format!(
                "{PREFIX} the fetched head's tree repeats a path, has a bad path component or cannot be parsed"
            )
        };
        let listing = match listing {
            Ok(listing) => listing,
            Err(ListingDefect::Unreadable) => return Err(cannot_list()),
            Err(ListingDefect::TooLarge) => {
                return Err(format!(
                    "{PREFIX} the fetched head's tree {LISTING_TOO_LARGE}"
                ));
            }
            Err(ListingDefect::PastDeadline) => {
                return Err(format!(
                    "{PREFIX} the fetched head's tree {LISTING_PAST_DEADLINE}"
                ));
            }
            Err(ListingDefect::Unparseable) => return Err(unparseable()),
            Err(ListingDefect::IdMismatch(tree)) => {
                // Named from the record, then the staged deletions, never from the tree.
                let mut recorded: Vec<&Path> = validated_rel.keys().map(PathBuf::as_path).collect();
                recorded.sort();
                let deleted = staged_files
                    .iter()
                    .map(|staged| staged.strip_prefix(repo).unwrap_or(staged))
                    .filter(|rel| !validated_rel.contains_key(*rel));
                return Err(
                    match first_path_under(&tree, recorded.into_iter().chain(deleted)) {
                        Some(path) => format!(
                            "{PREFIX} an object read for {} {OBJECT_ID_MISMATCH}",
                            path.display()
                        ),
                        None => format!("{PREFIX} a tree of the fetched head {OBJECT_ID_MISMATCH}"),
                    },
                );
            }
        };
        let Ok(listed_blobs) = tree_listing_blobs(&listing) else {
            return Err(unparseable());
        };

        match self.first_path_not_held(repo, &listed_blobs, &validated_rel) {
            Some((path, NotHeld::Content)) => {
                return Err(format!(
                    "{PREFIX} the fetched head does not hold the validated content of {}; a hook in the cache checkout most likely hid the change",
                    path.display()
                ));
            }
            Some((path, NotHeld::IdMismatch)) => {
                return Err(format!(
                    "{PREFIX} an object read for {} {OBJECT_ID_MISMATCH}",
                    path.display()
                ));
            }
            None => {}
        }
        // Any entry type counts as present, not only the regular files listed_blobs keeps.
        let listed = |path: &[u8]| {
            listing
                .split(|byte| *byte == 0)
                .filter_map(|record| record.splitn(2, |byte| *byte == b'\t').nth(1))
                .any(|listed_path| listed_path == path)
        };
        // Every tree in the listing was hashed when it was read, so a rewritten tree above
        // a deleted path was refused above and cannot leave the path out of the listing.
        for staged in staged_files {
            let rel = staged.strip_prefix(repo).unwrap_or(staged);
            if validated_rel.contains_key(rel) {
                continue;
            }
            if listed(&git_path_bytes(rel)) {
                return Err(format!(
                    "{PREFIX} {} is staged for deletion and still present in the fetched head; a hook in the cache checkout most likely hid the change",
                    rel.display()
                ));
            }
        }
        Ok(())
    }

    /// The exact bytes of the blob `oid`, past any replace ref. Unreadable on a spawn error,
    /// a non-zero exit (a gitlink's id, for one), an id that is not UTF-8 or an object that
    /// is not a blob: `cat-file blob` peels a tag, and a tree can list a tag at a file's
    /// place, which would read as the tagged bytes while the push sends the tag's own text
    /// (probed). The bytes must also hash to `oid` ([`Self::read_verified_object`]), because
    /// `cat-file` does not check that.
    fn read_blob_by_id(&self, repo: &Path, oid: &[u8]) -> Result<Vec<u8>, ObjectDefect> {
        #[cfg(test)]
        if let Some(reads) = &self.blob_reads {
            reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.read_verified_object(repo, oid, "blob")
    }

    /// The exact bytes of the object `oid` of type `kind` (`blob`, `tree` or `commit`) in `repo`, past
    /// any replace ref, checked against the id: git returns the bytes stored under an id
    /// without checking that they hash to it, for a blob read with `cat-file` and for a
    /// subtree walked by `ls-tree -r` (measured). The bytes are hashed with `git hash-object`
    /// in `repo`, so the repository's own object format applies, and the id it prints is
    /// compared whole with `oid`; an abbreviated id therefore never matches. `--literally`
    /// keeps the check to hashing and `--no-filters` on stdin hashes the bytes exactly. No
    /// write flag is passed, so the check writes nothing.
    fn read_verified_object(
        &self,
        repo: &Path,
        oid: &[u8],
        kind: &str,
    ) -> Result<Vec<u8>, ObjectDefect> {
        let oid = std::str::from_utf8(oid).map_err(|_| ObjectDefect::Unreadable)?;
        let mut object_type = self.git(Some(repo));
        object_type.args(["--no-replace-objects", "cat-file", "-t", oid]);
        let object_type = self
            .run_local(object_type, "git cat-file")
            .map_err(|_| ObjectDefect::Unreadable)?;
        if !object_type.status.success() || object_type.stdout.trim_ascii_end() != kind.as_bytes() {
            return Err(ObjectDefect::Unreadable);
        }
        let mut cat_file = self.git(Some(repo));
        cat_file.args(["--no-replace-objects", "cat-file", kind, oid]);
        let out = self
            .run_local(cat_file, "git cat-file")
            .map_err(|_| ObjectDefect::Unreadable)?;
        if !out.status.success() {
            return Err(ObjectDefect::Unreadable);
        }
        let mut hash_object = self.git(Some(repo));
        hash_object.args([
            "hash-object",
            "-t",
            kind,
            "--literally",
            "--no-filters",
            "--stdin",
        ]);
        let hashed = self
            .run_local_with_input(hash_object, "git hash-object", out.stdout.clone())
            .map_err(|_| ObjectDefect::Unreadable)?;
        if !hashed.status.success() {
            return Err(ObjectDefect::Unreadable);
        }
        let printed = hashed.stdout.strip_suffix(b"\n").unwrap_or(&hashed.stdout);
        if printed != oid.as_bytes() {
            return Err(ObjectDefect::IdMismatch);
        }
        Ok(out.stdout)
    }

    /// The listing `ls-tree -r -t -z --full-tree` prints for the tree `root`, built in process
    /// from tree bytes hashed against their ids ([`Self::read_verified_object`]), the root
    /// included, so no entry in it comes from bytes that were not hashed. Each record is the
    /// mode as six octal digits, a space, the type (`tree`, `commit` for a gitlink, `blob`),
    /// a space, the id as lowercase hex, a tab, the path as raw bytes, NUL; a tree's record
    /// comes before its contents and its contents before its next sibling, and a gitlink is
    /// not entered. The id length follows `root`'s (40 or 64 hex digits). Trees already in
    /// `verified` are not read again, and each tree read is kept there by id. The walk is
    /// iterative, so no depth can overflow the stack. Every record is taken from `budget`
    /// before it is pushed ([`Self::push_tree_entries`]), so the listing and the entries
    /// still pending together never hold more records or bytes than `budget` had left;
    /// past either, [`ListingDefect::TooLarge`]. Each tree checks `budget`'s deadline before
    /// it is read; at or past it, [`ListingDefect::PastDeadline`].
    fn verified_tree_listing(
        &self,
        repo: &Path,
        root: &str,
        verified: &mut HashMap<String, Vec<u8>>,
        budget: &mut ListingBudget,
    ) -> Result<Vec<u8>, ListingDefect> {
        let id_len = match root.len() {
            40 | 64 => root.len() / 2,
            _ => return Err(ListingDefect::Unreadable),
        };
        let mut listing = Vec::new();
        // Entries still to print, the next one last: (path, mode, id as hex).
        let mut pending = Vec::new();
        self.push_tree_entries(repo, root, &[], id_len, verified, &mut pending, budget)?;
        while let Some((path, mode, id)) = pending.pop() {
            let kind = listing_kind(mode);
            listing.extend_from_slice(format!("{mode:06o} {kind} {id}\t").as_bytes());
            listing.extend_from_slice(&path);
            listing.push(0);
            if mode == MODE_DIRECTORY {
                self.push_tree_entries(repo, &id, &path, id_len, verified, &mut pending, budget)?;
            }
        }
        Ok(listing)
    }

    /// Reads the tree `id` at listing path `path` through `verified` (as
    /// [`Self::verified_tree_listing`] describes) and pushes its entries onto `pending` in
    /// reverse stored order, so the first entry is popped next. Before it pushes any, it
    /// takes from `budget` one record per entry and the bytes the listing will write for
    /// them (mode, type word, id, the full path and the separators), summed with checked
    /// arithmetic, so an overflow is [`ListingDefect::TooLarge`] too.
    #[expect(
        clippy::too_many_arguments,
        reason = "the walk state verified_tree_listing threads through each tree"
    )]
    fn push_tree_entries(
        &self,
        repo: &Path,
        id: &str,
        path: &[u8],
        id_len: usize,
        verified: &mut HashMap<String, Vec<u8>>,
        pending: &mut Vec<(Vec<u8>, u32, String)>,
        budget: &mut ListingBudget,
    ) -> Result<(), ListingDefect> {
        // Before any read: the root and every tree the walk pops check the deadline. A blob
        // record between two checks costs no read.
        budget.check_time()?;
        if !verified.contains_key(id) {
            let bytes = match self.read_verified_object(repo, id.as_bytes(), "tree") {
                Ok(bytes) => bytes,
                Err(ObjectDefect::IdMismatch) => {
                    return Err(ListingDefect::IdMismatch(path.to_vec()));
                }
                Err(ObjectDefect::Unreadable) => return Err(ListingDefect::Unreadable),
            };
            verified.insert(id.to_string(), bytes);
        }
        let entries = parse_tree_object(&verified[id], id_len).ok_or(ListingDefect::Unparseable)?;
        // Each record: six mode digits, a space, the type word, a space, the hex id, a tab,
        // the parent's path and a `/` unless the parent is the root, the name, a NUL.
        let parent = if path.is_empty() { 0 } else { path.len() + 1 };
        let fixed = 6 + 1 + 1 + 2 * id_len + 1 + 1;
        let bytes = entries
            .iter()
            .try_fold(0usize, |sum, entry| {
                sum.checked_add(fixed)?
                    .checked_add(parent)?
                    .checked_add(listing_kind(entry.mode).len())?
                    .checked_add(entry.name.len())
            })
            .ok_or(ListingDefect::TooLarge)?;
        budget.take(entries.len(), bytes)?;
        for entry in entries.iter().rev() {
            let mut entry_path = path.to_vec();
            if !entry_path.is_empty() {
                entry_path.push(b'/');
            }
            entry_path.extend_from_slice(entry.name);
            pending.push((entry_path, entry.mode, lower_hex(entry.id)));
        }
        Ok(())
    }
}

/// Why [`SyncEngine::read_verified_object`] did not return an object.
#[derive(Debug, PartialEq, Eq)]
enum ObjectDefect {
    /// The id is not UTF-8, the object is missing or of another type, or a read or the hash
    /// failed.
    Unreadable,
    /// The bytes stored under the id do not hash to it.
    IdMismatch,
}

/// Why [`SyncEngine::first_path_not_held`] found a recorded path not held.
#[derive(Debug, PartialEq, Eq)]
enum NotHeld {
    /// The listing does not hold the path as a regular file with its recorded content.
    Content,
    /// An object read for the path does not hash to its id.
    IdMismatch,
}

/// The refusal text of an object whose bytes do not hash to its id.
const OBJECT_ID_MISMATCH: &str = "does not match its object id in the cache checkout; an object file there was most likely rewritten";

/// The refusal text of a listing over the records or the bytes one gate call may list
/// ([`ListingBudget`]). One text for both limits: it says the listing is too large, not
/// which amount ran out, and prints no number, so a test's lower limit prints it too.
const LISTING_TOO_LARGE: &str = "is larger than the publication gate lists for one push; a tree in the cache checkout most likely repeats a subtree or holds very long names";

/// The refusal text of a listing or diff not finished by the deadline of one gate call
/// ([`ListingBudget`]), the local time limit from the start of the walk.
const LISTING_PAST_DEADLINE: &str =
    "could not be listed within the publication gate's time limit for one push";

/// The records one publication gate call may list, over all its listings together
/// ([`ListingBudget`]). A tree whose subtrees repeat lists 2^d paths from d + 3 tree
/// objects, and git limits neither, so without a limit a hook in the cache checkout could
/// make the walk exhaust memory (review WR-600).
///
/// ## Decisions and assumptions recorded here
///
/// - This repository's whole tree lists 286 records (56 trees) at e9165f0, so one push's
///   two listings take about 570: the limit is over 400 times that.
/// - The cost of a gate call at the limit is measured by plan 02-27 Task 3, not assumed.
const MAX_LISTING_RECORDS: usize = 1 << 18;

/// The listing bytes one publication gate call may list, over all its listings together
/// ([`ListingBudget`]). A record holds its full path and git does not limit an entry
/// name's length, so a record limit alone does not bound memory.
///
/// ## Decisions and assumptions recorded here
///
/// - This repository's whole listing is 29 457 bytes at e9165f0, so 64 MiB is over 2000
///   times one push's two listings.
/// - It equals 2^18 records at an average of 256 bytes a record, so neither limit is much
///   looser than the other.
/// - The cost of a gate call at the limit is measured by plan 02-27 Task 3, not assumed.
const MAX_LISTING_BYTES: usize = 1 << 26;

/// What one publication gate call may still list, shared by every listing of that call
/// ([`SyncEngine::verified_tree_listing`]) and by its diff ([`listing_diff`]), so the
/// call's listings together never hold more records or listing bytes than it started
/// with, and its walk and diff stop at one deadline set when the walk starts.
struct ListingBudget {
    records_left: usize,
    bytes_left: usize,
    deadline: std::time::Instant,
}

impl ListingBudget {
    /// A budget of `max_records` records and `max_bytes` bytes whose deadline is `time`
    /// from now.
    fn new(max_records: usize, max_bytes: usize, time: Duration) -> Self {
        Self {
            records_left: max_records,
            bytes_left: max_bytes,
            deadline: std::time::Instant::now() + time,
        }
    }

    /// [`ListingDefect::PastDeadline`] at or past the budget's deadline.
    fn check_time(&self) -> Result<(), ListingDefect> {
        if is_past(self.deadline) {
            return Err(ListingDefect::PastDeadline);
        }
        Ok(())
    }

    /// Takes `records` records and `bytes` listing bytes from what is left;
    /// [`ListingDefect::TooLarge`], taking nothing, when either is more than is left.
    fn take(&mut self, records: usize, bytes: usize) -> Result<(), ListingDefect> {
        let (Some(records_left), Some(bytes_left)) = (
            self.records_left.checked_sub(records),
            self.bytes_left.checked_sub(bytes),
        ) else {
            return Err(ListingDefect::TooLarge);
        };
        self.records_left = records_left;
        self.bytes_left = bytes_left;
        Ok(())
    }
}

/// Whether `deadline` has come: true at or past it. The only deadline test of the gate's
/// walk and diff; it holds no comparison operator.
fn is_past(deadline: std::time::Instant) -> bool {
    std::time::Instant::now()
        .checked_duration_since(deadline)
        .is_some()
}

/// The type word of a listing record for an entry of mode `mode`: `tree`, `commit` for a
/// gitlink, `blob` otherwise.
fn listing_kind(mode: u32) -> &'static str {
    match mode {
        MODE_DIRECTORY => "tree",
        MODE_GITLINK => "commit",
        _ => "blob",
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

/// The parent ids of a raw commit object (`git cat-file commit` output), taken only from
/// its header lines, which end at the first empty line. A line counts only when it starts
/// with `parent `, so indented continuation lines of multi-line headers such as `gpgsig`
/// and any `parent` text in the message never count.
fn raw_commit_parents(raw: &[u8]) -> Vec<String> {
    raw.split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.strip_prefix(b"parent "))
        .map(|id| String::from_utf8_lossy(id).into_owned())
        .collect()
}

/// The bytes after `tree ` on the first line of a raw commit object (`git cat-file commit`
/// output), the only line git reads a commit's tree from, with no decoding. `None` when the
/// first line does not start with `tree `.
fn raw_commit_tree(raw: &[u8]) -> Option<&[u8]> {
    raw.split(|byte| *byte == b'\n')
        .next()?
        .strip_prefix(b"tree ")
}

/// Splits a raw commit object (`git cat-file commit` output) at its first empty line into
/// its header bytes and its message bytes, with no decoding and no trimming. `None` when
/// the object has no empty line.
fn raw_commit_parts(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    raw.windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|last_header_byte| (&raw[..=last_header_byte], &raw[last_header_byte + 2..]))
}

/// One record of `diff-tree -r -z --raw` output, as raw bytes.
struct RawDiffEntry<'a> {
    new_oid: &'a [u8],
    status: u8,
    path: &'a [u8],
}

/// Parses `diff-tree -r -z --raw` output (records `:<old mode> <new mode> <old id> <new id>
/// <status>` NUL `<path>` NUL) with no decoding. `None` for a record it cannot parse, so the
/// caller fails closed.
fn raw_diff_entries(raw: &[u8]) -> Option<Vec<RawDiffEntry<'_>>> {
    let mut entries = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let (record, after_record) = split_at_nul(rest)?;
        let (path, after_path) = split_at_nul(after_record)?;
        if path.is_empty() {
            return None;
        }
        let mut fields = record.strip_prefix(b":")?.split(|byte| *byte == b' ');
        let (_old_mode, _new_mode, _old_oid) = (fields.next()?, fields.next()?, fields.next()?);
        let (new_oid, status) = (fields.next()?, *fields.next()?.first()?);
        if fields.next().is_some() {
            return None;
        }
        entries.push(RawDiffEntry {
            new_oid,
            status,
            path,
        });
        rest = after_path;
    }
    Some(entries)
}

/// The bytes before the first NUL and the bytes after it; `None` when there is no NUL.
fn split_at_nul(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let nul = bytes.iter().position(|byte| *byte == 0)?;
    Some((&bytes[..nul], &bytes[nul + 1..]))
}

/// One record of an `ls-tree -r -t -z --full-tree` listing that is not a `tree`, as raw bytes.
struct ListingRecord<'a> {
    mode: &'a [u8],
    id: &'a [u8],
    path: &'a [u8],
}

/// The records of `listing` (`<mode> SP <type> SP <id> TAB <path>`, each ended by NUL) whose
/// type is not `tree`, in order. `None` for a record that does not split into a mode, a type,
/// an id and a path, or whose mode is not octal.
fn listing_records(listing: &[u8]) -> Option<Vec<ListingRecord<'_>>> {
    let mut records = Vec::new();
    for record in listing.split_inclusive(|byte| *byte == 0) {
        let record = record.strip_suffix(&[0])?;
        let tab = record.iter().position(|byte| *byte == b'\t')?;
        let (meta, path) = (&record[..tab], &record[tab + 1..]);
        let mut fields = meta.split(|byte| *byte == b' ');
        let (mode, kind, id) = (fields.next()?, fields.next()?, fields.next()?);
        if fields.next().is_some() || kind.is_empty() || id.is_empty() {
            return None;
        }
        mode_type_bits(mode)?;
        if kind != b"tree" {
            records.push(ListingRecord { mode, id, path });
        }
    }
    Some(records)
}

/// The type bits ([`MODE_TYPE_MASK`]) of a mode written in octal ASCII; `None` when it is not.
fn mode_type_bits(mode: &[u8]) -> Option<u32> {
    let mode = std::str::from_utf8(mode).ok()?;
    if mode.is_empty() || !mode.bytes().all(|digit| (b'0'..=b'7').contains(&digit)) {
        return None;
    }
    Some(u32::from_str_radix(mode, 8).ok()? & MODE_TYPE_MASK)
}

/// The bytes `diff-tree -r -z --raw --no-commit-id --no-renames <base> <commit>` prints
/// between the trees whose `ls-tree -r -t -z --full-tree` listings are `base` and `commit`,
/// so [`raw_diff_entries`] parses them as it parses git's. Only records whose type is not
/// `tree` count. A commit record matching an unmatched base record at its path in mode and id
/// is no change; any other is `M` against the first unmatched base record at its path (`T`
/// when the two modes' type bits differ), consuming it, or `A` with old mode `000000` and an
/// all-zero old id. Each base record left unmatched is `D`, with new mode `000000` and an
/// all-zero new id. A zero id has the length of the id beside it. Records are written as
/// `:<old mode> <new mode> <old id> <new id> <status>` NUL `<path>` NUL and sorted by path
/// bytes, stably: that is git's order, which compares a directory as its name followed by
/// `/`. A path listed twice is matched as a multiset, so a repeated entry the base does not
/// hold identically is a change. [`DiffDefect::Unparseable`] when either listing has a
/// record that does not parse. [`DiffDefect::PastDeadline`] when `deadline` has come at the
/// start of a pass of either loop: a path listed many times in both listings makes the
/// multiset match quadratic, which the record limit alone does not bound.
fn listing_diff(
    base: &[u8],
    commit: &[u8],
    deadline: std::time::Instant,
) -> Result<Vec<u8>, DiffDefect> {
    let base = listing_records(base).ok_or(DiffDefect::Unparseable)?;
    let commit = listing_records(commit).ok_or(DiffDefect::Unparseable)?;
    let mut base_at: HashMap<&[u8], Vec<usize>> = HashMap::new();
    for (index, record) in base.iter().enumerate() {
        base_at.entry(record.path).or_default().push(index);
    }
    let mut matched = vec![false; base.len()];
    let mut changes: Vec<(&[u8], Vec<u8>)> = Vec::new();
    for new in &commit {
        if is_past(deadline) {
            return Err(DiffDefect::PastDeadline);
        }
        let at_path = base_at.get(new.path).map(Vec::as_slice).unwrap_or_default();
        let unmatched = || at_path.iter().copied().filter(|index| !matched[*index]);
        let same =
            unmatched().find(|index| base[*index].mode == new.mode && base[*index].id == new.id);
        if let Some(index) = same {
            matched[index] = true;
            continue;
        }
        let first_unmatched = unmatched().next();
        let record = match first_unmatched {
            Some(index) => {
                matched[index] = true;
                let old = &base[index];
                let bits = |mode| mode_type_bits(mode).ok_or(DiffDefect::Unparseable);
                let status = if bits(old.mode)? == bits(new.mode)? {
                    b'M'
                } else {
                    b'T'
                };
                diff_record_bytes(old.mode, new.mode, old.id, new.id, status, new.path)
            }
            None => {
                let zero = vec![b'0'; new.id.len()];
                diff_record_bytes(b"000000", new.mode, &zero, new.id, b'A', new.path)
            }
        };
        changes.push((new.path, record));
    }
    for (old, _) in base.iter().zip(&matched).filter(|(_, matched)| !**matched) {
        if is_past(deadline) {
            return Err(DiffDefect::PastDeadline);
        }
        let zero = vec![b'0'; old.id.len()];
        let record = diff_record_bytes(old.mode, b"000000", old.id, &zero, b'D', old.path);
        changes.push((old.path, record));
    }
    changes.sort_by(|left, right| left.0.cmp(right.0));
    Ok(changes.into_iter().flat_map(|(_, record)| record).collect())
}

/// Why [`listing_diff`] did not produce a change list.
#[derive(Debug, PartialEq, Eq)]
enum DiffDefect {
    /// A record of either listing does not parse.
    Unparseable,
    /// The deadline came before the diff finished.
    PastDeadline,
}

/// One `diff-tree -z --raw` record: `:<old mode> <new mode> <old id> <new id> <status>` NUL
/// `<path>` NUL.
fn diff_record_bytes(
    old_mode: &[u8],
    new_mode: &[u8],
    old_id: &[u8],
    new_id: &[u8],
    status: u8,
    path: &[u8],
) -> Vec<u8> {
    [
        b":",
        old_mode,
        b" ",
        new_mode,
        b" ",
        old_id,
        b" ",
        new_id,
        b" ",
        &[status],
        b"\0",
        path,
        b"\0",
    ]
    .concat()
}

/// Removes every `GIT_TEST_*` variable from `command`'s environment: the keys of
/// `inherited` (UDE's own environment at the call sites) and the keys already set on the
/// command, matched on their raw bytes, case-sensitively. Only the spawned command changes,
/// never UDE's process environment, which every thread shares.
fn remove_git_test_env(command: &mut Command, inherited: impl IntoIterator<Item = OsString>) {
    let own: Vec<OsString> = command
        .get_envs()
        .map(|(key, _)| key.to_os_string())
        .collect();
    let test_keys: Vec<OsString> = inherited
        .into_iter()
        .chain(own)
        .filter(|key| key.as_encoded_bytes().starts_with(b"GIT_TEST_"))
        .collect();
    for key in test_keys {
        command.env_remove(key);
    }
}

/// Counts the `tree` records with no record directly below them in `ls-tree -r -t -z`
/// output (records `<mode> SP <type> SP <oid> TAB <path>`, each ended by NUL). A record is
/// directly below the path its own path holds before its last `/`, compared as raw bytes,
/// never through a lossy decode. Only that parent path is kept per record, so the work is
/// linear in the listing's bytes. Every listing it reads lists every intermediate tree
/// (as `ls-tree -r -t` does), so on names holding no `/` this is the count of trees with
/// no record anywhere below them; a name holding `/` can only add to the count, never
/// hide an empty tree. `None` for a record it cannot parse.
fn count_empty_directories(listing: &[u8]) -> Option<usize> {
    let mut trees = Vec::new();
    let mut parents = HashSet::new();
    for record in listing.split_inclusive(|byte| *byte == 0) {
        let record = record.strip_suffix(&[0])?;
        let mut fields = record.splitn(2, |byte| *byte == b'\t');
        let (meta, path) = (fields.next()?, fields.next()?);
        if meta.split(|byte| *byte == b' ').nth(1)? == b"tree" {
            trees.push(path);
        }
        if let Some(slash) = path.iter().rposition(|byte| *byte == b'/') {
            parents.insert(&path[..slash]);
        }
    }
    Some(
        trees
            .into_iter()
            .filter(|tree| !parents.contains(tree))
            .count(),
    )
}

/// Why [`tree_listing_blobs`] refused a listing.
#[derive(Debug, PartialEq, Eq)]
enum TreeDefect {
    /// A record it cannot parse.
    Unparseable,
    /// A path, blob or tree, listed twice.
    RepeatedPath,
    /// A path component that is empty, `.` or `..`.
    BadComponent,
}

/// Reads `ls-tree -r -t -z` output (records `<mode> SP <type> SP <oid> TAB <path>`, each ended
/// by NUL) and refuses a repeated path, blob or tree, and any path component that is empty,
/// `.` or `..`, compared as raw bytes. Returns path to object id for regular files only (type
/// blob, mode 100644 or 100755).
fn tree_listing_blobs(listing: &[u8]) -> Result<HashMap<&[u8], &[u8]>, TreeDefect> {
    let mut seen = HashSet::new();
    let mut blobs = HashMap::new();
    for record in listing.split_inclusive(|byte| *byte == 0) {
        let record = record.strip_suffix(&[0]).ok_or(TreeDefect::Unparseable)?;
        let mut fields = record.splitn(2, |byte| *byte == b'\t');
        let (meta, path) = match (fields.next(), fields.next()) {
            (Some(meta), Some(path)) => (meta, path),
            _ => return Err(TreeDefect::Unparseable),
        };
        let meta: Vec<&[u8]> = meta.split(|byte| *byte == b' ').collect();
        let [mode, kind, oid] = meta.as_slice() else {
            return Err(TreeDefect::Unparseable);
        };
        let bad_component = |component: &[u8]| matches!(component, b"" | b"." | b"..");
        if path.split(|byte| *byte == b'/').any(bad_component) {
            return Err(TreeDefect::BadComponent);
        }
        if !seen.insert(path) {
            return Err(TreeDefect::RepeatedPath);
        }
        if *kind == b"blob" && matches!(*mode, b"100644" | b"100755") {
            blobs.insert(path, *oid);
        }
    }
    Ok(blobs)
}

/// The raw bytes of `path`'s components joined by `/`, the way a git tree listing spells it.
fn git_path_bytes(path: &Path) -> Vec<u8> {
    let components: Vec<&[u8]> = path
        .components()
        .map(|component| component.as_os_str().as_encoded_bytes())
        .collect();
    components.join(&b'/')
}

/// The bits of a git mode that give the entry type.
const MODE_TYPE_MASK: u32 = 0o170000;
/// A regular file's type bits.
const MODE_REGULAR: u32 = 0o100000;
/// A symlink's mode.
const MODE_SYMLINK: u32 = 0o120000;
/// A directory's (tree's) mode.
const MODE_DIRECTORY: u32 = 0o040000;
/// A gitlink's (submodule commit's) mode.
const MODE_GITLINK: u32 = 0o160000;
/// The owner-execute bit that makes a regular file 100755.
const MODE_OWNER_EXECUTE: u32 = 0o100;

/// The mode git's `canon_mode` makes of a raw tree entry's mode: a regular file is 100755
/// when the owner-execute bit is set and 100644 otherwise, a symlink 120000, a directory
/// 040000, anything else a gitlink, 160000.
fn canonical_mode(mode: u32) -> u32 {
    match mode & MODE_TYPE_MASK {
        MODE_REGULAR if mode & MODE_OWNER_EXECUTE != 0 => 0o100755,
        MODE_REGULAR => 0o100644,
        MODE_SYMLINK => MODE_SYMLINK,
        MODE_DIRECTORY => MODE_DIRECTORY,
        _ => MODE_GITLINK,
    }
}

/// One entry of a raw tree object, as [`parse_tree_object`] decodes it.
struct TreeEntry<'a> {
    /// The mode, canonical as [`canonical_mode`] makes it.
    mode: u32,
    /// The entry's name: raw bytes, never empty, with no NUL.
    name: &'a [u8],
    /// The raw id bytes, exactly the repository's id length.
    id: &'a [u8],
}

/// Decodes a raw tree object (`cat-file tree` output) as git's tree decoder does: each entry
/// is one or more ASCII octal digits, a space, a non-empty name, NUL, then exactly `id_len`
/// raw id bytes. `None` where git's decoder refuses (no digit, a digit outside 0-7, an empty
/// name, no NUL, too few id bytes), and for a mode that overflows u32, which git's mode
/// parser accepts by wrapping: there this decoder fails closed. Empty input is an empty
/// tree.
fn parse_tree_object(bytes: &[u8], id_len: usize) -> Option<Vec<TreeEntry<'_>>> {
    let mut entries = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let space = rest.iter().position(|byte| *byte == b' ')?;
        let (digits, after_mode) = (&rest[..space], &rest[space + 1..]);
        if digits.is_empty() {
            return None;
        }
        let mut mode: u32 = 0;
        for digit in digits {
            if !(b'0'..=b'7').contains(digit) {
                return None;
            }
            mode = mode.checked_mul(8)?.checked_add(u32::from(digit - b'0'))?;
        }
        let (name, after_name) = split_at_nul(after_mode)?;
        if name.is_empty() || after_name.len() < id_len {
            return None;
        }
        let (id, after_id) = after_name.split_at(id_len);
        entries.push(TreeEntry {
            mode: canonical_mode(mode),
            name,
            id,
        });
        rest = after_id;
    }
    Some(entries)
}

/// Why [`SyncEngine::verified_tree_listing`] did not build a listing.
#[derive(Debug, PartialEq, Eq)]
enum ListingDefect {
    /// The root id is neither 40 nor 64 hex digits, or a tree is missing, of another type,
    /// or a read or its hash failed.
    Unreadable,
    /// A tree's bytes do not decode as git decodes a tree ([`parse_tree_object`]).
    Unparseable,
    /// The bytes stored under a tree's id do not hash to it. Holds that tree's listing
    /// path, empty for the root; it is tree-chosen text and never printed.
    IdMismatch(Vec<u8>),
    /// The listing would take more records or bytes than its [`ListingBudget`] had left.
    TooLarge,
    /// The walk reached a tree at or past its [`ListingBudget`]'s deadline.
    PastDeadline,
}

/// `bytes` as lowercase hex, the way git prints an object id.
fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The first of `candidates` (recorded, staged or allowlisted paths) that lies under the
/// listing path `tree`, or the first candidate at all when `tree` is empty (the root).
/// Compared as raw bytes ([`git_path_bytes`]).
fn first_path_under<'p>(
    tree: &[u8],
    candidates: impl IntoIterator<Item = &'p Path>,
) -> Option<&'p Path> {
    candidates.into_iter().find(|path| {
        tree.is_empty()
            || git_path_bytes(path)
                .strip_prefix(tree)
                .is_some_and(|below| below.starts_with(b"/"))
    })
}

/// `remote_url` unchanged, unless it is a relative path that exists, which is made
/// absolute against the process's working directory. `git clone` stores a relative path
/// absolute, but a read run from another directory (the filesystem root) would resolve it
/// there; URLs, scp-style addresses and paths that do not exist stay as given, and so
/// does a path whose absolute form is not UTF-8.
fn absolute_remote_url(remote_url: &str) -> String {
    let path = Path::new(remote_url);
    if path.is_relative()
        && path.exists()
        && let Ok(absolute) = std::path::absolute(path)
        && let Some(absolute) = absolute.to_str()
    {
        return absolute.to_string();
    }
    remote_url.to_string()
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

/// Whether `template` holds template logic that push must neither overwrite nor delete.
///
/// A UTF-8 template has logic when the shared render rule
/// ([`crate::template::should_render`], the rule init, scan and pull use) renders it and
/// it holds a template construct (`{{`, `{%` or `{#`) other than the parameterized path
/// placeholders: a push of its rendered text would erase that construct (CR-02), and a
/// deletion would remove it for every project. An absent template has none.
///
/// A template that is not UTF-8 is never rendered. It has logic only when it is a
/// `.jinja` template whose bytes hold such a construct after the same placeholder
/// removal (Phase 01 D-14: push never deletes a `.jinja` template that has template
/// logic). The check runs on the raw bytes with every non-ASCII byte read as NUL; every
/// placeholder and marker is ASCII, so this keeps every match and creates none.
pub fn template_has_logic(template: &Path) -> Result<bool, String> {
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
    let guarded = match std::str::from_utf8(&bytes) {
        Ok(text) => crate::template::should_render(template, text),
        Err(_) => template.extension().is_some_and(|ext| ext == "jinja"),
    };
    if !guarded {
        return Ok(false);
    }
    let mut text: String = bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii() {
                char::from(byte)
            } else {
                '\0'
            }
        })
        .collect();
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
        let remote_tree = || git.output(bare.path(), &["ls-tree", "-r", "--name-only", "main"]);

        let gitconfig = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::remove_file(&gitconfig).unwrap();
        // A deletion publishes no blob, so it needs no validated content.
        let pushed = engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            &[gitconfig],
            &ValidatedBlobs::default(),
        );
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
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&hook));
        let err = push_failure(engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            &[gitignore, hook],
            &validated,
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
        let check = |name: &str, bytes: Option<&[u8]>| {
            let path = dir.path().join(name);
            if let Some(bytes) = bytes {
                fs::write(&path, bytes).unwrap();
            }
            template_has_logic(&path).unwrap()
        };
        // Text after a byte that makes the template invalid UTF-8.
        let non_utf8 = |text: &str| [&[0xFF][..], text.as_bytes()].concat();
        for logic in [
            "{% if x %}a{% endif %}",
            "a = {{ project_name }}",
            "{# note #}",
        ] {
            assert!(check("logic.jinja", Some(logic.as_bytes())), "{logic}");
        }
        // A static template the render rule renders (it holds `{{`): its other
        // constructs are logic a push of rendered text would erase.
        assert!(
            check(
                "gitconfig.repo",
                Some(b"[user]\n\temail = {{ git_email }}\n")
            ),
            "static template with a git_email placeholder"
        );
        assert!(
            check(
                "gitconfig.repo",
                Some(b"{{ user.home }}/x\n{% if x %}y{% endif %}\n")
            ),
            "static template rendered for its path placeholder, holding an if block"
        );
        assert!(
            check("pre-push", Some(b"name = {{ project_name }}\n")),
            "hook without extension with a project_name placeholder"
        );
        // Not UTF-8, so never rendered; a `.jinja` template with logic stays protected
        // (Phase 01 D-14).
        assert!(
            check("logic.jinja", Some(&non_utf8("{{ x }}"))),
            "non-UTF-8 .jinja template with a placeholder"
        );
        // Only the placeholders push writes back: a rendered push loses nothing.
        assert!(!check(
            "params.jinja",
            Some(b"home = \"{{ user.home }}\"\ncfg = \"{{ user.xdg_config }}\"\n")
        ));
        assert!(
            !check(
                "gitconfig.repo",
                Some(b"path = {{ user.home }}/a\ncfg = {{ user.xdg_config }}/b\n")
            ),
            "static template with only the path placeholders"
        );
        assert!(
            !check(
                "params.jinja",
                Some(&non_utf8("home = \"{{ user.home }}\"\n"))
            ),
            "non-UTF-8 .jinja template with only a path placeholder"
        );
        assert!(!check("plain.jinja", Some(b"[tools]\nrust = \"latest\"\n")));
        assert!(
            !check(
                "plain.jinja",
                Some(&non_utf8("[tools]\nrust = \"latest\"\n"))
            ),
            "non-UTF-8 .jinja template without a construct"
        );
        assert!(!check("absent.jinja", None));
        assert!(!check("absent.repo", None), "absent static template");
        // A template that exists but cannot be read is an error, never "no logic".
        let unreadable = dir.path().join("unreadable.repo");
        fs::create_dir(&unreadable).unwrap();
        assert!(
            template_has_logic(&unreadable).is_err(),
            "template path that is a directory"
        );
        // Not rendered (no `{{`), so a push of the local text erases nothing.
        assert!(!check("gitconfig.repo", Some(b"{% if x %}a{% endif %}")));
        assert!(
            !check(
                "commit-msg",
                Some(b"if [ \"${#FIRST_LINE}\" -gt 72 ]; then\n")
            ),
            "shell length expansion in an unrendered hook"
        );
        // Neither UTF-8 nor `.jinja`: never rendered, so no logic.
        assert!(
            !check("gitconfig.repo", Some(&non_utf8("{{ x }}"))),
            "non-UTF-8 static template with a placeholder"
        );
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

    /// Writes `template_text` as the `.gitconfig` template and records `.gitconfig` with
    /// that template as push's own scan renders it (the host's git config supplies the
    /// values). Returns the context and the rendered text.
    fn record_rendered_gitconfig(
        local: &Path,
        upstream: &Path,
        template_text: &str,
    ) -> (crate::template::TemplateContext, String) {
        write_upstream_gitconfig(upstream, template_text.as_bytes());
        let ctx = crate::template::TemplateContext::detect_for_project(local, "project", "generic");
        let rendered = crate::template::TemplateRenderer::new()
            .render_str(template_text, &ctx)
            .unwrap();
        let mut state = ProjectState::default();
        state.record_file(
            Path::new(".gitconfig"),
            SyncEngine::fingerprint(rendered.as_bytes()),
            Some(rendered.clone()),
        );
        state.save_to_dir(local).unwrap();
        (ctx, rendered)
    }

    #[test]
    fn test_push_skips_static_template_with_logic() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let template_text = "[user]\n\temail = {{ git_email }}\n";
        let (ctx, rendered) =
            record_rendered_gitconfig(local.path(), upstream.path(), template_text);
        // Assertion messages never print file text: it holds the host's git email.
        assert!(
            !ctx.git_email.is_empty(),
            "precondition: push's scan context has a git email"
        );
        assert!(
            rendered.contains(&ctx.git_email),
            "precondition: the rendered .gitconfig holds the email"
        );
        fs::write(
            local.path().join(".gitconfig"),
            format!("{rendered}\tname = Alice\n"),
        )
        .unwrap();

        let result = push_dirs(&engine, local.path(), upstream.path());

        let PushResult::Success {
            synced_files,
            skipped_templates,
            ..
        } = &result
        else {
            panic!("push did not return Success");
        };
        assert_eq!(skipped_templates, &vec![PathBuf::from(".gitconfig")]);
        assert_eq!(*synced_files, 0);
        let published = fs::read(upstream.path().join("projects/generic/gitconfig.repo")).unwrap();
        assert!(
            published == template_text.as_bytes(),
            "push must leave the template's bytes unchanged"
        );
        assert!(
            !String::from_utf8_lossy(&published).contains(&ctx.git_email),
            "push must not publish the rendered email over its placeholder"
        );
    }

    #[test]
    fn test_push_never_deletes_static_template_with_logic() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let template_text = "[user]\n\temail = {{ git_email }}\n";
        let (ctx, _) = record_rendered_gitconfig(local.path(), upstream.path(), template_text);
        let status = engine
            .scan_project(local.path(), upstream.path(), Some(&ctx))
            .unwrap()
            .into_iter()
            .find(|s| s.relative_path == Path::new(".gitconfig"))
            .unwrap()
            .status;
        assert_eq!(status, SyncStatus::LocalDeleted, "precondition");

        let result = push_dirs(&engine, local.path(), upstream.path());

        let PushResult::Success {
            deleted,
            skipped_templates,
            ..
        } = &result
        else {
            panic!("push did not return Success");
        };
        assert_eq!(skipped_templates, &vec![PathBuf::from(".gitconfig")]);
        assert!(deleted.is_empty(), "deleted: {deleted:?}");
        let template = upstream.path().join("projects/generic/gitconfig.repo");
        assert!(
            fs::read(&template).unwrap() == template_text.as_bytes(),
            "push must keep the template and its bytes"
        );
    }

    /// Writes `template` (bytes that are not UTF-8) as the `mise.toml` template and
    /// records `mise.toml` as an unedited copy of it that was then deleted locally.
    /// Returns the template path.
    fn record_deleted_non_utf8_mise(local: &Path, upstream: &Path, template: &[u8]) -> PathBuf {
        assert!(
            std::str::from_utf8(template).is_err(),
            "fixture is not UTF-8"
        );
        let path = upstream.join("projects/agents/mise.toml.jinja");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, template).unwrap();
        let mut state = ProjectState::default();
        state.record_file(
            Path::new("mise.toml"),
            SyncEngine::fingerprint(template),
            None,
        );
        state.save_to_dir(local).unwrap();
        let ctx = crate::template::TemplateContext::detect_for_project(local, "project", "generic");
        let status = SyncEngine::new()
            .scan_project(local, upstream, Some(&ctx))
            .unwrap()
            .into_iter()
            .find(|s| s.relative_path == Path::new("mise.toml"))
            .unwrap()
            .status;
        assert_eq!(status, SyncStatus::LocalDeleted, "precondition");
        path
    }

    #[test]
    fn test_push_never_deletes_non_utf8_jinja_template_with_logic() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let bytes = [&[0xFF, 0xFE][..], b"[tools]\nrust = \"{{ x }}\"\n"].concat();
        let template = record_deleted_non_utf8_mise(local.path(), upstream.path(), &bytes);

        let result = push_dirs(&engine, local.path(), upstream.path());

        let PushResult::Success {
            deleted,
            skipped_templates,
            ..
        } = &result
        else {
            panic!("push: {result:?}");
        };
        assert_eq!(skipped_templates, &vec![PathBuf::from("mise.toml")]);
        assert!(deleted.is_empty(), "deleted: {deleted:?}");
        assert_eq!(fs::read(&template).unwrap(), bytes);
    }

    #[test]
    fn test_push_deletes_non_utf8_jinja_template_without_logic() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let bytes = [&[0xFF, 0xFE][..], b"[tools]\nrust = \"latest\"\n"].concat();
        let template = record_deleted_non_utf8_mise(local.path(), upstream.path(), &bytes);

        let result = push_dirs(&engine, local.path(), upstream.path());

        let PushResult::Success {
            deleted,
            skipped_templates,
            ..
        } = &result
        else {
            panic!("push: {result:?}");
        };
        assert_eq!(deleted, &vec![PathBuf::from("mise.toml")]);
        assert!(
            skipped_templates.is_empty(),
            "skipped: {skipped_templates:?}"
        );
        assert!(!template.exists(), "the upstream template must be deleted");
    }

    #[test]
    fn test_push_publishes_over_placeholder_only_static_template() {
        let engine = SyncEngine::new();
        let local = tempdir().unwrap();
        let upstream = tempdir().unwrap();
        let template_text = "[include]\n\tpath = {{ user.home }}/.gitconfig.local\n";
        let (_, rendered) = record_rendered_gitconfig(local.path(), upstream.path(), template_text);
        fs::write(
            local.path().join(".gitconfig"),
            format!("{rendered}[ude]\n\tx = 1\n"),
        )
        .unwrap();

        let result = push_dirs(&engine, local.path(), upstream.path());

        // Gate 2's decision only: the placeholder line depends on the host home that
        // push's own scan renders with (the e2e control asserts that it survives).
        let PushResult::Success {
            synced_files,
            skipped_templates,
            ..
        } = &result
        else {
            panic!("push: {result:?}");
        };
        assert!(
            skipped_templates.is_empty(),
            "skipped: {skipped_templates:?}"
        );
        assert_eq!(*synced_files, 1);
        let published =
            fs::read_to_string(upstream.path().join("projects/generic/gitconfig.repo")).unwrap();
        assert!(published.ends_with("[ude]\n\tx = 1\n"), "{published}");
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
    fn test_push_parameterizes_punctuation_delimited_home_paths() {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let home = "/home/alice";
        let xdg = "/home/alice/.config";
        let neighbours = "/home/alice2/data /home/alice.bak /home/alice-old /backup/home/alice /backup-/home/alice/x";
        let local = format!(
            "[core]\n\ta = (/home/alice/x)\n\tb = ${{W:-/home/alice/Github/x}}\n\tc = /home/alice,\n\td = \"${{H:-/home/alice}}\"\n\te = see /home/alice.\n\tf = {neighbours}\n\tg = /home/alice/.config/app\n"
        );
        fs::write(local_dir.path().join(".gitconfig"), &local).unwrap();

        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            home,
            xdg,
            false,
            true,
        );
        match res {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 1),
            other => panic!("Expected Success on push, got {other:?}"),
        }

        let published =
            fs::read_to_string(upstream_dir.path().join("projects/generic/gitconfig.repo"))
                .unwrap();
        let expected = format!(
            "[core]\n\ta = ({{{{ user.home }}}}/x)\n\tb = ${{W:-{{{{ user.home }}}}/Github/x}}\n\tc = {{{{ user.home }}}},\n\td = \"${{H:-{{{{ user.home }}}}}}\"\n\te = see {{{{ user.home }}}}.\n\tf = {neighbours}\n\tg = {{{{ user.xdg_config }}}}/app\n"
        );
        assert_eq!(published, expected);

        // Rendering the published template on the same machine gives the local file back.
        let ctx = crate::template::TemplateContext {
            user: crate::template::UserContext {
                home: home.to_string(),
                xdg_config: xdg.to_string(),
                xdg_data: format!("{home}/.local/share"),
            },
            user_home: home.to_string(),
            xdg_config: xdg.to_string(),
            xdg_data: format!("{home}/.local/share"),
            git_user_name: "Alice".to_string(),
            git_email: "alice@example.com".to_string(),
            project_name: "test-proj".to_string(),
            project_type: "generic".to_string(),
            maintainer_ssh_pubkey: None,
            agent_ssh_pubkey: None,
            extra: std::collections::HashMap::new(),
        };
        let states = engine
            .scan_project(local_dir.path(), upstream_dir.path(), Some(&ctx))
            .unwrap();
        let gitconfig = states
            .iter()
            .find(|s| s.relative_path == Path::new(".gitconfig"))
            .unwrap();
        assert_eq!(gitconfig.status, SyncStatus::InSync);
    }

    /// Pushes `local` as `.gitconfig` from fresh local and upstream dirs with the
    /// given roots; returns the push result and the published template, if any.
    fn push_gitconfig_with_roots(
        engine: &SyncEngine,
        local: &str,
        home: &str,
        xdg: &str,
    ) -> (PushResult, Option<String>) {
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        fs::write(local_dir.path().join(".gitconfig"), local).unwrap();
        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            home,
            xdg,
            false,
            true,
        );
        let published =
            fs::read_to_string(upstream_dir.path().join("projects/generic/gitconfig.repo")).ok();
        (res, published)
    }

    const HOME_LINES_GITCONFIG: &str = "[core]\n\ta = (/home/alice)\n\tb = /home/alice/x\n\tc = /home/alice/.config/app\n\td = /home/alice2/y\n";

    #[test]
    fn test_push_parameterizes_home_with_trailing_separator() {
        let engine = SyncEngine::new();
        // `d` is a neighbouring path that must stay byte-identical.
        let expected = "[core]\n\ta = ({{ user.home }})\n\tb = {{ user.home }}/x\n\tc = {{ user.xdg_config }}/app\n\td = /home/alice2/y\n";
        let mut failures = Vec::new();
        for (home, xdg) in [
            ("/home/alice/", "/home/alice/.config/"),
            ("/home/alice//", "/home/alice/.config//"),
        ] {
            let (res, published) =
                push_gitconfig_with_roots(&engine, HOME_LINES_GITCONFIG, home, xdg);
            match res {
                PushResult::Success {
                    synced_files: 1, ..
                } => {}
                other => failures.push(format!(
                    "home {home:?}, xdg {xdg:?}: expected Success with 1 file, got {other:?}"
                )),
            }
            if published.as_deref() != Some(expected) {
                failures.push(format!(
                    "home {home:?}, xdg {xdg:?}: expected {expected:?}, published {published:?}"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A home or XDG root made only of separators rewrites nothing.
    #[test]
    fn test_push_does_not_parameterize_root_home() {
        let engine = SyncEngine::new();
        for root in ["/", "//"] {
            let (res, published) =
                push_gitconfig_with_roots(&engine, HOME_LINES_GITCONFIG, root, root);
            match res {
                PushResult::Success { synced_files, .. } => {
                    assert_eq!(synced_files, 1, "root {root:?}")
                }
                other => panic!("root {root:?}: expected Success, got {other:?}"),
            }
            assert_eq!(
                published.as_deref(),
                Some(HOME_LINES_GITCONFIG),
                "root {root:?} rewrote the text"
            );
        }
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

    /// The shipped commit-msg hook, whose `${#FIRST_LINE}` opens a Jinja comment once
    /// the text is rendered.
    fn repo_commit_msg_hook() -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("templates/projects/generic/hooks/commit-msg"),
        )
        .expect("read the repository commit-msg hook")
    }

    /// Pushes `local_bytes` as `scripts/hooks/commit-msg` with home `/home/alice`.
    fn push_commit_msg_as_alice(local_bytes: &[u8]) -> (PushResult, tempfile::TempDir) {
        push_as_alice("scripts/hooks/commit-msg", local_bytes)
    }

    /// Pushes `local_bytes` as the managed file `rel` (no upstream template yet) with
    /// home `/home/alice`; returns the result and the upstream root.
    fn push_as_alice(rel: &str, local_bytes: &[u8]) -> (PushResult, tempfile::TempDir) {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        let file = local_dir.path().join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, local_bytes).unwrap();
        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/alice",
            "/home/alice/.config",
            false,
            true,
        );
        (res, upstream_dir)
    }

    /// A render context for home `/home/alice`, as push builds it for that home.
    fn alice_render_ctx() -> crate::template::TemplateContext {
        crate::template::TemplateContext {
            user: crate::template::UserContext {
                home: "/home/alice".to_string(),
                xdg_config: "/home/alice/.config".to_string(),
                xdg_data: "/home/alice/.local/share".to_string(),
            },
            user_home: "/home/alice".to_string(),
            xdg_config: "/home/alice/.config".to_string(),
            xdg_data: "/home/alice/.local/share".to_string(),
            git_user_name: "Alice".to_string(),
            git_email: "alice@example.com".to_string(),
            project_name: "test-proj".to_string(),
            project_type: "generic".to_string(),
            maintainer_ssh_pubkey: None,
            agent_ssh_pubkey: None,
            extra: std::collections::HashMap::new(),
        }
    }

    /// Runs push gate 5's check with the `/home/alice` context.
    fn render_back(target: &str, published: &str, local_lf: &str) -> Result<(), String> {
        SyncEngine::check_render_back(
            &crate::template::TemplateRenderer::new(),
            &alice_render_ctx(),
            Path::new(target),
            published,
            local_lf,
        )
    }

    #[test]
    fn test_push_refuses_irreversible_shapes() {
        // (local file, its content, upstream template, what the case shows)
        let cases: [(&str, &str, &str, &str); 4] = [
            (
                ".gitconfig",
                "[core]\n\tx = x{{/home/alice/y\n",
                "projects/generic/gitconfig.repo",
                "`{{` before a home path becomes `{{{{`",
            ),
            (
                ".gitconfig",
                "[core]\n\tt = ${{ secrets.TOKEN }}\n",
                "projects/generic/gitconfig.repo",
                "literal `${{ }}` with no home path (undefined value)",
            ),
            (
                "mise.toml",
                "a = \"{# not a comment\"\n",
                "projects/agents/mise.toml.jinja",
                "`{#` in text published to a .jinja template",
            ),
            (
                ".gitconfig",
                "[user]\n\temail = {{ git_email }}\n",
                "projects/generic/gitconfig.repo",
                "literal `{{ git_email }}` renders to different text",
            ),
        ];
        let mut failures = Vec::new();
        for (rel, local, upstream_rel, shape) in cases {
            let (res, upstream) = push_as_alice(rel, local.as_bytes());
            match &res {
                PushResult::ReversibilityBlocked { path, reason } if path == Path::new(rel) => {
                    if upstream.path().join(upstream_rel).exists() {
                        failures.push(format!("{shape}: refused but {upstream_rel} was written"));
                    }
                    if shape.contains("git_email")
                        && (!reason.contains("line 2") || reason.contains("email ="))
                    {
                        failures.push(format!("{shape}: reason {reason:?}"));
                    }
                }
                other => failures.push(format!(
                    "{shape}: expected ReversibilityBlocked, got {other:?}"
                )),
            }
        }
        assert!(
            failures.is_empty(),
            "{} of 4 irreversible shapes were not refused as planned:\n{}",
            failures.len(),
            failures.join("\n")
        );

        // Control: plain text published to a .jinja template renders back to itself.
        let (res, upstream) = push_as_alice("mise.toml", b"a = 1\n");
        assert!(
            matches!(
                res,
                PushResult::Success {
                    synced_files: 1,
                    ..
                }
            ),
            "Expected Success, got {res:?}"
        );
        assert_eq!(
            fs::read(upstream.path().join("projects/agents/mise.toml.jinja")).unwrap(),
            b"a = 1\n"
        );
    }

    #[test]
    fn test_check_render_back_reason_has_no_template_text() {
        let sentinel = "ude_sentinel_label";
        let filter = format!("[core]\n\tx = {{{{ 1 | {sentinel} }}}}\n");
        let reason = render_back("gitconfig.repo", &filter, "[core]\n\tx = 1\n").unwrap_err();
        assert!(reason.contains("unknown filter"), "{reason}");
        assert!(reason.contains("line 2"), "{reason}");
        assert!(!reason.contains(sentinel), "{reason}");

        let function = format!("[core]\n\tx = {{{{ {sentinel}() }}}}\n");
        let reason = render_back("gitconfig.repo", &function, "[core]\n\tx = 1\n").unwrap_err();
        assert!(reason.contains("unknown function"), "{reason}");
        assert!(!reason.contains(sentinel), "{reason}");
    }

    #[test]
    fn test_check_render_back_exact_contract() {
        // LF text with a placeholder renders back to the local text.
        assert_eq!(
            render_back("t.jinja", "p = {{ user.home }}/x\n", "p = /home/alice/x\n"),
            Ok(())
        );
        // A CRLF published copy of a CRLF local file: both compared after LF normalization.
        let local_crlf = "p = /home/alice/x\r\nq = 1\r\n";
        assert_eq!(
            render_back(
                "t.jinja",
                "p = {{ user.home }}/x\r\nq = 1\r\n",
                &SyncEngine::normalize_eol(local_crlf)
            ),
            Ok(())
        );
        // A trailing-newline difference is a mismatch on the extra line.
        let reason = render_back("t.jinja", "a\n", "a").unwrap_err();
        assert!(reason.contains("renders back differently"), "{reason}");
        assert!(reason.contains("line 2"), "{reason}");
        // No `{{` and a non-.jinja target: not rendered, so `{#` passes.
        assert_eq!(
            render_back("gitconfig.repo", "x {# y\n", "x {# y\n"),
            Ok(())
        );
        // The same text with a .jinja target is rendered and fails.
        let reason = render_back("t.jinja", "x {# y\n", "x {# y\n").unwrap_err();
        assert!(reason.contains("syntax error"), "{reason}");
    }

    #[test]
    fn test_push_secret_scan_precedes_reversibility() {
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let local = format!("[core]\n\tt = {prefix}{body}\n\tc = {{# /home/alice/x\n");
        let (res, upstream) = push_as_alice(".gitconfig", local.as_bytes());
        match &res {
            PushResult::SecretBlocked { path, .. } => assert_eq!(path, Path::new(".gitconfig")),
            other => panic!("Expected SecretBlocked, got {other:?}"),
        }
        assert!(
            !upstream
                .path()
                .join("projects/generic/gitconfig.repo")
                .exists()
        );

        // Control: without the token line the same file is a reversibility refusal.
        let local = "[core]\n\tc = {# /home/alice/x\n";
        let (res, _upstream) = push_as_alice(".gitconfig", local.as_bytes());
        assert!(
            matches!(res, PushResult::ReversibilityBlocked { .. }),
            "Expected ReversibilityBlocked, got {res:?}"
        );
    }

    #[test]
    fn test_push_refuses_unrenderable_parameterized_file() {
        let hook = repo_commit_msg_hook();
        let hook_text = String::from_utf8(hook.clone()).unwrap();
        assert!(hook_text.contains("${#FIRST_LINE}"), "fixture shape");
        assert!(
            !hook_text.contains("{{"),
            "fixture must hold no placeholder"
        );

        let mut local = hook.clone();
        local.extend_from_slice(b"# notes: /home/alice/notes\n");
        let (res, upstream) = push_commit_msg_as_alice(&local);
        match &res {
            PushResult::ReversibilityBlocked { path, reason } => {
                assert_eq!(path, &PathBuf::from("scripts/hooks/commit-msg"));
                assert!(reason.contains("does not render back"), "{reason}");
                assert!(reason.contains("syntax error"), "{reason}");
                assert!(reason.contains("line"), "{reason}");
                assert!(!reason.contains("/home/alice/notes"), "{reason}");
                assert!(!reason.contains("end of comment"), "{reason}");
            }
            other => panic!("Expected ReversibilityBlocked, got {other:?}"),
        }
        assert!(
            !upstream
                .path()
                .join("projects/generic/hooks/commit-msg")
                .exists()
        );

        // Control: the same hook without a home path is not rendered and is published
        // byte for byte.
        let (res, upstream) = push_commit_msg_as_alice(&hook);
        assert!(
            matches!(
                res,
                PushResult::Success {
                    synced_files: 1,
                    ..
                }
            ),
            "Expected Success, got {res:?}"
        );
        assert_eq!(
            fs::read(upstream.path().join("projects/generic/hooks/commit-msg")).unwrap(),
            hook
        );
    }

    /// Pushes a local `.gitconfig` whose second line is a comment holding `word`.
    fn push_gitconfig_comment(word: &str) -> PushResult {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        fs::write(
            local_dir.path().join(".gitconfig"),
            format!("[core]\n# build fingerprint {word}\n"),
        )
        .unwrap();
        engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
            false,
            true,
        )
    }

    #[test]
    fn test_push_blocks_prefixless_high_entropy_token() {
        // No known prefix, no assignment, no credential keyword: only R11 can see it.
        let tok = crate::sanitizer::entropy::class_cycling_token();
        match push_gitconfig_comment(&tok) {
            PushResult::SecretBlocked { path, reason } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
                assert!(reason.contains("high-entropy"), "{reason}");
                assert!(reason.contains("at line 2"), "{reason}");
                assert!(!reason.contains(&tok), "refusal leaked the token");
            }
            other => panic!("Expected SecretBlocked, got {other:?}"),
        }

        // Negative control: an ordinary long word in the same place pushes.
        match push_gitconfig_comment("ThisIsAPerfectlyNormalSentenceWithWords") {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 1),
            other => panic!("Expected Success, got {other:?}"),
        }
    }

    /// A Slack bot token shape (`xoxb-` + 12 digits + `-` + 13 digits + `-` + 24
    /// lowercase letters), drawn at runtime so no token literal sits in this file.
    fn seeded_slack_bot_token() -> String {
        let mut chars = crate::sanitizer::entropy::SeededChars::new();
        let digits = "0123456789";
        let first = chars.draw(digits, 12);
        let second = chars.draw(digits, 13);
        let tail = chars.draw("abcdefghijklmnopqrstuvwxyz", 24);
        format!("xoxb-{first}-{second}-{tail}")
    }

    /// Pushes a local `.gitconfig` whose content is `content`.
    fn push_gitconfig_content(content: &str) -> PushResult {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        fs::write(local_dir.path().join(".gitconfig"), content).unwrap();
        engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
            false,
            true,
        )
    }

    #[test]
    fn test_push_blocks_gitleaks_only_category() {
        let tok = seeded_slack_bot_token();
        let content = format!("[core]\n# slack {tok}\n");
        // Neither the named rules nor R11 see this token: only a gitleaks rule can.
        assert_eq!(
            crate::sanitizer::entropy::first_high_entropy_line(content.as_bytes()),
            None
        );
        match push_gitconfig_content(&content) {
            PushResult::SecretBlocked { path, reason } => {
                assert_eq!(path, PathBuf::from(".gitconfig"));
                assert!(reason.contains("slack-bot-token"), "{reason}");
                assert!(reason.contains("at line 2"), "{reason}");
                assert!(!reason.contains(&tok), "refusal leaked the token");
            }
            other => panic!("Expected SecretBlocked, got {other:?}"),
        }

        // Negative control: the same file without the token line pushes.
        match push_gitconfig_content("[core]\n") {
            PushResult::Success { synced_files, .. } => assert_eq!(synced_files, 1),
            other => panic!("Expected Success, got {other:?}"),
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

    /// Pushes a local `.gitconfig` holding `local_bytes` as home `/home/alice` and
    /// returns the bytes published to the upstream template.
    fn push_gitconfig_as_alice(local_bytes: &[u8]) -> Vec<u8> {
        let engine = SyncEngine::new();
        let local_dir = tempdir().unwrap();
        let upstream_dir = tempdir().unwrap();
        fs::write(local_dir.path().join(".gitconfig"), local_bytes).unwrap();
        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
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
            "Expected Success, got {res:?}"
        );
        fs::read(upstream_dir.path().join("projects/generic/gitconfig.repo")).unwrap()
    }

    #[test]
    fn test_push_never_parameterizes_non_utf8_file() {
        // Latin-1 e-acute (0xE9) makes the file non-UTF-8; the home path is published as is.
        let latin1 = b"[core]\n\tp = /home/alice/x caf\xe9\n".to_vec();
        assert!(
            std::str::from_utf8(&latin1).is_err(),
            "fixture must be non-UTF-8"
        );
        assert_eq!(push_gitconfig_as_alice(&latin1), latin1);

        // Control: the same text in UTF-8 is parameterized.
        assert_eq!(
            push_gitconfig_as_alice("[core]\n\tp = /home/alice/x caf\u{e9}\n".as_bytes()),
            "[core]\n\tp = {{ user.home }}/x caf\u{e9}\n".as_bytes()
        );
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
        let validated = ValidatedBlobs::from_files(&staged_allowlist);
        let _ = engine.push_to_remote(
            &PublishTarget {
                checkout: remote_repo.path(),
                remote_url: NO_REMOTE_URL,
                branch: "main",
                fetched_sha: &head,
            },
            false,
            &staged_allowlist,
            &validated,
        );

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
        let validated = ValidatedBlobs::from_files(&staged_allowlist);
        let res = engine.push_to_remote(
            &PublishTarget {
                checkout: remote_repo.path(),
                remote_url: NO_REMOTE_URL,
                branch: "main",
                fetched_sha: &head,
            },
            false,
            &staged_allowlist,
            &validated,
        );

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

    /// Runs push_to_remote on a repo whose one template passes through a clean filter
    /// running `filter_body` (a POSIX sh script body), and returns the push result
    /// plus `git diff --cached --name-only` afterwards.
    fn push_through_clean_filter(filter_body: &str) -> (Result<String, PushRemoteError>, String) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();
        git.run(remote_repo.path(), &["init", "-b", "main"]);
        let templates_dir = remote_repo.path().join("templates/projects/generic");
        fs::create_dir_all(&templates_dir).unwrap();
        let valid_file = templates_dir.join("gitconfig.repo");
        fs::write(&valid_file, "[core]\n  autocrlf = true\n").unwrap();
        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);
        fs::write(&valid_file, "[core]\n  autocrlf = false\n").unwrap();

        let script = remote_repo.path().join("inject_secret.sh");
        fs::write(&script, format!("#!/bin/sh\n{filter_body}")).unwrap();
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

        let head = git.output(remote_repo.path(), &["rev-parse", "HEAD"]);
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&valid_file));
        let res = engine.push_to_remote(
            &PublishTarget {
                checkout: remote_repo.path(),
                remote_url: NO_REMOTE_URL,
                branch: "main",
                fetched_sha: &head,
            },
            false,
            &[valid_file],
            &validated,
        );
        let diff_cached = git.output(remote_repo.path(), &["diff", "--cached", "--name-only"]);
        (res, diff_cached)
    }

    #[test]
    fn test_publication_gate_scans_raw_blob_bytes() {
        let prefix = "ghp_";
        let body = "999988887777666655554444333322221111";
        // Invalid UTF-8, then a token directly after a non-ASCII letter.
        let filter = format!("cat\nprintf '\\377\\376'\necho \"x=é{prefix}{body}\"\n");
        let (res, diff_cached) = push_through_clean_filter(&filter);

        let err_msg = push_failure(res);
        assert!(
            err_msg.contains("Publication gate violation: secret detected in staged index blob"),
            "Error was: {err_msg}"
        );
        assert!(
            err_msg.contains("GitHub Personal Access Token"),
            "{err_msg}"
        );
        assert!(err_msg.contains("at line"), "{err_msg}");
        assert!(!err_msg.contains(body), "gate error leaked the token body");
        assert!(diff_cached.is_empty(), "index must be reset: {diff_cached}");
    }

    #[test]
    fn test_publication_gate_scans_utf16_blob() {
        let prefix = "ghp_";
        let body = "999988887777666655554444333322221111";
        // The UTF-16 payload lives outside the repository so `git status` is unaffected.
        let side_dir = tempdir().unwrap();
        let side_file = side_dir.path().join("utf16-payload");
        let mut payload = vec![0xFF, 0xFE];
        for unit in format!("secret = {prefix}{body}\n").encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&side_file, &payload).unwrap();

        let filter = format!("cat\ncat '{}'\n", side_file.display());
        let (res, diff_cached) = push_through_clean_filter(&filter);

        let err_msg = push_failure(res);
        assert!(
            err_msg.contains("Publication gate violation: secret detected in staged index blob"),
            "Error was: {err_msg}"
        );
        assert!(
            err_msg.contains("GitHub Personal Access Token"),
            "{err_msg}"
        );
        assert!(!err_msg.contains(body), "gate error leaked the token body");
        assert!(diff_cached.is_empty(), "index must be reset: {diff_cached}");
    }

    #[test]
    fn test_publication_gate_blocks_gitleaks_only_category() {
        let tok = seeded_slack_bot_token();
        let filter = format!("cat\necho \"# slack {tok}\"\n");
        let (res, diff_cached) = push_through_clean_filter(&filter);

        let err_msg = push_failure(res);
        assert!(
            err_msg.contains("Publication gate violation: secret detected in staged index blob"),
            "Error was: {err_msg}"
        );
        assert!(err_msg.contains("slack-bot-token"), "{err_msg}");
        assert!(!err_msg.contains(&tok), "gate error leaked the token");
        assert!(diff_cached.is_empty(), "index must be reset: {diff_cached}");
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

        // Step 2: Push changes in PR mode (open_pr: true)
        let managed_gitconfig = tmpl_dir.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig,
            "[core]\n  autocrlf = false\n# pr edit\n",
        )
        .unwrap();
        let staged = vec![managed_gitconfig.clone()];
        let validated = ValidatedBlobs::from_files(&staged);
        let pr_res =
            engine.push_to_remote(&cached.publish_target("main"), true, &staged, &validated);
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
        let managed_gitconfig_2 = tmpl_dir_2.join("projects/generic/gitconfig.repo");
        fs::write(
            &managed_gitconfig_2,
            "[core]\n  autocrlf = input\n# direct edit\n",
        )
        .unwrap();
        let staged_2 = vec![managed_gitconfig_2.clone()];
        let validated_2 = ValidatedBlobs::from_files(&staged_2);
        let direct_res = engine.push_to_remote(
            &cached_2.publish_target("main"),
            false,
            &staged_2,
            &validated_2,
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

    /// The remote URL of a [`PublishTarget`] over a plain repository with no remote: those
    /// tests stop at the staged gate or at `git status`, before any push or read.
    const NO_REMOTE_URL: &str = "/nonexistent/ude-test-remote.git";

    /// A [`PublishTarget`] over the plain repository `checkout` with no remote, for tests
    /// that call `stage_publication` directly. `fetched_sha` matters only on the
    /// empty-status route, which requires HEAD to be it.
    fn checkout_target<'a>(checkout: &'a Path, fetched_sha: &'a str) -> PublishTarget<'a> {
        PublishTarget {
            checkout,
            remote_url: NO_REMOTE_URL,
            branch: "main",
            fetched_sha,
        }
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

    #[test]
    fn test_push_updates_records_validated_digests() {
        let local = "[core]\n\tx = /home/alice/y\n";
        let (res, _upstream) = push_as_alice(".gitconfig", local.as_bytes());
        let PushResult::Success {
            staged_files,
            validated_blobs,
            ..
        } = &res
        else {
            panic!("Expected Success, got {res:?}");
        };
        assert_eq!(staged_files.len(), 1, "{staged_files:?}");
        assert_eq!(validated_blobs.digests.len(), 1, "{validated_blobs:?}");
        let published = fs::read(&staged_files[0]).unwrap();
        assert!(
            String::from_utf8_lossy(&published).contains("{{ user.home }}"),
            "{}",
            String::from_utf8_lossy(&published)
        );
        assert_eq!(
            validated_blobs.digests.get(&staged_files[0]),
            Some(&SyncEngine::fingerprint(&published))
        );
        // The record is the published text, not the local text.
        assert_ne!(
            validated_blobs.digests.get(&staged_files[0]),
            Some(&SyncEngine::fingerprint(local.as_bytes()))
        );
    }

    /// A repository whose `templates/projects/generic/gitconfig.repo` is committed as
    /// `[core]\n  autocrlf = true\n`; returns the repository and that file's path.
    fn gate_repo(git: &GitFixture) -> (tempfile::TempDir, PathBuf) {
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let file = repo
            .path()
            .join("templates/projects/generic/gitconfig.repo");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "[core]\n  autocrlf = true\n").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "base commit"]);
        (repo, file)
    }

    /// Runs the template in `repo` through a clean filter whose script is
    /// `filter_body` (POSIX sh).
    fn set_clean_filter(git: &GitFixture, repo: &Path, filter_body: &str) {
        let script = repo.join("rewrite_blob.sh");
        fs::write(&script, format!("#!/bin/sh\n{filter_body}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let filter_cmd = script.display().to_string();
        git.run(repo, &["config", "filter.rewriteblob.clean", &filter_cmd]);
        let info_dir = repo.join(".git/info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(
            info_dir.join("attributes"),
            "templates/projects/generic/gitconfig.repo filter=rewriteblob\n",
        )
        .unwrap();
    }

    const GATE_REL: &str = "templates/projects/generic/gitconfig.repo";

    #[test]
    fn test_publication_gate_refuses_blob_changed_by_clean_filter() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, file) = gate_repo(&git);
        fs::write(&file, "[core]\n\tx = 1\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));

        // The filter appends an irreversible template the reversibility check never saw.
        set_clean_filter(
            &git,
            repo.path(),
            "cat\necho '{# unterminated'\necho '{{ user.home }}'\n",
        );
        let err = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect_err("a blob changed by a clean filter must be refused");
        assert!(
            err.contains("Publication gate violation: staged index blob"),
            "{err}"
        );
        assert!(err.contains("differs from the validated content"), "{err}");
        assert_eq!(
            git.output(repo.path(), &["diff", "--cached", "--name-only"]),
            ""
        );

        // Control: a pass-through filter stores the validated bytes and is published.
        set_clean_filter(&git, repo.path(), "cat\n");
        let staged = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect("a pass-through filter must publish");
        assert!(
            staged
                .as_ref()
                .is_some_and(|staged| staged.paths.contains(Path::new(GATE_REL))),
            "{staged:?}"
        );
    }

    #[test]
    fn test_publication_gate_refuses_rewritten_working_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, file) = gate_repo(&git);
        let original = "[core]\n\tx = 1\n";
        fs::write(&file, original).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));

        // A filter writing `%f` or a hook rewrites the working tree too, so index and
        // working tree agree after `git add`.
        let rewritten = "[core]\n{# unterminated\n{{ user.home }}\n";
        fs::write(&file, rewritten).unwrap();
        let err = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect_err("a rewritten file must be refused");
        assert!(err.contains("differs from the validated content"), "{err}");
        assert_eq!(
            git.output(repo.path(), &["diff", "--cached", "--name-only"]),
            ""
        );

        // Opposite direction: git stores the rewritten bytes exactly, so a gate that
        // re-read the working tree would have passed this blob.
        git.run(repo.path(), &["add", "--", GATE_REL]);
        let stored = git
            .checked(repo.path(), &["show", &format!(":{GATE_REL}")])
            .stdout;
        assert_eq!(stored, rewritten.as_bytes());
        git.run(repo.path(), &["reset", "-q", "HEAD", "--"]);

        // Control: the validated text restored is published.
        fs::write(&file, original).unwrap();
        let staged = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect("the validated text must publish");
        assert!(staged.is_some(), "{staged:?}");

        // Control: a staged path with no record is refused.
        let err = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &ValidatedBlobs::default(),
            )
            .expect_err("a staged path with no record must be refused");
        assert!(err.contains("no validated content recorded for"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_post_commit_hook_rewrite_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let remote_main = || git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let checkout = cached.checkout_dir.as_path();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        let managed_rel = managed
            .strip_prefix(checkout)
            .unwrap()
            .display()
            .to_string();
        fs::write(&managed, "[core]\n\tx = 1\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));

        // post-commit runs despite --no-verify. The hook amends the commit once; the
        // marker stops the amend's own post-commit from amending again.
        let marker = checkout.join(".git/ude-test-amended");
        let hook = checkout.join(".git/hooks/post-commit");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\n[ -e .git/ude-test-amended ] && exit 0\n: > .git/ude-test-amended\nprintf '%s\\n' '{{# unterminated' >> '{managed_rel}'\ngit add -- '{managed_rel}'\ngit commit --amend --no-edit --no-verify -q\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            std::slice::from_ref(&managed),
            &validated,
        );
        let err = push_failure(res);
        assert!(
            err.contains("Post-commit publication gate violation: committed HEAD blob"),
            "{err}"
        );
        assert!(err.contains("differs from the validated content"), "{err}");
        assert_eq!(remote_main(), cached.fetched_sha);
        // Opposite direction: the hook did run, so the refusal is its rewrite caught.
        assert!(marker.exists(), "the post-commit hook never ran");

        // Control: without the hook the same content is published.
        fs::remove_file(&hook).unwrap();
        fs::remove_file(&marker).unwrap();
        fs::write(&managed, "[core]\n\tx = 1\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            std::slice::from_ref(&managed),
            &validated,
        );
        assert!(res.is_ok(), "{res:?}");
        assert_ne!(remote_main(), cached.fetched_sha);
    }

    /// The template bytes every hooked push validates.
    #[cfg(unix)]
    const HOOKED_TEMPLATE: &str = "[core]\n\tx = 1\n";

    /// Shell lines shared by the hook tails below, as literals so `concat!` can use them.
    #[cfg(unix)]
    macro_rules! amend_inject_lines {
        () => {
            r#"printf '%s\n' INJECTED_BY_HOOK >> "$F"
git add -- "$F"
git commit --amend --no-edit --no-verify -q
"#
        };
    }

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! add_file_lines {
        () => {
            r#"printf '%s\n' INJECTED_BY_HOOK > templates/hook-extra.txt
git add -- templates/hook-extra.txt
git commit --amend --no-edit --no-verify -q
"#
        };
    }

    /// Hook tail: appends a line to the managed file and amends UDE's commit.
    #[cfg(unix)]
    const HOOK_AMEND_INJECT: &str = amend_inject_lines!();

    /// Hook tail: makes HEAD a merge whose first parent is HEAD's parent and whose second
    /// parent is HEAD, with HEAD's tree.
    #[cfg(unix)]
    const HOOK_MERGE_WRAP: &str = r#"M=$(git commit-tree "$(git rev-parse 'HEAD^{tree}')" -p "$(git rev-parse HEAD~1)" -p "$(git rev-parse HEAD)" -m merge)
git update-ref HEAD "$M"
"#;

    /// Hook tail: stacks an empty commit on HEAD.
    #[cfg(unix)]
    const HOOK_EXTRA_COMMIT: &str = "git commit --allow-empty --no-verify -q -m extra\n";

    /// Hook tail: replaces HEAD with a parentless commit of the same tree.
    #[cfg(unix)]
    const HOOK_PARENTLESS: &str = r#"R=$(git commit-tree "$(git rev-parse 'HEAD^{tree}')" -m root)
git update-ref HEAD "$R"
"#;

    /// Hook tail: amends a file nobody validated into UDE's commit.
    #[cfg(unix)]
    const HOOK_ADD_FILE: &str = add_file_lines!();

    /// Hook tail: amends injected bytes in, then replaces the injected blob with UDE's
    /// validated blob, so a reader honouring replace refs sees the validated bytes.
    #[cfg(unix)]
    const HOOK_REPLACE_BLOB: &str = concat!(
        "V=$(git rev-parse \"HEAD:$F\")\n",
        amend_inject_lines!(),
        "I=$(git rev-parse \"HEAD:$F\")\n",
        "git replace -f \"$I\" \"$V\"\n",
        "printf '%s\\n' \"$I\" > .git/ude-test-injected-blob\n",
    );

    /// Hook tail: makes HEAD a merge holding the amended commit as its second parent, then
    /// replaces that merge with UDE's validated single-parent commit.
    #[cfg(unix)]
    const HOOK_REPLACE_MERGE: &str = concat!(
        "O=$(git rev-parse HEAD)\n",
        amend_inject_lines!(),
        "M=$(git commit-tree \"$(git rev-parse \"$O^{tree}\")\" -p \"$(git rev-parse HEAD~1)\" -p \"$(git rev-parse HEAD)\" -m merge)\n",
        "git update-ref HEAD \"$M\"\n",
        "git replace -f \"$M\" \"$O\"\n",
        "printf '%s\\n' \"$M\" > .git/ude-test-merge\n",
    );

    /// Hook tail: amends an unvalidated file in, then replaces the amended root tree with
    /// UDE's validated root tree, so a listing honouring replace refs omits that file.
    #[cfg(unix)]
    const HOOK_REPLACE_TREE: &str = concat!(
        "V=$(git rev-parse 'HEAD^{tree}')\n",
        add_file_lines!(),
        "I=$(git rev-parse 'HEAD^{tree}')\n",
        "git replace -f \"$I\" \"$V\"\n",
        "git rev-parse HEAD > .git/ude-test-amended\n",
    );

    /// Hook tail: amends the branch with injected bytes, then detaches HEAD back on UDE's
    /// validated commit, leaving the branch on the amended one.
    #[cfg(unix)]
    const HOOK_DETACH_TO_VALIDATED: &str = concat!(
        "O=$(git rev-parse HEAD)\n",
        amend_inject_lines!(),
        "git update-ref --no-deref HEAD \"$O\"\n",
    );

    /// Hook tail: turns on tag following in the cache repository config and creates an
    /// annotated tag on UDE's commit.
    #[cfg(unix)]
    const HOOK_TAG_FOLLOW: &str = r#"git config push.followTags true
git tag -a ude-hook-tag -m INJECTED_TAG_MESSAGE HEAD
git rev-parse ude-hook-tag > .git/ude-test-tag
"#;

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! amend_message_lines {
        () => {
            r#"git commit --amend --no-verify -q -m "$(git log -1 --format=%B)" -m INJECTED_BY_HOOK
"#
        };
    }

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! empty_dir_lines {
        () => {
            r#"P=$(git rev-parse HEAD~1)
git cat-file commit HEAD | sed '1,/^$/d' > .git/ude-test-msg
E=$(git mktree </dev/null)
R=$( { git ls-tree HEAD; printf '040000 tree %s\tINJECTED_DIR_NAME\n' "$E"; } | git mktree )
N=$(git commit-tree "$R" -p "$P" -F .git/ude-test-msg)
git update-ref HEAD "$N"
"#
        };
    }

    /// prepare-commit-msg tail: appends a line to the message file git passes as `$1`.
    #[cfg(unix)]
    const HOOK_PCM_APPEND: &str = r#"printf 'INJECTED_BY_HOOK\n' >> "$1"
"#;

    /// Hook tail: amends a second paragraph into UDE's commit message.
    #[cfg(unix)]
    const HOOK_AMEND_MESSAGE: &str = amend_message_lines!();

    /// Hook tail: amends the message, then replaces the amended commit with UDE's commit,
    /// so a reader honouring replace refs sees the sync message.
    #[cfg(unix)]
    const HOOK_REPLACE_MESSAGE: &str = concat!(
        "O=$(git rev-parse HEAD)\n",
        amend_message_lines!(),
        "A=$(git rev-parse HEAD)\n",
        "git replace -f \"$A\" \"$O\"\n",
        "printf '%s\\n' \"$A\" > .git/ude-test-amended-message\n",
    );

    /// Hook tail: rebuilds UDE's commit (same parent, same message bytes, same managed
    /// blob) with an empty directory added to its root tree.
    #[cfg(unix)]
    const HOOK_EMPTY_DIR: &str = empty_dir_lines!();

    /// Hook tail: [`HOOK_EMPTY_DIR`], then replaces the new root tree with UDE's validated
    /// root tree, so a listing honouring replace refs omits the directory.
    #[cfg(unix)]
    const HOOK_EMPTY_DIR_REPLACED: &str = concat!(
        "V=$(git rev-parse 'HEAD^{tree}')\n",
        empty_dir_lines!(),
        "git replace -f \"$R\" \"$V\"\n",
        "printf '%s\\n' \"$N\" > .git/ude-test-emptydir-commit\n",
    );

    /// Opens a rebuild of UDE's commit: the parent and message bytes are kept, `V` is the
    /// validated blob and `I` an injected one, whose id is left for the test.
    #[cfg(unix)]
    macro_rules! rebuild_start_lines {
        () => {
            r#"P=$(git rev-parse HEAD~1)
git cat-file commit HEAD | sed '1,/^$/d' > .git/ude-test-msg
V=$(git rev-parse "HEAD:$F")
I=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
printf '%s\n' "$I" > .git/ude-test-injected-blob
"#
        };
    }

    /// Closes a rebuild: wraps `T1` (the tree of `projects/`) up to a root tree and moves
    /// HEAD onto a commit of it, whose id is left for the test.
    #[cfg(unix)]
    macro_rules! rebuild_end_lines {
        () => {
            r#"T2=$(printf '040000 tree %s\tprojects\n' "$T1" | git mktree)
R=$(printf '040000 tree %s\ttemplates\n' "$T2" | git mktree)
N=$(git commit-tree "$R" -p "$P" -F .git/ude-test-msg)
printf '%s\n' "$N" > .git/ude-test-rebuilt-commit
git update-ref HEAD "$N"
"#
        };
    }

    /// Makes `T1` the tree holding `G` (the tree of `generic/`) as its only entry.
    #[cfg(unix)]
    macro_rules! one_generic_line {
        () => {
            r#"T1=$(printf '040000 tree %s\tgeneric\n' "$G" | git mktree)
"#
        };
    }

    /// Hook tail: rebuilds UDE's commit with the managed file listed twice, the validated
    /// blob first and an injected blob second.
    #[cfg(unix)]
    const HOOK_DUPLICATE_ENTRY: &str = concat!(
        rebuild_start_lines!(),
        r#"G=$(printf '100644 blob %s\tgitconfig.repo\n100644 blob %s\tgitconfig.repo\n' "$V" "$I" | git mktree)
"#,
        one_generic_line!(),
        rebuild_end_lines!(),
    );

    /// Hook tail: rebuilds UDE's commit with the managed file listed twice, both entries
    /// the validated blob.
    #[cfg(unix)]
    const HOOK_REPEATED_SAME_BLOB: &str = concat!(
        rebuild_start_lines!(),
        r#"G=$(printf '100644 blob %s\tgitconfig.repo\n100644 blob %s\tgitconfig.repo\n' "$V" "$V" | git mktree)
"#,
        one_generic_line!(),
        rebuild_end_lines!(),
    );

    /// Hook tail: rebuilds UDE's commit with the managed file's directory listed twice,
    /// the second one holding an injected blob under the managed name.
    #[cfg(unix)]
    const HOOK_DUPLICATE_DIRECTORY: &str = concat!(
        rebuild_start_lines!(),
        r#"G1=$(printf '100644 blob %s\tgitconfig.repo\n' "$V" | git mktree)
G2=$(printf '100644 blob %s\tgitconfig.repo\n' "$I" | git mktree)
T1=$(printf '040000 tree %s\tgeneric\n040000 tree %s\tgeneric\n' "$G1" "$G2" | git mktree)
"#,
        rebuild_end_lines!(),
    );

    /// Hook tail: rebuilds UDE's commit with a subtree named `.` next to the managed file,
    /// holding the validated blob under the managed name.
    #[cfg(unix)]
    const HOOK_DOT_SUBTREE: &str = concat!(
        rebuild_start_lines!(),
        r#"D=$(printf '100644 blob %s\tgitconfig.repo\n' "$V" | git mktree)
G=$(printf '100644 blob %s\tgitconfig.repo\n040000 tree %s\t.\n' "$V" "$D" | git mktree)
"#,
        one_generic_line!(),
        rebuild_end_lines!(),
    );

    /// Hook tail: amends the managed file out of UDE's commit.
    #[cfg(unix)]
    const HOOK_DELETE_VALIDATED: &str = r#"git rm -q --cached "$F"
git commit --amend --no-edit --no-verify -q --allow-empty
git rev-parse HEAD > .git/ude-test-rebuilt-commit
"#;

    /// Hook tail: amends the managed file back to the fetched head's content.
    #[cfg(unix)]
    const HOOK_REVERT_VALIDATED: &str = r#"git checkout -q HEAD~1 -- "$F"
git commit --amend --no-edit --no-verify -q --allow-empty
git rev-parse HEAD > .git/ude-test-rebuilt-commit
"#;

    /// Hook tail: amends UDE's commit, keeping its tree and message.
    #[cfg(unix)]
    const HOOK_KEPT_AMEND: &str = "git commit --amend --no-edit --no-verify -q\n";

    /// Hook tail: amends the exec bit of the managed file into UDE's commit, leaving its
    /// bytes as validated.
    #[cfg(unix)]
    const HOOK_EXEC_FLIP: &str = r#"git update-index --chmod=+x "$F"
git commit --amend --no-edit --no-verify -q
git rev-parse HEAD > .git/ude-test-rebuilt-commit
"#;

    /// post-checkout tail: removes the managed file from the index.
    #[cfg(unix)]
    const HOOK_CHECKOUT_REMOVE: &str = "git rm -q --cached \"$F\"\n";

    /// Opens a post-index-change rewrite of the index: `V` is the staged validated blob and
    /// `I` an injected one, whose id is left for the test.
    #[cfg(unix)]
    macro_rules! index_start_lines {
        () => {
            r#"V=$(git rev-parse ":$F")
I=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
printf '%s\n' "$I" > .git/ude-test-injected-blob
"#
        };
    }

    /// Closes a post-index-change rewrite: wraps `G` (the tree of `generic/`) up to a root
    /// tree and reads it into the index, which primes the cache tree `git write-tree` uses.
    #[cfg(unix)]
    macro_rules! read_tree_lines {
        () => {
            concat!(
                one_generic_line!(),
                r#"T2=$(printf '040000 tree %s\tprojects\n' "$T1" | git mktree)
R=$(printf '040000 tree %s\ttemplates\n' "$T2" | git mktree)
git read-tree "$R"
"#
            )
        };
    }

    /// post-index-change tail: lists the managed file three times, the injected blob
    /// between two validated ones, so a read by path returns the validated bytes.
    #[cfg(unix)]
    const HOOK_INDEX_HIDDEN_BLOB: &str = concat!(
        index_start_lines!(),
        r#"G=$(printf '100644 blob %s\tgitconfig.repo\n100644 blob %s\tgitconfig.repo\n100644 blob %s\tgitconfig.repo\n' "$V" "$I" "$V" | git mktree)
"#,
        read_tree_lines!(),
    );

    /// post-index-change tail: lists the managed file twice, both the validated blob.
    #[cfg(unix)]
    const HOOK_INDEX_REPEATED_SAME_BLOB: &str = concat!(
        index_start_lines!(),
        r#"G=$(printf '100644 blob %s\tgitconfig.repo\n100644 blob %s\tgitconfig.repo\n' "$V" "$V" | git mktree)
"#,
        read_tree_lines!(),
    );

    /// post-index-change tail: adds an empty directory next to the managed file.
    #[cfg(unix)]
    const HOOK_INDEX_EMPTY_DIRECTORY: &str = concat!(
        index_start_lines!(),
        r#"E=$(git mktree </dev/null)
G=$(printf '100644 blob %s\tgitconfig.repo\n040000 tree %s\tINJECTED_DIR_NAME\n' "$V" "$E" | git mktree)
"#,
        read_tree_lines!(),
    );

    /// post-index-change tail: removes the managed file from the index.
    #[cfg(unix)]
    const HOOK_INDEX_REMOVE: &str = "git rm -q --cached \"$F\"\n";

    /// post-index-change tail: makes the managed file a symlink entry holding the validated
    /// blob.
    #[cfg(unix)]
    const HOOK_INDEX_SYMLINK: &str = r#"V=$(git rev-parse ":$F")
git update-index --add --cacheinfo 120000,"$V","$F"
"#;

    /// post-index-change tail: sets the exec bit of the managed file in the index.
    #[cfg(unix)]
    const HOOK_INDEX_EXEC_FLIP: &str = "git update-index --chmod=+x \"$F\"\n";

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! injected_commit_lines {
        () => {
            r#"IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
IT=$(printf '100644 blob %s\tinjected.txt\n' "$IB" | git mktree)
X=$(git commit-tree "$IT" -p "$(git rev-parse HEAD~1)" -m injected)
printf '%s\n' "$IB" > .git/ude-test-injected-blob
"#
        };
    }

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! graft_lines {
        () => {
            concat!(
                injected_commit_lines!(),
                r#"printf '%s %s %s\n' "$(git rev-parse HEAD)" "$(git rev-parse HEAD~1)" "$X" >> .git/info/grafts
"#
            )
        };
    }

    /// See [`amend_inject_lines`].
    #[cfg(unix)]
    macro_rules! forged_graph_lines {
        () => {
            concat!(
                injected_commit_lines!(),
                r#"git update-ref refs/ude-test/x "$X"
git commit-graph write --reachable
git update-ref -d refs/ude-test/x
sh "$FORGE" .git/objects/info/commit-graph "$(git rev-parse HEAD)" parent "$X"
"#
            )
        };
    }

    /// Hook tail: writes an injected commit and its blob into the object store, referenced
    /// by nothing UDE's commit reaches.
    #[cfg(unix)]
    const HOOK_INJECTED_COMMIT: &str = injected_commit_lines!();

    /// Hook tail: grafts the injected commit onto UDE's commit as a second parent.
    #[cfg(unix)]
    const HOOK_GRAFT: &str = graft_lines!();

    /// post-commit tail: the injected commit now, and a grafts file written during the push.
    /// The hook leaves a bounded background process and points `remote.$U.receivepack` at
    /// a command that signals it and waits for its write before running the real
    /// receive-pack. A push to a URL runs that key only for a URL not starting with `/`, so
    /// this needs [`push_over_file_url`]. The write lands after the gate and before push's
    /// object walk, and leaves `.git/ude-ack` behind. Both loops stop after about 10 s, so a
    /// refused push leaves nothing running for long.
    #[cfg(unix)]
    const HOOK_GRAFTS_WRITTEN_DURING_PUSH: &str = concat!(
        injected_commit_lines!(),
        r#"C=$(git rev-parse HEAD); B=$(git rev-parse HEAD~1); D=$(pwd)/.git
git config "remote.$U.receivepack" "n=0; : > '$D/ude-go'; while [ ! -e '$D/ude-ack' ] && [ \$n -lt 1000 ]; do sleep 0.01; n=\$((n+1)); done; git receive-pack"
( n=0; while [ ! -e "$D/ude-go" ] && [ $n -lt 1000 ]; do sleep 0.01; n=$((n+1)); done; printf '%s %s %s\n' "$C" "$B" "$X" >> "$D/info/grafts"; : > "$D/ude-ack" ) >/dev/null 2>&1 &
"#
    );

    /// post-commit tail: the shallow file names UDE's commit, so git sees it as having no
    /// parent and calls it a non-fast-forward.
    #[cfg(unix)]
    const HOOK_SHALLOW_NAMES_COMMIT: &str = "git rev-parse HEAD > .git/shallow\n";

    /// post-commit tail: replaces UDE's commit with a parentless one. A push that honours
    /// replace refs sees no fast-forward from the fetched head; UDE's pushes read past
    /// replace refs and publish the commit itself.
    #[cfg(unix)]
    const HOOK_REPLACE_WITH_PARENTLESS: &str = r#"T=$(git rev-parse 'HEAD^{tree}')
R=$(git commit-tree "$T" -m parentless)
git replace HEAD "$R"
"#;

    /// Hook tail: writes a commit-graph holding the injected commit, then forges UDE's
    /// commit in it to have the injected commit as a second parent. Needs a `FORGE` line.
    #[cfg(unix)]
    const HOOK_FORGED_COMMIT_GRAPH: &str = forged_graph_lines!();

    /// Hook tail: [`HOOK_FORGED_COMMIT_GRAPH`] with the graph in a split chain.
    #[cfg(unix)]
    const HOOK_FORGED_SPLIT_COMMIT_GRAPH: &str = concat!(
        injected_commit_lines!(),
        r#"git update-ref refs/ude-test/x "$X"
git commit-graph write --reachable --split
git update-ref -d refs/ude-test/x
sh "$FORGE" ".git/objects/info/commit-graphs/graph-$(tail -n 1 .git/objects/info/commit-graphs/commit-graph-chain).graph" "$(git rev-parse HEAD)" parent "$X"
"#
    );

    /// Hook tail: [`HOOK_FORGED_COMMIT_GRAPH`] with the graph moved into an alternate
    /// object directory.
    #[cfg(unix)]
    const HOOK_FORGED_ALTERNATE_COMMIT_GRAPH: &str = concat!(
        forged_graph_lines!(),
        r#"mkdir -p .git/ude-test-alt/objects/info
mv .git/objects/info/commit-graph .git/ude-test-alt/objects/info/commit-graph
printf '%s\n' "$(pwd)/.git/ude-test-alt/objects" > .git/objects/info/alternates
"#
    );

    /// Hook tail: builds a pack bitmap while the graft exists, then removes the graft.
    #[cfg(unix)]
    const HOOK_BITMAP_AFTER_GRAFT: &str = concat!(
        graft_lines!(),
        "git repack -adb -q\n",
        "rm -f .git/info/grafts\n",
    );

    /// Hook tail: builds a multi-pack-index bitmap while the graft exists, then removes
    /// the graft.
    #[cfg(unix)]
    const HOOK_MIDX_BITMAP_AFTER_GRAFT: &str = concat!(
        graft_lines!(),
        "git repack -ad -q\n",
        "git multi-pack-index write --bitmap\n",
        "rm -f .git/info/grafts\n",
    );

    /// POSIX sh, arguments: graph file, commit C, the word `parent`, commit X. Sets C's
    /// second parent to X in the graph file. Git does not verify the trailing checksum
    /// when reading, so it is left as it is. Exits non-zero when a chunk or id is missing.
    #[cfg(unix)]
    const FORGE_COMMIT_GRAPH_SH: &str = r#"#!/bin/sh
set -eu
G=$1
C=$2
X=$4
hex() { od -A n -t x1 -v -j "$1" -N "$2" "$G" | tr -d ' \n'; }
num() { echo $((0x$(hex "$1" "$2"))); }
chmod u+w "$G"
case $(num 5 1) in
  1) H=20 ;;
  2) H=32 ;;
  *) exit 1 ;;
esac
CHUNKS=$(num 6 1)
chunk() {
  I=0
  while [ "$I" -lt "$CHUNKS" ]; do
    E=$((8 + I * 12))
    if [ "$(hex "$E" 4)" = "$1" ]; then
      num $((E + 4)) 8
      return 0
    fi
    I=$((I + 1))
  done
  exit 1
}
OIDF=$(chunk 4f494446)
OIDL=$(chunk 4f49444c)
CDAT=$(chunk 43444154)
N=$(num $((OIDF + 255 * 4)) 4)
pos() {
  P=$(hex "$OIDL" $((N * H)) | fold -w $((2 * H)) | grep -n -x "$1" | cut -d: -f1)
  [ -n "$P" ] || exit 1
  echo $((P - 1))
}
PC=$(pos "$C")
PX=$(pos "$X")
OFF=$((CDAT + PC * (H + 16) + H + 4))
byte() { printf "\\$(printf '%03o' "$1")"; }
{
  byte $((PX >> 24 & 255))
  byte $((PX >> 16 & 255))
  byte $((PX >> 8 & 255))
  byte $((PX & 255))
} | dd of="$G" bs=1 seek="$OFF" count=4 conv=notrunc 2>/dev/null
"#;

    /// What a test inspects after [`push_with_hook`].
    #[cfg(unix)]
    struct HookedPush {
        res: Result<String, PushRemoteError>,
        bare: tempfile::TempDir,
        _cache: tempfile::TempDir,
        checkout: PathBuf,
        fetched_sha: String,
        managed_rel: String,
        /// The hook's marker exists: a refusal or a publish without it proves nothing.
        hook_ran: bool,
    }

    /// [`push_with_hook`] with a post-commit hook.
    #[cfg(unix)]
    fn push_with_post_commit_hook(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_tail: &str,
    ) -> HookedPush {
        push_with_hook(git, engine, open_pr, "post-commit", hook_tail)
    }

    /// [`push_with_guarded_hook`] with no guard.
    #[cfg(unix)]
    fn push_with_hook(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_name: &str,
        hook_tail: &str,
    ) -> HookedPush {
        push_with_guarded_hook(git, engine, open_pr, hook_name, "", hook_tail)
    }

    /// [`push_with_guarded_hook`] with a post-index-change hook that acts once something is
    /// staged: the hook also fires on `git status`, before anything is.
    #[cfg(unix)]
    fn push_with_index_hook(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_tail: &str,
    ) -> HookedPush {
        push_with_guarded_hook(
            git,
            engine,
            open_pr,
            "post-index-change",
            "git diff --cached --quiet && exit 0",
            hook_tail,
        )
    }

    /// [`push_with_guarded_hook`] through a `file://` URL of the bare remote, with no guard.
    /// Git honours `remote.<url>.*` for a URL not starting with `/`, and a `file://` clone
    /// is really shallow, unlike a clone of a path.
    #[cfg(unix)]
    fn push_over_file_url(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_name: &str,
        hook_tail: &str,
    ) -> HookedPush {
        push_with_hook_at(git, engine, open_pr, hook_name, "", hook_tail, true)
    }

    /// [`push_with_hook_at`] through the bare remote's path.
    #[cfg(unix)]
    fn push_with_guarded_hook(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_name: &str,
        guard: &str,
        hook_tail: &str,
    ) -> HookedPush {
        push_with_hook_at(git, engine, open_pr, hook_name, guard, hook_tail, false)
    }

    /// Publishes [`HOOKED_TEMPLATE`] over the seeded template of a fresh bare remote with
    /// the hook `hook_name` installed in the cache checkout; the remote is named by its
    /// `file://` URL when `file_url`, by its path otherwise. The hook runs `guard`, then
    /// its body once (a marker stops an amend's own post-commit), with `F` set to the
    /// managed file's checkout-relative path and `U` to the URL the push goes to, then
    /// `hook_tail`.
    #[cfg(unix)]
    fn push_with_hook_at(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        hook_name: &str,
        guard: &str,
        hook_tail: &str,
        file_url: bool,
    ) -> HookedPush {
        use std::os::unix::fs::PermissionsExt;
        let bare = seeded_bare_remote(git);
        let url = if file_url {
            format!("file://{}", bare.path().display())
        } else {
            bare.path().to_str().unwrap().to_string()
        };
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(&url, "main", cache.path())
            .unwrap();
        let push_url = cached.remote_url.clone();
        let checkout = cached.checkout_dir.clone();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        let managed_rel = managed
            .strip_prefix(&checkout)
            .unwrap()
            .display()
            .to_string();
        fs::write(&managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));

        let hook = checkout.join(".git/hooks").join(hook_name);
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\n{guard}\n[ -e .git/ude-test-hook-ran ] && exit 0\n: > .git/ude-test-hook-ran\nF='{managed_rel}'\nU='{push_url}'\n{hook_tail}"
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            open_pr,
            std::slice::from_ref(&managed),
            &validated,
        );
        let hook_ran = checkout.join(".git/ude-test-hook-ran").exists();
        let fetched_sha = cached.fetched_sha.clone();
        drop(cached);
        HookedPush {
            res,
            bare,
            _cache: cache,
            checkout,
            fetched_sha,
            managed_rel,
            hook_ran,
        }
    }

    /// `(refname, object id)` of every `refs/heads/*` of the bare remote.
    #[cfg(unix)]
    fn remote_heads(git: &GitFixture, bare: &Path) -> Vec<(String, String)> {
        git.output(
            bare,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ],
        )
        .lines()
        .map(|line| {
            let (name, id) = line.split_once(' ').expect("refname and object id");
            (name.to_string(), id.to_string())
        })
        .collect()
    }

    /// The remote's only head is still `main` at the fetched head: nothing was published,
    /// and no `ude-sync-*` branch exists.
    #[cfg(unix)]
    fn assert_remote_unchanged(git: &GitFixture, pushed: &HookedPush) {
        assert_eq!(
            remote_heads(git, pushed.bare.path()),
            vec![("refs/heads/main".to_string(), pushed.fetched_sha.clone())],
            "the remote changed; push result: {:?}",
            pushed.res
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_merge_over_amended_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let tail = format!("{HOOK_AMEND_INJECT}{HOOK_MERGE_WRAP}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(err.contains("exactly one parent"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_merge_over_amended_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let tail = format!("{HOOK_AMEND_INJECT}{HOOK_MERGE_WRAP}");
        let pushed = push_with_post_commit_hook(&git, &engine, true, &tail);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("exactly one parent"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_added_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let tail = format!("{HOOK_AMEND_INJECT}{HOOK_EXTRA_COMMIT}");
        let pushed = push_with_post_commit_hook(&git, &engine, true, &tail);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("Branch continuity check failed"), "{err}");
    }

    /// The exact bytes of the managed template on `refname` of the bare remote.
    #[cfg(unix)]
    fn remote_managed_blob(git: &GitFixture, pushed: &HookedPush, refname: &str) -> Vec<u8> {
        git.checked(
            pushed.bare.path(),
            &["show", &format!("{refname}:{}", pushed.managed_rel)],
        )
        .stdout
    }

    /// The name of the one `refs/heads/ude-sync-*` ref of the bare remote; `main` must
    /// still be at the fetched head and nothing else may exist.
    #[cfg(unix)]
    fn single_sync_branch(git: &GitFixture, pushed: &HookedPush) -> String {
        let heads = remote_heads(git, pushed.bare.path());
        let (sync, others): (Vec<_>, Vec<_>) = heads
            .into_iter()
            .partition(|(name, _)| name.starts_with("refs/heads/ude-sync-"));
        assert_eq!(
            others,
            vec![("refs/heads/main".to_string(), pushed.fetched_sha.clone())]
        );
        assert_eq!(sync.len(), 1, "{sync:?}");
        sync[0].0.clone()
    }

    #[test]
    fn test_raw_commit_parents() {
        let id = |digit: char| digit.to_string().repeat(40);
        let (tree, p1, p2, p8, p9) = (id('0'), id('1'), id('2'), id('8'), id('9'));
        let people = "author a <a@x> 1 +0000\ncommitter c <c@x> 1 +0000\n";
        let mut invalid_message = format!("tree {tree}\nparent {p1}\n{people}\nmsg ").into_bytes();
        invalid_message.extend_from_slice(&[0xFF, 0xFE, b'\n']);
        let rows: Vec<(&str, Vec<u8>, Vec<String>)> = vec![
            (
                "one parent",
                format!("tree {tree}\nparent {p1}\n{people}\nmsg\n").into_bytes(),
                vec![p1.clone()],
            ),
            (
                "two parents in order",
                format!("tree {tree}\nparent {p1}\nparent {p2}\n{people}\nmsg\n").into_bytes(),
                vec![p1.clone(), p2.clone()],
            ),
            (
                "no parent",
                format!("tree {tree}\n{people}\nmsg\n").into_bytes(),
                vec![],
            ),
            (
                "parent text in the message",
                format!("tree {tree}\nparent {p1}\n{people}\nparent {p9}\n").into_bytes(),
                vec![p1.clone()],
            ),
            (
                "indented header continuation line",
                format!(
                    "tree {tree}\nparent {p1}\n{people}gpgsig -----BEGIN\n parent {p8}\n -----END\n\nmsg\n"
                )
                .into_bytes(),
                vec![p1.clone()],
            ),
            (
                "message with invalid UTF-8",
                invalid_message,
                vec![p1.clone()],
            ),
        ];
        let mut failures = Vec::new();
        for (name, raw, expected) in rows {
            let actual = raw_commit_parents(&raw);
            if actual != expected {
                failures.push(format!("{name}: {actual:?} != {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The grafts check's verdict when `git` is a shim that prints `printf_args` and exits 0,
    /// which reaches the outcomes a real git does not produce.
    #[cfg(unix)]
    fn grafts_verdict_with_git_printing(git: &GitFixture, printf_args: &str) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        let shim_dir = tempdir().unwrap();
        let shim = shim_dir.path().join("git");
        fs::write(&shim, format!("#!/bin/sh\nprintf {printf_args}\n")).unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        let inherited = std::env::var_os("PATH").unwrap();
        let path = std::env::join_paths(
            std::iter::once(shim_dir.path().to_path_buf()).chain(std::env::split_paths(&inherited)),
        )
        .unwrap();
        let mut env = git.gitconfig_env();
        env.push(("PATH".into(), path));
        SyncEngine::new()
            .with_extra_env(env)
            .refuse_grafts_file(shim_dir.path())
    }

    #[test]
    #[cfg(unix)]
    fn test_refuse_grafts_file_outcomes() {
        const PRESENT: &str = "which can make git push send objects outside the validated commit";
        const UNLOCATED: &str = "cannot locate the grafts file";
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let new_repo = || {
            let repo = tempdir().unwrap();
            git.run(repo.path(), &["init", "-b", "main"]);
            fs::create_dir_all(repo.path().join(".git/info")).unwrap();
            repo
        };
        let grafts = |repo: &tempfile::TempDir| repo.path().join(".git/info/grafts");
        let mut failures = Vec::new();
        let mut check = |name: &str, verdict: Result<(), String>, expected: Option<&str>| {
            let as_expected = match (&verdict, expected) {
                (Ok(()), None) => true,
                (Err(err), Some(text)) => err.contains(text),
                _ => false,
            };
            if !as_expected {
                failures.push(format!("{name}: {verdict:?}, expected {expected:?}"));
            }
        };

        let absent = new_repo();
        check("no file", engine.refuse_grafts_file(absent.path()), None);

        let present = new_repo();
        fs::write(grafts(&present), "").unwrap();
        check(
            "an empty file",
            engine.refuse_grafts_file(present.path()),
            Some(PRESENT),
        );

        let linked = new_repo();
        let target = linked.path().join("target");
        fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink(&target, grafts(&linked)).unwrap();
        check(
            "a symlink to a file",
            engine.refuse_grafts_file(linked.path()),
            Some(PRESENT),
        );

        // Git resolves the link before printing the path, and reads no graft through it.
        let dangling = new_repo();
        std::os::unix::fs::symlink("nowhere", grafts(&dangling)).unwrap();
        check(
            "a dangling symlink",
            engine.refuse_grafts_file(dangling.path()),
            None,
        );

        let info_is_file = new_repo();
        let info = info_is_file.path().join(".git/info");
        fs::remove_dir_all(&info).unwrap();
        fs::write(&info, "").unwrap();
        check(
            "a file where the info directory belongs",
            engine.refuse_grafts_file(info_is_file.path()),
            Some(UNLOCATED),
        );

        let not_repo = tempdir().unwrap();
        check(
            "not a repository",
            engine.refuse_grafts_file(not_repo.path()),
            Some(UNLOCATED),
        );

        check(
            "git prints nothing",
            grafts_verdict_with_git_printing(&git, "''"),
            Some(UNLOCATED),
        );
        check(
            "git prints invalid UTF-8",
            grafts_verdict_with_git_printing(&git, r"'\377\n'"),
            Some(UNLOCATED),
        );
        let file = tempfile::NamedTempFile::new().unwrap();
        check(
            "the path cannot be inspected",
            grafts_verdict_with_git_printing(
                &git,
                &format!("'%s\\n' '{}/grafts'", file.path().display()),
            ),
            Some("cannot inspect"),
        );

        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_remove_git_test_env() {
        let mut cmd = Command::new("git");
        cmd.env("GIT_TEST_FROM_EXTRA_ENV", "1")
            .env("GIT_TESTY", "1");
        let mut inherited: Vec<OsString> = [
            "GIT_TEST_COMMIT_GRAPH",
            "GIT_TEST_SPLIT_INDEX",
            "PATH",
            "GIT_TESTX",
            "git_test_lower",
        ]
        .map(OsString::from)
        .to_vec();
        let mut removed = vec![
            OsString::from("GIT_TEST_COMMIT_GRAPH"),
            OsString::from("GIT_TEST_SPLIT_INDEX"),
            OsString::from("GIT_TEST_FROM_EXTRA_ENV"),
        ];
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let non_utf8 = OsString::from_vec(b"GIT_TEST_\xFF".to_vec());
            inherited.push(non_utf8.clone());
            removed.push(non_utf8);
        }

        remove_git_test_env(&mut cmd, inherited);

        let envs: HashMap<&OsStr, Option<&OsStr>> = cmd.get_envs().collect();
        let mut failures = Vec::new();
        for key in &removed {
            if envs.get(key.as_os_str()) != Some(&None) {
                failures.push(format!("{key:?} was not removed"));
            }
        }
        if envs.get(OsStr::new("GIT_TESTY")) != Some(&Some(OsStr::new("1"))) {
            failures.push("GIT_TESTY set on the command was not kept".to_string());
        }
        for key in ["PATH", "GIT_TESTX", "git_test_lower"] {
            if envs.contains_key(OsStr::new(key)) {
                failures.push(format!("{key} outside the prefix was touched"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_raw_commit_parts() {
        let header = b"tree t\nparent p\nauthor a <a@x> 1 +0000\ncommitter c <c@x> 1 +0000\n";
        let with_message = |message: &[u8]| {
            let mut raw = header.to_vec();
            raw.push(b'\n');
            raw.extend_from_slice(message);
            raw
        };
        let rows: [(&str, &[u8]); 6] = [
            ("message with a final LF", b"msg\n"),
            ("message without a final LF", b"msg"),
            ("message ending in two LFs", b"msg\n\n"),
            ("a parent line in the message", b"parent p9\n"),
            ("message with invalid UTF-8", &[0xFF, 0xFE]),
            ("message with an empty line of its own", b"one\n\ntwo\n"),
        ];
        let mut failures = Vec::new();
        for (name, message) in rows {
            let raw = with_message(message);
            let parts = raw_commit_parts(&raw);
            if parts != Some((header.as_slice(), message)) {
                failures.push(format!("{name}: {parts:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert_eq!(raw_commit_parts(b"tree t\nauthor a\ncommitter c\n"), None);
        assert_eq!(raw_commit_parts(b""), None);
    }

    #[test]
    fn test_count_empty_directories() {
        let oid = "0".repeat(40);
        let entry = |kind: &str, path: &[u8]| {
            let mode = if kind == "tree" { "040000" } else { "100644" };
            let mut record = format!("{mode} {kind} {oid}\t").into_bytes();
            record.extend_from_slice(path);
            record.push(0);
            record
        };
        let listing = |entries: &[(&str, &[u8])]| {
            entries
                .iter()
                .flat_map(|(kind, path)| entry(kind, path))
                .collect::<Vec<u8>>()
        };
        let rows: Vec<(&str, Vec<u8>, Option<usize>)> = vec![
            ("no records", Vec::new(), Some(0)),
            (
                "blobs only",
                listing(&[("blob", b"f"), ("blob", b"g")]),
                Some(0),
            ),
            (
                "a directory holding a blob",
                listing(&[("tree", b"a"), ("blob", b"a/f")]),
                Some(0),
            ),
            (
                "a directory with nothing below it",
                listing(&[("tree", b"a"), ("blob", b"f")]),
                Some(1),
            ),
            (
                "a directory holding only an empty directory",
                listing(&[("tree", b"a"), ("tree", b"a/b")]),
                Some(1),
            ),
            (
                "a sibling sharing a name prefix is not below it",
                listing(&[("tree", b"a"), ("blob", b"a-b/x")]),
                Some(1),
            ),
            (
                "a blob named like a directory counts nothing",
                listing(&[("blob", b"a"), ("blob", b"a-b")]),
                Some(0),
            ),
            (
                "a deep blob holds every directory above it",
                listing(&[("tree", b"a"), ("tree", b"a/b"), ("blob", b"a/b/c")]),
                Some(0),
            ),
            (
                "tab and newline bytes in a path",
                listing(&[("tree", b"a\tb\nc"), ("blob", b"a\tb\nc/f")]),
                Some(0),
            ),
            (
                "an empty directory whose name holds a tab",
                listing(&[("tree", b"a\tb")]),
                Some(1),
            ),
            (
                "a path of invalid UTF-8",
                listing(&[
                    ("tree", &[0xFF]),
                    ("tree", &[0xFE]),
                    ("blob", &[0xFE, b'/', 0xFD]),
                ]),
                Some(1),
            ),
            ("a record without a tab", b"100644 blob x\0".to_vec(), None),
            (
                "a record without a type field",
                b"100644\tf\0".to_vec(),
                None,
            ),
            (
                "a final record without its NUL",
                format!("100644 blob {oid}\tf").into_bytes(),
                None,
            ),
        ];
        let mut failures = Vec::new();
        for (name, raw, expected) in rows {
            let actual = count_empty_directories(&raw);
            if actual != expected {
                failures.push(format!("{name}: {actual:?} != {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_committed_gate_compares_full_object_ids() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let base_file = repo.path().join("base.txt");
        fs::write(&base_file, "initial").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "base commit"]);
        let base = git.output(repo.path(), &["rev-parse", "HEAD"]);
        fs::write(&base_file, "updated\n").unwrap();
        git.run(repo.path(), &["add", "base.txt"]);
        git.run(repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let head = git.output(repo.path(), &["rev-parse", "HEAD"]);
        let expected_set: HashSet<PathBuf> = [PathBuf::from("base.txt")].into_iter().collect();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));

        let rows: [(&str, &str, Option<&str>); 4] = [
            ("full id", &base, None),
            (
                "first 12 characters",
                &base[..12],
                Some("Branch continuity check failed"),
            ),
            ("empty base", "", Some("no fetched head")),
            (
                "the commit's own id",
                &head,
                Some("Branch continuity check failed"),
            ),
        ];
        let mut failures = Vec::new();
        for (name, row_base, expected) in rows {
            let verdict = engine.validate_committed_tree(
                repo.path(),
                &head,
                row_base,
                &expected_set,
                &validated,
                &commit_tree_id(&git, repo.path(), &head),
            );
            let as_expected = match (&verdict, expected) {
                (Ok(()), None) => true,
                (Err(err), Some(text)) => err.contains(text),
                _ => false,
            };
            if !as_expected {
                failures.push(format!("{name}: {verdict:?}, expected {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_parentless_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_PARENTLESS);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("has 0 parents"), "{err}");
        assert!(err.contains("exactly one parent"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_added_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let tail = format!("{HOOK_AMEND_INJECT}{HOOK_EXTRA_COMMIT}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("Branch continuity check failed"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_added_file() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_ADD_FILE);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("unvalidated file committed in HEAD"), "{err}");
        assert!(!err.contains("hook-extra"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_benign_post_commit_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, "");

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let main = git.output(pushed.bare.path(), &["rev-parse", "refs/heads/main"]);
        assert_ne!(main, pushed.fetched_sha, "remote main did not advance");
        assert_eq!(
            remote_managed_blob(&git, &pushed, "refs/heads/main"),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    /// A hook artifact holding one object id, from `.git/` of the cache checkout.
    #[cfg(unix)]
    fn hook_recorded_id(pushed: &HookedPush, name: &str) -> String {
        fs::read_to_string(pushed.checkout.join(".git").join(name))
            .unwrap()
            .trim()
            .to_string()
    }

    /// Nothing of `ude-hook-tag` reached the remote though the hook asked for tag
    /// following; the same push without `--no-follow-tags` publishes it to a second bare
    /// repository, so the hook's config and tag were really in effect.
    #[cfg(unix)]
    fn assert_hook_tag_not_published(git: &GitFixture, pushed: &HookedPush, published: &str) {
        let bare = pushed.bare.path();
        assert_eq!(git.output(bare, &["for-each-ref", "refs/tags"]), "");
        let tag = hook_recorded_id(pushed, "ude-test-tag");
        let tag_object = git
            .cmd(bare)
            .args(["cat-file", "-e", &tag])
            .output()
            .unwrap();
        assert!(!tag_object.status.success(), "the tag object was sent");

        let second = tempdir().unwrap();
        git.run(second.path(), &["init", "--bare", "-b", "main"]);
        git.run(
            &pushed.checkout,
            &[
                "push",
                second.path().to_str().unwrap(),
                &format!("{published}:refs/heads/control"),
            ],
        );
        assert_eq!(
            git.output(
                second.path(),
                &["for-each-ref", "--format=%(refname)", "refs/tags"]
            ),
            "refs/tags/ude-hook-tag"
        );
        let message = git.output(second.path(), &["cat-file", "tag", "ude-hook-tag"]);
        assert!(message.contains("INJECTED_TAG_MESSAGE"), "{message}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_replaced_blob() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_REPLACE_BLOB);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);

        // Opposite direction: a default read is fooled, a raw read sees the hook's blob.
        let injected = hook_recorded_id(&pushed, "ude-test-injected-blob");
        let default_read = git.checked(&pushed.checkout, &["cat-file", "blob", &injected]);
        assert_eq!(default_read.stdout, HOOKED_TEMPLATE.as_bytes());
        let raw_read = git.output(
            &pushed.checkout,
            &["--no-replace-objects", "cat-file", "blob", &injected],
        );
        assert!(raw_read.contains("INJECTED_BY_HOOK"), "{raw_read}");

        let err = push_failure(pushed.res);
        assert!(
            err.contains("Post-commit publication gate violation: committed HEAD blob"),
            "{err}"
        );
        assert!(err.contains("differs from the validated content"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_merge_replaced_by_validated_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_REPLACE_MERGE);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);

        // Opposite direction: a default read shows one parent, a raw read shows the merge.
        let merge = hook_recorded_id(&pushed, "ude-test-merge");
        let parent_lines = |args: &[&str]| {
            git.output(&pushed.checkout, args)
                .lines()
                .filter(|line| line.starts_with("parent "))
                .count()
        };
        assert_eq!(parent_lines(&["cat-file", "commit", &merge]), 1);
        assert_eq!(
            parent_lines(&["--no-replace-objects", "cat-file", "commit", &merge]),
            2
        );

        let err = push_failure(pushed.res);
        assert!(err.contains("has 2 parents"), "{err}");
        assert!(err.contains("exactly one parent"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_replaced_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_REPLACE_TREE);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);

        // Opposite direction: a default listing omits the added file, a raw listing has it.
        let amended = hook_recorded_id(&pushed, "ude-test-amended");
        let listing = |prefix: &[&str]| {
            let mut args: Vec<&str> = prefix.to_vec();
            args.extend([
                "diff-tree",
                "-r",
                "--name-only",
                &pushed.fetched_sha,
                &amended,
            ]);
            git.output(&pushed.checkout, &args)
        };
        let default_listing = listing(&[]);
        assert!(
            !default_listing.contains("templates/hook-extra.txt"),
            "{default_listing}"
        );
        let raw_listing = listing(&["--no-replace-objects"]);
        assert!(
            raw_listing.contains("templates/hook-extra.txt"),
            "{raw_listing}"
        );

        let err = push_failure(pushed.res);
        assert!(err.contains("unvalidated file committed in HEAD"), "{err}");
        assert!(!err.contains("hook-extra"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_the_validated_commit_not_the_branch_tip() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_DETACH_TO_VALIDATED);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let sync = single_sync_branch(&git, &pushed);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &sync),
            HOOKED_TEMPLATE.as_bytes()
        );
        // Opposite direction: the branch a push by name would send holds the hook's bytes.
        let local_blob = git.output(
            &pushed.checkout,
            &["show", &format!("{sync}:{}", pushed.managed_rel)],
        );
        assert!(local_blob.contains("INJECTED_BY_HOOK"), "{local_blob}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_the_validated_commit_after_head_moves() {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        let real_git = String::from_utf8(real_git.stdout).unwrap();
        // No hook runs between the committed gate and the push, so the engine's `git` is a
        // shim that moves HEAD at exactly that point, right before the real push starts.
        let shim_dir = tempdir().unwrap();
        let shim = shim_dir.path().join("git");
        fs::write(
            &shim,
            format!(
                "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = push ]; then\n    F=templates/projects/generic/gitconfig.repo\n    printf '%s\\n' INJECTED_BY_HOOK >> \"$F\"\n    '{real}' add -- \"$F\"\n    '{real}' commit --amend --no-edit --no-verify -q\n  fi\ndone\nexec '{real}' \"$@\"\n",
                real = real_git.trim()
            ),
        )
        .unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        let inherited = std::env::var_os("PATH").unwrap();
        let path = std::env::join_paths(
            std::iter::once(shim_dir.path().to_path_buf()).chain(std::env::split_paths(&inherited)),
        )
        .unwrap();
        let mut engine_env = git.gitconfig_env();
        engine_env.push(("PATH".into(), path));
        let engine = SyncEngine::new().with_extra_env(engine_env);

        let pushed = push_with_post_commit_hook(&git, &engine, false, "");

        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        assert_eq!(
            remote_managed_blob(&git, &pushed, "refs/heads/main"),
            HOOKED_TEMPLATE.as_bytes()
        );
        // Opposite direction: HEAD did move to the shim's commit, so a push of HEAD would
        // have published the injected line.
        let head_blob = git.output(
            &pushed.checkout,
            &["show", &format!("HEAD:{}", pushed.managed_rel)],
        );
        assert!(head_blob.contains("INJECTED_BY_HOOK"), "{head_blob}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_does_not_publish_hook_tag() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_TAG_FOLLOW);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let main = git.output(pushed.bare.path(), &["rev-parse", "refs/heads/main"]);
        assert_ne!(main, pushed.fetched_sha, "remote main did not advance");
        assert_eq!(
            remote_managed_blob(&git, &pushed, "refs/heads/main"),
            HOOKED_TEMPLATE.as_bytes()
        );
        assert_hook_tag_not_published(&git, &pushed, &main);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_does_not_publish_hook_tag() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_TAG_FOLLOW);

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let sync = single_sync_branch(&git, &pushed);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &sync),
            HOOKED_TEMPLATE.as_bytes()
        );
        let published = git.output(pushed.bare.path(), &["rev-parse", &sync]);
        assert_hook_tag_not_published(&git, &pushed, &published);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_benign_post_commit_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, "");

        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let sync = single_sync_branch(&git, &pushed);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &sync),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    /// The commit message bytes of a raw `cat-file commit` output, as UTF-8 text.
    #[cfg(unix)]
    fn message_of_raw_commit(raw: Vec<u8>) -> String {
        let text = String::from_utf8(raw).unwrap();
        let (_, message) = text.split_once("\n\n").expect("a commit has an empty line");
        message.to_string()
    }

    /// A hook that writes commit text was refused with the message naming the likely
    /// cause, nothing was published and the error does not print the hook's text.
    #[cfg(unix)]
    fn assert_refused_as_foreign_message(git: &GitFixture, pushed: HookedPush) {
        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("is not the sync commit message"), "{err}");
        assert!(
            err.contains("prepare-commit-msg or post-commit hook"),
            "{err}"
        );
        assert!(!err.contains("INJECTED_BY_HOOK"), "{err}");
    }

    #[test]
    fn test_committed_gate_requires_exact_sync_message() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let base_file = repo.path().join("base.txt");
        fs::write(&base_file, "initial").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "base commit"]);
        let base = git.output(repo.path(), &["rev-parse", "HEAD"]);
        fs::write(&base_file, "updated\n").unwrap();
        git.run(repo.path(), &["add", "base.txt"]);
        git.run(repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let tree = git.output(repo.path(), &["rev-parse", "HEAD^{tree}"]);
        let expected_set: HashSet<PathBuf> = [PathBuf::from("base.txt")].into_iter().collect();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        let message_dir = tempdir().unwrap();
        let message_file = message_dir.path().join("message");

        let refusal = "is not the sync commit message";
        let rows: [(&str, String, Option<&str>); 4] = [
            ("one LF", format!("{SYNC_COMMIT_MESSAGE}\n"), None),
            ("no LF", SYNC_COMMIT_MESSAGE.to_string(), Some(refusal)),
            (
                "two LFs",
                format!("{SYNC_COMMIT_MESSAGE}\n\n"),
                Some(refusal),
            ),
            (
                "a line added",
                format!("{SYNC_COMMIT_MESSAGE}\nINJECTED_BY_HOOK\n"),
                Some(refusal),
            ),
        ];
        let mut failures = Vec::new();
        for (name, message, expected) in rows {
            fs::write(&message_file, &message).unwrap();
            let commit = git.output(
                repo.path(),
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    &base,
                    "-F",
                    message_file.to_str().unwrap(),
                ],
            );
            let verdict = engine.validate_committed_tree(
                repo.path(),
                &commit,
                &base,
                &expected_set,
                &validated,
                &tree,
            );
            let as_expected = match (&verdict, expected) {
                (Ok(()), None) => true,
                (Err(err), Some(text)) => err.contains(text) && !err.contains("INJECTED_BY_HOOK"),
                _ => false,
            };
            if !as_expected {
                failures.push(format!("{name}: {verdict:?}, expected {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_prepare_commit_msg_hook_text() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, false, "prepare-commit-msg", HOOK_PCM_APPEND);
        assert_refused_as_foreign_message(&git, pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_prepare_commit_msg_hook_text() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "prepare-commit-msg", HOOK_PCM_APPEND);
        assert_refused_as_foreign_message(&git, pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_amended_message() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_AMEND_MESSAGE);
        assert_refused_as_foreign_message(&git, pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_amended_message() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_AMEND_MESSAGE);
        assert_refused_as_foreign_message(&git, pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_benign_prepare_commit_msg_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, false, "prepare-commit-msg", "");

        assert!(pushed.hook_ran, "the hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let main = git.output(pushed.bare.path(), &["rev-parse", "refs/heads/main"]);
        assert_ne!(main, pushed.fetched_sha, "remote main did not advance");
        assert_eq!(
            remote_managed_blob(&git, &pushed, "refs/heads/main"),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_benign_prepare_commit_msg_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "prepare-commit-msg", "");

        assert!(pushed.hook_ran, "the hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let sync = single_sync_branch(&git, &pushed);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &sync),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_replaced_message() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_REPLACE_MESSAGE);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);

        // Opposite direction: a default read shows the sync message, a raw read the hook's.
        let amended = hook_recorded_id(&pushed, "ude-test-amended-message");
        let default_read = git.checked(&pushed.checkout, &["cat-file", "commit", &amended]);
        assert_eq!(
            message_of_raw_commit(default_read.stdout),
            format!("{SYNC_COMMIT_MESSAGE}\n")
        );
        let raw_read = git.checked(
            &pushed.checkout,
            &["--no-replace-objects", "cat-file", "commit", &amended],
        );
        let raw_message = message_of_raw_commit(raw_read.stdout);
        assert!(raw_message.contains("INJECTED_BY_HOOK"), "{raw_message}");

        let err = push_failure(pushed.res);
        assert!(err.contains("is not the sync commit message"), "{err}");
        assert!(!err.contains("INJECTED_BY_HOOK"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_secret_in_commit_header() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let tail = format!(
            "git commit --amend --no-edit --no-verify -q --author=\"{prefix}{body} <hook@example.com>\"\n"
        );
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(
            err.contains("secret detected in the header of the commit to publish"),
            "{err}"
        );
        assert!(err.contains("GitHub Personal Access Token"), "{err}");
        assert!(!err.contains(body), "gate error leaked the token body");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_empty_directory() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_EMPTY_DIR);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("contains an empty directory"), "{err}");
        assert!(!err.contains("INJECTED_DIR_NAME"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_hook_replaced_tree_hiding_empty_directory() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_EMPTY_DIR_REPLACED);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);

        // Opposite direction: a default listing omits the directory, a raw listing has it.
        let commit = hook_recorded_id(&pushed, "ude-test-emptydir-commit");
        let default_listing = git.output(
            &pushed.checkout,
            &["ls-tree", "-r", "-t", "--name-only", &commit],
        );
        assert!(
            !default_listing.contains("INJECTED_DIR_NAME"),
            "{default_listing}"
        );
        let raw_listing = git.output(
            &pushed.checkout,
            &[
                "--no-replace-objects",
                "ls-tree",
                "-r",
                "-t",
                "--name-only",
                &commit,
            ],
        );
        assert!(raw_listing.contains("INJECTED_DIR_NAME"), "{raw_listing}");

        let err = push_failure(pushed.res);
        assert!(err.contains("contains an empty directory"), "{err}");
        assert!(!err.contains("INJECTED_DIR_NAME"), "{err}");
    }

    /// The forge script in its own directory, outside the cache checkout, and the hook line
    /// naming it. The directory must outlive the push.
    #[cfg(unix)]
    fn forge_script() -> (tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        let script = dir.path().join("forge.sh");
        fs::write(&script, FORGE_COMMIT_GRAPH_SH).unwrap();
        let line = format!("FORGE='{}'\n", script.display());
        (dir, line)
    }

    /// The commit the push published: remote main, or the single `ude-sync-*` branch's.
    #[cfg(unix)]
    fn published_commit(git: &GitFixture, pushed: &HookedPush, open_pr: bool) -> String {
        let refname = if open_pr {
            single_sync_branch(git, pushed)
        } else {
            "refs/heads/main".to_string()
        };
        git.output(pushed.bare.path(), &["rev-parse", &refname])
    }

    /// Pushes `commit` from `checkout` to a new bare repository, with `global_opts` before
    /// `push` and optionally `GIT_TEST_COMMIT_GRAPH=1` set on that command.
    #[cfg(unix)]
    fn push_to_fresh_bare(
        git: &GitFixture,
        checkout: &Path,
        commit: &str,
        global_opts: &[&str],
        test_commit_graph: bool,
    ) -> tempfile::TempDir {
        let bare = tempdir().unwrap();
        git.run(bare.path(), &["init", "--bare", "-b", "main"]);
        // A `file://` clone is shallow, and an empty repository refuses a shallow push.
        git.run(bare.path(), &["config", "receive.shallowUpdate", "true"]);
        let mut push = git.cmd(checkout);
        push.args(global_opts).args([
            "push",
            "--no-follow-tags",
            bare.path().to_str().unwrap(),
            &format!("{commit}:refs/heads/control"),
        ]);
        if test_commit_graph {
            push.env("GIT_TEST_COMMIT_GRAPH", "1");
        }
        let out = push.output().unwrap();
        assert!(
            out.status.success(),
            "control push failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        bare
    }

    /// The repository holds the object `oid`.
    #[cfg(unix)]
    fn holds_object(git: &GitFixture, repo: &Path, oid: &str) -> bool {
        git.cmd(repo)
            .args(["cat-file", "-e", oid])
            .output()
            .unwrap()
            .status
            .success()
    }

    /// `dir` holds a file whose name starts with `prefix` and ends with `suffix`.
    #[cfg(unix)]
    fn dir_has_file(dir: &Path, prefix: &str, suffix: &str) -> bool {
        fs::read_dir(dir).unwrap().any(|entry| {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            name.starts_with(prefix) && name.ends_with(suffix)
        })
    }

    /// The hook ran, the push succeeded and the published commit holds exactly the
    /// validated bytes. Returns that commit.
    #[cfg(unix)]
    fn assert_published_exact(git: &GitFixture, pushed: &HookedPush, open_pr: bool) -> String {
        assert!(pushed.hook_ran, "the hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let published = published_commit(git, pushed, open_pr);
        assert_ne!(published, pushed.fetched_sha, "nothing was published");
        assert_eq!(
            remote_managed_blob(git, pushed, &published),
            HOOKED_TEMPLATE.as_bytes()
        );
        published
    }

    /// In the cache checkout the published commit reads as having two parents through the
    /// forged graph and one in the raw object, so the forge was really in effect.
    #[cfg(unix)]
    fn assert_graph_forge_in_effect(git: &GitFixture, pushed: &HookedPush, published: &str) {
        let walked = git.output(
            &pushed.checkout,
            &["rev-list", "--parents", "-1", published],
        );
        assert_eq!(walked.split_whitespace().count(), 3, "{walked}");
        let raw = git.output(
            &pushed.checkout,
            &["--no-replace-objects", "cat-file", "commit", published],
        );
        let parents = raw
            .lines()
            .filter(|line| line.starts_with("parent "))
            .count();
        assert_eq!(parents, 1, "{raw}");
    }

    /// The injected blob reaches a fresh bare repository through a push with `control_opts`
    /// (the opposite direction: the forged state really leaks) and did not reach the remote.
    #[cfg(unix)]
    fn assert_leak_blocked(
        git: &GitFixture,
        pushed: &HookedPush,
        published: &str,
        control_opts: &[&str],
        control_test_commit_graph: bool,
    ) {
        let injected = hook_recorded_id(pushed, "ude-test-injected-blob");
        let control = push_to_fresh_bare(
            git,
            &pushed.checkout,
            published,
            control_opts,
            control_test_commit_graph,
        );
        assert!(
            holds_object(git, control.path(), &injected),
            "the control push did not send the injected blob, so the hook's state had no effect"
        );
        assert!(
            !holds_object(git, pushed.bare.path(), &injected),
            "the remote holds the injected blob"
        );
    }

    /// How many times each test of a grafts file written during the push repeats.
    #[cfg(unix)]
    const GRAFTS_DURING_PUSH_RUNS: usize = 3;

    /// A hook wrote a grafts file: nothing was published and the injected blob stayed home.
    #[cfg(unix)]
    fn assert_refused_for_grafts(git: &GitFixture, pushed: HookedPush) {
        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(git, &pushed);
        let injected = hook_recorded_id(&pushed, "ude-test-injected-blob");
        assert!(!holds_object(git, pushed.bare.path(), &injected));
        let err = push_failure(pushed.res);
        assert!(
            err.contains("which can make git push send objects outside the validated commit"),
            "{err}"
        );
        assert!(err.contains("info/grafts"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_grafts_file() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_GRAFT);
        assert_refused_for_grafts(&git, pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_grafts_file() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_GRAFT);
        assert_refused_for_grafts(&git, pushed);
    }

    /// A hook wrote a grafts file after the gate: the push succeeded with exactly the
    /// validated bytes and the injected blob stayed home, while the published commit pushed
    /// from the same checkout without the null grafts file delivers it (the opposite
    /// direction: the grafts file was really in effect).
    #[cfg(unix)]
    fn assert_no_injected_blob_with_control(git: &GitFixture, pushed: &HookedPush, open_pr: bool) {
        assert!(pushed.hook_ran, "the hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let injected = hook_recorded_id(pushed, "ude-test-injected-blob");
        assert!(
            !holds_object(git, pushed.bare.path(), &injected),
            "the remote holds the injected blob"
        );
        let published = assert_published_exact(git, pushed, open_pr);
        let control = push_to_fresh_bare(git, &pushed.checkout, &published, &[], false);
        assert!(
            holds_object(git, control.path(), &injected),
            "the control push did not send the injected blob, so the grafts file had no effect"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_sends_no_object_from_pre_push_grafts() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, false, "pre-push", HOOK_GRAFT);
        assert_no_injected_blob_with_control(&git, &pushed, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_sends_no_object_from_pre_push_grafts() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "pre-push", HOOK_GRAFT);
        assert_no_injected_blob_with_control(&git, &pushed, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_sends_no_object_from_grafts_written_during_push() {
        for _ in 0..GRAFTS_DURING_PUSH_RUNS {
            assert_grafts_written_during_push_ignored(false);
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_sends_no_object_from_grafts_written_during_push() {
        for _ in 0..GRAFTS_DURING_PUSH_RUNS {
            assert_grafts_written_during_push_ignored(true);
        }
    }

    /// One run of [`HOOK_GRAFTS_WRITTEN_DURING_PUSH`] over a `file://` URL: the hook's
    /// `remote.$U.receivepack` ran and the background write happened (`.git/ude-ack`), so a
    /// run where the grafts file was never written cannot pass.
    #[cfg(unix)]
    fn assert_grafts_written_during_push_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_over_file_url(
            &git,
            &engine,
            open_pr,
            "post-commit",
            HOOK_GRAFTS_WRITTEN_DURING_PUSH,
        );
        assert!(
            pushed.checkout.join(".git/ude-ack").exists(),
            "the grafts file was never written during the push; result: {:?}",
            pushed.res
        );
        assert_no_injected_blob_with_control(&git, &pushed, open_pr);
    }

    /// Publishes [`HOOKED_TEMPLATE`] to a seeded bare remote whose pre-receive hook prints
    /// `REMOTE_REFUSED_THIS_PUSH` and refuses.
    #[cfg(unix)]
    fn push_refused_by_remote(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
    ) -> Result<String, PushRemoteError> {
        use std::os::unix::fs::PermissionsExt;
        let bare = seeded_bare_remote(git);
        let refuser = bare.path().join("hooks/pre-receive");
        fs::create_dir_all(refuser.parent().unwrap()).unwrap();
        fs::write(
            &refuser,
            "#!/bin/sh\necho REMOTE_REFUSED_THIS_PUSH >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&refuser, fs::Permissions::from_mode(0o755)).unwrap();

        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        engine.push_to_remote(
            &cached.publish_target("main"),
            open_pr,
            std::slice::from_ref(&managed),
            &validated,
        )
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_failure_excerpt_has_no_grafts_hint() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let err = push_failure(push_refused_by_remote(&git, &engine, false));
        assert!(err.contains("REMOTE_REFUSED_THIS_PUSH"), "{err}");
        assert!(!err.contains("graftFileDeprecated"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_failure_excerpt_has_no_grafts_hint() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let err = push_failure(push_refused_by_remote(&git, &engine, true));
        assert!(err.contains("REMOTE_REFUSED_THIS_PUSH"), "{err}");
        assert!(!err.contains("graftFileDeprecated"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_benign_pre_push_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, false, "pre-push", "");
        assert_published_exact(&git, &pushed, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_benign_pre_push_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "pre-push", "");
        assert_published_exact(&git, &pushed, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_hook_commit_and_no_grafts_file() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_INJECTED_COMMIT);

        assert_published_exact(&git, &pushed, false);
        let injected = hook_recorded_id(&pushed, "ude-test-injected-blob");
        assert!(!holds_object(&git, pushed.bare.path(), &injected));
        assert!(!pushed.checkout.join(".git/info/grafts").exists());
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_sends_no_object_from_forged_commit_graph() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        let published = assert_published_exact(&git, &pushed, false);
        assert_graph_forge_in_effect(&git, &pushed, &published);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_sends_no_object_from_forged_commit_graph() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, true, &tail);

        let published = assert_published_exact(&git, &pushed, true);
        assert_graph_forge_in_effect(&git, &pushed, &published);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    #[test]
    #[cfg(unix)]
    fn test_push_sends_no_object_from_forged_split_commit_graph() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_SPLIT_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        let published = assert_published_exact(&git, &pushed, false);
        let info = pushed.checkout.join(".git/objects/info");
        assert!(info.join("commit-graphs/commit-graph-chain").exists());
        assert!(!info.join("commit-graph").exists());
        assert_graph_forge_in_effect(&git, &pushed, &published);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    #[test]
    #[cfg(unix)]
    fn test_push_sends_no_object_from_forged_alternate_commit_graph() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_ALTERNATE_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        let published = assert_published_exact(&git, &pushed, false);
        let dot_git = pushed.checkout.join(".git");
        assert!(dot_git.join("objects/info/alternates").exists());
        assert!(!dot_git.join("objects/info/commit-graph").exists());
        assert!(
            dot_git
                .join("ude-test-alt/objects/info/commit-graph")
                .exists()
        );
        assert_graph_forge_in_effect(&git, &pushed, &published);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    /// A pack bitmap built under a graft is what is left in the cache checkout, and the
    /// grafts file is gone, so neither the refusal nor a graft at push time explains the
    /// result.
    #[cfg(unix)]
    fn assert_pack_bitmap_without_grafts(pushed: &HookedPush) {
        let dot_git = pushed.checkout.join(".git");
        assert!(!dot_git.join("info/grafts").exists());
        assert!(dir_has_file(
            &dot_git.join("objects/pack"),
            "pack-",
            ".bitmap"
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_sends_no_object_from_graft_built_bitmap() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_BITMAP_AFTER_GRAFT);

        let published = assert_published_exact(&git, &pushed, false);
        assert_pack_bitmap_without_grafts(&pushed);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_sends_no_object_from_graft_built_bitmap() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_BITMAP_AFTER_GRAFT);

        let published = assert_published_exact(&git, &pushed, true);
        assert_pack_bitmap_without_grafts(&pushed);
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    #[test]
    #[cfg(unix)]
    fn test_push_sends_no_object_from_graft_built_multi_pack_index_bitmap() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_MIDX_BITMAP_AFTER_GRAFT);

        let published = assert_published_exact(&git, &pushed, false);
        let dot_git = pushed.checkout.join(".git");
        assert!(!dot_git.join("info/grafts").exists());
        let pack_dir = dot_git.join("objects/pack");
        assert!(dir_has_file(&pack_dir, "multi-pack-index-", ".bitmap"));
        assert!(!dir_has_file(&pack_dir, "pack-", ".bitmap"));
        assert_leak_blocked(&git, &pushed, &published, &[], false);
    }

    /// The options the pushes run with, for the control that shows the variable overrides
    /// them.
    #[cfg(unix)]
    const PUSH_HISTORY_OPTIONS: [&str; 4] = [
        "-c",
        "core.commitGraph=false",
        "-c",
        "pack.useBitmaps=false",
    ];

    /// An engine whose every git command inherits `GIT_TEST_COMMIT_GRAPH=1`.
    #[cfg(unix)]
    fn engine_with_test_commit_graph(git: &GitFixture) -> SyncEngine {
        let mut env = git.gitconfig_env();
        env.push(("GIT_TEST_COMMIT_GRAPH".into(), "1".into()));
        SyncEngine::new().with_extra_env(env)
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_inherited_git_test_commit_graph() {
        let git = GitFixture::new();
        let engine = engine_with_test_commit_graph(&git);
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, false, &tail);

        let published = assert_published_exact(&git, &pushed, false);
        assert_leak_blocked(&git, &pushed, &published, &PUSH_HISTORY_OPTIONS, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_inherited_git_test_commit_graph() {
        let git = GitFixture::new();
        let engine = engine_with_test_commit_graph(&git);
        let (_forge_dir, forge) = forge_script();
        let tail = format!("{forge}{HOOK_FORGED_COMMIT_GRAPH}");
        let pushed = push_with_post_commit_hook(&git, &engine, true, &tail);

        let published = assert_published_exact(&git, &pushed, true);
        assert_leak_blocked(&git, &pushed, &published, &PUSH_HISTORY_OPTIONS, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_push_from_clone_of_gc_remote_publishes() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        git.run(bare.path(), &["gc", "-q"]);
        assert!(bare.path().join("objects/info/commit-graph").exists());
        assert!(dir_has_file(
            &bare.path().join("objects/pack"),
            "",
            ".bitmap"
        ));

        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let dot_git = cached.checkout_dir.join(".git");
        assert!(dot_git.join("objects/info/commit-graph").exists());
        assert!(dir_has_file(&dot_git.join("objects/pack"), "", ".bitmap"));

        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            std::slice::from_ref(&managed),
            &validated,
        );

        assert!(res.is_ok(), "{res:?}");
        let main = git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        assert_ne!(main, cached.fetched_sha, "remote main did not advance");
        let blob = git.checked(
            bare.path(),
            &[
                "show",
                "refs/heads/main:templates/projects/generic/gitconfig.repo",
            ],
        );
        assert_eq!(blob.stdout, HOOKED_TEMPLATE.as_bytes());
    }

    #[test]
    fn test_publication_gate_allows_line_ending_normalization() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, file) = gate_repo(&git);
        let crlf = "[core]\r\n\tx = 1\r\n";
        fs::write(&file, crlf).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));
        let info_dir = repo.path().join(".git/info");
        fs::create_dir_all(&info_dir).unwrap();
        fs::write(
            info_dir.join("attributes"),
            format!("{GATE_REL} text eol=lf\n"),
        )
        .unwrap();

        let staged = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect("CRLF-to-LF normalization must still publish");
        assert!(staged.is_some(), "{staged:?}");
        // The stored blob differs from the working-tree bytes, so the filter ran and
        // the pass is the fingerprint's CRLF tolerance.
        let stored = git
            .checked(repo.path(), &["show", &format!(":{GATE_REL}")])
            .stdout;
        assert_eq!(stored, b"[core]\n\tx = 1\n");
        assert_ne!(stored, crlf.as_bytes());
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
            .stage_publication(
                &checkout_target(not_repo.path(), ""),
                std::slice::from_ref(&edited),
                &ValidatedBlobs::default(),
            )
            .expect_err("a failed git status must be an error");
        assert!(err.contains("git status"), "{err}");
        let pushed = engine.push_to_remote(
            &PublishTarget {
                checkout: not_repo.path(),
                remote_url: NO_REMOTE_URL,
                branch: "main",
                fetched_sha: "",
            },
            false,
            &[edited],
            &ValidatedBlobs::default(),
        );
        assert!(
            matches!(pushed, Err(PushRemoteError::Failed(_))),
            "{pushed:?}"
        );

        // Negative control: in a real checkout a clean tree whose HEAD (the fetched head)
        // holds the record is "nothing to push", and an edit is staged.
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let tracked = repo.path().join(rel);
        fs::create_dir_all(tracked.parent().unwrap()).unwrap();
        fs::write(&tracked, "[core]\n").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "seed"]);
        let head = git.output(repo.path(), &["rev-parse", "HEAD"]);
        assert_eq!(
            engine.stage_publication(
                &checkout_target(repo.path(), &head),
                std::slice::from_ref(&tracked),
                &ValidatedBlobs::from_files(std::slice::from_ref(&tracked))
            ),
            Ok(None)
        );
        fs::write(&tracked, "[core]\n  autocrlf = false\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&tracked));
        let staged = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&tracked),
                &validated,
            )
            .unwrap()
            .expect("the edit must be staged");
        assert!(staged.paths.contains(rel), "{staged:?}");
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
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, content).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            &[managed],
            &validated,
        )
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
    #[cfg(unix)]
    fn test_direct_push_shallow_file_refusal_does_not_say_upstream_moved() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_SHALLOW_NAMES_COMMIT);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("still at the commit UDE fetched"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_with_shallow_file_publishes_nothing() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_SHALLOW_NAMES_COMMIT);

        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(err.contains("git push failed for branch"), "{err}");
    }

    /// Hook tail: builds a child of UDE's commit holding an injected blob under the managed
    /// path and points the ref `<prefix><id of UDE's commit>` at it. Leaves the injected
    /// blob, UDE's commit and the child in `.git/ude-test-injected-blob`,
    /// `.git/ude-test-validated` and `.git/ude-test-shadow-target`.
    #[cfg(unix)]
    fn shadow_ref_tail(prefix: &str) -> String {
        format!(
            r#"C=$(git rev-parse HEAD)
IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree HEAD
git update-index --add --cacheinfo 100644,"$IB","$F"
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p "$C" -m injected)
printf '%s\n' "$IB" > .git/ude-test-injected-blob
printf '%s\n' "$C" > .git/ude-test-validated
printf '%s\n' "$N" > .git/ude-test-shadow-target
git update-ref "{prefix}$C" "$N"
"#
        )
    }

    /// The hook ran, the push succeeded, the object recorded in `.git/<marker>` is not on
    /// the remote, and the published commit is the one recorded in `.git/ude-test-validated`
    /// with exactly the validated bytes. Returns that commit.
    #[cfg(unix)]
    fn assert_shadow_ignored(
        git: &GitFixture,
        pushed: &HookedPush,
        open_pr: bool,
        marker: &str,
    ) -> String {
        assert!(pushed.hook_ran, "the hook never ran");
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let object = hook_recorded_id(pushed, marker);
        assert!(
            !holds_object(git, pushed.bare.path(), &object),
            "the remote holds the object recorded in {marker}"
        );
        let validated = hook_recorded_id(pushed, "ude-test-validated");
        let published = assert_published_exact(git, pushed, open_pr);
        assert_eq!(
            published, validated,
            "the push published another commit than the validated one"
        );
        validated
    }

    /// A plain push of the validated id from the cache checkout to a fresh bare repository
    /// delivers the object recorded in `.git/<marker>`, so the ref the hook left was really
    /// in effect.
    #[cfg(unix)]
    fn assert_plain_id_delivers(git: &GitFixture, pushed: &HookedPush, marker: &str) {
        let validated = hook_recorded_id(pushed, "ude-test-validated");
        let object = hook_recorded_id(pushed, marker);
        let control = push_to_fresh_bare(git, &pushed.checkout, &validated, &[], false);
        assert!(
            holds_object(git, control.path(), &object),
            "a plain push of the validated id did not deliver the object recorded in {marker}, so the hook's ref had no effect"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_tag_named_after_the_commit() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed =
            push_with_post_commit_hook(&git, &engine, false, &shadow_ref_tail("refs/tags/"));
        assert_shadow_ignored(&git, &pushed, false, "ude-test-injected-blob");
        assert_plain_id_delivers(&git, &pushed, "ude-test-injected-blob");
    }

    /// A post-commit hook points `<prefix><id>` at a child holding an injected blob; the
    /// push publishes the validated commit and a plain push of the id delivers the blob.
    #[cfg(unix)]
    fn assert_shadow_ref_ignored(open_pr: bool, prefix: &str) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &shadow_ref_tail(prefix));
        assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-injected-blob");
        assert_plain_id_delivers(&git, &pushed, "ude-test-injected-blob");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_tag_named_after_the_commit() {
        assert_shadow_ref_ignored(true, "refs/tags/");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_branch_named_after_the_commit() {
        assert_shadow_ref_ignored(false, "refs/heads/");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_branch_named_after_the_commit() {
        assert_shadow_ref_ignored(true, "refs/heads/");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_remote_ref_named_after_the_commit() {
        assert_shadow_ref_ignored(false, "refs/remotes/");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_remote_ref_named_after_the_commit() {
        assert_shadow_ref_ignored(true, "refs/remotes/");
    }

    /// `refs/<id>` does not shadow even a plain id (probed), so there is no delivering
    /// control; the checkout still has the ref, so the hook really made it.
    #[cfg(unix)]
    fn assert_top_level_ref_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &shadow_ref_tail("refs/"));
        let validated = assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-injected-blob");
        assert_eq!(
            git.output(
                &pushed.checkout,
                &["rev-parse", "--verify", &format!("refs/{validated}")]
            ),
            hook_recorded_id(&pushed, "ude-test-shadow-target"),
            "the hook's ref is missing"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_top_level_ref_named_after_the_commit() {
        assert_top_level_ref_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_top_level_ref_named_after_the_commit() {
        assert_top_level_ref_ignored(true);
    }

    /// Hook tail: writes a loose ref file whose name is the push's source spelled out,
    /// `refs/tags/<id>^{commit}`, holding a child with an injected blob.
    #[cfg(unix)]
    const HOOK_REF_FILE_SPELLED_LIKE_SOURCE: &str = r#"C=$(git rev-parse HEAD)
IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree HEAD
git update-index --add --cacheinfo 100644,"$IB","$F"
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p "$C" -m injected)
printf '%s\n' "$IB" > .git/ude-test-injected-blob
printf '%s\n' "$C" > .git/ude-test-validated
mkdir -p .git/refs/tags
printf '%s\n' "$N" > ".git/refs/tags/$C^{commit}"
"#;

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_ref_file_spelled_like_the_source() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed =
            push_with_post_commit_hook(&git, &engine, false, HOOK_REF_FILE_SPELLED_LIKE_SOURCE);
        let validated = assert_shadow_ignored(&git, &pushed, false, "ude-test-injected-blob");
        assert!(
            pushed
                .checkout
                .join(format!(".git/refs/tags/{validated}^{{commit}}"))
                .is_file(),
            "the hook's ref file is missing"
        );
    }

    /// The shadow ref's injected blob is a GitHub PAT shape: absent from the remote and from
    /// the published file, delivered by a plain push of the id.
    #[cfg(unix)]
    fn assert_token_behind_shadow_ref_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let tail = with_injected_token(&shadow_ref_tail("refs/tags/"));
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &tail);
        let published = assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-injected-blob");
        let file = remote_managed_blob(&git, &pushed, &published);
        assert!(
            !String::from_utf8_lossy(&file).contains(HOOK_TOKEN_BODY),
            "the published file holds the token"
        );
        assert_plain_id_delivers(&git, &pushed, "ude-test-injected-blob");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_token_behind_shadow_ref() {
        assert_token_behind_shadow_ref_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_token_behind_shadow_ref() {
        assert_token_behind_shadow_ref_ignored(true);
    }

    /// `tail` (which moves HEAD to a rebuilt commit) behind a shadow tag: HEAD is put back on
    /// UDE's commit and `refs/tags/<id>` points at the rebuilt one. Leaves UDE's commit and
    /// the rebuilt one in `.git/ude-test-validated` and `.git/ude-test-shadow-target`.
    #[cfg(unix)]
    fn behind_shadow_tag(tail: &str) -> String {
        format!(
            r#"C=$(git rev-parse HEAD)
printf '%s\n' "$C" > .git/ude-test-validated
{tail}N=$(git rev-parse HEAD)
printf '%s\n' "$N" > .git/ude-test-shadow-target
git update-ref HEAD "$C"
git update-ref "refs/tags/$C" "$N"
"#
        )
    }

    /// A rebuilt commit the gate would refuse, behind a shadow tag, stays off the remote,
    /// and a plain push of the id delivers it.
    #[cfg(unix)]
    fn assert_rebuilt_commit_behind_shadow_ignored(
        open_pr: bool,
        tail: &str,
    ) -> (GitFixture, HookedPush) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &behind_shadow_tag(tail));
        assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-shadow-target");
        assert_plain_id_delivers(&git, &pushed, "ude-test-shadow-target");
        (git, pushed)
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_duplicate_tree_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(false, HOOK_DUPLICATE_ENTRY);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_duplicate_tree_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(true, HOOK_DUPLICATE_ENTRY);
    }

    /// [`assert_rebuilt_commit_behind_shadow_ignored`] with the `.` subtree, and the remote
    /// still clones.
    #[cfg(unix)]
    fn assert_dot_tree_behind_shadow_ignored(open_pr: bool) {
        let (git, pushed) = assert_rebuilt_commit_behind_shadow_ignored(open_pr, HOOK_DOT_SUBTREE);
        let clone = tempdir().unwrap();
        let out = git
            .cmd(clone.path())
            .args(["clone", "-q", pushed.bare.path().to_str().unwrap(), "c"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "cloning the remote failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_dot_tree_behind_shadow_ref() {
        assert_dot_tree_behind_shadow_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_dot_tree_behind_shadow_ref() {
        assert_dot_tree_behind_shadow_ignored(true);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_deleted_file_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(false, HOOK_DELETE_VALIDATED);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_deleted_file_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(true, HOOK_DELETE_VALIDATED);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_reverted_file_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(false, HOOK_REVERT_VALIDATED);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_reverted_file_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(true, HOOK_REVERT_VALIDATED);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_amended_message_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(false, HOOK_AMEND_MESSAGE);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_amended_message_behind_shadow_ref() {
        assert_rebuilt_commit_behind_shadow_ignored(true, HOOK_AMEND_MESSAGE);
    }

    /// A pass-through clean filter that, during `git add`, installs a post-commit hook
    /// running [`shadow_ref_tail`] with `refs/tags/`: the filter and the hook ran, the
    /// validated commit is published, and a plain push of the id delivers the blob.
    #[cfg(unix)]
    fn assert_shadow_ref_left_by_clean_filter_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let hook = format!(
            "#!/bin/sh\n[ -e .git/ude-test-hook-ran ] && exit 0\n: > .git/ude-test-hook-ran\nF='{GATE_REL}'\n{}",
            shadow_ref_tail("refs/tags/")
        );
        let filter = format!(
            r#"cd "$(git rev-parse --show-toplevel)" || exit 1
if [ ! -e .git/ude-test-filter-ran ]; then
  : > .git/ude-test-filter-ran
  mkdir -p .git/hooks
  cat > .git/hooks/post-commit <<'UDE_HOOK'
{hook}UDE_HOOK
  chmod +x .git/hooks/post-commit
fi
cat
"#
        );
        let mut pushed = push_managed_file(&git, &engine, open_pr, HOOKED_TEMPLATE, |repo| {
            set_clean_filter(&git, repo, &filter);
        });
        assert!(
            pushed.checkout.join(".git/ude-test-filter-ran").exists(),
            "the clean filter never ran"
        );
        pushed.hook_ran = pushed.checkout.join(".git/ude-test-hook-ran").exists();
        assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-injected-blob");
        assert_plain_id_delivers(&git, &pushed, "ude-test-injected-blob");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_shadow_ref_left_by_clean_filter() {
        assert_shadow_ref_left_by_clean_filter_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_shadow_ref_left_by_clean_filter() {
        assert_shadow_ref_left_by_clean_filter_ignored(true);
    }

    /// [`HOOK_REPLACE_WITH_PARENTLESS`] was in effect in the cache checkout (the published
    /// commit walks as parentless there) and the remote's raw commit still names the
    /// fetched head as its parent.
    #[cfg(unix)]
    fn assert_parentless_replace_in_effect(git: &GitFixture, pushed: &HookedPush, published: &str) {
        let walked = git.output(
            &pushed.checkout,
            &["rev-list", "--parents", "-1", published],
        );
        assert_eq!(walked.split_whitespace().count(), 1, "{walked}");
        let raw = git.output(
            pushed.bare.path(),
            &["--no-replace-objects", "cat-file", "commit", published],
        );
        assert!(
            raw.lines()
                .any(|line| line == format!("parent {}", pushed.fetched_sha)),
            "{raw}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_validated_commit_past_replace_ref() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_REPLACE_WITH_PARENTLESS);
        let published = assert_published_exact(&git, &pushed, false);
        assert_parentless_replace_in_effect(&git, &pushed, &published);

        // Control: the same source without --no-replace-objects, to a bare clone reset to
        // the fetched head, is refused as not a fast-forward.
        let clone = tempdir().unwrap();
        git.run(
            clone.path(),
            &[
                "clone",
                "--bare",
                "-q",
                pushed.bare.path().to_str().unwrap(),
                ".",
            ],
        );
        git.run(
            clone.path(),
            &["update-ref", "refs/heads/main", &pushed.fetched_sha],
        );
        let out = git
            .cmd(&pushed.checkout)
            .args([
                "push",
                "--no-follow-tags",
                clone.path().to_str().unwrap(),
                &format!("{published}^{{commit}}:refs/heads/main"),
            ])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "the control push succeeded: {stderr}"
        );
        assert!(stderr.contains("non-fast-forward"), "{stderr}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_validated_commit_past_replace_ref() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_REPLACE_WITH_PARENTLESS);
        let published = assert_published_exact(&git, &pushed, true);
        assert_parentless_replace_in_effect(&git, &pushed, &published);
    }

    /// post-commit tail: builds a commit holding an injected blob on UDE's parent, wraps it
    /// in an annotated tag and replaces UDE's commit with that tag, so `<id>^{commit}` peels
    /// to the injected commit wherever replace refs are honoured.
    #[cfg(unix)]
    const HOOK_REPLACE_WITH_TAG: &str = r#"C=$(git rev-parse HEAD)
IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree HEAD
git update-index --add --cacheinfo 100644,"$IB","$F"
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p "$(git rev-parse HEAD~1)" -m injected)
printf '%s\n' "$IB" > .git/ude-test-injected-blob
printf '%s\n' "$C" > .git/ude-test-validated
TG=$(printf 'object %s\ntype commit\ntag x\ntagger t <t@e> 1 +0000\n\nm\n' "$N" | git mktag)
git replace -f "$C" "$TG"
"#;

    /// A replace ref from UDE's commit to a tag holding the injected commit changes nothing
    /// that is published; the same source without `--no-replace-objects` delivers the blob.
    #[cfg(unix)]
    fn assert_replace_ref_to_tag_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, HOOK_REPLACE_WITH_TAG);
        let validated = assert_shadow_ignored(&git, &pushed, open_pr, "ude-test-injected-blob");
        let injected = hook_recorded_id(&pushed, "ude-test-injected-blob");
        let control = push_to_fresh_bare(
            &git,
            &pushed.checkout,
            &format!("{validated}^{{commit}}"),
            &[],
            false,
        );
        assert!(
            holds_object(&git, control.path(), &injected),
            "the control push did not deliver the injected blob, so the replace ref had no effect"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_replace_ref_to_a_tag() {
        assert_replace_ref_to_tag_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_replace_ref_to_a_tag() {
        assert_replace_ref_to_tag_ignored(true);
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
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, "[core]\n  autocrlf = false\n").unwrap();
        let start = std::time::Instant::now();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            false,
            &[managed],
            &validated,
        );
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
        git.run(remote_repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        let clean_check = engine.validate_committed_tree(
            remote_repo.path(),
            &git.output(remote_repo.path(), &["rev-parse", "HEAD"]),
            &base_head_sha,
            &expected_set,
            &validated,
            &commit_tree_id(&git, remote_repo.path(), "HEAD"),
        );
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
        git.run(remote_repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);

        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        let secret_check = engine.validate_committed_tree(
            remote_repo.path(),
            &git.output(remote_repo.path(), &["rev-parse", "HEAD"]),
            &git.output(remote_repo.path(), &["rev-parse", "HEAD~1"]),
            &expected_set,
            &validated,
            &commit_tree_id(&git, remote_repo.path(), "HEAD"),
        );
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

    /// Commits `bytes` as base.txt on top of an initial commit and returns the
    /// validate_committed_tree verdict for that HEAD.
    fn validate_committed_base_bytes(bytes: &[u8]) -> Result<(), String> {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();
        git.run(remote_repo.path(), &["init", "-b", "main"]);
        let base_file = remote_repo.path().join("base.txt");
        fs::write(&base_file, "initial").unwrap();
        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);
        fs::write(&base_file, bytes).unwrap();
        git.run(remote_repo.path(), &["add", "base.txt"]);
        git.run(remote_repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let expected_set: std::collections::HashSet<PathBuf> =
            [PathBuf::from("base.txt")].into_iter().collect();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        engine.validate_committed_tree(
            remote_repo.path(),
            &git.output(remote_repo.path(), &["rev-parse", "HEAD"]),
            &git.output(remote_repo.path(), &["rev-parse", "HEAD~1"]),
            &expected_set,
            &validated,
            &commit_tree_id(&git, remote_repo.path(), "HEAD"),
        )
    }

    #[test]
    fn test_post_commit_gate_scans_raw_blob_bytes() {
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let mut clean = vec![0xFF, 0xFE, b'\n'];
        clean.extend_from_slice("x=é".as_bytes());
        let mut dirty = clean.clone();
        dirty.extend_from_slice(format!("{prefix}{body}\n").as_bytes());
        clean.push(b'\n');

        let err = validate_committed_base_bytes(&dirty).expect_err("committed token must fail");
        assert!(
            err.contains("secret detected in committed HEAD blob"),
            "{err}"
        );
        assert!(err.contains("GitHub Personal Access Token"), "{err}");
        assert!(!err.contains(body), "gate error leaked the token body");

        // Negative control: the same bytes without the token validate.
        assert_eq!(validate_committed_base_bytes(&clean), Ok(()));
    }

    #[test]
    fn test_post_commit_gate_scans_utf16_blob() {
        let utf16be_bom = |text: &str| {
            let mut out = vec![0xFE, 0xFF];
            for unit in text.encode_utf16() {
                out.extend_from_slice(&unit.to_be_bytes());
            }
            out
        };
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let dirty = utf16be_bom(&format!("token = {prefix}{body}\n"));

        let err = validate_committed_base_bytes(&dirty).expect_err("UTF-16 token must fail");
        assert!(
            err.contains("secret detected in committed HEAD blob"),
            "{err}"
        );
        assert!(err.contains("GitHub Personal Access Token"), "{err}");
        assert!(!err.contains(body), "gate error leaked the token body");

        // Negative control: benign UTF-16BE validates.
        assert_eq!(
            validate_committed_base_bytes(&utf16be_bom("benign words\n")),
            Ok(())
        );
    }

    #[test]
    fn test_post_commit_gate_blocks_gitleaks_only_category() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let remote_repo = tempdir().unwrap();
        git.run(remote_repo.path(), &["init", "-b", "main"]);
        let base_file = remote_repo.path().join("base.txt");
        fs::write(&base_file, "initial").unwrap();
        git.run(remote_repo.path(), &["add", "-A"]);
        git.run(remote_repo.path(), &["commit", "-m", "base commit"]);
        let expected_set: std::collections::HashSet<PathBuf> =
            [PathBuf::from("base.txt")].into_iter().collect();

        // Negative control: a clean commit validates.
        fs::write(&base_file, "updated benign content\n").unwrap();
        git.run(remote_repo.path(), &["add", "base.txt"]);
        git.run(remote_repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        assert_eq!(
            engine.validate_committed_tree(
                remote_repo.path(),
                &git.output(remote_repo.path(), &["rev-parse", "HEAD"]),
                &git.output(remote_repo.path(), &["rev-parse", "HEAD~1"]),
                &expected_set,
                &validated,
                &commit_tree_id(&git, remote_repo.path(), "HEAD"),
            ),
            Ok(())
        );

        let tok = seeded_slack_bot_token();
        fs::write(&base_file, format!("notify = {tok}\n")).unwrap();
        git.run(remote_repo.path(), &["add", "base.txt"]);
        git.run(remote_repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&base_file));
        let err = engine
            .validate_committed_tree(
                remote_repo.path(),
                &git.output(remote_repo.path(), &["rev-parse", "HEAD"]),
                &git.output(remote_repo.path(), &["rev-parse", "HEAD~1"]),
                &expected_set,
                &validated,
                &commit_tree_id(&git, remote_repo.path(), "HEAD"),
            )
            .expect_err("a committed Slack bot token must fail the gate");
        assert!(
            err.contains("secret detected in committed HEAD blob"),
            "{err}"
        );
        assert!(err.contains("slack-bot-token"), "{err}");
        assert!(!err.contains(&tok), "gate error leaked the token");

        engine.rollback_commit(remote_repo.path());
        let head_content = git.output(remote_repo.path(), &["show", "HEAD:base.txt"]);
        assert!(
            !head_content.contains("xoxb-"),
            "HEAD must not hold the token after rollback: {head_content}"
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

    /// The hook ran, nothing reached the remote, and the push failed hard with the
    /// committed gate's refusal holding none of the hook's text. Returns the refusal.
    #[cfg(unix)]
    fn assert_hook_refused(git: &GitFixture, pushed: &HookedPush) -> String {
        assert!(pushed.hook_ran, "the hook never ran");
        assert_remote_unchanged(git, pushed);
        let err = match &pushed.res {
            Err(PushRemoteError::Failed(err)) => err.clone(),
            other => panic!("expected a hard push failure, got {other:?}"),
        };
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(!err.contains("INJECTED"), "{err}");
        err
    }

    /// The injected blob did not reach the remote, and the commit the hook built delivers
    /// it to a fresh bare repository, so the hook's tree really carried it.
    #[cfg(unix)]
    fn assert_injected_blob_not_delivered(git: &GitFixture, pushed: &HookedPush) {
        let blob = hook_recorded_id(pushed, "ude-test-injected-blob");
        let rebuilt = hook_recorded_id(pushed, "ude-test-rebuilt-commit");
        assert!(
            !holds_object(git, pushed.bare.path(), &blob),
            "the remote holds the injected blob"
        );
        let control = push_to_fresh_bare(git, &pushed.checkout, &rebuilt, &[], false);
        assert!(
            holds_object(git, control.path(), &blob),
            "a plain push of the hook's commit did not deliver the injected blob"
        );
    }

    /// Pushes with `tail` as a post-commit hook and requires the refusal.
    #[cfg(unix)]
    fn post_commit_hook_refusal(open_pr: bool, tail: &str) -> (GitFixture, HookedPush, String) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, tail);
        let err = assert_hook_refused(&git, &pushed);
        (git, pushed, err)
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_duplicate_entry() {
        let (git, pushed, _) = post_commit_hook_refusal(false, HOOK_DUPLICATE_ENTRY);
        assert_injected_blob_not_delivered(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_duplicate_entry() {
        let (git, pushed, _) = post_commit_hook_refusal(true, HOOK_DUPLICATE_ENTRY);
        assert_injected_blob_not_delivered(&git, &pushed);
    }

    /// The body of the GitHub PAT shape the hook tails below carry, joined at runtime so no
    /// token literal is committed.
    #[cfg(unix)]
    const HOOK_TOKEN_BODY: &str = "111122223333444455556666777788889999";

    /// `tail` with its injected blob's text replaced by a GitHub PAT shape.
    #[cfg(unix)]
    fn with_injected_token(tail: &str) -> String {
        let prefix = "ghp_";
        tail.replace("INJECTED_BY_HOOK", &format!("{prefix}{HOOK_TOKEN_BODY}"))
    }

    /// A token-shaped second entry is refused, its body is not printed, the checkout is
    /// back at the fetched head, and the token blob did not reach the remote.
    #[cfg(unix)]
    fn assert_hook_duplicate_token_refused(open_pr: bool) {
        let tail = with_injected_token(HOOK_DUPLICATE_ENTRY);
        let (git, pushed, err) = post_commit_hook_refusal(open_pr, &tail);
        assert!(
            !err.contains(HOOK_TOKEN_BODY),
            "the refusal leaked the token"
        );
        assert_eq!(
            git.output(&pushed.checkout, &["rev-parse", "HEAD"]),
            pushed.fetched_sha,
            "the cache commit was not rolled back"
        );
        assert_injected_blob_not_delivered(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_duplicate_token_entry() {
        assert_hook_duplicate_token_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_duplicate_token_entry() {
        assert_hook_duplicate_token_refused(true);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_duplicate_directory() {
        let (git, pushed, _) = post_commit_hook_refusal(false, HOOK_DUPLICATE_DIRECTORY);
        assert_injected_blob_not_delivered(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_duplicate_directory() {
        let (git, pushed, _) = post_commit_hook_refusal(true, HOOK_DUPLICATE_DIRECTORY);
        assert_injected_blob_not_delivered(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_repeated_same_blob() {
        post_commit_hook_refusal(false, HOOK_REPEATED_SAME_BLOB);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_repeated_same_blob() {
        post_commit_hook_refusal(true, HOOK_REPEATED_SAME_BLOB);
    }

    /// A subtree named `.` is refused, and the commit the hook built, delivered to a fresh
    /// bare repository, cannot be cloned from it.
    #[cfg(unix)]
    fn assert_hook_dot_subtree_refused(open_pr: bool) {
        let (git, pushed, _) = post_commit_hook_refusal(open_pr, HOOK_DOT_SUBTREE);
        let rebuilt = hook_recorded_id(&pushed, "ude-test-rebuilt-commit");
        let control = push_to_fresh_bare(&git, &pushed.checkout, &rebuilt, &[], false);
        let clone_parent = tempdir().unwrap();
        let clone = git
            .cmd(clone_parent.path())
            .env("LC_ALL", "C")
            .args([
                "clone",
                "-b",
                "control",
                control.path().to_str().unwrap(),
                "clone",
            ])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&clone.stderr);
        assert!(!clone.status.success(), "the clone succeeded: {stderr}");
        assert!(stderr.contains("invalid path"), "{stderr}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_dot_subtree() {
        assert_hook_dot_subtree_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_dot_subtree() {
        assert_hook_dot_subtree_refused(true);
    }

    /// A commit that lost the managed file is refused, and the hook's commit really lacks it.
    #[cfg(unix)]
    fn assert_hook_deleted_file_refused(open_pr: bool) {
        let (git, pushed, _) = post_commit_hook_refusal(open_pr, HOOK_DELETE_VALIDATED);
        let rebuilt = hook_recorded_id(&pushed, "ude-test-rebuilt-commit");
        let listed = git.output(
            &pushed.checkout,
            &["ls-tree", "-r", "--name-only", &rebuilt],
        );
        assert!(!listed.contains(&pushed.managed_rel), "{listed}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_deleted_file() {
        assert_hook_deleted_file_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_deleted_file() {
        assert_hook_deleted_file_refused(true);
    }

    /// A commit holding the fetched content of the managed file is refused, and the hook's
    /// commit really holds that content.
    #[cfg(unix)]
    fn assert_hook_reverted_file_refused(open_pr: bool) {
        let (git, pushed, _) = post_commit_hook_refusal(open_pr, HOOK_REVERT_VALIDATED);
        let rebuilt = hook_recorded_id(&pushed, "ude-test-rebuilt-commit");
        let held = git.output(
            &pushed.checkout,
            &["show", &format!("{rebuilt}:{}", pushed.managed_rel)],
        );
        assert!(held.contains("autocrlf"), "{held}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_reverted_file() {
        assert_hook_reverted_file_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_reverted_file() {
        assert_hook_reverted_file_refused(true);
    }

    /// A commit whose only difference is the managed file's mode is refused as a changed
    /// tree, and the hook's commit really has that mode.
    #[cfg(unix)]
    fn assert_hook_exec_flip_refused(open_pr: bool) {
        let (git, pushed, err) = post_commit_hook_refusal(open_pr, HOOK_EXEC_FLIP);
        assert!(err.contains("is not the tree UDE staged"), "{err}");
        let rebuilt = hook_recorded_id(&pushed, "ude-test-rebuilt-commit");
        let listed = git.output(
            &pushed.checkout,
            &["ls-tree", &rebuilt, &pushed.managed_rel],
        );
        assert!(listed.starts_with("100755"), "{listed}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_exec_flip() {
        assert_hook_exec_flip_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_exec_flip() {
        assert_hook_exec_flip_refused(true);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_post_checkout_index_change() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "post-checkout", HOOK_CHECKOUT_REMOVE);
        assert_hook_refused(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_content_kept_amend() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, false, HOOK_KEPT_AMEND);
        assert_published_exact(&git, &pushed, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_content_kept_amend() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, true, HOOK_KEPT_AMEND);
        assert_published_exact(&git, &pushed, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_benign_post_checkout_hook() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_hook(&git, &engine, true, "post-checkout", "");
        assert_published_exact(&git, &pushed, true);
    }

    /// Publishes `content` over the seeded template of a fresh bare remote with no hook;
    /// `setup` prepares the cache checkout first. `hook_ran` is false.
    #[cfg(unix)]
    fn push_managed_file(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        content: &str,
        setup: impl FnOnce(&Path),
    ) -> HookedPush {
        let bare = seeded_bare_remote(git);
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let checkout = cached.checkout_dir.clone();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        let managed_rel = managed
            .strip_prefix(&checkout)
            .unwrap()
            .display()
            .to_string();
        fs::write(&managed, content).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        setup(&checkout);
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            open_pr,
            std::slice::from_ref(&managed),
            &validated,
        );
        let fetched_sha = cached.fetched_sha.clone();
        drop(cached);
        HookedPush {
            res,
            bare,
            _cache: cache,
            checkout,
            fetched_sha,
            managed_rel,
            hook_ran: false,
        }
    }

    /// A CRLF working file under a `text` attribute publishes, and its blob is stored LF.
    #[cfg(unix)]
    fn assert_crlf_under_text_attribute_publishes(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_managed_file(&git, &engine, open_pr, "[core]\r\n\tx = 1\r\n", |repo| {
            fs::write(repo.join(".git/info/attributes"), "*.repo text\n").unwrap();
        });
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let published = published_commit(&git, &pushed, open_pr);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &published),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_crlf_file_under_text_attribute() {
        assert_crlf_under_text_attribute_publishes(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_crlf_file_under_text_attribute() {
        assert_crlf_under_text_attribute_publishes(true);
    }

    /// A pass-through clean filter publishes the validated bytes.
    #[cfg(unix)]
    fn assert_pass_through_clean_filter_publishes(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_managed_file(&git, &engine, open_pr, HOOKED_TEMPLATE, |repo| {
            set_clean_filter(&git, repo, "cat\n");
        });
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let published = published_commit(&git, &pushed, open_pr);
        assert_eq!(
            remote_managed_blob(&git, &pushed, &published),
            HOOKED_TEMPLATE.as_bytes()
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_publishes_with_pass_through_clean_filter() {
        assert_pass_through_clean_filter_publishes(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_with_pass_through_clean_filter() {
        assert_pass_through_clean_filter_publishes(true);
    }

    /// A clean filter that rewrites the blob is refused at the staged gate.
    #[cfg(unix)]
    fn assert_rewriting_clean_filter_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_managed_file(&git, &engine, open_pr, HOOKED_TEMPLATE, |repo| {
            set_clean_filter(&git, repo, "sed s/x/INJECTED/\n");
        });
        assert_remote_unchanged(&git, &pushed);
        let err = push_failure(pushed.res);
        assert!(
            err.contains("Publication gate violation: staged index blob"),
            "{err}"
        );
        assert!(err.contains("differs from the validated content"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_rewriting_clean_filter() {
        assert_rewriting_clean_filter_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_rewriting_clean_filter() {
        assert_rewriting_clean_filter_refused(true);
    }

    /// The id of the tree of `commit`, read the way the committed gate reads it.
    fn commit_tree_id(git: &GitFixture, repo: &Path, commit: &str) -> String {
        git.output(
            repo,
            &[
                "--no-replace-objects",
                "rev-parse",
                &format!("{commit}^{{tree}}"),
            ],
        )
    }

    #[test]
    fn test_raw_commit_tree() {
        let id = |digit: char| digit.to_string().repeat(40);
        let (tree, other) = (id('0'), id('1'));
        let people = "author a <a@x> 1 +0000\ncommitter c <c@x> 1 +0000\n";
        let rows = vec![
            (
                "the first line",
                format!("tree {tree}\nparent {other}\n{people}\nmsg\n").into_bytes(),
                Some(tree.clone().into_bytes()),
            ),
            (
                "a second tree line in the header",
                format!("tree {tree}\ntree {other}\n{people}\nmsg\n").into_bytes(),
                Some(tree.clone().into_bytes()),
            ),
            (
                "a first line that is no tree line, with one below it",
                format!("parent {other}\ntree {tree}\n{people}\nmsg\n").into_bytes(),
                None,
            ),
            (
                "only the message starts with tree",
                format!("{people}\ntree {tree}\n").into_bytes(),
                None,
            ),
            (
                "bytes that are not UTF-8 stay as they are",
                vec![b't', b'r', b'e', b'e', b' ', 0xFF, 0xFE, b'\n'],
                Some(vec![0xFF, 0xFE]),
            ),
            ("empty input", Vec::new(), None),
        ];
        let mut failures = Vec::new();
        for (name, raw, expected) in rows {
            let actual = raw_commit_tree(&raw).map(<[u8]>::to_vec);
            if actual != expected {
                failures.push(format!("{name}: {actual:?} != {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    proptest::proptest! {
        /// `tree <id>` counts only on the first line, whatever bytes follow it, and a raw
        /// object a hook wrote never panics the read.
        #[test]
        fn prop_raw_commit_tree_reads_only_the_first_line(
            id in "[0-9a-f]{40}|[0-9a-f]{64}",
            tail in proptest::collection::vec(proptest::num::u8::ANY, 0..64),
            random in proptest::collection::vec(proptest::num::u8::ANY, 0..256),
        ) {
            let mut first = format!("tree {id}\n").into_bytes();
            first.extend_from_slice(&tail);
            proptest::prop_assert_eq!(raw_commit_tree(&first), Some(id.as_bytes()));

            let mut second = format!("parent {id}\ntree {id}\n").into_bytes();
            second.extend_from_slice(&tail);
            proptest::prop_assert_eq!(raw_commit_tree(&second), None);

            let _ = raw_commit_tree(&random);
        }
    }

    #[test]
    fn test_committed_gate_requires_the_staged_tree() {
        const REFUSAL: &str = "is not the tree UDE staged";
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        let file = repo.path().join("base.txt");
        fs::write(&file, "initial").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "base commit"]);
        let base = git.output(repo.path(), &["rev-parse", "HEAD"]);
        let base_tree = commit_tree_id(&git, repo.path(), "HEAD");
        fs::write(&file, "updated\n").unwrap();
        git.run(repo.path(), &["add", "base.txt"]);
        git.run(repo.path(), &["commit", "-m", SYNC_COMMIT_MESSAGE]);
        let head = git.output(repo.path(), &["rev-parse", "HEAD"]);
        let tree = commit_tree_id(&git, repo.path(), "HEAD");
        let paths: HashSet<PathBuf> = [PathBuf::from("base.txt")].into_iter().collect();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));

        let rows: [(&str, &str, Option<&str>); 4] = [
            ("the commit's own tree", &tree, None),
            ("another tree", &base_tree, Some(REFUSAL)),
            ("first 12 characters", &tree[..12], Some(REFUSAL)),
            ("empty", "", Some(REFUSAL)),
        ];
        let mut failures = Vec::new();
        for (name, recorded, expected) in rows {
            let verdict = engine.validate_committed_tree(
                repo.path(),
                &head,
                &base,
                &paths,
                &validated,
                recorded,
            );
            let as_expected = match (&verdict, expected) {
                (Ok(()), None) => true,
                (Err(err), Some(text)) => err.contains(text),
                _ => false,
            };
            if !as_expected {
                failures.push(format!("{name}: {verdict:?}, expected {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_stage_publication_records_the_staged_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, file) = gate_repo(&git);
        fs::write(&file, "[core]\n\tx = 1\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));
        let staged = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .unwrap()
            .expect("the edit must be staged");
        assert_eq!(staged.tree, git.output(repo.path(), &["write-tree"]));
        assert_ne!(staged.tree, commit_tree_id(&git, repo.path(), "HEAD"));
    }

    #[test]
    #[cfg(unix)]
    fn test_stage_publication_refuses_when_the_tree_cannot_be_recorded() {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let (repo, file) = gate_repo(&git);
        fs::write(&file, "[core]\n\tx = 1\n").unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));

        let inherited = std::env::var_os("PATH").unwrap();
        let real_git = std::env::split_paths(&inherited)
            .map(|dir| dir.join("git"))
            .find(|candidate| candidate.is_file())
            .expect("git on PATH");
        let shim_dir = tempdir().unwrap();
        let shim = shim_dir.path().join("git");
        fs::write(
            &shim,
            format!(
                "#!/bin/sh\nfor arg in \"$@\"; do\n  [ \"$arg\" = write-tree ] && exit 1\ndone\nexec '{}' \"$@\"\n",
                real_git.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(
            std::iter::once(shim_dir.path().to_path_buf()).chain(std::env::split_paths(&inherited)),
        )
        .unwrap();
        let mut env = git.gitconfig_env();
        env.push(("PATH".into(), path));
        let engine = SyncEngine::new().with_extra_env(env);

        let err = engine
            .stage_publication(
                &checkout_target(repo.path(), ""),
                std::slice::from_ref(&file),
                &validated,
            )
            .expect_err("an unrecorded tree must refuse");
        assert!(err.contains("cannot record the staged tree"), "{err}");
        assert_eq!(
            git.output(repo.path(), &["diff", "--cached", "--name-only"]),
            ""
        );
    }

    /// Pushes with `tail` as a post-index-change hook and requires the refusal.
    #[cfg(unix)]
    fn index_hook_refusal(open_pr: bool, tail: &str) -> (GitFixture, HookedPush, String) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_index_hook(&git, &engine, open_pr, tail);
        let err = assert_hook_refused(&git, &pushed);
        (git, pushed, err)
    }

    /// The injected entry the staged gate cannot see is refused, and its blob stays off
    /// the remote. The tree lists the managed file three times, so it is refused for the
    /// repeat before the injected blob is read.
    #[cfg(unix)]
    fn assert_index_hook_hidden_blob_refused(open_pr: bool) {
        let (git, pushed, err) = index_hook_refusal(open_pr, HOOK_INDEX_HIDDEN_BLOB);
        assert!(err.contains("repeats a path"), "{err}");
        let blob = hook_recorded_id(&pushed, "ude-test-injected-blob");
        assert!(
            !holds_object(&git, pushed.bare.path(), &blob),
            "the remote holds the injected blob"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_index_hook_hidden_blob() {
        assert_index_hook_hidden_blob_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_index_hook_hidden_blob() {
        assert_index_hook_hidden_blob_refused(true);
    }

    /// A token-shaped entry the staged gate cannot see is refused, and its body is not
    /// printed. The tree lists the managed file three times, so it is refused for the repeat
    /// before the token-shaped blob is read or scanned.
    #[cfg(unix)]
    fn assert_index_hook_hidden_token_refused(open_pr: bool) {
        let tail = with_injected_token(HOOK_INDEX_HIDDEN_BLOB);
        let (_git, _pushed, err) = index_hook_refusal(open_pr, &tail);
        assert!(err.contains("repeats a path"), "{err}");
        assert!(
            !err.contains(HOOK_TOKEN_BODY),
            "the refusal leaked the token"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_index_hook_hidden_token() {
        assert_index_hook_hidden_token_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_index_hook_hidden_token() {
        assert_index_hook_hidden_token_refused(true);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_index_hook_repeated_same_blob() {
        let (_git, _pushed, err) = index_hook_refusal(false, HOOK_INDEX_REPEATED_SAME_BLOB);
        assert!(err.contains("repeats a path"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_index_hook_repeated_same_blob() {
        let (_git, _pushed, err) = index_hook_refusal(true, HOOK_INDEX_REPEATED_SAME_BLOB);
        assert!(err.contains("repeats a path"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_index_hook_removed_file() {
        let (_git, _pushed, err) = index_hook_refusal(false, HOOK_INDEX_REMOVE);
        assert!(err.contains("does not hold the validated content"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_index_hook_removed_file() {
        let (_git, _pushed, err) = index_hook_refusal(true, HOOK_INDEX_REMOVE);
        assert!(err.contains("does not hold the validated content"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_index_hook_symlink() {
        let (_git, _pushed, err) = index_hook_refusal(false, HOOK_INDEX_SYMLINK);
        assert!(err.contains("does not hold the validated content"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_push_refuses_index_hook_empty_directory() {
        let (_git, _pushed, err) = index_hook_refusal(false, HOOK_INDEX_EMPTY_DIRECTORY);
        assert!(err.contains("contains an empty directory"), "{err}");
    }

    /// A mode set before the tree id is recorded reaches the remote with the validated
    /// bytes. This pins present behaviour so a change to it is seen; it endorses nothing.
    #[test]
    #[cfg(unix)]
    fn test_index_hook_exec_flip_reaches_the_remote() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_index_hook(&git, &engine, false, HOOK_INDEX_EXEC_FLIP);
        let published = assert_published_exact(&git, &pushed, false);
        let listed = git.output(
            pushed.bare.path(),
            &["ls-tree", &published, &pushed.managed_rel],
        );
        assert!(listed.starts_with("100755"), "{listed}");
    }

    /// Runs `git <args>` in `repo` with `input` on stdin; returns its trimmed stdout.
    fn git_with_input(git: &GitFixture, repo: &Path, args: &[&str], input: &[u8]) -> String {
        use std::io::Write as _;
        let mut child = git
            .cmd(repo)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "fixture step `git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Writes `bytes` as a blob of `repo` and returns its id.
    fn write_blob(git: &GitFixture, repo: &Path, bytes: &[u8]) -> String {
        git_with_input(git, repo, &["hash-object", "-w", "--stdin"], bytes)
    }

    /// A commit on `base` with the sync message whose root tree holds only `templates/`,
    /// listing `templates_listing` (`git mktree` input).
    fn commit_of_templates(
        git: &GitFixture,
        repo: &Path,
        base: &str,
        templates_listing: &str,
    ) -> String {
        let templates = git_with_input(git, repo, &["mktree"], templates_listing.as_bytes());
        let root = git_with_input(
            git,
            repo,
            &["mktree"],
            format!("040000 tree {templates}\ttemplates\n").as_bytes(),
        );
        git_with_input(
            git,
            repo,
            &["commit-tree", &root, "-p", base],
            format!("{SYNC_COMMIT_MESSAGE}\n").as_bytes(),
        )
    }

    /// A repository with `files` committed under `templates/`; returns it and the commit.
    fn templates_repo(git: &GitFixture, files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let repo = tempdir().unwrap();
        git.run(repo.path(), &["init", "-b", "main"]);
        fs::create_dir_all(repo.path().join("templates")).unwrap();
        for (name, content) in files {
            fs::write(repo.path().join("templates").join(name), content).unwrap();
        }
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-m", "base commit"]);
        let base = git.output(repo.path(), &["rev-parse", "HEAD"]);
        (repo, base)
    }

    /// The record of `entries` (a name under `templates/`, the validated bytes) in `repo`.
    fn templates_record(repo: &Path, entries: &[(&str, &[u8])]) -> ValidatedBlobs {
        let mut validated = ValidatedBlobs::default();
        for (name, bytes) in entries {
            validated.record(repo.join("templates").join(name), bytes);
        }
        validated
    }

    #[test]
    fn test_committed_gate_requires_validated_content() {
        const NOT_HELD: &str = "does not hold the validated content";
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) =
            templates_repo(&git, &[("f", "f base\n"), ("g", "g base\n"), ("h", "h\n")]);
        let dir = repo.path();
        let blob = |bytes: &[u8]| write_blob(&git, dir, bytes);
        let (f_new, g_old, g_new) = (blob(b"f new\n"), blob(b"g base\n"), blob(b"g new\n"));
        let (g_crlf, h) = (blob(b"g new\r\n"), blob(b"h\n"));
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f"), PathBuf::from("templates/g")]
            .into_iter()
            .collect();
        let both = templates_record(dir, &[("f", b"f new\n"), ("g", b"g new\n")]);
        let only_f = templates_record(dir, &[("f", b"f new\n")]);
        let g_kept = templates_record(dir, &[("f", b"f new\n"), ("g", b"g base\n")]);
        let refused_read = "failed to inspect committed blob";

        let rows = [
            (
                "both changed",
                format!("100644 blob {g_new}\tg\n"),
                &both,
                None,
            ),
            ("g deleted", String::new(), &both, Some(NOT_HELD)),
            (
                "g reverted to the base's blob",
                format!("100644 blob {g_old}\tg\n"),
                &both,
                Some(NOT_HELD),
            ),
            (
                "g a symlink entry holding the validated blob",
                format!("120000 blob {g_new}\tg\n"),
                &both,
                Some(NOT_HELD),
            ),
            (
                "g a gitlink",
                format!("160000 commit {base}\tg\n"),
                &both,
                Some(refused_read),
            ),
            (
                "g deleted with no record for g",
                String::new(),
                &only_f,
                None,
            ),
            (
                "g holding the record's content with CRLF line endings",
                format!("100644 blob {g_crlf}\tg\n"),
                &both,
                None,
            ),
            (
                "g with mode 100755",
                format!("100755 blob {g_new}\tg\n"),
                &both,
                None,
            ),
            (
                "a validated path the commit did not change",
                format!("100644 blob {g_old}\tg\n"),
                &g_kept,
                None,
            ),
        ];
        let mut failures = Vec::new();
        for (name, g_entry, validated, expected) in rows {
            let listing = format!("100644 blob {f_new}\tf\n{g_entry}100644 blob {h}\th\n");
            let commit = commit_of_templates(&git, dir, &base, &listing);
            let verdict = engine.validate_committed_tree(
                dir,
                &commit,
                &base,
                &paths,
                validated,
                &commit_tree_id(&git, dir, &commit),
            );
            let as_expected = match (&verdict, expected) {
                (Ok(()), None) => true,
                (Err(err), Some(text)) => err.contains(text),
                _ => false,
            };
            if !as_expected {
                failures.push(format!("{name}: {verdict:?}, expected {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_committed_gate_refuses_bad_path_components() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let validated_blob = write_blob(&git, dir, b"f new\n");
        let subtree = git_with_input(
            &git,
            dir,
            &["mktree"],
            format!("100644 blob {validated_blob}\tf\n").as_bytes(),
        );
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);

        let rows = [
            (".", "path component"),
            ("..", "path component"),
            ("", "unparseable tree entry"),
        ];
        let mut failures = Vec::new();
        for (component, expected) in rows {
            let listing =
                format!("100644 blob {validated_blob}\tf\n040000 tree {subtree}\t{component}\n");
            let commit = commit_of_templates(&git, dir, &base, &listing);
            let verdict = engine.validate_committed_tree(
                dir,
                &commit,
                &base,
                &paths,
                &validated,
                &commit_tree_id(&git, dir, &commit),
            );
            if !matches!(&verdict, Err(err) if err.contains(expected)) {
                failures.push(format!(
                    "subtree named {component:?}: {verdict:?}, expected {expected:?}"
                ));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_committed_gate_reads_blob_past_replace_ref() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let validated_blob = write_blob(&git, dir, b"f new\n");
        let injected_blob = write_blob(&git, dir, b"INJECTED_BY_HOOK\n");
        let listing = format!("100644 blob {injected_blob}\tf\n");
        let commit = commit_of_templates(&git, dir, &base, &listing);
        git.run(dir, &["replace", "-f", &injected_blob, &validated_blob]);
        // Opposite direction: a read honouring replace refs returns the validated bytes.
        assert_eq!(
            git.output(dir, &["cat-file", "blob", &injected_blob]),
            "f new"
        );

        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);
        let err = engine
            .validate_committed_tree(
                dir,
                &commit,
                &base,
                &paths,
                &validated,
                &commit_tree_id(&git, dir, &commit),
            )
            .expect_err("the injected blob must be read past the replace ref");
        assert!(err.contains("differs from the validated content"), "{err}");
    }

    /// `:100644 100644 <id> <id> <status>` NUL `<path>` NUL, the shape `diff-tree -z --raw` prints.
    fn raw_diff_record(id: &str, status: &str, path: &[u8]) -> Vec<u8> {
        let mut record = format!(":100644 100644 {id} {id} {status}\0").into_bytes();
        record.extend_from_slice(path);
        record.push(0);
        record
    }

    #[test]
    fn test_raw_diff_entries() {
        let id = "a".repeat(40);
        let long_id = "b".repeat(64);
        let zero = "0".repeat(40);
        let deletion = format!(":100644 000000 {id} {zero} D\0gone\0").into_bytes();
        let type_change = format!(":100644 120000 {id} {id} T\0link\0").into_bytes();
        let non_utf8: &[u8] = &[b'a', 0xFF, b'\t', b'\n', b'z'];
        let as_tuples = |raw: &[u8]| {
            raw_diff_entries(raw).map(|entries| {
                entries
                    .iter()
                    .map(|e| (e.status, e.new_oid.to_vec(), e.path.to_vec()))
                    .collect::<Vec<_>>()
            })
        };

        let rows = vec![
            ("empty input", Vec::new(), Some(vec![])),
            (
                "modify then add",
                [
                    raw_diff_record(&id, "M", b"a/b"),
                    raw_diff_record(&id, "A", b"c"),
                ]
                .concat(),
                Some(vec![
                    (b'M', id.clone().into_bytes(), b"a/b".to_vec()),
                    (b'A', id.clone().into_bytes(), b"c".to_vec()),
                ]),
            ),
            (
                "a deletion has an all-zero new id",
                deletion,
                Some(vec![(b'D', zero.clone().into_bytes(), b"gone".to_vec())]),
            ),
            (
                "a type change",
                type_change,
                Some(vec![(b'T', id.clone().into_bytes(), b"link".to_vec())]),
            ),
            (
                "a path with a tab, a newline and non-UTF-8 bytes",
                raw_diff_record(&id, "M", non_utf8),
                Some(vec![(b'M', id.clone().into_bytes(), non_utf8.to_vec())]),
            ),
            (
                "a 64-hex id",
                raw_diff_record(&long_id, "M", b"p"),
                Some(vec![(b'M', long_id.clone().into_bytes(), b"p".to_vec())]),
            ),
            (
                "a missing path field",
                format!(":100644 100644 {id} {id} M\0").into_bytes(),
                None,
            ),
            (
                "an unterminated path",
                format!(":100644 100644 {id} {id} M\0p").into_bytes(),
                None,
            ),
            (
                "an empty path",
                format!(":100644 100644 {id} {id} M\0\0").into_bytes(),
                None,
            ),
            (
                "a record without a colon",
                format!("100644 100644 {id} {id} M\0p\0").into_bytes(),
                None,
            ),
            (
                "too few fields",
                format!(":100644 100644 {id} M\0p\0").into_bytes(),
                None,
            ),
            (
                "too many fields",
                format!(":100644 100644 {id} {id} M x\0p\0").into_bytes(),
                None,
            ),
            (
                "an empty status",
                format!(":100644 100644 {id} {id} \0p\0").into_bytes(),
                None,
            ),
        ];
        let mut failures = Vec::new();
        for (name, raw, expected) in rows {
            let actual = as_tuples(&raw);
            if actual != expected {
                failures.push(format!("{name}: {actual:?} != {expected:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    proptest::proptest! {
        /// Whatever bytes a hook puts in a path, the parse returns each status, new id and
        /// path unchanged and in order; output cut before its last path never parses; and
        /// no input panics.
        #[test]
        fn prop_raw_diff_entries_round_trips(
            entries in proptest::collection::vec(
                (
                    proptest::sample::select(vec!["M", "A", "D", "T"]),
                    "[0-9a-f]{40}|[0-9a-f]{64}",
                    proptest::collection::vec(1u8..=255u8, 1..=23usize),
                ),
                0..=5usize,
            ),
            random in proptest::collection::vec(proptest::num::u8::ANY, 0..256usize),
        ) {
            let raw: Vec<u8> = entries
                .iter()
                .flat_map(|(status, id, path)| raw_diff_record(id, status, path))
                .collect();
            let parsed = raw_diff_entries(&raw).expect("generated output parses");
            proptest::prop_assert_eq!(parsed.len(), entries.len());
            for (entry, (status, id, path)) in parsed.iter().zip(&entries) {
                proptest::prop_assert_eq!(entry.status, status.as_bytes()[0]);
                proptest::prop_assert_eq!(entry.new_oid, id.as_bytes());
                proptest::prop_assert_eq!(entry.path, path.as_slice());
            }
            if let Some((_, _, last_path)) = entries.last() {
                let cut = &raw[..raw.len() - last_path.len() - 1];
                proptest::prop_assert!(raw_diff_entries(cut).is_none());
            }
            let _ = raw_diff_entries(&random);
        }
    }

    /// `<mode> <kind> <id>` TAB `<path>` NUL, the shape `ls-tree -r -t -z` prints.
    fn tree_listing_record(mode: &str, kind: &str, id: &str, path: &[u8]) -> Vec<u8> {
        let mut record = format!("{mode} {kind} {id}\t").into_bytes();
        record.extend_from_slice(path);
        record.push(0);
        record
    }

    #[test]
    fn test_tree_listing_blobs() {
        use TreeDefect::{BadComponent, RepeatedPath, Unparseable};
        let id = "c".repeat(40);
        let blob = |path: &[u8]| tree_listing_record("100644", "blob", &id, path);
        let tree = |path: &[u8]| tree_listing_record("040000", "tree", &id, path);
        let non_utf8: &[u8] = &[b'a', 0xFF, b'\t', b'z'];

        // The paths a listing must expose as regular files; each row starts from no defect.
        let good = vec![
            ("a 100644 blob", blob(b"a"), vec![b"a".as_slice()]),
            (
                "a 100755 blob",
                tree_listing_record("100755", "blob", &id, b"a"),
                vec![b"a".as_slice()],
            ),
            (
                "a symlink is not a regular file",
                tree_listing_record("120000", "blob", &id, b"a"),
                vec![],
            ),
            (
                "a gitlink is not a regular file",
                tree_listing_record("160000", "commit", &id, b"a"),
                vec![],
            ),
            ("a tree is not a regular file", tree(b"a"), vec![]),
            (
                "names merely containing dots",
                [blob(b"a.b"), blob(b"...")].concat(),
                vec![b"a.b".as_slice(), b"...".as_slice()],
            ),
            (
                "a path with a tab and non-UTF-8 bytes",
                blob(non_utf8),
                vec![non_utf8],
            ),
            ("empty input", Vec::new(), vec![]),
        ];
        let bad = vec![
            (
                "a repeated blob path",
                [blob(b"a"), blob(b"a")].concat(),
                RepeatedPath,
            ),
            (
                "a repeated tree path",
                [tree(b"a"), tree(b"a")].concat(),
                RepeatedPath,
            ),
            (
                "a blob and a tree at one path",
                [blob(b"a"), tree(b"a")].concat(),
                RepeatedPath,
            ),
            ("a/./b", blob(b"a/./b"), BadComponent),
            ("a/../b", blob(b"a/../b"), BadComponent),
            ("a//b", blob(b"a//b"), BadComponent),
            (".", tree(b"."), BadComponent),
            ("..", tree(b".."), BadComponent),
            ("a leading slash", blob(b"/a"), BadComponent),
            ("a trailing slash", blob(b"a/"), BadComponent),
            (
                "a record without a tab",
                format!("100644 blob {id}\0").into_bytes(),
                Unparseable,
            ),
            (
                "too few metadata fields",
                format!("100644 {id}\ta\0").into_bytes(),
                Unparseable,
            ),
            (
                "too many metadata fields",
                format!("100644 blob {id} x\ta\0").into_bytes(),
                Unparseable,
            ),
            (
                "an unterminated record",
                format!("100644 blob {id}\ta").into_bytes(),
                Unparseable,
            ),
        ];
        let mut failures = Vec::new();
        for (name, listing, expected_paths) in good {
            match tree_listing_blobs(&listing) {
                Ok(blobs) => {
                    let holds_all = expected_paths
                        .iter()
                        .all(|path| blobs.get(path) == Some(&id.as_bytes()));
                    if !holds_all || blobs.len() != expected_paths.len() {
                        failures.push(format!("{name}: {blobs:?}"));
                    }
                }
                Err(defect) => failures.push(format!("{name}: {defect:?}")),
            }
        }
        for (name, listing, expected) in bad {
            match tree_listing_blobs(&listing) {
                Err(defect) if defect == expected => {}
                other => failures.push(format!("{name}: {other:?}, expected {expected:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// One to three path components, each 1 to 5 bytes from 1 to 255, none of them `/`, `.`
    /// or `..`.
    fn listed_path_strategy() -> impl proptest::strategy::Strategy<Value = Vec<u8>> {
        use proptest::strategy::Strategy as _;
        let component = proptest::collection::vec(1u8..=255u8, 1..=5usize)
            .prop_filter("a name git can list", |name| {
                !name.contains(&b'/') && name != b"." && name != b".."
            });
        proptest::collection::vec(component, 1..=3usize).prop_map(|names| names.join(&b'/'))
    }

    proptest::proptest! {
        /// Whatever bytes a hook puts in a name, every listed path comes back with its id;
        /// one more record for a listed path, or a record below it through `.`, is refused;
        /// and no input panics.
        #[test]
        fn prop_tree_listing_blobs_round_trips(
            paths in proptest::collection::hash_set(listed_path_strategy(), 1..=5usize),
            id in "[0-9a-f]{40}",
            random in proptest::collection::vec(proptest::num::u8::ANY, 0..256usize),
        ) {
            let listing: Vec<u8> = paths
                .iter()
                .flat_map(|path| tree_listing_record("100644", "blob", &id, path))
                .collect();
            let blobs = tree_listing_blobs(&listing).expect("generated output parses");
            proptest::prop_assert_eq!(blobs.len(), paths.len());
            for path in &paths {
                proptest::prop_assert_eq!(blobs.get(path.as_slice()), Some(&id.as_bytes()));
            }

            let listed = paths.iter().next().expect("at least one path");
            let mut repeated = listing.clone();
            repeated.extend(tree_listing_record("100644", "blob", &id, listed));
            proptest::prop_assert_eq!(
                tree_listing_blobs(&repeated).err(),
                Some(TreeDefect::RepeatedPath)
            );

            let mut below_dot = listing.clone();
            let dotted = [listed.as_slice(), b"/./x"].concat();
            below_dot.extend(tree_listing_record("100644", "blob", &id, &dotted));
            proptest::prop_assert_eq!(
                tree_listing_blobs(&below_dot).err(),
                Some(TreeDefect::BadComponent)
            );

            let _ = tree_listing_blobs(&random);
        }
    }

    #[test]
    fn test_git_path_bytes() {
        let joined = Path::new("templates").join("a").join("b");
        assert_eq!(git_path_bytes(Path::new("templates/a/b")), b"templates/a/b");
        assert_eq!(git_path_bytes(&joined), b"templates/a/b");
    }

    /// A tree object whose only entry is `mode name` pointing at `oid`, whatever type that
    /// object has; `git mktree` refuses a type that disagrees with the mode.
    #[cfg(unix)]
    fn literal_tree(git: &GitFixture, repo: &Path, mode_and_name: &str, oid: &str) -> String {
        let mut raw = format!("{mode_and_name}\0").into_bytes();
        for pair in oid.as_bytes().chunks(2) {
            let pair = std::str::from_utf8(pair).unwrap();
            raw.push(u8::from_str_radix(pair, 16).unwrap());
        }
        git_with_input(
            git,
            repo,
            &["hash-object", "-w", "-t", "tree", "--literally", "--stdin"],
            &raw,
        )
    }

    /// An annotated tag on `blob`, its text a hook's, in `repo`; returns the tag object's id.
    #[cfg(unix)]
    fn tag_on_blob(git: &GitFixture, repo: &Path, blob: &str) -> String {
        git.run(
            repo,
            &["tag", "-a", "ude-test-tag", "-m", "INJECTED_TAG_TEXT", blob],
        );
        git.output(repo, &["rev-parse", "ude-test-tag"])
    }

    #[test]
    #[cfg(unix)]
    fn test_committed_gate_refuses_tag_object_at_validated_path() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let validated_blob = write_blob(&git, dir, b"f new\n");
        let tag = tag_on_blob(&git, dir, &validated_blob);
        let templates = literal_tree(&git, dir, "100644 f", &tag);
        let root = git_with_input(
            &git,
            dir,
            &["mktree"],
            format!("040000 tree {templates}\ttemplates\n").as_bytes(),
        );
        let commit = git_with_input(
            &git,
            dir,
            &["commit-tree", &root, "-p", &base],
            format!("{SYNC_COMMIT_MESSAGE}\n").as_bytes(),
        );
        // Opposite direction: the plain read peels the tag to the validated bytes, and a
        // plain push sends the tag object with its text.
        assert_eq!(git.output(dir, &["cat-file", "blob", &tag]), "f new");
        let control = push_to_fresh_bare(&git, dir, &commit, &[], false);
        assert!(holds_object(&git, control.path(), &tag));

        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);
        let err = engine
            .validate_committed_tree(dir, &commit, &base, &paths, &validated, &root)
            .expect_err("a tag object at a validated path must be refused");
        assert!(err.contains("failed to inspect committed blob"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_committed_gate_requires_validated_content_refuses_tag_object() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, _base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let blob = write_blob(&git, dir, b"f new\n");
        let tag = tag_on_blob(&git, dir, &blob);
        let digest = SyncEngine::fingerprint(b"f new\n");
        let record: HashMap<PathBuf, &str> = [(PathBuf::from("templates/f"), digest.as_str())]
            .into_iter()
            .collect();

        // Opposite direction: the same record is satisfied by the blob the tag points at.
        let holds_blob: HashMap<&[u8], &[u8]> = [(b"templates/f".as_slice(), blob.as_bytes())]
            .into_iter()
            .collect();
        engine
            .require_validated_content(dir, &holds_blob, &record)
            .expect("the blob holds the recorded content");

        let holds_tag: HashMap<&[u8], &[u8]> = [(b"templates/f".as_slice(), tag.as_bytes())]
            .into_iter()
            .collect();
        let err = engine
            .require_validated_content(dir, &holds_tag, &record)
            .expect_err("a tag object must not count as the recorded content");
        assert!(err.contains("does not hold the validated content"), "{err}");
    }

    /// Pushes with a `core.fsmonitor` script that reads `swapped_root` (made by `build_root`
    /// from the checkout) into the index as the staged gate reads the last blob, so the tree
    /// recorded next holds what the gate never read. The script stands in for any process
    /// that changes the index in that window. `hook_ran` is the script's marker.
    #[cfg(target_os = "linux")]
    fn push_with_index_swap(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        build_root: impl FnOnce(&Path) -> String,
    ) -> HookedPush {
        let mut pushed = push_managed_file(git, engine, open_pr, HOOKED_TEMPLATE, |repo| {
            use std::os::unix::fs::PermissionsExt;
            let swapped_root = build_root(repo);
            let script = repo.join(".git/ude-test-fsmonitor");
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\ncase \"$(tr '\\0' ' ' < /proc/$PPID/cmdline)\" in\n  *' show :'*) ;;\n  *) exit 1 ;;\nesac\n[ -e .git/ude-test-hook-ran ] && exit 1\n: > .git/ude-test-hook-ran\ngit read-tree {swapped_root}\nexit 1\n"
                ),
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            git.run(
                repo,
                &["config", "core.fsmonitor", script.to_str().unwrap()],
            );
        });
        pushed.hook_ran = pushed.checkout.join(".git/ude-test-hook-ran").exists();
        pushed
    }

    /// The root tree holding `generic` (the tree of `generic/`) on the managed file's path.
    #[cfg(target_os = "linux")]
    fn root_over_generic(git: &GitFixture, repo: &Path, generic: &str) -> String {
        let mktree_one = |entry: String| git_with_input(git, repo, &["mktree"], entry.as_bytes());
        let projects = mktree_one(format!("040000 tree {generic}\tgeneric\n"));
        let templates = mktree_one(format!("040000 tree {projects}\tprojects\n"));
        mktree_one(format!("040000 tree {templates}\ttemplates\n"))
    }

    /// A tag object swapped in at the managed path is refused, reaches no remote, and a
    /// plain push of the same tree delivers it.
    #[cfg(target_os = "linux")]
    fn assert_index_swap_to_tag_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_index_swap(&git, &engine, open_pr, |repo| {
            let validated_blob = write_blob(&git, repo, HOOKED_TEMPLATE.as_bytes());
            let tag = tag_on_blob(&git, repo, &validated_blob);
            let generic = literal_tree(&git, repo, "100644 gitconfig.repo", &tag);
            root_over_generic(&git, repo, &generic)
        });
        let err = assert_hook_refused(&git, &pushed);
        assert!(err.contains("failed to inspect committed blob"), "{err}");
        let tag = git.output(&pushed.checkout, &["rev-parse", "ude-test-tag"]);
        assert!(
            !holds_object(&git, pushed.bare.path(), &tag),
            "the remote holds the tag object"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_direct_push_refuses_index_swap_to_tag_object() {
        assert_index_swap_to_tag_refused(false);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_pr_push_refuses_index_swap_to_tag_object() {
        assert_index_swap_to_tag_refused(true);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_push_refuses_index_swap_to_injected_blob() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_index_swap(&git, &engine, false, |repo| {
            let injected = write_blob(&git, repo, b"INJECTED_BY_HOOK\n");
            let generic = git_with_input(
                &git,
                repo,
                &["mktree"],
                format!("100644 blob {injected}\tgitconfig.repo\n").as_bytes(),
            );
            root_over_generic(&git, repo, &generic)
        });
        let err = assert_hook_refused(&git, &pushed);
        assert!(err.contains("differs from the validated content"), "{err}");
    }

    /// The same index read back unchanged publishes, so the refusals above come from what
    /// the swap put in the tree and not from the script running.
    #[test]
    #[cfg(target_os = "linux")]
    fn test_push_publishes_with_index_swap_to_the_staged_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_index_swap(&git, &engine, false, |repo| {
            let validated_blob = write_blob(&git, repo, HOOKED_TEMPLATE.as_bytes());
            let generic = git_with_input(
                &git,
                repo,
                &["mktree"],
                format!("100644 blob {validated_blob}\tgitconfig.repo\n").as_bytes(),
            );
            root_over_generic(&git, repo, &generic)
        });
        assert_published_exact(&git, &pushed, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_publishes_deletion() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = tempdir().unwrap();
        let cache = tempdir().unwrap();
        git.run(bare.path(), &["init", "--bare", "-b", "main"]);
        let seed = tempdir().unwrap();
        git.run(seed.path(), &["init", "-b", "main"]);
        let generic = seed.path().join("templates/projects/generic");
        fs::create_dir_all(&generic).unwrap();
        fs::write(generic.join("gitconfig.repo"), "[core]\n").unwrap();
        fs::write(generic.join("gitignore.repo"), "target/\n").unwrap();
        git.run(seed.path(), &["add", "-A"]);
        git.run(seed.path(), &["commit", "-m", "seed"]);
        git.run(
            seed.path(),
            &["push", bare.path().to_str().unwrap(), "main"],
        );

        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let gitconfig = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::remove_file(&gitconfig).unwrap();
        // A deletion publishes no blob, so it needs no validated content.
        let pushed = engine.push_to_remote(
            &cached.publish_target("main"),
            true,
            &[gitconfig],
            &ValidatedBlobs::default(),
        );
        assert!(pushed.is_ok(), "deletion push: {pushed:?}");

        let heads = remote_heads(&git, bare.path());
        let sync: Vec<_> = heads
            .iter()
            .filter(|(name, _)| name.starts_with("refs/heads/ude-sync-"))
            .collect();
        assert_eq!(sync.len(), 1, "{heads:?}");
        let tree = git.output(bare.path(), &["ls-tree", "-r", "--name-only", &sync[0].0]);
        assert!(!tree.contains("gitconfig.repo"), "{tree}");
        assert!(
            tree.contains("templates/projects/generic/gitignore.repo"),
            "{tree}"
        );
    }

    /// post-commit tail: points HEAD at a commit over UDE's tree whose only `parent` line,
    /// holding `text` (a printf format), comes after `author`. Git reads no parent from
    /// such a commit, and raw_commit_parents reads one, so the gate reaches its
    /// one-parent arm with the hook's bytes.
    #[cfg(unix)]
    fn hook_parent_line(text: &str) -> String {
        format!(
            r#"T=$(git rev-parse 'HEAD^{{tree}}')
N=$(printf 'tree %s\nauthor a <a@b> 1 +0000\ncommitter a <a@b> 1 +0000\nparent {text}\n\n{SYNC_COMMIT_MESSAGE}\n' "$T" | git hash-object --literally -w -t commit --stdin)
git update-ref HEAD "$N"
"#
        )
    }

    /// A hook-built `parent` line is refused by the continuity check, which names the
    /// fetched head and prints neither `absent` nor an ESC byte; nothing is published.
    #[cfg(unix)]
    fn assert_parent_line_refused(open_pr: bool, text: &str, absent: &str) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &hook_parent_line(text));
        assert!(pushed.hook_ran, "the post-commit hook never ran");
        assert_remote_unchanged(&git, &pushed);
        let fetched_sha = pushed.fetched_sha.clone();
        let err = push_failure(pushed.res);
        assert!(err.contains("Branch continuity check failed"), "{err:?}");
        assert!(
            err.contains(&format!("is not the fetched head {fetched_sha}")),
            "{err:?}"
        );
        assert!(
            !err.contains(absent),
            "the refusal printed the hook's text: {err:?}"
        );
        assert!(
            !err.contains('\u{1b}'),
            "the refusal printed an ESC byte: {err:?}"
        );
    }

    /// The parent text of the escape-sequence rows, as a printf format: `\033` is ESC.
    #[cfg(unix)]
    const HOOK_PARENT_ESCAPE: &str = r"HOOK_CHOSEN_TEXT_\033[31m";

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refusal_omits_hook_text_in_parent_line() {
        assert_parent_line_refused(false, HOOK_PARENT_ESCAPE, "HOOK_CHOSEN_TEXT_");
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refusal_omits_hook_text_in_parent_line() {
        assert_parent_line_refused(true, HOOK_PARENT_ESCAPE, "HOOK_CHOSEN_TEXT_");
    }

    /// A GitHub PAT shape as the parent text, built at runtime.
    #[cfg(unix)]
    fn hook_parent_token() -> String {
        let prefix = "ghp_";
        format!("{prefix}{HOOK_TOKEN_BODY}")
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refusal_omits_token_in_parent_line() {
        assert_parent_line_refused(false, &hook_parent_token(), HOOK_TOKEN_BODY);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refusal_omits_token_in_parent_line() {
        assert_parent_line_refused(true, &hook_parent_token(), HOOK_TOKEN_BODY);
    }

    /// The opening of every refusal on the empty-status route.
    #[cfg(unix)]
    const EMPTY_STATUS_REFUSAL: &str = "git status reports nothing to push";

    /// The refusal of a record the fetched head does not hold.
    const NOT_HELD: &str = "does not hold the validated content";

    /// The number of the bare remote's heads whose managed template is exactly `bytes`.
    #[cfg(unix)]
    fn remote_heads_holding(git: &GitFixture, bare: &Path, bytes: &[u8]) -> usize {
        remote_heads(git, bare)
            .iter()
            .filter(|(name, _)| {
                git.cmd(bare)
                    .args(["show", &format!("{name}:{GATE_REL}")])
                    .output()
                    .unwrap()
                    .stdout
                    == bytes
            })
            .count()
    }

    /// No `ude-sync-*` branch is left in the cache checkout.
    #[cfg(unix)]
    fn assert_no_local_sync_branch(git: &GitFixture, checkout: &Path) {
        assert_eq!(
            git.output(checkout, &["branch", "--list", "ude-sync-*"]),
            "",
            "a sync branch was left in the cache checkout"
        );
    }

    /// Installs, after a first ensure, a post-index-change hook that runs `tail` once
    /// during the ensure under test, then pushes [`HOOKED_TEMPLATE`].
    #[cfg(unix)]
    fn push_with_index_hook_during_ensure(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        tail: &str,
    ) -> EnsuredPush {
        let setup = |_: &Path, checkout: &Path| {
            install_checkout_hook(checkout, "post-index-change", "", tail);
        };
        push_after_hooked_ensure(git, engine, open_pr, Some(&setup), true)
    }

    /// A hook that sets assume-unchanged on the template while ensure resets the checkout
    /// hides the edit from `git status`: the push is refused because the fetched head does
    /// not hold the record, and no remote head moves. Control: the same hook without the
    /// flag publishes the edit.
    #[cfg(unix)]
    fn assert_flag_set_during_ensure_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let flag = format!("git update-index --assume-unchanged '{GATE_REL}'\n");
        let pushed = push_with_index_hook_during_ensure(&git, &engine, open_pr, &flag);
        assert_hook_ran(&pushed);
        match &pushed.res {
            Err(PushRemoteError::Failed(e)) => {
                assert!(e.contains(EMPTY_STATUS_REFUSAL), "{e}");
                assert!(e.contains(NOT_HELD), "{e}");
                assert!(e.contains(GATE_REL), "{e}");
            }
            other => panic!("expected the empty-status refusal, got {other:?}"),
        }
        assert_eq!(
            remote_heads(&git, pushed.bare.path()),
            pushed.heads_before,
            "a remote head moved"
        );
        assert_no_local_sync_branch(&git, &pushed.checkout);

        let control = push_with_index_hook_during_ensure(&git, &engine, open_pr, ":\n");
        assert_hook_ran(&control);
        assert!(control.res.is_ok(), "{:?}", control.res);
        assert_eq!(
            remote_heads_holding(&git, control.bare.path(), HOOKED_TEMPLATE.as_bytes()),
            1,
            "the control did not publish the edit"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_assume_unchanged_set_during_ensure() {
        assert_flag_set_during_ensure_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_assume_unchanged_set_during_ensure() {
        assert_flag_set_during_ensure_refused(true);
    }

    /// What a test inspects after [`push_with_empty_status`].
    #[cfg(unix)]
    struct EmptyStatusPush {
        res: Result<String, PushRemoteError>,
        bare: tempfile::TempDir,
        _cache: tempfile::TempDir,
        checkout: PathBuf,
        fetched_sha: String,
        /// The remote's heads just before the push.
        heads_before: Vec<(String, String)>,
    }

    /// What a step hands the push: the staged files and the record.
    #[cfg(unix)]
    type StepResult = (Vec<PathBuf>, ValidatedBlobs);

    /// Seeds a bare remote, runs [`SyncEngine::ensure_remote_repo_for_push`], then
    /// `step(checkout, managed template)`, and pushes what the step returns. Requires
    /// `git status` to print nothing after the step, so the push takes the empty-status
    /// route.
    #[cfg(unix)]
    fn push_with_empty_status(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        step: &dyn Fn(&Path, &Path) -> StepResult,
    ) -> EmptyStatusPush {
        let checked = |checkout: &Path, managed: &Path| {
            let pushed = step(checkout, managed);
            assert_eq!(
                git.output(checkout, &["status", "--porcelain"]),
                "",
                "the step left git status non-empty, so the push would not take the empty-status route"
            );
            pushed
        };
        push_after_step(git, engine, open_pr, &checked)
    }

    /// Seeds a bare remote, runs [`SyncEngine::ensure_remote_repo_for_push`], then
    /// `step(checkout, managed template)`, and pushes what the step returns, on whichever
    /// route `git status` then selects.
    #[cfg(unix)]
    fn push_after_step(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        step: &dyn Fn(&Path, &Path) -> StepResult,
    ) -> EmptyStatusPush {
        let bare = seeded_bare_remote(git);
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo_for_push(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let checkout = cached.checkout_dir.clone();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        let (staged, validated) = step(&checkout, &managed);
        let heads_before = remote_heads(git, bare.path());
        let res =
            engine.push_to_remote(&cached.publish_target("main"), open_pr, &staged, &validated);
        let fetched_sha = cached.fetched_sha.clone();
        drop(cached);
        EmptyStatusPush {
            res,
            bare,
            _cache: cache,
            checkout,
            fetched_sha,
            heads_before,
        }
    }

    /// The push failed hard, no remote head moved and no sync branch is left; returns the
    /// refusal.
    #[cfg(unix)]
    fn assert_empty_status_refused(git: &GitFixture, pushed: &EmptyStatusPush) -> String {
        let err = match &pushed.res {
            Err(PushRemoteError::Failed(e)) => e.clone(),
            other => panic!("expected a hard push failure, got {other:?}"),
        };
        assert_eq!(
            remote_heads(git, pushed.bare.path()),
            pushed.heads_before,
            "a remote head moved: {err}"
        );
        assert_no_local_sync_branch(git, &pushed.checkout);
        err
    }

    /// Writes [`HOOKED_TEMPLATE`] over the managed template and records it.
    #[cfg(unix)]
    fn write_hooked_template(managed: &Path) -> StepResult {
        fs::write(managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(&[managed.to_path_buf()]);
        (vec![managed.to_path_buf()], validated)
    }

    /// assume-unchanged set after ensure, on the edited template: refused, nothing moved.
    /// No assertion on which refusal fires.
    #[cfg(unix)]
    fn assert_flag_set_after_ensure_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = write_hooked_template(managed);
            git.run(checkout, &["update-index", "--assume-unchanged", GATE_REL]);
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        assert_empty_status_refused(&git, &pushed);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_assume_unchanged_set_after_ensure() {
        assert_flag_set_after_ensure_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_assume_unchanged_set_after_ensure() {
        assert_flag_set_after_ensure_refused(true);
    }

    /// A commit holding the validated content on top of the fetched head leaves status
    /// empty and would read as held, while nothing reached the remote: refused because
    /// HEAD is not the fetched head, with neither id in the message.
    #[cfg(unix)]
    fn assert_head_off_the_fetched_head_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = write_hooked_template(managed);
            git.run(checkout, &["commit", "-q", "-a", "-m", "hook commit"]);
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        let head = git.output(&pushed.checkout, &["rev-parse", "HEAD"]);
        assert_ne!(head, pushed.fetched_sha, "the step made no commit");
        assert_eq!(
            git.output(&pushed.checkout, &["show", &format!("HEAD:{GATE_REL}")]),
            HOOKED_TEMPLATE.trim_end(),
            "HEAD does not hold the validated content, so the HEAD check is not what refuses"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains("is not the fetched head"), "{err}");
        assert!(!err.contains(&head), "the refusal printed HEAD's id: {err}");
        assert!(
            !err.contains(&pushed.fetched_sha),
            "the refusal printed the fetched head's id: {err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_head_off_the_fetched_head() {
        assert_head_off_the_fetched_head_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_head_off_the_fetched_head() {
        assert_head_off_the_fetched_head_refused(true);
    }

    /// assume-unchanged on a template removed from the working tree hides the deletion:
    /// the staged path with no record is still in the fetched head, so the push is
    /// refused naming it. test_push_to_remote_deletion is the control: a real deletion
    /// publishes.
    #[cfg(unix)]
    fn assert_deleted_path_still_held_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            git.run(checkout, &["update-index", "--assume-unchanged", GATE_REL]);
            fs::remove_file(managed).unwrap();
            (vec![managed.to_path_buf()], ValidatedBlobs::default())
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(
            err.contains(&format!(
                "{GATE_REL} is staged for deletion and still present"
            )),
            "{err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_deleted_path_still_held() {
        assert_deleted_path_still_held_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_deleted_path_still_held() {
        assert_deleted_path_still_held_refused(true);
    }

    /// A record equal to what the fetched head holds is the legitimate no-change case:
    /// Ok("No changes..."), the remote's heads unchanged, no sync branch. With
    /// `gone_deletion` the staged list also holds a deletion with no record whose path the
    /// fetched head no longer lists, while it still lists other paths: still no change.
    #[cfg(unix)]
    fn assert_no_change_publishes_nothing(open_pr: bool, gone_deletion: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |_: &Path, managed: &Path| {
            let validated = ValidatedBlobs::from_files(&[managed.to_path_buf()]);
            let mut staged = vec![managed.to_path_buf()];
            if gone_deletion {
                let gone = managed.with_file_name("already-deleted.repo");
                assert!(!gone.exists(), "the deleted path must be absent");
                staged.push(gone);
            }
            (staged, validated)
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        assert_eq!(
            pushed.res,
            Ok("No changes to push to remote repository.".to_string())
        );
        assert_eq!(remote_heads(&git, pushed.bare.path()), pushed.heads_before);
        assert_no_local_sync_branch(&git, &pushed.checkout);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_publishes_nothing_when_content_is_held() {
        assert_no_change_publishes_nothing(false, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_publishes_nothing_when_content_is_held() {
        assert_no_change_publishes_nothing(true, false);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_publishes_nothing_when_content_is_held_and_deletion_is_gone() {
        assert_no_change_publishes_nothing(false, true);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_publishes_nothing_when_content_is_held_and_deletion_is_gone() {
        assert_no_change_publishes_nothing(true, true);
    }

    /// The refusal of an object whose bytes do not hash to its id. A literal of its own, so
    /// a changed production text fails these rows.
    const ID_MISMATCH: &str = "does not match its object id in the cache checkout";

    /// A second template, written in a directory the seed does not have.
    #[cfg(unix)]
    const ADDED_TEMPLATE: &str = "[added]\n\tx = 2\n";

    /// Rewrites the loose object file of `target` in the repository at `dir` with the bytes
    /// of `source`'s object file, so a read of `target` returns `source`'s content. Panics
    /// unless both files exist, because a packed object would leave the forgery unread. The
    /// target file is removed before the new one is written: a local clone hardlinks loose
    /// objects, so writing in place would rewrite the bare remote's object too.
    fn forge_loose_object(dir: &Path, target: &str, source: &str) {
        let object_file = |id: &str| dir.join(".git/objects").join(&id[..2]).join(&id[2..]);
        let (target_file, source_file) = (object_file(target), object_file(source));
        assert!(
            target_file.is_file(),
            "no loose object file for {target}; a packed object would leave the forgery unread"
        );
        assert!(source_file.is_file(), "no loose object file for {source}");
        let bytes = fs::read(&source_file).unwrap();
        fs::remove_file(&target_file).unwrap();
        fs::write(&target_file, bytes).unwrap();
    }

    /// The bytes `cat-file <kind> <id>` prints in the bare remote hash to `id`.
    #[cfg(unix)]
    fn remote_object_matches_id(git: &GitFixture, bare: &Path, id: &str, kind: &str) -> bool {
        // Raw stdout: GitFixture::output trims and decodes lossily.
        let out = git.cmd(bare).args(["cat-file", kind, id]).output().unwrap();
        assert!(
            out.status.success(),
            "the bare remote cannot read {kind} {id}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        git_with_input(
            git,
            bare,
            &["hash-object", "-t", kind, "--literally", "--stdin"],
            &out.stdout,
        ) == id
    }

    /// The id of the seed template at [`GATE_REL`] on the bare remote's `main`.
    #[cfg(unix)]
    fn seed_blob_id(git: &GitFixture, bare: &Path) -> String {
        git.output(bare, &["rev-parse", &format!("main:{GATE_REL}")])
    }

    /// Writes [`HOOKED_TEMPLATE`] over the managed template, records it and hides the edit
    /// from `git status` with assume-unchanged.
    #[cfg(unix)]
    fn hide_hooked_template(git: &GitFixture, checkout: &Path, managed: &Path) -> StepResult {
        let pushed = write_hooked_template(managed);
        git.run(checkout, &["update-index", "--assume-unchanged", GATE_REL]);
        pushed
    }

    /// [`hide_hooked_template`], plus [`ADDED_TEMPLATE`] at `projects/added/added.repo`
    /// beside `projects/generic`; both are recorded and both are staged.
    #[cfg(unix)]
    fn hide_hooked_template_beside_added(
        git: &GitFixture,
        checkout: &Path,
        managed: &Path,
    ) -> StepResult {
        let (mut staged, _) = hide_hooked_template(git, checkout, managed);
        let added = managed
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("added/added.repo");
        fs::create_dir_all(added.parent().unwrap()).unwrap();
        fs::write(&added, ADDED_TEMPLATE).unwrap();
        staged.push(added);
        let validated = ValidatedBlobs::from_files(&staged);
        (staged, validated)
    }

    /// Gives the seed blob at [`GATE_REL`] in `checkout` the object file of the
    /// [`HOOKED_TEMPLATE`] blob, so a read of the seed id returns the validated bytes.
    #[cfg(unix)]
    fn forge_seed_blob(git: &GitFixture, checkout: &Path) {
        let seed = git.output(checkout, &["rev-parse", &format!("HEAD:{GATE_REL}")]);
        let validated = write_blob(git, checkout, HOOKED_TEMPLATE.as_bytes());
        forge_loose_object(checkout, &seed, &validated);
    }

    /// The push failed hard and no remote head moved; returns the refusal.
    #[cfg(unix)]
    fn assert_push_refused(git: &GitFixture, pushed: &EmptyStatusPush) -> String {
        let err = match &pushed.res {
            Err(PushRemoteError::Failed(e)) => e.clone(),
            other => panic!("expected a hard push failure, got {other:?}"),
        };
        assert_eq!(
            remote_heads(git, pushed.bare.path()),
            pushed.heads_before,
            "a remote head moved: {err}"
        );
        err
    }

    /// The seed blob the remote holds, its object file in the cache checkout rewritten to
    /// hold the validated bytes while status is empty: refused by the id check, the remote
    /// untouched. Control: the same step without the rewrite is refused as not held.
    #[cfg(unix)]
    fn assert_no_change_blob_not_matching_its_id_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            forge_seed_blob(&git, checkout);
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        let seed = seed_blob_id(&git, pushed.bare.path());
        // What a read without the check sees.
        let read = git
            .cmd(&pushed.checkout)
            .args(["cat-file", "blob", &seed])
            .output()
            .unwrap();
        assert_eq!(
            read.stdout,
            HOOKED_TEMPLATE.as_bytes(),
            "the forgery is not read"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(NOT_HELD), "{err}");
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &seed, "blob"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| hide_hooked_template(&git, checkout, managed);
        let control = push_with_empty_status(&git, &engine, open_pr, &step);
        let err = assert_empty_status_refused(&git, &control);
        assert!(err.contains(NOT_HELD), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_blob_not_matching_its_id() {
        assert_no_change_blob_not_matching_its_id_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_blob_not_matching_its_id() {
        assert_no_change_blob_not_matching_its_id_refused(true);
    }

    /// The committed route: GATE_REL kept at the seed blob by assume-unchanged beside a new
    /// file in another directory, and the seed's object file rewritten to hold the validated
    /// bytes. Refused by the id check, the remote untouched. Control: the same steps without
    /// the rewrite are refused because the commit does not hold the validated content.
    #[cfg(unix)]
    fn assert_unchanged_blob_not_matching_its_id_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template_beside_added(&git, checkout, managed);
            forge_seed_blob(&git, checkout);
            pushed
        };
        let pushed = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &pushed);
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let seed = seed_blob_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &seed, "blob"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| {
            hide_hooked_template_beside_added(&git, checkout, managed)
        };
        let control = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &control);
        assert!(
            err.contains("the commit to publish does not hold the validated content of"),
            "{err}"
        );
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_unchanged_blob_not_matching_its_id() {
        assert_unchanged_blob_not_matching_its_id_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_unchanged_blob_not_matching_its_id() {
        assert_unchanged_blob_not_matching_its_id_refused(true);
    }

    /// A changed blob whose object file holds the validated bytes under another id: the
    /// push would not send such a blob when the remote already holds its id, so the gate
    /// refuses it naming the path. Opposite direction: the genuine validated blob passes.
    #[test]
    fn test_committed_gate_refuses_changed_blob_not_matching_its_id() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let other = write_blob(&git, dir, b"f other\n");
        let validated_blob = write_blob(&git, dir, b"f new\n");
        forge_loose_object(dir, &other, &validated_blob);
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);

        let genuine = commit_of_templates(
            &git,
            dir,
            &base,
            &format!("100644 blob {validated_blob}\tf\n"),
        );
        engine
            .validate_committed_tree(
                dir,
                &genuine,
                &base,
                &paths,
                &validated,
                &commit_tree_id(&git, dir, &genuine),
            )
            .expect("the genuine validated blob passes");

        let forged = commit_of_templates(&git, dir, &base, &format!("100644 blob {other}\tf\n"));
        let err = engine
            .validate_committed_tree(
                dir,
                &forged,
                &base,
                &paths,
                &validated,
                &commit_tree_id(&git, dir, &forged),
            )
            .expect_err("a blob whose bytes do not hash to its id must be refused");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains("templates/f"), "{err}");
    }

    /// A commit to publish whose object file holds another commit's bytes is refused by the
    /// commit's own id check. Opposite direction: the same commit unforged passes.
    #[test]
    fn test_committed_gate_refuses_commit_not_matching_its_id() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let f_new = write_blob(&git, dir, b"f new\n");
        let f_other = write_blob(&git, dir, b"f other\n");
        let to_publish =
            commit_of_templates(&git, dir, &base, &format!("100644 blob {f_new}\tf\n"));
        let other = commit_of_templates(&git, dir, &base, &format!("100644 blob {f_other}\tf\n"));
        // Read before the forgery, as stage_publication records it.
        let staged_tree = commit_tree_id(&git, dir, &to_publish);
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);
        let validate = || {
            engine.validate_committed_tree(
                dir,
                &to_publish,
                &base,
                &paths,
                &validated,
                &staged_tree,
            )
        };

        validate().expect("the genuine commit passes");
        forge_loose_object(dir, &to_publish, &other);
        let err =
            validate().expect_err("a commit whose bytes do not hash to its id must be refused");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains("the commit to publish"), "{err}");
    }

    /// A fetched-head templates tree whose stored bytes do not match its id hides an added
    /// unvalidated file, or an unvalidated deletion, from a diff against the fetched head.
    /// Refused by the id check naming the recorded path. Control for each, first: the same
    /// commit over the honest fetched head is refused as unvalidated.
    #[test]
    fn test_committed_gate_refuses_base_tree_not_matching_its_id() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let mut failures = Vec::new();
        for (case, hidden_addition) in [("hidden addition", true), ("hidden deletion", false)] {
            let (repo, base) = templates_repo(&git, &[("f", "f base\n"), ("g", "g base\n")]);
            let dir = repo.path();
            let f_new = write_blob(&git, dir, b"f new\n");
            let f_base = git.output(dir, &["rev-parse", "HEAD:templates/f"]);
            let g_base = git.output(dir, &["rev-parse", "HEAD:templates/g"]);
            let base_templates = git.output(dir, &["rev-parse", "HEAD:templates"]);
            let (commit_listing, forged_listing) = if hidden_addition {
                let h = write_blob(&git, dir, b"h\n");
                (
                    format!(
                        "100644 blob {f_new}\tf\n100644 blob {g_base}\tg\n100644 blob {h}\th\n"
                    ),
                    format!(
                        "100644 blob {f_base}\tf\n100644 blob {g_base}\tg\n100644 blob {h}\th\n"
                    ),
                )
            } else {
                (
                    format!("100644 blob {f_new}\tf\n"),
                    format!("100644 blob {f_base}\tf\n"),
                )
            };
            let commit = commit_of_templates(&git, dir, &base, &commit_listing);
            // Read before the forgery, as stage_publication records it.
            let staged_tree = commit_tree_id(&git, dir, &commit);
            let validated = templates_record(dir, &[("f", b"f new\n")]);
            let validate = || {
                engine.validate_committed_tree(
                    dir,
                    &commit,
                    &base,
                    &paths,
                    &validated,
                    &staged_tree,
                )
            };

            match validate() {
                Err(err) if err.contains("unvalidated file committed in HEAD") => {}
                other => failures.push(format!("{case}, honest fetched head: {other:?}")),
            }
            let forged = git_with_input(&git, dir, &["mktree"], forged_listing.as_bytes());
            forge_loose_object(dir, &base_templates, &forged);
            match validate() {
                Err(err) if err.contains(ID_MISMATCH) && err.contains("templates/f") => {}
                other => failures.push(format!("{case}, forged fetched head: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A raw tree entry: `mode` SP `name` NUL `id`.
    fn raw_tree_entry(mode: &[u8], name: &[u8], id: &[u8]) -> Vec<u8> {
        [mode, b" ", name, b"\0", id].concat()
    }

    #[test]
    fn test_parse_tree_object() {
        const SHA1_LEN: usize = 20;
        const SHA256_LEN: usize = 32;
        let (one, two) = ([1u8; SHA1_LEN], [2u8; SHA1_LEN]);
        let decoded = |bytes: &[u8], id_len: usize| {
            parse_tree_object(bytes, id_len).map(|entries| {
                entries
                    .iter()
                    .map(|entry| (entry.mode, entry.name.to_vec(), entry.id.to_vec()))
                    .collect::<Vec<_>>()
            })
        };

        let pair = [
            raw_tree_entry(b"100644", b"a", &one),
            raw_tree_entry(b"40000", b"b", &two),
        ]
        .concat();
        assert_eq!(
            decoded(&pair, SHA1_LEN),
            Some(vec![
                (0o100644, b"a".to_vec(), one.to_vec()),
                (0o040000, b"b".to_vec(), two.to_vec()),
            ])
        );

        let modes: [(&[u8], u32); 7] = [
            (b"100644", 0o100644),
            (b"100755", 0o100755),
            (b"100664", 0o100644),
            (b"100775", 0o100755),
            (b"120000", 0o120000),
            (b"40000", 0o040000),
            (b"160000", 0o160000),
        ];
        for (mode, canonical) in modes {
            assert_eq!(
                decoded(&raw_tree_entry(mode, b"x", &one), SHA1_LEN),
                Some(vec![(canonical, b"x".to_vec(), one.to_vec())]),
                "mode {}",
                String::from_utf8_lossy(mode)
            );
        }

        let long = [3u8; SHA256_LEN];
        assert_eq!(
            decoded(&raw_tree_entry(b"100644", b"x", &long), SHA256_LEN),
            Some(vec![(0o100644, b"x".to_vec(), long.to_vec())])
        );

        let odd_name = b"a b\tc\nd\"e\\f\xff";
        assert_eq!(
            decoded(&raw_tree_entry(b"100644", odd_name, &one), SHA1_LEN),
            Some(vec![(0o100644, odd_name.to_vec(), one.to_vec())])
        );

        assert_eq!(decoded(b"", SHA1_LEN), Some(Vec::new()));

        // What git's tree decoder refuses, and a mode git would wrap, which fails closed here.
        let refused: [(&str, Vec<u8>); 6] = [
            ("no mode digit", raw_tree_entry(b"", b"x", &one)),
            ("a digit 8", raw_tree_entry(b"100648", b"x", &one)),
            ("an empty name", raw_tree_entry(b"100644", b"", &one)),
            ("no NUL", [b"100644 x".as_slice(), &one].concat()),
            (
                "a short id",
                raw_tree_entry(b"100644", b"x", &one[..SHA1_LEN - 1]),
            ),
            (
                "a mode overflowing u32",
                raw_tree_entry(b"777777777777", b"x", &one),
            ),
        ];
        let accepted: Vec<&str> = refused
            .iter()
            .filter(|(_, bytes)| parse_tree_object(bytes, SHA1_LEN).is_some())
            .map(|(name, _)| *name)
            .collect();
        assert!(accepted.is_empty(), "accepted: {accepted:?}");
    }

    /// The raw stdout of `--no-replace-objects ls-tree -r -t -z --full-tree <commit>` in
    /// `repo`; panics unless it exits 0.
    fn raw_ls_tree(git: &GitFixture, repo: &Path, commit: &str) -> Vec<u8> {
        let out = git
            .cmd(repo)
            .args([
                "--no-replace-objects",
                "ls-tree",
                "-r",
                "-t",
                "-z",
                "--full-tree",
                commit,
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "ls-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    /// The raw bytes of the hex object id `hex`.
    fn raw_id(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
            .collect()
    }

    /// [`SyncEngine::verified_tree_listing`] of `commit`'s root tree in `repo`.
    fn built_listing(
        engine: &SyncEngine,
        git: &GitFixture,
        repo: &Path,
        commit: &str,
    ) -> Result<Vec<u8>, ListingDefect> {
        let root = git.output(repo, &["rev-parse", &format!("{commit}^{{tree}}")]);
        engine.verified_tree_listing(
            repo,
            &root,
            &mut HashMap::new(),
            &mut engine.listing_budget(),
        )
    }

    /// Whether `needle` occurs in `haystack`.
    fn holds_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    /// On honest trees the built listing is byte for byte git's own, in a SHA-1 and a
    /// SHA-256 repository. Negative control: a subtree whose object file holds another
    /// tree's bytes still lists with git, and is refused by the built listing naming it.
    #[test]
    fn test_verified_tree_listing_matches_ls_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        let dir = repo.path();
        git.run(dir, &["init", "-b", "main"]);
        let blob = |bytes: &[u8]| write_blob(&git, dir, bytes);
        let mktree = |listing: &str| git_with_input(&git, dir, &["mktree"], listing.as_bytes());
        let commit = |tree: &str| git_with_input(&git, dir, &["commit-tree", tree], b"listing\n");
        let record = |mode: &str, kind: &str, id: &str, name: &[u8]| {
            [format!("{mode} {kind} {id}\t").as_bytes(), name, b"\0"].concat()
        };

        let (template, other, plain) = (blob(b"[core]\n"), blob(b"x\n"), blob(b"plain\n"));
        let generic = mktree(&format!("100644 blob {template}\tgitconfig.repo\n"));
        let other_dir = mktree(&format!("100644 blob {other}\tx.repo\n"));
        let projects = mktree(&format!(
            "040000 tree {generic}\tgeneric\n040000 tree {other_dir}\tother\n"
        ));
        let templates = mktree(&format!("040000 tree {projects}\tprojects\n"));
        let inner = mktree(&format!("100644 blob {plain}\tinside\n"));
        let empty = mktree("");
        let empty_commit = commit(&empty);
        // A legacy mode git still reads and lists as 100644; mktree would not write it.
        let legacy_bytes = [b"100664 legacy.repo\0".as_slice(), &raw_id(&plain)].concat();
        let legacy = git_with_input(
            &git,
            dir,
            &["hash-object", "-t", "tree", "--literally", "-w", "--stdin"],
            &legacy_bytes,
        );
        let link = blob(b"templates/projects\n");
        let mut records = [
            record("040000", "tree", &templates, b"templates"),
            record("100755", "blob", &plain, b"run.sh"),
            record("120000", "blob", &link, b"link"),
            record("160000", "commit", &empty_commit, b"module"),
            record("040000", "tree", &inner, b"a"),
            record("100644", "blob", &plain, b"a-b"),
            record("100644", "blob", &plain, b"a.b"),
            record("100644", "blob", &plain, b"a0"),
            record("040000", "tree", &empty, b"empty"),
            record("040000", "tree", &legacy, b"legacy"),
        ]
        .concat();
        for name in [
            b"with space".as_slice(),
            b"with\ttab",
            b"with\nnewline",
            b"with\"quote",
            b"with\\backslash",
            b"with\xffbyte",
        ] {
            records.extend(record("100644", "blob", &plain, name));
        }
        let root = git_with_input(&git, dir, &["mktree", "-z"], &records);
        let full = commit(&root);

        let listed = raw_ls_tree(&git, dir, &full);
        // The fixture holds what it claims, as git lists it.
        for claimed in [
            format!("100644 blob {plain}\tlegacy/legacy.repo\0").into_bytes(),
            format!("040000 tree {empty}\tempty\0").into_bytes(),
            format!("160000 commit {empty_commit}\tmodule\0").into_bytes(),
            format!("120000 blob {link}\tlink\0").into_bytes(),
            format!("100755 blob {plain}\trun.sh\0").into_bytes(),
            [
                format!("100644 blob {plain}\twith").as_bytes(),
                b"\xffbyte\0",
            ]
            .concat(),
            b"\ttemplates/projects/generic/gitconfig.repo\0".to_vec(),
            b"\twith\nnewline\0".to_vec(),
        ] {
            assert!(
                holds_bytes(&listed, &claimed),
                "git's listing lacks {:?}",
                String::from_utf8_lossy(&claimed)
            );
        }
        assert_eq!(built_listing(&engine, &git, dir, &full), Ok(listed));

        let listed_empty = raw_ls_tree(&git, dir, &empty_commit);
        assert!(listed_empty.is_empty(), "{listed_empty:?}");
        assert_eq!(
            built_listing(&engine, &git, dir, &empty_commit),
            Ok(listed_empty)
        );

        let sha256 = tempdir().unwrap();
        let dir256 = sha256.path();
        git.run(dir256, &["init", "--object-format=sha256", "-b", "main"]);
        fs::create_dir_all(dir256.join("templates/projects/generic")).unwrap();
        fs::write(
            dir256.join("templates/projects/generic/gitconfig.repo"),
            "[core]\n",
        )
        .unwrap();
        fs::write(dir256.join("run.sh"), "#!/bin/sh\n").unwrap();
        git.run(dir256, &["add", "-A"]);
        git.run(dir256, &["update-index", "--chmod=+x", "run.sh"]);
        git.run(dir256, &["commit", "-q", "-m", "sha256"]);
        let head = git.output(dir256, &["rev-parse", "HEAD"]);
        assert_eq!(head.len(), 64, "not a SHA-256 id: {head}");
        let listed256 = raw_ls_tree(&git, dir256, &head);
        assert!(holds_bytes(&listed256, b"100755 blob "), "{listed256:?}");
        assert!(
            holds_bytes(&listed256, b"\ttemplates/projects/generic\0"),
            "{listed256:?}"
        );
        assert_eq!(built_listing(&engine, &git, dir256, &head), Ok(listed256));

        forge_loose_object(dir, &generic, &other_dir);
        raw_ls_tree(&git, dir, &full);
        assert_eq!(
            built_listing(&engine, &git, dir, &full),
            Err(ListingDefect::IdMismatch(
                b"templates/projects/generic".to_vec()
            ))
        );
    }

    /// One `ls-tree -r -t -z` record: `<mode> <type> <id>` TAB `<path>` NUL.
    fn listing_record(mode: &str, kind: &str, id: &str, path: &[u8]) -> Vec<u8> {
        [format!("{mode} {kind} {id}\t").as_bytes(), path, b"\0"].concat()
    }

    /// One `diff-tree -z --raw` record with both modes and ids given.
    fn raw_change(
        old_mode: &str,
        new_mode: &str,
        old_id: &str,
        new_id: &str,
        status: &str,
        path: &[u8],
    ) -> Vec<u8> {
        [
            format!(":{old_mode} {new_mode} {old_id} {new_id} {status}\0").as_bytes(),
            path,
            b"\0",
        ]
        .concat()
    }

    /// [`listing_diff`] with a deadline an hour away: Some for a change list, None for an
    /// unparseable listing; a deadline this far cannot come, so it panics if it does.
    fn diff_unbounded(base: &[u8], commit: &[u8]) -> Option<Vec<u8>> {
        let far = std::time::Instant::now() + Duration::from_secs(3600);
        match listing_diff(base, commit, far) {
            Ok(changes) => Some(changes),
            Err(DiffDefect::Unparseable) => None,
            Err(DiffDefect::PastDeadline) => panic!("a deadline an hour away has come"),
        }
    }

    #[test]
    fn test_listing_diff() {
        let (x, y, z) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let zero = "0".repeat(40);
        let blob = |id: &str, path: &[u8]| listing_record("100644", "blob", id, path);
        let base = [blob(&x, b"f"), blob(&y, b"g")].concat();

        assert_eq!(diff_unbounded(&base, &base), Some(Vec::new()), "equal");
        assert_eq!(
            diff_unbounded(b"", &base),
            Some(
                [
                    raw_change("000000", "100644", &zero, &x, "A", b"f"),
                    raw_change("000000", "100644", &zero, &y, "A", b"g"),
                ]
                .concat()
            ),
            "an empty base"
        );
        assert_eq!(
            diff_unbounded(&base, b""),
            Some(
                [
                    raw_change("100644", "000000", &x, &zero, "D", b"f"),
                    raw_change("100644", "000000", &y, &zero, "D", b"g"),
                ]
                .concat()
            ),
            "an empty commit"
        );
        let changed = [blob(&z, b"f"), listing_record("100755", "blob", &y, b"g")].concat();
        assert_eq!(
            diff_unbounded(&base, &changed),
            Some(
                [
                    raw_change("100644", "100644", &x, &z, "M", b"f"),
                    raw_change("100644", "100755", &y, &y, "M", b"g"),
                ]
                .concat()
            ),
            "a content change and an executable flip"
        );
        let retyped = [
            listing_record("120000", "blob", &x, b"f"),
            listing_record("160000", "commit", &z, b"g"),
        ]
        .concat();
        assert_eq!(
            diff_unbounded(&base, &retyped),
            Some(
                [
                    raw_change("100644", "120000", &x, &x, "T", b"f"),
                    raw_change("100644", "160000", &y, &z, "T", b"g"),
                ]
                .concat()
            ),
            "a blob that becomes a symlink, and one that becomes a gitlink"
        );
        let directory = [listing_record("040000", "tree", &z, b"a"), blob(&x, b"a/x")].concat();
        assert_eq!(
            diff_unbounded(&directory, &blob(&y, b"a")),
            Some(
                [
                    raw_change("000000", "100644", &zero, &y, "A", b"a"),
                    raw_change("100644", "000000", &x, &zero, "D", b"a/x"),
                ]
                .concat()
            ),
            "a directory replaced by a file"
        );
        assert_eq!(
            diff_unbounded(&blob(&x, b"b"), &blob(&y, b"c")),
            Some(
                [
                    raw_change("100644", "000000", &x, &zero, "D", b"b"),
                    raw_change("000000", "100644", &zero, &y, "A", b"c"),
                ]
                .concat()
            ),
            "records in path order, a deletion before a later addition"
        );
        let other_tree = [listing_record("040000", "tree", &y, b"a"), blob(&x, b"a/x")].concat();
        assert_eq!(
            diff_unbounded(&directory, &other_tree),
            Some(Vec::new()),
            "tree records are ignored"
        );
        let repeated = [blob(&x, b"p"), blob(&y, b"p")].concat();
        assert_eq!(
            diff_unbounded(&blob(&x, b"p"), &repeated),
            Some(raw_change("000000", "100644", &zero, &y, "A", b"p")),
            "a repeated path"
        );
        let long = "d".repeat(64);
        assert_eq!(
            diff_unbounded(b"", &blob(&long, b"f")),
            Some(raw_change(
                "000000",
                "100644",
                &"0".repeat(64),
                &long,
                "A",
                b"f"
            )),
            "64-hex ids"
        );
        let no_tab = format!("100644 blob {x} f\0").into_bytes();
        assert_eq!(diff_unbounded(b"", &no_tab), None, "a record without a tab");
        assert_eq!(
            diff_unbounded(&no_tab, b""),
            None,
            "a base record without a tab"
        );
    }

    /// The raw stdout of `--no-replace-objects diff-tree -r -z --raw --no-commit-id
    /// --no-renames <base> <commit>` in `repo`; panics unless it exits 0.
    fn raw_diff_tree(git: &GitFixture, repo: &Path, base: &str, commit: &str) -> Vec<u8> {
        let out = git
            .cmd(repo)
            .args([
                "--no-replace-objects",
                "diff-tree",
                "-r",
                "-z",
                "--raw",
                "--no-commit-id",
                "--no-renames",
                base,
                commit,
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "diff-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    /// listing_diff over the built listings of `base` and `commit` equals git's diff-tree
    /// between them, in both directions; returns git's forward output.
    fn assert_listing_diff_matches(
        engine: &SyncEngine,
        git: &GitFixture,
        repo: &Path,
        base: &str,
        commit: &str,
    ) -> Vec<u8> {
        let base_listing = built_listing(engine, git, repo, base).unwrap();
        let commit_listing = built_listing(engine, git, repo, commit).unwrap();
        let forward = raw_diff_tree(git, repo, base, commit);
        assert_eq!(
            diff_unbounded(&base_listing, &commit_listing)
                .map(|raw| String::from_utf8_lossy(&raw).into_owned()),
            Some(String::from_utf8_lossy(&forward).into_owned()),
            "{base} to {commit}"
        );
        assert_eq!(
            diff_unbounded(&base_listing, &commit_listing),
            Some(forward.clone()),
            "{base} to {commit}, as bytes"
        );
        assert_eq!(
            diff_unbounded(&commit_listing, &base_listing),
            Some(raw_diff_tree(git, repo, commit, base)),
            "{commit} to {base}"
        );
        forward
    }

    /// On honest trees listing_diff is byte for byte git's own diff-tree, both directions, in
    /// a SHA-1 and a SHA-256 repository. The fixture asserts git's output holds every status
    /// and the odd names, so the comparison is not over an empty diff.
    #[test]
    fn test_listing_diff_matches_diff_tree() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        let dir = repo.path();
        git.run(dir, &["init", "-b", "main"]);
        let blob = |bytes: &[u8]| write_blob(&git, dir, bytes);
        let mktree =
            |records: &[Vec<u8>]| git_with_input(&git, dir, &["mktree", "-z"], &records.concat());
        let commit = |tree: &str| git_with_input(&git, dir, &["commit-tree", tree], b"diff\n");
        let file = |id: &str, name: &[u8]| listing_record("100644", "blob", id, name);
        let tree = |id: &str, name: &[u8]| listing_record("040000", "tree", id, name);

        let (one, two, three) = (blob(b"one\n"), blob(b"two\n"), blob(b"three\n"));
        let link = blob(b"target");
        let empty = mktree(&[]);
        let module = commit(&empty);
        let inner = mktree(&[file(&one, b"x")]);
        let same_leaf = mktree(&[file(&one, b"f")]);
        let same_nested = mktree(&[tree(&same_leaf, b"deep")]);
        let base_leaf = mktree(&[file(&one, b"f")]);
        let commit_leaf = mktree(&[file(&two, b"f"), file(&three, b"g")]);
        let y_dir = mktree(&[file(&two, b"y")]);
        let odd = |changed: &str| {
            vec![
                file(changed, b"with space"),
                file(&one, b"with\ttab"),
                file(changed, b"with\nnewline"),
                file(&one, b"with\"quote"),
                file(changed, b"with\\backslash"),
                file(changed, b"with\xffbyte"),
            ]
        };
        let mut base_records = vec![
            file(&one, b"changed"),
            file(&one, b"flip"),
            file(&one, b"deleted"),
            file(&one, b"to_link"),
            file(&one, b"to_module"),
            tree(&inner, b"a"),
            file(&one, b"a-b"),
            file(&one, b"a.b"),
            file(&one, b"a0"),
            file(&one, b"b"),
            tree(&same_nested, b"nested_same"),
            tree(&mktree(&[tree(&base_leaf, b"deep")]), b"nested"),
            tree(&empty, b"empty"),
        ];
        base_records.extend(odd(&one));
        let mut commit_records = vec![
            file(&one, b"added"),
            file(&two, b"changed"),
            listing_record("100755", "blob", &one, b"flip"),
            listing_record("120000", "blob", &link, b"to_link"),
            listing_record("160000", "commit", &module, b"to_module"),
            file(&two, b"a"),
            file(&two, b"a-b"),
            file(&one, b"a.b"),
            file(&two, b"a0"),
            tree(&y_dir, b"b"),
            tree(&same_nested, b"nested_same"),
            tree(&mktree(&[tree(&commit_leaf, b"deep")]), b"nested"),
            tree(&empty, b"empty"),
        ];
        commit_records.extend(odd(&two));
        let base = commit(&mktree(&base_records));
        let full = commit(&mktree(&commit_records));

        let forward = assert_listing_diff_matches(&engine, &git, dir, &base, &full);
        for claimed in [
            b" A\0added\0".as_slice(),
            b" M\0changed\0",
            b" M\0flip\0",
            b" D\0deleted\0",
            b" T\0to_link\0",
            b" T\0to_module\0",
            b" A\0a\0",
            b" D\0a/x\0",
            b" D\0b\0",
            b" A\0b/y\0",
            b" M\0nested/deep/f\0",
            b" A\0nested/deep/g\0",
            b" M\0with\nnewline\0",
            b" M\0with\xffbyte\0",
        ] {
            assert!(
                holds_bytes(&forward, claimed),
                "git's diff lacks {:?}",
                String::from_utf8_lossy(claimed)
            );
        }
        for unchanged in [
            b"\0with\ttab\0".as_slice(),
            b"\0a.b\0",
            b"nested_same",
            b"empty",
        ] {
            assert!(
                !holds_bytes(&forward, unchanged),
                "git's diff lists {:?}",
                String::from_utf8_lossy(unchanged)
            );
        }

        let empty_commit = commit(&empty);
        let from_empty = assert_listing_diff_matches(&engine, &git, dir, &empty_commit, &full);
        let statuses = raw_diff_entries(&from_empty).unwrap();
        assert!(!statuses.is_empty() && statuses.iter().all(|entry| entry.status == b'A'));
        let to_empty = raw_diff_tree(&git, dir, &full, &empty_commit);
        let statuses = raw_diff_entries(&to_empty).unwrap();
        assert!(!statuses.is_empty() && statuses.iter().all(|entry| entry.status == b'D'));

        let sha256 = tempdir().unwrap();
        let dir256 = sha256.path();
        git.run(dir256, &["init", "--object-format=sha256", "-b", "main"]);
        let blob256 = |bytes: &[u8]| write_blob(&git, dir256, bytes);
        let mktree256 = |records: &[Vec<u8>]| {
            git_with_input(&git, dir256, &["mktree", "-z"], &records.concat())
        };
        let commit256 =
            |tree: &str| git_with_input(&git, dir256, &["commit-tree", tree], b"diff\n");
        let (one, two) = (blob256(b"one\n"), blob256(b"two\n"));
        let base_deep = mktree256(&[file(&one, b"h")]);
        let commit_deep = mktree256(&[file(&two, b"h")]);
        let base256 = commit256(&mktree256(&[
            file(&one, b"f"),
            file(&one, b"g"),
            tree(&mktree256(&[tree(&base_deep, b"deep")]), b"nested"),
        ]));
        let head256 = commit256(&mktree256(&[
            file(&two, b"f"),
            file(&one, b"i"),
            tree(&mktree256(&[tree(&commit_deep, b"deep")]), b"nested"),
        ]));
        assert_eq!(head256.len(), 64, "not a SHA-256 id: {head256}");
        let forward256 = assert_listing_diff_matches(&engine, &git, dir256, &base256, &head256);
        for claimed in [
            b" M\0f\0".as_slice(),
            b" D\0g\0",
            b" A\0i\0",
            b" M\0nested/deep/h\0",
        ] {
            assert!(
                holds_bytes(&forward256, claimed),
                "git's SHA-256 diff lacks {:?}",
                String::from_utf8_lossy(claimed)
            );
        }
    }

    /// Whether `cat-file -e <id>` finds the object in `repo`.
    fn object_exists(git: &GitFixture, repo: &Path, id: &str) -> bool {
        git.cmd(repo)
            .args(["cat-file", "-e", id])
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn test_read_verified_object_checks_bytes_against_the_id() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, _base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let blob = git.output(dir, &["rev-parse", "HEAD:templates/f"]);
        let tree = git.output(dir, &["rev-parse", "HEAD:templates"]);
        let raw_tree = git
            .cmd(dir)
            .args(["cat-file", "tree", &tree])
            .output()
            .unwrap()
            .stdout;

        // Opposite direction: genuine objects read with their exact bytes.
        assert_eq!(
            engine.read_verified_object(dir, blob.as_bytes(), "blob"),
            Ok(b"f base\n".to_vec())
        );
        assert_eq!(
            engine.read_verified_object(dir, tree.as_bytes(), "tree"),
            Ok(raw_tree)
        );

        let other_tree = git_with_input(
            &git,
            dir,
            &["mktree"],
            format!("100644 blob {blob}\tg\n").as_bytes(),
        );
        forge_loose_object(dir, &tree, &other_tree);
        assert_eq!(
            engine.read_verified_object(dir, tree.as_bytes(), "tree"),
            Err(ObjectDefect::IdMismatch)
        );

        let source = write_blob(&git, dir, b"f other\n");
        forge_loose_object(dir, &blob, &source);
        let object_file = dir
            .join(".git/objects")
            .join(&source[..2])
            .join(&source[2..]);
        fs::remove_file(object_file).unwrap();
        assert!(
            !object_exists(&git, dir, &source),
            "the source blob is still there"
        );
        assert_eq!(
            engine.read_verified_object(dir, blob.as_bytes(), "blob"),
            Err(ObjectDefect::IdMismatch)
        );
        assert!(
            !object_exists(&git, dir, &source),
            "the check wrote the object its bytes hash to"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_read_verified_object_refuses_unreadable_ids() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, _base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let blob = git.output(dir, &["rev-parse", "HEAD:templates/f"]);
        let tag = tag_on_blob(&git, dir, &blob);
        let missing = "1".repeat(blob.len());
        assert!(!object_exists(&git, dir, &missing));

        let rows: [(&str, &[u8], &str); 4] = [
            ("a tag on a blob read as a blob", tag.as_bytes(), "blob"),
            ("a missing full-length id", missing.as_bytes(), "blob"),
            ("a non-UTF-8 id", b"\xff\xfe", "blob"),
            ("a blob read as a tree", blob.as_bytes(), "tree"),
        ];
        let mut failures = Vec::new();
        for (name, oid, kind) in rows {
            let verdict = engine.read_verified_object(dir, oid, kind);
            if verdict != Err(ObjectDefect::Unreadable) {
                failures.push(format!("{name}: {verdict:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn test_read_verified_object_in_sha256_repository() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let repo = tempdir().unwrap();
        let dir = repo.path();
        git.run(dir, &["init", "--object-format=sha256", "-b", "main"]);
        let blob = write_blob(&git, dir, b"sha256 blob\n");
        assert_eq!(blob.len(), 64, "not a SHA-256 id: {blob}");
        assert_eq!(
            engine.read_verified_object(dir, blob.as_bytes(), "blob"),
            Ok(b"sha256 blob\n".to_vec())
        );

        let source = write_blob(&git, dir, b"other\n");
        forge_loose_object(dir, &blob, &source);
        assert_eq!(
            engine.read_verified_object(dir, blob.as_bytes(), "blob"),
            Err(ObjectDefect::IdMismatch)
        );
    }

    #[test]
    fn test_read_verified_object_refuses_an_abbreviated_id() {
        const ABBREVIATED_HEX: usize = 12;
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, _base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let blob = git.output(dir, &["rev-parse", "HEAD:templates/f"]);
        let abbreviated = &blob[..ABBREVIATED_HEX];
        // The abbreviation resolves, so only the whole-id compare refuses it.
        assert_eq!(
            git.output(dir, &["cat-file", "blob", abbreviated]),
            "f base"
        );
        assert_eq!(
            engine.read_verified_object(dir, abbreviated.as_bytes(), "blob"),
            Err(ObjectDefect::IdMismatch)
        );
    }

    /// The directory of [`GATE_REL`].
    #[cfg(unix)]
    const GENERIC_REL: &str = "templates/projects/generic";

    /// Primes the index's cache-tree of `checkout` with `git write-tree`, as a hook can, so a
    /// rewritten subtree object stays out of `git status` and out of the staged diff.
    #[cfg(unix)]
    fn prime_cache_tree(git: &GitFixture, checkout: &Path) {
        git.run(checkout, &["write-tree"]);
    }

    /// Gives the generic tree of `checkout`'s HEAD the object file of a tree made with
    /// `git mktree` from `listing`, so a walk of HEAD reads `listing` there.
    #[cfg(unix)]
    fn forge_generic_tree(git: &GitFixture, checkout: &Path, listing: &str) {
        let generic = git.output(checkout, &["rev-parse", &format!("HEAD:{GENERIC_REL}")]);
        let forged = git_with_input(git, checkout, &["mktree"], listing.as_bytes());
        forge_loose_object(checkout, &generic, &forged);
    }

    /// The `mktree` input of a generic tree listing the [`HOOKED_TEMPLATE`] blob, which it
    /// writes, at `gitconfig.repo`.
    #[cfg(unix)]
    fn generic_listing_of_validated_blob(git: &GitFixture, checkout: &Path) -> String {
        let validated = write_blob(git, checkout, HOOKED_TEMPLATE.as_bytes());
        format!("100644 blob {validated}\tgitconfig.repo\n")
    }

    /// The id of the remote's generic tree on `main`.
    #[cfg(unix)]
    fn generic_tree_id(git: &GitFixture, bare: &Path) -> String {
        git.output(bare, &["rev-parse", &format!("main:{GENERIC_REL}")])
    }

    /// The fetched head's generic tree object, rewritten in the cache checkout to list the
    /// validated blob while status is empty: refused by the id check, the remote untouched.
    /// Control: the same step without the rewrite is refused as not held.
    #[cfg(unix)]
    fn assert_no_change_subtree_not_matching_its_id_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            let listing = generic_listing_of_validated_blob(&git, checkout);
            forge_generic_tree(&git, checkout, &listing);
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        let validated = git_with_input(
            &git,
            &pushed.checkout,
            &["hash-object", "--stdin"],
            HOOKED_TEMPLATE.as_bytes(),
        );
        // What a walk without the check sees.
        let walked = git.output(
            &pushed.checkout,
            &[
                "--no-replace-objects",
                "ls-tree",
                "-r",
                &pushed.fetched_sha,
                "--",
                GATE_REL,
            ],
        );
        assert!(
            walked.contains(&validated),
            "the forgery is not walked: {walked}"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            pushed
        };
        let control = push_with_empty_status(&git, &engine, open_pr, &step);
        let err = assert_empty_status_refused(&git, &control);
        assert!(err.contains(NOT_HELD), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_subtree_not_matching_its_id() {
        assert_no_change_subtree_not_matching_its_id_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_subtree_not_matching_its_id() {
        assert_no_change_subtree_not_matching_its_id_refused(true);
    }

    /// A deleted GATE_REL hidden by assume-unchanged, under a generic tree object rewritten
    /// to list only another file: the deletion reads as gone while the remote still holds
    /// the path. Refused by the id check naming the staged path. Control: the same step
    /// without the rewrite is refused because the path is still present.
    #[cfg(unix)]
    fn assert_no_change_subtree_hiding_a_deleted_path_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let delete = |checkout: &Path, managed: &Path| {
            git.run(checkout, &["update-index", "--assume-unchanged", GATE_REL]);
            fs::remove_file(managed).unwrap();
            prime_cache_tree(&git, checkout);
            (vec![managed.to_path_buf()], ValidatedBlobs::default())
        };
        let step = |checkout: &Path, managed: &Path| {
            let pushed = delete(checkout, managed);
            let other = write_blob(&git, checkout, b"other\n");
            forge_generic_tree(
                &git,
                checkout,
                &format!("100644 blob {other}\tother.repo\n"),
            );
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        let names = git.output(
            &pushed.checkout,
            &[
                "--no-replace-objects",
                "ls-tree",
                "-r",
                "--name-only",
                &pushed.fetched_sha,
            ],
        );
        assert!(
            !names.lines().any(|name| name == GATE_REL),
            "the forgery does not hide the path: {names}"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let control = push_with_empty_status(&git, &engine, open_pr, &delete);
        let err = assert_empty_status_refused(&git, &control);
        assert!(
            err.contains(&format!(
                "{GATE_REL} is staged for deletion and still present"
            )),
            "{err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_subtree_hiding_a_deleted_path() {
        assert_no_change_subtree_hiding_a_deleted_path_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_subtree_hiding_a_deleted_path() {
        assert_no_change_subtree_hiding_a_deleted_path_refused(true);
    }

    /// A loose object file and the bytes to put back into it.
    #[cfg(unix)]
    type SavedObjectFile = (PathBuf, Vec<u8>);

    /// A listing hook ([`SyncEngine::with_after_tree_listing`]) that, once armed, puts an
    /// object file's honest bytes back right after a gate's tree listing is read, so the
    /// listing sees a forgery the reads after it do not. Counts its calls.
    #[cfg(unix)]
    #[derive(Clone, Default)]
    struct ListingSwap {
        slot: std::sync::Arc<std::sync::Mutex<Option<SavedObjectFile>>>,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[cfg(unix)]
    impl ListingSwap {
        /// The hook: counts the call and, when armed, restores the saved bytes once. The file
        /// is removed before it is written, because a local clone hardlinks loose objects.
        fn hook(&self) -> impl Fn() + Send + Sync + 'static {
            let (slot, calls) = (self.slot.clone(), self.calls.clone());
            move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some((file, bytes)) = slot.lock().unwrap().take() {
                    fs::remove_file(&file).unwrap();
                    fs::write(&file, bytes).unwrap();
                }
            }
        }

        /// Saves the current bytes of `id`'s loose object file in `checkout`, to be put
        /// back by the hook. Panics unless that file exists.
        fn arm(&self, checkout: &Path, id: &str) {
            let file = checkout.join(".git/objects").join(&id[..2]).join(&id[2..]);
            assert!(file.is_file(), "no loose object file for {id}");
            let bytes = fs::read(&file).unwrap();
            *self.slot.lock().unwrap() = Some((file, bytes));
        }

        /// How many times the hook ran.
        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// The id of the generic tree of `checkout`'s HEAD, read through the root tree.
    #[cfg(unix)]
    fn checkout_generic_tree_id(git: &GitFixture, checkout: &Path) -> String {
        git.output(checkout, &["rev-parse", &format!("HEAD:{GENERIC_REL}")])
    }

    /// The fetched head's generic tree object rewritten to list the validated blob, and
    /// put back right after the gate's listing is read: refused by the id check, the
    /// remote untouched. Control: the same steps with the hook unarmed and no rewrite are
    /// refused as not held.
    #[cfg(unix)]
    fn assert_no_change_subtree_rewritten_only_while_listed_refused(open_pr: bool) {
        let git = GitFixture::new();
        let swap = ListingSwap::default();
        let engine = SyncEngine::new()
            .with_extra_env(git.gitconfig_env())
            .with_after_tree_listing(swap.hook());
        let generic = std::cell::RefCell::new(String::new());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            let listing = generic_listing_of_validated_blob(&git, checkout);
            let id = checkout_generic_tree_id(&git, checkout);
            swap.arm(checkout, &id);
            forge_generic_tree(&git, checkout, &listing);
            let validated = write_blob(&git, checkout, HOOKED_TEMPLATE.as_bytes());
            // What a walk without the check sees.
            let walked = git.output(
                checkout,
                &[
                    "--no-replace-objects",
                    "ls-tree",
                    "-r",
                    "HEAD",
                    "--",
                    GATE_REL,
                ],
            );
            assert!(
                walked.contains(&validated),
                "the forgery is not walked: {walked}"
            );
            *generic.borrow_mut() = id;
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        assert!(swap.calls() >= 1, "the listing hook did not run");
        assert!(
            remote_object_matches_id(&git, &pushed.checkout, &generic.borrow(), "tree"),
            "the hook did not put the honest generic tree back"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(NOT_HELD), "{err}");
        let remote_generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &remote_generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            pushed
        };
        let control = push_with_empty_status(&git, &engine, open_pr, &step);
        let err = assert_empty_status_refused(&git, &control);
        assert!(err.contains(NOT_HELD), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_subtree_rewritten_only_while_listed() {
        assert_no_change_subtree_rewritten_only_while_listed_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_subtree_rewritten_only_while_listed() {
        assert_no_change_subtree_rewritten_only_while_listed_refused(true);
    }

    /// A deleted GATE_REL hidden by assume-unchanged, under a generic tree object rewritten
    /// to list only another file and put back right after the gate's listing is read:
    /// refused by the id check naming the staged path, the remote untouched. Control: the
    /// same steps with the hook unarmed and no rewrite are refused because the path is
    /// still present.
    #[cfg(unix)]
    fn assert_no_change_deletion_hidden_only_while_listed_refused(open_pr: bool) {
        let git = GitFixture::new();
        let swap = ListingSwap::default();
        let engine = SyncEngine::new()
            .with_extra_env(git.gitconfig_env())
            .with_after_tree_listing(swap.hook());
        let delete = |checkout: &Path, managed: &Path| {
            git.run(checkout, &["update-index", "--assume-unchanged", GATE_REL]);
            fs::remove_file(managed).unwrap();
            prime_cache_tree(&git, checkout);
            (vec![managed.to_path_buf()], ValidatedBlobs::default())
        };
        let generic = std::cell::RefCell::new(String::new());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = delete(checkout, managed);
            let other = write_blob(&git, checkout, b"other\n");
            let id = checkout_generic_tree_id(&git, checkout);
            swap.arm(checkout, &id);
            forge_generic_tree(
                &git,
                checkout,
                &format!("100644 blob {other}\tother.repo\n"),
            );
            let names = git.output(
                checkout,
                &[
                    "--no-replace-objects",
                    "ls-tree",
                    "-r",
                    "--name-only",
                    "HEAD",
                ],
            );
            assert!(
                !names.lines().any(|name| name == GATE_REL),
                "the forgery does not hide the path: {names}"
            );
            *generic.borrow_mut() = id;
            pushed
        };
        let pushed = push_with_empty_status(&git, &engine, open_pr, &step);
        assert!(swap.calls() >= 1, "the listing hook did not run");
        assert!(
            remote_object_matches_id(&git, &pushed.checkout, &generic.borrow(), "tree"),
            "the hook did not put the honest generic tree back"
        );
        let err = assert_empty_status_refused(&git, &pushed);
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let remote_generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &remote_generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let control = push_with_empty_status(&git, &engine, open_pr, &delete);
        let err = assert_empty_status_refused(&git, &control);
        assert!(
            err.contains(&format!(
                "{GATE_REL} is staged for deletion and still present"
            )),
            "{err}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_deletion_hidden_only_while_listed() {
        assert_no_change_deletion_hidden_only_while_listed_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_deletion_hidden_only_while_listed() {
        assert_no_change_deletion_hidden_only_while_listed_refused(true);
    }

    /// The committed route for the tree above GATE_REL: after write-tree primes the
    /// cache-tree, the generic tree object is rewritten to list the validated blob, and the
    /// commit, whose new file is in another directory, reuses that tree's id. Refused by the
    /// id check, the remote untouched. Control: the same steps without the rewrite are
    /// refused because the commit does not hold the validated content.
    #[cfg(unix)]
    fn assert_unchanged_subtree_not_matching_its_id_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template_beside_added(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            let listing = generic_listing_of_validated_blob(&git, checkout);
            forge_generic_tree(&git, checkout, &listing);
            pushed
        };
        let pushed = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &pushed);
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template_beside_added(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            pushed
        };
        let control = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &control);
        assert!(
            err.contains("the commit to publish does not hold the validated content of"),
            "{err}"
        );
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_unchanged_subtree_not_matching_its_id() {
        assert_unchanged_subtree_not_matching_its_id_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_unchanged_subtree_not_matching_its_id() {
        assert_unchanged_subtree_not_matching_its_id_refused(true);
    }

    /// The step of the base-side rows: the template edited and recorded, a hook-style
    /// `update-index` adding [`HIDDEN_REL`] to the index at the seed blob (which the remote
    /// holds), and `pack.window 0` in the checkout's config. With `forge`, the fetched head's
    /// generic tree object is also given the bytes of a tree listing [`HIDDEN_REL`] at the same
    /// blob, so neither the staged diff nor a diff against the fetched head sees the addition.
    #[cfg(unix)]
    fn add_file_hidden_by_base_tree(
        git: &GitFixture,
        checkout: &Path,
        managed: &Path,
        forge: bool,
    ) -> StepResult {
        let pushed = write_hooked_template(managed);
        let seed = git.output(checkout, &["rev-parse", &format!("HEAD:{GATE_REL}")]);
        git.run(
            checkout,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{seed},{HIDDEN_REL}"),
            ],
        );
        if forge {
            forge_generic_tree(
                git,
                checkout,
                &format!("100644 blob {seed}\tgitconfig.repo\n100644 blob {seed}\thidden.repo\n"),
            );
        }
        git.run(checkout, &["config", "pack.window", "0"]);
        if forge {
            let walked = git.output(
                checkout,
                &[
                    "--no-replace-objects",
                    "ls-tree",
                    "-r",
                    "--name-only",
                    "HEAD",
                ],
            );
            assert!(
                walked.lines().any(|path| path == HIDDEN_REL),
                "the walk of HEAD does not read the forged generic tree: {walked}"
            );
        }
        pushed
    }

    /// The path the base-side rows add and hide.
    #[cfg(unix)]
    const HIDDEN_REL: &str = "templates/projects/generic/hidden.repo";

    /// The base-side channel: an added file hidden by a fetched-head tree that does not match
    /// its id. Refused by the committed gate's id check naming [`GATE_REL`] and no name read
    /// from a tree; no remote head moves, the remote's main does not list the hidden path and
    /// its generic tree hashes to its id. Control: the same steps without the rewrite are
    /// refused by the staged gate.
    #[cfg(unix)]
    fn assert_added_file_hidden_by_base_tree_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let forged = |checkout: &Path, managed: &Path| {
            add_file_hidden_by_base_tree(&git, checkout, managed, true)
        };
        let pushed = push_after_step(&git, &engine, open_pr, &forged);
        let err = assert_push_refused(&git, &pushed);
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        assert!(
            !err.contains("hidden.repo"),
            "the refusal printed a name read from a tree: {err}"
        );
        let published = git.output(
            pushed.bare.path(),
            &["ls-tree", "-r", "--name-only", "main"],
        );
        assert!(
            !published.lines().any(|path| path == HIDDEN_REL),
            "the remote's main lists the hidden path: {published}"
        );
        let generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let honest = |checkout: &Path, managed: &Path| {
            add_file_hidden_by_base_tree(&git, checkout, managed, false)
        };
        let control = push_after_step(&git, &engine, open_pr, &honest);
        let err = assert_push_refused(&git, &control);
        assert!(
            err.contains("unvalidated file staged in remote checkout"),
            "{err}"
        );
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id() {
        assert_added_file_hidden_by_base_tree_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_added_file_hidden_by_base_tree_not_matching_its_id() {
        assert_added_file_hidden_by_base_tree_refused(true);
    }

    /// The committed route: the generic tree object rewritten to list the validated blob, the
    /// commit (whose new file is in another directory) reusing that tree's id, and the honest
    /// bytes put back right after the gate's listing is read: refused by the id check, the
    /// remote untouched. Control: the same steps with the hook unarmed and no rewrite are
    /// refused because the commit does not hold the validated content.
    #[cfg(unix)]
    fn assert_unchanged_subtree_rewritten_only_while_listed_refused(open_pr: bool) {
        let git = GitFixture::new();
        let swap = ListingSwap::default();
        let engine = SyncEngine::new()
            .with_extra_env(git.gitconfig_env())
            .with_after_tree_listing(swap.hook());
        let generic = std::cell::RefCell::new(String::new());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template_beside_added(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            let listing = generic_listing_of_validated_blob(&git, checkout);
            let id = checkout_generic_tree_id(&git, checkout);
            swap.arm(checkout, &id);
            forge_generic_tree(&git, checkout, &listing);
            let validated = write_blob(&git, checkout, HOOKED_TEMPLATE.as_bytes());
            // What a walk without the check sees.
            let walked = git.output(
                checkout,
                &[
                    "--no-replace-objects",
                    "ls-tree",
                    "-r",
                    "HEAD",
                    "--",
                    GATE_REL,
                ],
            );
            assert!(
                walked.contains(&validated),
                "the forgery is not walked: {walked}"
            );
            *generic.borrow_mut() = id;
            pushed
        };
        let pushed = push_after_step(&git, &engine, open_pr, &step);
        assert!(swap.calls() >= 1, "the listing hook did not run");
        assert!(
            remote_object_matches_id(&git, &pushed.checkout, &generic.borrow(), "tree"),
            "the hook did not put the honest generic tree back"
        );
        let err = assert_push_refused(&git, &pushed);
        assert!(
            err.contains("Post-commit publication gate violation"),
            "{err}"
        );
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        let remote_generic = generic_tree_id(&git, pushed.bare.path());
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &remote_generic, "tree"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template_beside_added(&git, checkout, managed);
            prime_cache_tree(&git, checkout);
            pushed
        };
        let control = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &control);
        assert!(
            err.contains("the commit to publish does not hold the validated content of"),
            "{err}"
        );
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(ID_MISMATCH), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_unchanged_subtree_rewritten_only_while_listed() {
        assert_unchanged_subtree_rewritten_only_while_listed_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_unchanged_subtree_rewritten_only_while_listed() {
        assert_unchanged_subtree_rewritten_only_while_listed_refused(true);
    }

    /// Writes, in `repo`, a root tree listing `templates/projects/generic/gitconfig.repo` at
    /// the [`HOOKED_TEMPLATE`] blob (which it writes) and nothing else, through a `mktree`
    /// chain; returns its id. A HEAD whose root tree reads as these bytes holds the record.
    #[cfg(unix)]
    fn root_listing_validated_blob(git: &GitFixture, repo: &Path) -> String {
        let mktree = |listing: String| git_with_input(git, repo, &["mktree"], listing.as_bytes());
        let validated = write_blob(git, repo, HOOKED_TEMPLATE.as_bytes());
        let generic = mktree(format!("100644 blob {validated}\tgitconfig.repo\n"));
        let projects = mktree(format!("040000 tree {generic}\tgeneric\n"));
        let templates = mktree(format!("040000 tree {projects}\tprojects\n"));
        mktree(format!("040000 tree {templates}\ttemplates\n"))
    }

    /// The fetched head's root tree object, rewritten in the cache checkout to the bytes of
    /// a root that would read as holding the record ([`root_listing_validated_blob`]):
    /// refused at `git status`, which is git's own check of the root tree and runs before
    /// the gate, and the remote untouched. The gate's own root check is pinned by
    /// test_fetched_head_holds_refuses_root_tree_not_matching_its_id. Control: the same
    /// step without the rewrite reaches the gate and is refused as not held.
    #[cfg(unix)]
    fn assert_fetched_root_tree_not_matching_its_id_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let step = |checkout: &Path, managed: &Path| {
            let pushed = hide_hooked_template(&git, checkout, managed);
            let forged = root_listing_validated_blob(&git, checkout);
            let root = git.output(checkout, &["rev-parse", "HEAD^{tree}"]);
            forge_loose_object(checkout, &root, &forged);
            pushed
        };
        let pushed = push_after_step(&git, &engine, open_pr, &step);
        let err = assert_push_refused(&git, &pushed);
        assert!(err.contains("git status failed"), "{err}");
        let root = git.output(pushed.bare.path(), &["rev-parse", "main^{tree}"]);
        assert!(
            remote_object_matches_id(&git, pushed.bare.path(), &root, "tree"),
            "the forgery reached the bare remote's object"
        );

        let step = |checkout: &Path, managed: &Path| hide_hooked_template(&git, checkout, managed);
        let control = push_with_empty_status(&git, &engine, open_pr, &step);
        let err = assert_empty_status_refused(&git, &control);
        assert!(err.contains(NOT_HELD), "{err}");
        assert!(!err.contains("git status failed"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_fetched_root_tree_not_matching_its_id() {
        assert_fetched_root_tree_not_matching_its_id_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_fetched_root_tree_not_matching_its_id() {
        assert_fetched_root_tree_not_matching_its_id_refused(true);
    }

    /// The gate's own check of the fetched head's root tree, called directly so `git status`
    /// never runs. The record is [`HOOKED_TEMPLATE`] at GATE_REL. At the base commit the
    /// head does not hold it. A commit whose root is [`root_listing_validated_blob`] holds
    /// it: the same bytes under their own id read as held. With HEAD back on the base commit
    /// and its root tree file given those bytes, the gate refuses on the id check alone.
    #[test]
    #[cfg(unix)]
    fn test_fetched_head_holds_refuses_root_tree_not_matching_its_id() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, file) = gate_repo(&git);
        let dir = repo.path();
        fs::write(&file, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));
        let staged = std::slice::from_ref(&file);
        let base = git.output(dir, &["rev-parse", "HEAD"]);
        let holds = |head: &str| {
            engine.require_fetched_head_holds(&checkout_target(dir, head), staged, &validated)
        };

        let err = holds(&base).expect_err("the base commit does not hold the record");
        assert!(err.contains(NOT_HELD), "{err}");

        let root = root_listing_validated_blob(&git, dir);
        let holding = git_with_input(
            &git,
            dir,
            &["commit-tree", &root, "-p", &base],
            b"holds the record\n",
        );
        git.run(dir, &["update-ref", "refs/heads/main", &holding]);
        assert_eq!(holds(&holding), Ok(()));

        git.run(dir, &["update-ref", "refs/heads/main", &base]);
        let base_root = git.output(dir, &["rev-parse", &format!("{base}^{{tree}}")]);
        forge_loose_object(dir, &base_root, &root);
        let err = holds(&base).expect_err("a root tree not matching its id must be refused");
        assert!(err.contains(ID_MISMATCH), "{err}");
        assert!(err.contains(GATE_REL), "{err}");
        assert!(!err.contains(NOT_HELD), "{err}");
    }

    /// post-checkout tail, run by `checkout -B` before ensure's `reset --hard`: points
    /// `refs/replace/<HEAD's tree>` at that tree with the blob of
    /// `.git/ude-test-validated-bytes` at [`GATE_REL`], so the reset fills the index and the
    /// working tree with the validated bytes. Leaves the tree id and the blob id in
    /// `.git/ude-test-replaced-tree` and `.git/ude-test-injected-blob`.
    #[cfg(unix)]
    const HOOK_REPLACE_FETCHED_TREE: &str = r#"B=$(git hash-object -w --no-filters .git/ude-test-validated-bytes)
T=$(git rev-parse 'HEAD^{tree}')
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree "$T"
git update-index --add --cacheinfo 100644,"$B",templates/projects/generic/gitconfig.repo
R=$(git write-tree)
unset GIT_INDEX_FILE
git update-ref "refs/replace/$T" "$R"
printf '%s\n' "$T" > .git/ude-test-replaced-tree
printf '%s\n' "$B" > .git/ude-test-injected-blob
"#;

    /// A replace ref on the fetched head's tree makes status empty with the validated bytes
    /// in the working tree and no index flag. The read past replace refs sees the seed's
    /// blob, so the push is refused on the empty-status route and nothing moves. Control:
    /// an ordinary `ls-tree` of the fetched head lists the validated blob, so a read
    /// without the option would have found the content held.
    #[cfg(unix)]
    fn assert_replace_ref_on_fetched_tree_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = |_: &Path, checkout: &Path| {
            fs::write(
                checkout.join(".git/ude-test-validated-bytes"),
                HOOKED_TEMPLATE,
            )
            .unwrap();
            install_checkout_hook(checkout, "post-checkout", "", HOOK_REPLACE_FETCHED_TREE);
        };
        let pushed = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), true);
        assert_hook_ran(&pushed);
        let tree = fs::read_to_string(pushed.checkout.join(".git/ude-test-replaced-tree"))
            .unwrap()
            .trim()
            .to_string();
        let blob = fs::read_to_string(pushed.checkout.join(".git/ude-test-injected-blob"))
            .unwrap()
            .trim()
            .to_string();
        git.run(
            &pushed.checkout,
            &["rev-parse", "--verify", &format!("refs/replace/{tree}")],
        );
        match &pushed.res {
            Err(PushRemoteError::Failed(e)) => {
                assert!(e.contains(EMPTY_STATUS_REFUSAL), "{e}");
                assert!(e.contains(NOT_HELD), "{e}");
            }
            other => panic!("expected the empty-status refusal, got {other:?}"),
        }
        assert_eq!(
            remote_heads(&git, pushed.bare.path()),
            pushed.heads_before,
            "a remote head moved"
        );
        assert_no_local_sync_branch(&git, &pushed.checkout);

        let fetched = pushed
            .fetched_sha
            .clone()
            .expect("the ensure returned a cache");
        let seed_blob = git.output(
            pushed.bare.path(),
            &["rev-parse", &format!("main:{GATE_REL}")],
        );
        assert_ne!(seed_blob, blob);
        let replaced = git.output(&pushed.checkout, &["ls-tree", &fetched, GATE_REL]);
        assert!(replaced.contains(&blob), "{replaced}");
        let raw = git.output(
            &pushed.checkout,
            &["--no-replace-objects", "ls-tree", &fetched, GATE_REL],
        );
        assert!(raw.contains(&seed_blob), "{raw}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_no_changes_refuses_replace_ref_on_the_fetched_tree() {
        assert_replace_ref_on_fetched_tree_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_no_changes_refuses_replace_ref_on_the_fetched_tree() {
        assert_replace_ref_on_fetched_tree_refused(true);
    }

    /// The empty-status check on its own, over a plain repository with two committed
    /// templates: one recorded path held passes; one held and one not refuses naming the
    /// one not held; a staged path with no record present in HEAD refuses and absent
    /// passes; nothing staged and nothing recorded passes without reading HEAD (the
    /// fetched head is empty, which any read refuses: last row); HEAD other than the
    /// fetched head refuses.
    #[test]
    fn test_require_fetched_head_holds_edges() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let (repo, held) = gate_repo(&git);
        let other_rel = "templates/projects/generic/gitignore.repo";
        let other = repo.path().join(other_rel);
        fs::write(&other, "target/\n").unwrap();
        git.run(repo.path(), &["add", "-A"]);
        git.run(repo.path(), &["commit", "-q", "-m", "second template"]);
        let head = git.output(repo.path(), &["rev-parse", "HEAD"]);
        let at_head = checkout_target(repo.path(), &head);
        let check = |target: &PublishTarget<'_>, staged: &[PathBuf], record: &ValidatedBlobs| {
            engine.require_fetched_head_holds(target, staged, record)
        };

        let one = ValidatedBlobs::from_files(std::slice::from_ref(&held));
        assert_eq!(check(&at_head, std::slice::from_ref(&held), &one), Ok(()));

        fs::write(&other, "target/\nnode_modules/\n").unwrap();
        let both_paths = [held.clone(), other.clone()];
        let both = ValidatedBlobs::from_files(&both_paths);
        let err = check(&at_head, &both_paths, &both).expect_err("one path is not held");
        assert!(err.contains(&format!("{NOT_HELD} of {other_rel}")), "{err}");
        assert!(!err.contains(GATE_REL), "{err}");
        fs::write(&other, "target/\n").unwrap();

        let none = ValidatedBlobs::default();
        let err = check(&at_head, std::slice::from_ref(&other), &none)
            .expect_err("a deleted path still in HEAD");
        assert!(
            err.contains(&format!(
                "{other_rel} is staged for deletion and still present"
            )),
            "{err}"
        );
        let absent = repo.path().join("templates/projects/generic/absent.repo");
        assert_eq!(check(&at_head, &[absent], &none), Ok(()));

        let no_fetched_head = checkout_target(repo.path(), "");
        assert_eq!(check(&no_fetched_head, &[], &none), Ok(()));

        let err = check(&no_fetched_head, std::slice::from_ref(&held), &one)
            .expect_err("HEAD is not the fetched head");
        assert!(err.contains("is not the fetched head"), "{err}");
    }

    /// Shell lines that write `templates/leak.txt` into a child of
    /// `refs/remotes/origin/main` and move that tracking ref to it, so the fetched head is a
    /// commit the remote never held; with `reset` the worktree is also reset to it. The
    /// blob's id goes to `.git/ude-test-injected-blob`. Runs in a cache checkout's root.
    #[cfg(unix)]
    fn moved_tracking_ref_tail(reset: bool) -> String {
        let reset_line = if reset {
            "git reset -q --hard \"$N\"\n"
        } else {
            ""
        };
        format!(
            r#"IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
printf '%s\n' "$IB" > .git/ude-test-injected-blob
export GIT_INDEX_FILE=.git/ude-probe-idx
git read-tree refs/remotes/origin/main
git update-index --add --cacheinfo 100644,"$IB",templates/leak.txt
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p refs/remotes/origin/main -m injected-base)
git update-ref refs/remotes/origin/main "$N"
{reset_line}"#
        )
    }

    /// What a test inspects after [`push_after_hooked_ensure`].
    #[cfg(unix)]
    struct EnsuredPush {
        /// The ensure's refusal, or the push's result.
        res: Result<String, PushRemoteError>,
        bare: tempfile::TempDir,
        _cache: tempfile::TempDir,
        /// The cache checkout, found on disk: a refusing ensure returns no cache.
        checkout: PathBuf,
        /// The remote's heads just before the ensure under test.
        heads_before: Vec<(String, String)>,
        /// The fetched head the ensure under test returned, if it returned a cache.
        fetched_sha: Option<String>,
    }

    /// A channel's setup, run as `setup(bare, checkout)` after a first ensure.
    #[cfg(unix)]
    type ChannelSetup<'a> = &'a dyn Fn(&Path, &Path);

    /// The single cache checkout under `cache` (named `remote-*`, not its `.lock`).
    #[cfg(unix)]
    fn only_cache_checkout(cache: &Path) -> PathBuf {
        let checkouts: Vec<PathBuf> = fs::read_dir(cache)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy();
                name.starts_with("remote-") && !name.ends_with(".lock")
            })
            .collect();
        assert_eq!(checkouts.len(), 1, "{checkouts:?}");
        checkouts.into_iter().next().unwrap()
    }

    /// Seeds a bare remote; with `setup`, runs [`SyncEngine::ensure_remote_repo`] once and
    /// then `setup(bare, checkout)`; without it the ensure under test clones. The ensure
    /// under test is [`SyncEngine::ensure_remote_repo_for_push`] when `checked`, plain
    /// [`SyncEngine::ensure_remote_repo`] otherwise (the delivering control). On Ok it
    /// writes [`HOOKED_TEMPLATE`] over the managed template and pushes it.
    #[cfg(unix)]
    fn push_after_hooked_ensure(
        git: &GitFixture,
        engine: &SyncEngine,
        open_pr: bool,
        setup: Option<ChannelSetup<'_>>,
        checked: bool,
    ) -> EnsuredPush {
        let bare = seeded_bare_remote(git);
        let cache = tempdir().unwrap();
        let url = bare.path().to_str().unwrap();
        if let Some(setup) = setup {
            let first = engine
                .ensure_remote_repo(url, "main", cache.path())
                .unwrap();
            let checkout = first.checkout_dir.clone();
            drop(first);
            setup(bare.path(), &checkout);
        }
        let heads_before = remote_heads(git, bare.path());
        let ensured = if checked {
            engine.ensure_remote_repo_for_push(url, "main", cache.path())
        } else {
            engine
                .ensure_remote_repo(url, "main", cache.path())
                .map_err(PushRemoteError::Failed)
        };
        let checkout = only_cache_checkout(cache.path());
        let (res, fetched_sha) = match ensured {
            Err(e) => (Err(e), None),
            Ok(cached) => {
                let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
                fs::write(&managed, HOOKED_TEMPLATE).unwrap();
                let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
                let res = engine.push_to_remote(
                    &cached.publish_target("main"),
                    open_pr,
                    std::slice::from_ref(&managed),
                    &validated,
                );
                (res, Some(cached.fetched_sha.clone()))
            }
        };
        EnsuredPush {
            res,
            bare,
            _cache: cache,
            checkout,
            heads_before,
            fetched_sha,
        }
    }

    /// The text of the head check's refusal of a moved or redirected base.
    #[cfg(unix)]
    const MOVED_BASE_REFUSAL: &str = "fetched head is not the commit the remote branch holds";

    /// The hook's marker exists in the cache checkout.
    #[cfg(unix)]
    fn assert_hook_ran(pushed: &EnsuredPush) {
        assert!(
            pushed.checkout.join(".git/ude-test-hook-ran").exists(),
            "the hook never ran; result: {:?}",
            pushed.res
        );
    }

    /// The checked route refused with the head check's text: the hook ran, no remote head
    /// moved, and the injected blob is not on the remote.
    #[cfg(unix)]
    fn assert_checked_refused(git: &GitFixture, pushed: &EnsuredPush) {
        assert_hook_ran(pushed);
        match &pushed.res {
            Err(PushRemoteError::Failed(e)) => {
                assert!(e.contains(MOVED_BASE_REFUSAL), "{e}");
            }
            other => panic!("expected the head check's refusal, got {other:?}"),
        }
        assert_eq!(
            remote_heads(git, pushed.bare.path()),
            pushed.heads_before,
            "a remote head moved"
        );
        let blob = fs::read_to_string(pushed.checkout.join(".git/ude-test-injected-blob"))
            .unwrap()
            .trim()
            .to_string();
        assert!(
            !holds_object(git, pushed.bare.path(), &blob),
            "the remote holds the injected blob"
        );
    }

    /// The delivering control: without the head check the same state publishes, the
    /// fetched head is not the remote's head before the push, and `destination` (the
    /// repository the push reached) holds the injected blob.
    #[cfg(unix)]
    fn assert_control_delivers(git: &GitFixture, pushed: &EnsuredPush, destination: &Path) {
        assert_hook_ran(pushed);
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let main_before = pushed
            .heads_before
            .iter()
            .find(|(name, _)| name == "refs/heads/main")
            .map(|(_, id)| id.clone());
        assert_ne!(
            pushed.fetched_sha, main_before,
            "the fetched head was the remote's head, so the hook moved nothing"
        );
        let blob = fs::read_to_string(pushed.checkout.join(".git/ude-test-injected-blob"))
            .unwrap()
            .trim()
            .to_string();
        assert!(
            holds_object(git, destination, &blob),
            "the control did not deliver the injected blob"
        );
    }

    /// Installs the executable hook `name` in `checkout` behind `guard` and the one-shot
    /// marker `.git/ude-test-hook-ran`, then `tail`.
    #[cfg(unix)]
    fn install_checkout_hook(checkout: &Path, name: &str, guard: &str, tail: &str) {
        use std::os::unix::fs::PermissionsExt;
        let hook = checkout.join(".git/hooks").join(name);
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\n{guard}\n[ -e .git/ude-test-hook-ran ] && exit 0\n: > .git/ude-test-hook-ran\n{tail}"
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Runs `script` with `sh` in `dir`, with the fixture's git environment.
    #[cfg(unix)]
    fn sh_in(git: &GitFixture, dir: &Path, script: &str) {
        let mut cmd = Command::new("sh");
        cmd.current_dir(dir).arg("-c").arg(script);
        for name in process::GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        let out = cmd
            .envs(git.gitconfig_env())
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "sh step failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Runs the channel `setup` through the checked route (refused) and through plain
    /// ensure (the control, delivering to the remote).
    #[cfg(unix)]
    fn assert_channel_refused_with_control(open_pr: bool, setup: &dyn Fn(&Path, &Path)) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let checked = push_after_hooked_ensure(&git, &engine, open_pr, Some(setup), true);
        assert_checked_refused(&git, &checked);
        let control = push_after_hooked_ensure(&git, &engine, open_pr, Some(setup), false);
        assert_control_delivers(&git, &control, control.bare.path());
    }

    /// Setup: a post-checkout hook in the checkout runs `tail`.
    #[cfg(unix)]
    fn post_checkout_setup(tail: String) -> impl Fn(&Path, &Path) {
        move |_bare: &Path, checkout: &Path| {
            install_checkout_hook(checkout, "post-checkout", "", &tail);
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_moved_by_post_checkout_hook() {
        assert_channel_refused_with_control(
            false,
            &post_checkout_setup(moved_tracking_ref_tail(false)),
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_moved_by_post_checkout_hook() {
        assert_channel_refused_with_control(
            true,
            &post_checkout_setup(moved_tracking_ref_tail(false)),
        );
    }

    /// The moved base's blob is a GitHub PAT shape: refused by the head check, delivered
    /// by plain ensure, because the gate lists changes from the fetched head and never sees
    /// a file of the base.
    #[cfg(unix)]
    fn assert_token_on_moved_tracking_ref_refused(open_pr: bool) {
        let tail = with_injected_token(&moved_tracking_ref_tail(false));
        assert!(tail.contains(HOOK_TOKEN_BODY), "{tail}");
        assert_channel_refused_with_control(open_pr, &post_checkout_setup(tail));
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_token_on_moved_tracking_ref() {
        assert_token_on_moved_tracking_ref_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_token_on_moved_tracking_ref() {
        assert_token_on_moved_tracking_ref_refused(true);
    }

    /// The clone path: a global `core.hooksPath` post-checkout hook runs in the clone,
    /// moves the tracking ref and resets the worktree to it. There is no earlier ensure.
    #[cfg(unix)]
    fn assert_tracking_ref_moved_on_clone_path_refused(open_pr: bool) {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let hooks = tempdir().unwrap();
        let hook = hooks.path().join("post-checkout");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\n[ -e .git/ude-test-hook-ran ] && exit 0\n: > .git/ude-test-hook-ran\n{}",
                moved_tracking_ref_tail(true)
            ),
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let global = hooks.path().join("gitconfig");
        fs::write(
            &global,
            format!(
                "{FIXTURE_GITCONFIG}[core]\n\thooksPath = {}\n",
                hooks.path().display()
            ),
        )
        .unwrap();
        let engine = SyncEngine::new().with_extra_env(vec![
            ("GIT_CONFIG_GLOBAL".into(), global.into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            (
                "GIT_CEILING_DIRECTORIES".into(),
                std::env::temp_dir().into(),
            ),
        ]);
        let checked = push_after_hooked_ensure(&git, &engine, open_pr, None, true);
        assert_checked_refused(&git, &checked);
        let control = push_after_hooked_ensure(&git, &engine, open_pr, None, false);
        assert_control_delivers(&git, &control, control.bare.path());
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_moved_on_clone_path() {
        assert_tracking_ref_moved_on_clone_path_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_moved_on_clone_path() {
        assert_tracking_ref_moved_on_clone_path_refused(true);
    }

    /// Setup: a reference-transaction hook moves the tracking ref once a transaction of
    /// the next ensure is committed. The marker matters: `git update-ref` re-enters it.
    #[cfg(unix)]
    fn reference_transaction_setup(_bare: &Path, checkout: &Path) {
        install_checkout_hook(
            checkout,
            "reference-transaction",
            "[ \"$1\" = committed ] || exit 0",
            &moved_tracking_ref_tail(false),
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_moved_by_reference_transaction_hook() {
        assert_channel_refused_with_control(false, &reference_transaction_setup);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_moved_by_reference_transaction_hook() {
        assert_channel_refused_with_control(true, &reference_transaction_setup);
    }

    /// Setup, as an earlier hook would leave it: the tracking ref already moved, and
    /// `remote.origin.fetch` maps nothing onto it, so the next fetch leaves it in place.
    /// No hook runs during the ensure under test; the setup writes the marker itself.
    #[cfg(unix)]
    fn fetch_config_setup(git: &GitFixture) -> impl Fn(&Path, &Path) + '_ {
        move |_bare: &Path, checkout: &Path| {
            sh_in(
                git,
                checkout,
                &format!(
                    ": > .git/ude-test-hook-ran\n{}",
                    moved_tracking_ref_tail(false)
                ),
            );
            git.run(
                checkout,
                &[
                    "config",
                    "remote.origin.fetch",
                    "+refs/heads/does-not-exist:refs/remotes/origin/nothing",
                ],
            );
        }
    }

    /// The fetch config channel, both routes; each run checks the config was still in
    /// place after the ensure.
    #[cfg(unix)]
    fn assert_fetch_config_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = fetch_config_setup(&git);
        let kept = |pushed: &EnsuredPush| {
            assert_eq!(
                git.output(&pushed.checkout, &["config", "remote.origin.fetch"]),
                "+refs/heads/does-not-exist:refs/remotes/origin/nothing"
            );
        };
        let checked = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), true);
        kept(&checked);
        assert_checked_refused(&git, &checked);
        let control = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), false);
        kept(&control);
        assert_control_delivers(&git, &control, control.bare.path());
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_kept_by_hook_written_fetch_config() {
        assert_fetch_config_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_kept_by_hook_written_fetch_config() {
        assert_fetch_config_refused(true);
    }

    /// A bare clone of `bare` at `<checkout>/.git/ude-test-other.git` whose main is a child
    /// of the remote's main holding `templates/leak.txt`; the blob's id goes to
    /// `<checkout>/.git/ude-test-injected-blob`.
    #[cfg(unix)]
    fn other_remote_with_injected_base(git: &GitFixture, bare: &Path, checkout: &Path) -> PathBuf {
        let other = checkout.join(".git/ude-test-other.git");
        git.run(
            checkout,
            &[
                "clone",
                "-q",
                "--bare",
                bare.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        sh_in(
            git,
            &other,
            r#"IB=$(printf 'INJECTED_BY_HOOK\n' | git hash-object -w --stdin)
export GIT_INDEX_FILE=ude-probe-idx
git read-tree main
git update-index --add --cacheinfo 100644,"$IB",templates/leak.txt
T=$(git write-tree)
unset GIT_INDEX_FILE
N=$(git commit-tree "$T" -p main -m injected-base)
git update-ref refs/heads/main "$N"
"#,
        );
        let blob = git.output(&other, &["rev-parse", "main:templates/leak.txt"]);
        fs::write(
            checkout.join(".git/ude-test-injected-blob"),
            format!("{blob}\n"),
        )
        .unwrap();
        let base = git.output(&other, &["rev-parse", "main"]);
        fs::write(checkout.join(".git/ude-test-injected-base"), base).unwrap();
        other
    }

    /// Where a redirect channel's config sends a push to the caller's URL.
    #[cfg(unix)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum PushGoesTo {
        /// The push names the URL, so `remote.origin.*` does not apply to it.
        CallersRemote,
        /// The config rewrites the URL itself, so the push follows it.
        OtherRepository,
    }

    /// A redirect channel: `config(checkout, other)` points the checkout's fetch at another
    /// repository holding an injected base. The checked route refuses. The control
    /// fetches the injected base and publishes a commit on top of it. Where `push` is
    /// [`PushGoesTo::CallersRemote`] that commit and the injected blob reach the caller's
    /// remote and the result is Ok. Where it is [`PushGoesTo::OtherRepository`] the commit
    /// lands in the other repository, the read-back refuses, and the caller's remote keeps
    /// its heads.
    #[cfg(unix)]
    fn assert_redirect_refused(
        open_pr: bool,
        push: PushGoesTo,
        config: &dyn Fn(&GitFixture, &Path, &Path),
    ) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = |bare: &Path, checkout: &Path| {
            let other = other_remote_with_injected_base(&git, bare, checkout);
            fs::write(checkout.join(".git/ude-test-hook-ran"), "").unwrap();
            config(&git, checkout, &other);
        };
        let checked = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), true);
        assert_checked_refused(&git, &checked);
        let control = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), false);
        let other = control.checkout.join(".git/ude-test-other.git");
        let base =
            fs::read_to_string(control.checkout.join(".git/ude-test-injected-base")).unwrap();
        assert_eq!(
            control.fetched_sha.as_deref(),
            Some(base.as_str()),
            "the control did not fetch the other repository's injected base"
        );
        let landed_in = match push {
            PushGoesTo::CallersRemote => {
                assert_control_delivers(&git, &control, control.bare.path());
                control.bare.path().to_path_buf()
            }
            PushGoesTo::OtherRepository => {
                match &control.res {
                    Err(PushRemoteError::Failed(e)) => assert!(e.contains(NOT_PUBLISHED), "{e}"),
                    res => panic!("expected the read-back's refusal, got {res:?}"),
                }
                assert_eq!(
                    remote_heads(&git, control.bare.path()),
                    control.heads_before,
                    "the caller's remote changed"
                );
                other
            }
        };
        let published = remote_heads(&git, &landed_in)
            .into_iter()
            .find(|(name, id)| {
                (open_pr != (name == "refs/heads/main")) && Some(id) != control.fetched_sha.as_ref()
            })
            .map(|(_, id)| id)
            .expect("the control's commit");
        assert_eq!(
            git.output(&landed_in, &["rev-parse", &format!("{published}~1")]),
            control.fetched_sha.clone().unwrap(),
            "the control's commit does not build on the injected base"
        );
    }

    /// Writes `remote.origin.url` in the checkout, as a hook could.
    #[cfg(unix)]
    fn origin_url_config(git: &GitFixture, checkout: &Path, other: &Path) {
        git.run(
            checkout,
            &["config", "remote.origin.url", other.to_str().unwrap()],
        );
    }

    /// Writes `url.<other>.insteadOf <remote>` in the checkout, as a hook could.
    #[cfg(unix)]
    fn instead_of_config(git: &GitFixture, checkout: &Path, other: &Path) {
        let origin = git.output(checkout, &["config", "remote.origin.url"]);
        git.run(
            checkout,
            &[
                "config",
                &format!("url.{}.insteadOf", other.display()),
                &origin,
            ],
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_fetch_redirected_by_hook_written_origin_url() {
        assert_redirect_refused(false, PushGoesTo::CallersRemote, &origin_url_config);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_fetch_redirected_by_hook_written_origin_url() {
        assert_redirect_refused(true, PushGoesTo::CallersRemote, &origin_url_config);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_fetch_redirected_by_hook_written_instead_of() {
        assert_redirect_refused(false, PushGoesTo::OtherRepository, &instead_of_config);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_fetch_redirected_by_hook_written_instead_of() {
        assert_redirect_refused(true, PushGoesTo::OtherRepository, &instead_of_config);
    }

    /// Setup: the managed template runs through a filter whose `kind` (`clean` or
    /// `smudge`) side passes the bytes through and, once, moves the tracking ref. With
    /// `clean` the file is made stat-dirty (mtime set back), so `git checkout -B` re-hashes
    /// it; with `smudge` its content is changed, so `git reset --hard` rewrites it.
    #[cfg(unix)]
    fn filter_setup<'a>(git: &'a GitFixture, kind: &'static str) -> impl Fn(&Path, &Path) + 'a {
        move |_bare: &Path, checkout: &Path| {
            use std::os::unix::fs::PermissionsExt;
            let script = checkout.join(".git/ude-test-filter.sh");
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\nif [ ! -e .git/ude-test-hook-ran ]; then\n: > .git/ude-test-hook-ran\n{{\n{}}} >&2\nfi\ncat\n",
                    moved_tracking_ref_tail(false)
                ),
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            git.run(
                checkout,
                &[
                    "config",
                    &format!("filter.udetest.{kind}"),
                    script.to_str().unwrap(),
                ],
            );
            fs::write(
                checkout.join(".git/info/attributes"),
                "templates/projects/generic/gitconfig.repo filter=udetest\n",
            )
            .unwrap();
            let managed = checkout.join("templates/projects/generic/gitconfig.repo");
            if kind == "smudge" {
                fs::write(&managed, "[core]\n  autocrlf = dirty\n").unwrap();
            } else {
                let old = std::time::SystemTime::now() - Duration::from_secs(3600);
                fs::File::options()
                    .write(true)
                    .open(&managed)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
        }
    }

    /// The clean filter channel, both routes.
    #[cfg(unix)]
    fn assert_clean_filter_refused(open_pr: bool) {
        let git = GitFixture::new();
        let setup = filter_setup(&git, "clean");
        assert_channel_refused_with_control(open_pr, &setup);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_moved_by_clean_filter() {
        assert_clean_filter_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_moved_by_clean_filter() {
        assert_clean_filter_refused(true);
    }

    /// A smudge filter moves the ref during `git reset --hard`, after the reset chose its
    /// commit, so HEAD stays at the remote's head while the tracking ref moves. The checked
    /// route refuses at the head check; plain ensure is refused too, by the gate's
    /// continuity check (the commit's parent is not the fetched head). There is therefore
    /// no delivering control for this channel; the unchecked run pins that it publishes
    /// nothing either.
    #[cfg(unix)]
    fn assert_smudge_filter_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = filter_setup(&git, "smudge");
        let checked = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), true);
        assert_checked_refused(&git, &checked);
        let unchecked = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), false);
        assert_hook_ran(&unchecked);
        assert!(
            matches!(unchecked.res, Err(PushRemoteError::Failed(_))),
            "{:?}",
            unchecked.res
        );
        assert_eq!(
            remote_heads(&git, unchecked.bare.path()),
            unchecked.heads_before
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tracking_ref_moved_by_smudge_filter() {
        assert_smudge_filter_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tracking_ref_moved_by_smudge_filter() {
        assert_smudge_filter_refused(true);
    }

    /// A tag named `origin/main` makes `git checkout -B main origin/main` fail, so the
    /// ensure refuses before any read of the base matters; nothing moves on the remote.
    #[test]
    #[cfg(unix)]
    fn test_push_refuses_tag_named_after_the_tracking_ref() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = |_bare: &Path, checkout: &Path| {
            sh_in(&git, checkout, &moved_tracking_ref_tail(false));
            let injected = git.output(checkout, &["rev-parse", "refs/remotes/origin/main"]);
            git.run(
                checkout,
                &["update-ref", "refs/remotes/origin/main", "main"],
            );
            git.run(
                checkout,
                &["update-ref", "refs/tags/origin/main", &injected],
            );
        };
        let pushed = push_after_hooked_ensure(&git, &engine, false, Some(&setup), true);
        assert!(
            git.output(
                &pushed.checkout,
                &["rev-parse", "--verify", "refs/tags/origin/main"]
            )
            .len()
                >= 40,
            "the tag is missing"
        );
        let err = push_failure(pushed.res);
        assert!(err.contains("git checkout -B main failed"), "{err}");
        assert_eq!(remote_heads(&git, pushed.bare.path()), pushed.heads_before);
    }

    /// A `remote.origin.uploadpack` wrapper moves the bare's main to a commit built in the
    /// bare, then serves the fetch, so the remote really moved during the fetch: the first
    /// read saw the old head and the second read sees the fetched one.
    #[test]
    #[cfg(unix)]
    fn test_ensure_for_push_reports_real_upstream_move_as_upstream_moved() {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = |bare: &Path, checkout: &Path| {
            let tree = git.output(bare, &["rev-parse", "main^{tree}"]);
            let moved = git.output(
                bare,
                &[
                    "commit-tree",
                    &tree,
                    "-p",
                    "main",
                    "-m",
                    "concurrent publish",
                ],
            );
            let marker = checkout.join(".git/ude-test-hook-ran");
            let wrapper = checkout.join(".git/ude-test-upload-pack.sh");
            fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\nif [ ! -e '{marker}' ]; then\n: > '{marker}'\ngit --git-dir='{bare}' update-ref refs/heads/main {moved}\nfi\nexec git upload-pack \"$@\"\n",
                    marker = marker.display(),
                    bare = bare.display(),
                ),
            )
            .unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
            git.run(
                checkout,
                &[
                    "config",
                    "remote.origin.uploadpack",
                    wrapper.to_str().unwrap(),
                ],
            );
            fs::write(checkout.join(".git/ude-test-moved-to"), &moved).unwrap();
        };
        let pushed = push_after_hooked_ensure(&git, &engine, false, Some(&setup), true);
        assert_hook_ran(&pushed);
        assert_eq!(
            pushed.res,
            Err(PushRemoteError::UpstreamMoved {
                branch: "main".to_string()
            })
        );
        let moved = fs::read_to_string(pushed.checkout.join(".git/ude-test-moved-to")).unwrap();
        assert_eq!(
            remote_heads(&git, pushed.bare.path()),
            vec![("refs/heads/main".to_string(), moved)],
            "the remote's main must be the wrapper's commit, with nothing on top"
        );
    }

    /// A post-checkout hook that moves no ref: the checked route publishes, with the
    /// fetched head equal to the remote's head before the push.
    #[cfg(unix)]
    fn assert_benign_post_checkout_hook_publishes(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let setup = post_checkout_setup("echo benign >/dev/null\n".to_string());
        let pushed = push_after_hooked_ensure(&git, &engine, open_pr, Some(&setup), true);
        assert_hook_ran(&pushed);
        assert!(pushed.res.is_ok(), "{:?}", pushed.res);
        let main_before = pushed
            .heads_before
            .iter()
            .find(|(name, _)| name == "refs/heads/main")
            .map(|(_, id)| id.clone());
        assert_eq!(pushed.fetched_sha, main_before);
        assert_ne!(
            remote_heads(&git, pushed.bare.path()),
            pushed.heads_before,
            "nothing was published"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_with_head_check_publishes_with_benign_post_checkout_hook() {
        assert_benign_post_checkout_hook_publishes(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_with_head_check_publishes_with_benign_post_checkout_hook() {
        assert_benign_post_checkout_hook_publishes(true);
    }

    #[test]
    #[cfg(unix)]
    fn test_remote_head() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let url = bare.path().to_str().unwrap();
        let main = git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        let tree = git.output(bare.path(), &["rev-parse", "main^{tree}"]);
        let other = git.output(
            bare.path(),
            &["commit-tree", &tree, "-p", "main", "-m", "x"],
        );
        git.run(
            bare.path(),
            &["update-ref", "refs/heads/a/refs/heads/main", &other],
        );
        // The row below is only worth something if ls-remote lists the decoy first.
        let raw = git.output(
            Path::new("/"),
            &["ls-remote", "--refs", "--", url, "refs/heads/main"],
        );
        assert_eq!(
            raw.lines().next(),
            Some(format!("{other}\trefs/heads/a/refs/heads/main").as_str()),
            "{raw}"
        );

        let head = engine.remote_head(url, "main");
        let missing_branch = engine.remote_head(url, "does-not-exist");
        let unreachable = engine.remote_head(&format!("{url}-missing"), "main");
        let rows = [
            (
                "main's head, past a branch whose name ends in refs/heads/main",
                head.as_ref() == Ok(&Some(main.clone())),
                format!("{head:?}"),
            ),
            (
                "a branch that does not exist",
                missing_branch == Ok(None),
                format!("{missing_branch:?}"),
            ),
            (
                "an unreachable URL",
                unreachable.is_err(),
                format!("{unreachable:?}"),
            ),
            (
                "the id is whole, not a prefix of it",
                head.as_ref().is_ok_and(|id| {
                    id.as_deref() != Some(&main[..12])
                        && id.as_ref().is_some_and(|id| id.len() == main.len())
                }),
                format!("{head:?}"),
            ),
        ];
        let failures: Vec<String> = rows
            .iter()
            .filter(|(_, ok, _)| !ok)
            .map(|(name, _, got)| format!("{name}: got {got}"))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// `url.<other>.insteadOf` in a checkout's config redirects `ls-remote` run in that
    /// checkout (the negative control), not remote_head, which runs from the root.
    #[test]
    #[cfg(unix)]
    fn test_remote_head_ignores_repository_config() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let url = bare.path().to_str().unwrap();
        let real = git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        let checkout = tempdir().unwrap();
        git.run(checkout.path(), &["clone", "-q", url, "."]);
        let other = other_remote_with_injected_base(&git, bare.path(), checkout.path());
        let injected = git.output(&other, &["rev-parse", "main"]);
        assert_ne!(injected, real);
        instead_of_config(&git, checkout.path(), &other);

        let in_checkout = git.output(
            checkout.path(),
            &["ls-remote", "--refs", "--", url, "refs/heads/main"],
        );
        assert_eq!(in_checkout, format!("{injected}\trefs/heads/main"));
        assert_eq!(engine.remote_head(url, "main"), Ok(Some(real)));
    }

    #[test]
    fn test_absolute_remote_url() {
        let cwd = std::env::current_dir().unwrap();
        assert!(
            Path::new("src").exists(),
            "the test runs from the crate root"
        );
        let mut failures = Vec::new();
        let rows = [
            ("src", cwd.join("src").to_str().unwrap().to_string()),
            (cwd.to_str().unwrap(), cwd.to_str().unwrap().to_string()),
            ("https://host/x.git", "https://host/x.git".to_string()),
            ("git@host:x/y.git", "git@host:x/y.git".to_string()),
            ("no-such-dir/x.git", "no-such-dir/x.git".to_string()),
        ];
        for (input, want) in rows {
            let got = absolute_remote_url(input);
            if got != want {
                failures.push(format!("{input}: got {got}, want {want}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A post-commit hook points `remote.origin.pushurl` at an empty bare repository and
    /// records UDE's commit in `.git/ude-test-validated`. The push goes to the caller's URL,
    /// so it reaches the real remote and the other repository stays empty, while a push
    /// naming `origin` from the same checkout lands there (the config was in effect).
    #[cfg(unix)]
    fn assert_origin_pushurl_ignored(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let other = tempdir().unwrap();
        git.run(other.path(), &["init", "-q", "--bare", "-b", "main"]);
        let tail = format!(
            "git config remote.origin.pushurl '{}'\ngit rev-parse HEAD > .git/ude-test-validated\n",
            other.path().display()
        );
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &tail);

        let published = assert_published_exact(&git, &pushed, open_pr);
        assert_eq!(published, hook_recorded_id(&pushed, "ude-test-validated"));
        assert_eq!(
            remote_heads(&git, other.path()),
            vec![],
            "the other repository received a ref"
        );
        git.run(
            &pushed.checkout,
            &[
                "push",
                "-q",
                "origin",
                &format!("{published}:refs/heads/control"),
            ],
        );
        assert_eq!(
            remote_heads(&git, other.path()),
            vec![("refs/heads/control".to_string(), published)],
            "a push naming origin did not follow the hook's pushurl"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_ignores_hook_written_origin_pushurl() {
        assert_origin_pushurl_ignored(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_ignores_hook_written_origin_pushurl() {
        assert_origin_pushurl_ignored(true);
    }

    /// An empty bare repository a redirect sends the push to. With `shallow` it accepts a
    /// push from a shallow clone (a `file://` clone is one).
    #[cfg(unix)]
    fn redirect_target(git: &GitFixture, shallow: bool) -> tempfile::TempDir {
        let other = tempdir().unwrap();
        git.run(other.path(), &["init", "-q", "--bare", "-b", "main"]);
        if shallow {
            git.run(other.path(), &["config", "receive.shallowUpdate", "true"]);
        }
        other
    }

    /// The push was redirected and refused: Err with the read-back's text, the real remote
    /// unchanged with no `ude-sync-*` branch, and `other` holding exactly one head, a commit
    /// on the fetched head with the validated bytes (the redirect took effect). Direct: the
    /// checkout's HEAD is back at the fetched head. PR: no local `ude-sync-*` branch.
    /// Returns the commit `other` holds.
    #[cfg(unix)]
    fn assert_redirected_and_refused(
        git: &GitFixture,
        pushed: &HookedPush,
        other: &Path,
        open_pr: bool,
    ) -> String {
        match &pushed.res {
            Err(PushRemoteError::Failed(e)) => assert!(e.contains(NOT_PUBLISHED), "{e}"),
            res => panic!("expected the read-back's refusal, got {res:?}"),
        }
        assert_remote_unchanged(git, pushed);
        let heads = remote_heads(git, other);
        assert_eq!(
            heads.len(),
            1,
            "the redirect did not take effect: {heads:?}"
        );
        let (name, landed) = heads.into_iter().next().unwrap();
        assert_eq!(name.starts_with("refs/heads/ude-sync-"), open_pr, "{name}");
        assert_eq!(
            git.output(other, &["rev-parse", &format!("{landed}~1")]),
            pushed.fetched_sha
        );
        assert_eq!(
            git.checked(
                other,
                &["show", &format!("{landed}:{}", pushed.managed_rel)]
            )
            .stdout,
            HOOKED_TEMPLATE.as_bytes()
        );
        if open_pr {
            assert_eq!(
                git.output(
                    &pushed.checkout,
                    &[
                        "for-each-ref",
                        "--format=%(refname)",
                        "refs/heads/ude-sync-*"
                    ]
                ),
                "",
                "the local sync branch was left behind"
            );
        } else {
            assert_eq!(
                git.output(&pushed.checkout, &["rev-parse", "HEAD"]),
                pushed.fetched_sha,
                "the cache commit was not rolled back"
            );
        }
        landed
    }

    /// A post-commit hook writes `config_line` (with `O` the other repository and `U` the
    /// push URL) and records UDE's commit; the push is refused and that commit landed in
    /// the other repository.
    #[cfg(unix)]
    fn assert_hook_redirect_refused(open_pr: bool, file_url: bool, config_line: &str) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let other = redirect_target(&git, file_url);
        let tail = format!(
            "O='{}'\n{config_line}\ngit rev-parse HEAD > .git/ude-test-validated\n",
            other.path().display()
        );
        let pushed = push_with_hook_at(&git, &engine, open_pr, "post-commit", "", &tail, file_url);
        assert!(pushed.hook_ran, "the hook never ran");
        let landed = assert_redirected_and_refused(&git, &pushed, other.path(), open_pr);
        assert_eq!(landed, hook_recorded_id(&pushed, "ude-test-validated"));
    }

    /// `url.<other>.pushInsteadOf <url>`, written by a hook.
    const PUSH_INSTEAD_OF_LINE: &str = r#"git config "url.$O.pushInsteadOf" "$U""#;

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_written_push_instead_of() {
        assert_hook_redirect_refused(false, false, PUSH_INSTEAD_OF_LINE);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_written_push_instead_of() {
        assert_hook_redirect_refused(true, false, PUSH_INSTEAD_OF_LINE);
    }

    /// `url.<other>.insteadOf <url>`, written by a hook after the fetch.
    const INSTEAD_OF_LINE: &str = r#"git config "url.$O.insteadOf" "$U""#;

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_written_instead_of_after_the_fetch() {
        assert_hook_redirect_refused(false, false, INSTEAD_OF_LINE);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_written_instead_of_after_the_fetch() {
        assert_hook_redirect_refused(true, false, INSTEAD_OF_LINE);
    }

    /// `remote.<url>.pushurl <other>`, written by a hook; honoured for a `file://` URL.
    const URL_KEYED_PUSHURL_LINE: &str = r#"git config "remote.$U.pushurl" "$O""#;

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_hook_written_url_keyed_pushurl() {
        assert_hook_redirect_refused(false, true, URL_KEYED_PUSHURL_LINE);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_hook_written_url_keyed_pushurl() {
        assert_hook_redirect_refused(true, true, URL_KEYED_PUSHURL_LINE);
    }

    /// A clean filter on the managed template writes `url.<other>.pushInsteadOf` during
    /// `git add` and passes the bytes through.
    #[cfg(unix)]
    fn assert_filter_redirect_refused(open_pr: bool) {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let other = redirect_target(&git, false);
        let body = format!(
            "U=$(git config remote.origin.url)\ngit config \"url.{}.pushInsteadOf\" \"$U\" >&2\n: > .git/ude-test-hook-ran\ncat\n",
            other.path().display()
        );
        let pushed = push_managed_file(&git, &engine, open_pr, HOOKED_TEMPLATE, |repo| {
            set_clean_filter(&git, repo, &body);
        });
        assert!(
            pushed.checkout.join(".git/ude-test-hook-ran").exists(),
            "the clean filter never ran"
        );
        assert_redirected_and_refused(&git, &pushed, other.path(), open_pr);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_redirect_written_by_clean_filter() {
        assert_filter_redirect_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_redirect_written_by_clean_filter() {
        assert_filter_redirect_refused(true);
    }

    /// `url.<other>.pushInsteadOf <url>` only in the GIT_CONFIG_GLOBAL file the engine's git
    /// reads; the fixture's own git keeps the plain config, so seeding is not redirected.
    /// The push follows the key and `git ls-remote` does not, so the read-back refuses.
    #[cfg(unix)]
    fn assert_global_push_instead_of_refused(open_pr: bool) {
        let git = GitFixture::new();
        let bare = seeded_bare_remote(&git);
        let url = bare.path().to_str().unwrap();
        let other = redirect_target(&git, false);
        let config_dir = tempdir().unwrap();
        let global = config_dir.path().join("gitconfig");
        fs::write(
            &global,
            format!(
                "{FIXTURE_GITCONFIG}[url \"{}\"]\n\tpushInsteadOf = {url}\n",
                other.path().display()
            ),
        )
        .unwrap();
        let engine = SyncEngine::new().with_extra_env(vec![
            ("GIT_CONFIG_GLOBAL".into(), global.into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            (
                "GIT_CEILING_DIRECTORIES".into(),
                std::env::temp_dir().into(),
            ),
        ]);
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo_for_push(url, "main", cache.path())
            .unwrap();
        let checkout = cached.checkout_dir.clone();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        let managed_rel = managed
            .strip_prefix(&checkout)
            .unwrap()
            .display()
            .to_string();
        fs::write(&managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            open_pr,
            std::slice::from_ref(&managed),
            &validated,
        );
        let fetched_sha = cached.fetched_sha.clone();
        drop(cached);
        let pushed = HookedPush {
            res,
            bare,
            _cache: cache,
            checkout,
            fetched_sha,
            managed_rel,
            hook_ran: false,
        };
        assert_redirected_and_refused(&git, &pushed, other.path(), open_pr);
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_global_push_instead_of() {
        assert_global_push_instead_of_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_global_push_instead_of() {
        assert_global_push_instead_of_refused(true);
    }

    /// The bare remote's post-receive hook moves each pushed ref to a new commit built
    /// there on top of it, and records the pushed id in `ude-test-pushed`.
    #[cfg(unix)]
    const REMOTE_MOVES_PUSHED_REF: &str = r#"#!/bin/sh
while read old new ref; do
  T=$(git rev-parse "$new^{tree}")
  M=$(GIT_AUTHOR_NAME=other GIT_AUTHOR_EMAIL=other@example.com GIT_COMMITTER_NAME=other GIT_COMMITTER_EMAIL=other@example.com git commit-tree "$T" -p "$new" -m moved-after-push)
  git update-ref "$ref" "$M"
  printf '%s\n' "$new" > ude-test-pushed
done
"#;

    /// The pushed ref moves on right after the push: refused, not Ok, and the remote's ref
    /// is a child of the pushed commit, so the move really happened after it landed.
    #[cfg(unix)]
    fn assert_moved_after_push_refused(open_pr: bool) {
        use std::os::unix::fs::PermissionsExt;
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let hook = bare.path().join("hooks/post-receive");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, REMOTE_MOVES_PUSHED_REF).unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let cache = tempdir().unwrap();
        let cached = engine
            .ensure_remote_repo(bare.path().to_str().unwrap(), "main", cache.path())
            .unwrap();
        let managed = cached.templates_dir.join("projects/generic/gitconfig.repo");
        fs::write(&managed, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&managed));
        let res = engine.push_to_remote(
            &cached.publish_target("main"),
            open_pr,
            std::slice::from_ref(&managed),
            &validated,
        );
        let err = push_failure(res);
        assert!(err.contains(NOT_PUBLISHED), "{err}");
        let pushed = fs::read_to_string(bare.path().join("ude-test-pushed")).unwrap();
        let moved = remote_heads(&git, bare.path())
            .into_iter()
            .find(|(name, _)| open_pr != (name == "refs/heads/main"))
            .map(|(_, id)| id)
            .expect("the pushed ref");
        assert_eq!(
            git.output(bare.path(), &["rev-parse", &format!("{moved}~1")]),
            pushed.trim(),
            "the remote's ref is not a child of the pushed commit"
        );
        if !open_pr {
            assert_eq!(
                git.output(&cached.checkout_dir, &["rev-parse", "HEAD"]),
                cached.fetched_sha,
                "the cache commit was not rolled back"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_when_remote_branch_moved_after_push() {
        assert_moved_after_push_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_when_remote_branch_moved_after_push() {
        assert_moved_after_push_refused(true);
    }

    /// The text every refused confirmation carries.
    const NOT_PUBLISHED: &str = "not at the validated commit";

    #[test]
    fn test_confirm_published() {
        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let bare = seeded_bare_remote(&git);
        let url = bare.path().to_str().unwrap();
        let fetched = git.output(bare.path(), &["rev-parse", "refs/heads/main"]);
        let tree = git.output(bare.path(), &["rev-parse", "main^{tree}"]);
        let child_of = |parent: &str| {
            git.output(
                bare.path(),
                &["commit-tree", &tree, "-p", parent, "-m", "step"],
            )
        };
        let commit = child_of(&fetched);
        let one_step_on = child_of(&commit);
        git.run(bare.path(), &["update-ref", "refs/heads/main", &commit]);
        let zeros = "0".repeat(commit.len());
        let unreachable_url = format!("{url}-missing");
        let target = |remote_url| PublishTarget {
            checkout: bare.path(),
            remote_url,
            branch: "main",
            fetched_sha: &fetched,
        };

        let confirm = |remote_url, branch, id: &str| {
            engine.confirm_published(&target(remote_url), branch, id)
        };
        let refused =
            |res: &Result<(), String>| res.as_ref().is_err_and(|e| e.contains(NOT_PUBLISHED));
        let rows = [
            ("the ref at the commit", confirm(url, "main", &commit), true),
            (
                "a prefix of the id",
                confirm(url, "main", &commit[..12]),
                false,
            ),
            (
                "another id of the same length",
                confirm(url, "main", &zeros),
                false,
            ),
            (
                "the fetched head: nothing moved",
                confirm(url, "main", &fetched),
                false,
            ),
            (
                "a commit one step on",
                confirm(url, "main", &one_step_on),
                false,
            ),
            (
                "an absent branch",
                confirm(url, "ude-absent", &commit),
                false,
            ),
            (
                "an unreachable URL",
                confirm(&unreachable_url, "main", &commit),
                false,
            ),
        ];
        let mut failures: Vec<String> = rows
            .iter()
            .filter(|(_, res, passes)| if *passes { res.is_err() } else { !refused(res) })
            .map(|(name, res, _)| format!("{name}: got {res:?}"))
            .collect();
        let failed_read = &rows[6].1;
        if !failed_read
            .as_ref()
            .is_err_and(|e| e.contains(&format!("{NOT_PUBLISHED}: git ls-remote failed")))
        {
            failures.push(format!(
                "a failed read names the read's error: got {failed_read:?}"
            ));
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The refusal text of a listing over either bound of one gate call (records or bytes);
    /// a literal of its own, so a changed production text fails the rows.
    const TOO_LARGE: &str = "is larger than the publication gate lists for one push";

    /// A `mktree` tree holding `f` (100644) at `leaf`, then `depth` times a tree listing `a`
    /// and `b`, both `040000 tree` at the tree below; returns the top id. It lists
    /// 3 * 2^depth - 2 records below its own record, from depth + 1 tree objects.
    fn shared_subtree_tree(git: &GitFixture, repo: &Path, leaf: &str, depth: u32) -> String {
        let mktree = |listing: String| git_with_input(git, repo, &["mktree"], listing.as_bytes());
        let mut tree = mktree(format!("100644 blob {leaf}\tf\n"));
        for _ in 0..depth {
            tree = mktree(format!("040000 tree {tree}\ta\n040000 tree {tree}\tb\n"));
        }
        tree
    }

    /// The bytes `--no-replace-objects ls-tree <args>` prints in `repo` and how many of them
    /// are `end`, counted as git writes them, so a large listing is never held; panics
    /// unless git exits 0.
    fn ls_tree_counts(git: &GitFixture, repo: &Path, args: &[&str], end: u8) -> (usize, usize) {
        use std::io::Read as _;
        let mut child = git
            .cmd(repo)
            .args(["--no-replace-objects", "ls-tree"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (mut bytes, mut ends) = (0, 0);
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = stdout.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes += read;
            ends += buffer[..read].iter().filter(|byte| **byte == end).count();
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "ls-tree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (bytes, ends)
    }

    /// The number of lines `--no-replace-objects ls-tree -r -t <treeish>` prints in `repo`:
    /// git's record count, one line a record (no fixture name holds a newline).
    fn ls_tree_record_count(git: &GitFixture, repo: &Path, treeish: &str) -> usize {
        ls_tree_counts(git, repo, &["-r", "-t", treeish], b'\n').1
    }

    /// The byte count of `--no-replace-objects ls-tree -r -t -z --full-tree <treeish>` in
    /// `repo`: the bytes git's own listing of the tree takes.
    fn ls_tree_byte_count(git: &GitFixture, repo: &Path, treeish: &str) -> usize {
        ls_tree_counts(git, repo, &["-r", "-t", "-z", "--full-tree", treeish], 0).0
    }

    /// Hook tail: rebuilds UDE's commit with `templates/bomb`, a shared-subtree tree of
    /// `depth` levels ([`shared_subtree_tree`]) whose leaves are the validated blob, beside
    /// `templates/projects`; the rebuilt commit's id is left for the test.
    #[cfg(unix)]
    fn hook_shared_subtree(depth: u32) -> String {
        [
            rebuild_start_lines!(),
            r#"L=$(printf '100644 blob %s\tf\n' "$V" | git mktree)
i=0
while [ "$i" -lt "#,
            &depth.to_string(),
            r#" ]; do
L=$(printf '040000 tree %s\ta\n040000 tree %s\tb\n' "$L" "$L" | git mktree)
i=$((i + 1))
done
G=$(printf '100644 blob %s\tgitconfig.repo\n' "$V" | git mktree)
"#,
            one_generic_line!(),
            r#"T2=$(printf '040000 tree %s\tbomb\n040000 tree %s\tprojects\n' "$L" "$T1" | git mktree)
R=$(printf '040000 tree %s\ttemplates\n' "$T2" | git mktree)
N=$(git commit-tree "$R" -p "$P" -F .git/ude-test-msg)
printf '%s\n' "$N" > .git/ude-test-rebuilt-commit
git update-ref HEAD "$N"
"#,
        ]
        .concat()
    }

    /// [`root_listing_validated_blob`]'s chain with the templates tree also listing `bomb`, a
    /// shared-subtree tree of `depth` levels whose leaf is the [`HOOKED_TEMPLATE`] blob;
    /// returns the root tree's id.
    #[cfg(unix)]
    fn root_with_shared_subtree(git: &GitFixture, repo: &Path, depth: u32) -> String {
        let mktree = |listing: String| git_with_input(git, repo, &["mktree"], listing.as_bytes());
        let validated = write_blob(git, repo, HOOKED_TEMPLATE.as_bytes());
        let bomb = shared_subtree_tree(git, repo, &validated, depth);
        let generic = mktree(format!("100644 blob {validated}\tgitconfig.repo\n"));
        let projects = mktree(format!("040000 tree {generic}\tgeneric\n"));
        let templates = mktree(format!(
            "040000 tree {bomb}\tbomb\n040000 tree {projects}\tprojects\n"
        ));
        mktree(format!("040000 tree {templates}\ttemplates\n"))
    }

    /// A post-commit hook rebuilds UDE's commit with `templates/bomb`, a shared-subtree tree
    /// of depth 6, so the commit lists 195 records. With a record limit of 64 the committed
    /// gate refuses the commit's listing as too large, naming no tree text, and nothing
    /// reaches the remote. Control: the same hook under the production limit is refused as
    /// unvalidated, so the limit decided, not the tree's shape.
    #[cfg(unix)]
    fn assert_tree_over_the_record_limit_refused(open_pr: bool) {
        const RECORD_LIMIT: usize = 64;
        let git = GitFixture::new();
        let engine = SyncEngine::new()
            .with_extra_env(git.gitconfig_env())
            .with_listing_record_limit(RECORD_LIMIT);
        let pushed = push_with_post_commit_hook(&git, &engine, open_pr, &hook_shared_subtree(6));
        let err = assert_hook_refused(&git, &pushed);
        let rebuilt = hook_recorded_id(&pushed, "ude-test-rebuilt-commit");
        let records = ls_tree_record_count(&git, &pushed.checkout, &rebuilt);
        assert_eq!(
            records, 195,
            "templates, bomb, 190 below it, projects, generic and gitconfig.repo"
        );
        assert!(records > RECORD_LIMIT, "{records}");
        assert!(err.contains(TOO_LARGE), "{err}");
        assert!(err.contains("the tree of the commit to publish"), "{err}");
        assert!(!err.contains("unvalidated file committed"), "{err}");
        assert!(!err.contains("bomb"), "{err}");

        let git = GitFixture::new();
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        let control = push_with_post_commit_hook(&git, &engine, open_pr, &hook_shared_subtree(6));
        let err = assert_hook_refused(&git, &control);
        assert!(err.contains("unvalidated file committed in HEAD"), "{err}");
        assert!(!err.contains(TOO_LARGE), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn test_direct_push_refuses_tree_over_the_record_limit() {
        assert_tree_over_the_record_limit_refused(false);
    }

    #[test]
    #[cfg(unix)]
    fn test_pr_push_refuses_tree_over_the_record_limit() {
        assert_tree_over_the_record_limit_refused(true);
    }

    /// The empty-status route's listing is bounded by the same record limit. The fetched
    /// head is a commit whose templates tree also lists `bomb`, a shared-subtree tree of
    /// depth 4. Just-under control: a limit of exactly git's record count lists it, and the
    /// head holds the record. One record fewer refuses it as too large, naming no tree text.
    #[test]
    #[cfg(unix)]
    fn test_fetched_head_holds_refuses_tree_over_the_record_limit() {
        let git = GitFixture::new();
        let (repo, file) = gate_repo(&git);
        let dir = repo.path();
        fs::write(&file, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));
        let staged = std::slice::from_ref(&file);
        let base = git.output(dir, &["rev-parse", "HEAD"]);
        let root = root_with_shared_subtree(&git, dir, 4);
        let holding = git_with_input(
            &git,
            dir,
            &["commit-tree", &root, "-p", &base],
            b"holds the record\n",
        );
        git.run(dir, &["update-ref", "refs/heads/main", &holding]);
        let records = ls_tree_record_count(&git, dir, &holding);
        assert_eq!(
            records, 51,
            "templates, bomb, 46 below it, projects, generic and gitconfig.repo"
        );
        let holds = |limit: usize| {
            SyncEngine::new()
                .with_extra_env(git.gitconfig_env())
                .with_listing_record_limit(limit)
                .require_fetched_head_holds(&checkout_target(dir, &holding), staged, &validated)
        };

        assert_eq!(holds(records), Ok(()));
        let err = match holds(records - 1) {
            Err(err) => err,
            other => panic!("a limit one under git's record count must refuse, got {other:?}"),
        };
        assert!(err.contains(TOO_LARGE), "{err}");
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains("the fetched head's tree"), "{err}");
        assert!(!err.contains("bomb"), "{err}");
    }

    /// The committed gate's validate call with an engine carrying the fixture gitconfig and
    /// `engine_for` applied, the allowlist `{templates/f}` and the record `f new\n`.
    fn validate_templates_commit(
        git: &GitFixture,
        dir: &Path,
        commit: &str,
        fetched: &str,
        engine_for: impl Fn(SyncEngine) -> SyncEngine,
    ) -> Result<(), String> {
        let paths: HashSet<PathBuf> = [PathBuf::from("templates/f")].into_iter().collect();
        let validated = templates_record(dir, &[("f", b"f new\n")]);
        let staged_tree = commit_tree_id(git, dir, commit);
        engine_for(SyncEngine::new().with_extra_env(git.gitconfig_env())).validate_committed_tree(
            dir,
            commit,
            fetched,
            &paths,
            &validated,
            &staged_tree,
        )
    }

    /// `templates_repo` with `f` = `f base\n`, and a fetched head on it whose templates tree
    /// lists `f` at its base blob and `bomb`, a shared-subtree tree of depth 4 whose leaf is
    /// that blob. Returns the repository, the fetched head, bomb's id and f's new blob.
    fn fetched_head_with_shared_subtree(
        git: &GitFixture,
    ) -> (tempfile::TempDir, String, String, String) {
        let (repo, base) = templates_repo(git, &[("f", "f base\n")]);
        let dir = repo.path();
        let f_base = git.output(dir, &["rev-parse", "HEAD:templates/f"]);
        let bomb = shared_subtree_tree(git, dir, &f_base, 4);
        let fetched = commit_of_templates(
            git,
            dir,
            &base,
            &format!("040000 tree {bomb}\tbomb\n100644 blob {f_base}\tf\n"),
        );
        let f_new = write_blob(git, dir, b"f new\n");
        (repo, fetched, bomb, f_new)
    }

    /// The fetched head's listing, the one listing_diff reads beside the commit's, is bounded
    /// by the same limit. The fetched head lists `bomb`, a shared-subtree tree of depth 4; the
    /// commit changes `f` and deletes bomb. Control, first: a limit of git's record count of
    /// the commit plus the fetched head lists both, and the deletion is refused as
    /// unvalidated. One record fewer: the fetched head's listing is refused as too large.
    #[test]
    fn test_committed_gate_refuses_fetched_head_tree_over_the_record_limit() {
        let git = GitFixture::new();
        let (repo, fetched, _, f_new) = fetched_head_with_shared_subtree(&git);
        let dir = repo.path();
        let commit = commit_of_templates(&git, dir, &fetched, &format!("100644 blob {f_new}\tf\n"));
        let counts = (
            ls_tree_record_count(&git, dir, &commit),
            ls_tree_record_count(&git, dir, &fetched),
        );
        assert_eq!(
            counts,
            (2, 49),
            "the commit: templates and f; the fetched head: 49"
        );
        let limit = counts.0 + counts.1;
        let validate = |records: usize| {
            validate_templates_commit(&git, dir, &commit, &fetched, |engine| {
                engine.with_listing_record_limit(records)
            })
        };

        let err = validate(limit).expect_err("the deletion of bomb is unvalidated");
        assert!(err.contains("unvalidated file committed in HEAD"), "{err}");
        assert!(!err.contains(TOO_LARGE), "{err}");

        let err = validate(limit - 1).expect_err("one record under the count must refuse");
        assert!(err.contains(TOO_LARGE), "{err}");
        assert!(err.contains("the tree of the fetched head"), "{err}");
        assert!(!err.contains("bomb"), "{err}");
    }

    /// Both listings of one gate call draw on one budget. The commit keeps the fetched head's
    /// `bomb` and changes `f`, so each listing lists 49 records, each alone under the limit
    /// below. Just-under control: a limit of both counts together passes. One fewer refuses
    /// the fetched head's listing, which a budget per listing would have passed.
    #[test]
    fn test_committed_gate_counts_both_listings_against_one_record_limit() {
        let git = GitFixture::new();
        let (repo, fetched, bomb, f_new) = fetched_head_with_shared_subtree(&git);
        let dir = repo.path();
        let commit = commit_of_templates(
            &git,
            dir,
            &fetched,
            &format!("040000 tree {bomb}\tbomb\n100644 blob {f_new}\tf\n"),
        );
        let (nc, nb) = (
            ls_tree_record_count(&git, dir, &commit),
            ls_tree_record_count(&git, dir, &fetched),
        );
        assert_eq!((nc, nb), (49, 49));
        let limit = nc + nb - 1;
        assert!(
            nc < limit && nb < limit,
            "each listing alone fits under {limit}"
        );
        let validate = |records: usize| {
            validate_templates_commit(&git, dir, &commit, &fetched, |engine| {
                engine.with_listing_record_limit(records)
            })
        };

        assert_eq!(validate(nc + nb), Ok(()));
        let err = match validate(limit) {
            Err(err) => err,
            other => panic!("both listings together are over {limit}, got {other:?}"),
        };
        assert!(err.contains(TOO_LARGE), "{err}");
        assert!(err.contains("the tree of the fetched head"), "{err}");
    }

    /// The listing bytes are bounded too, so long entry names cannot make the listing large
    /// under the record limit. The commit adds `long`, a tree holding one entry whose name is
    /// 4096 bytes. Control, first: a byte limit of git's byte count of both listings lists
    /// them, and the addition is refused as unvalidated. One byte fewer: the fetched head's
    /// listing runs out of the shared budget. One byte under the commit's own count: the
    /// commit's listing is refused. No refusal holds the name.
    #[test]
    fn test_committed_gate_refuses_commit_tree_over_the_byte_limit() {
        let git = GitFixture::new();
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let f_new = write_blob(&git, dir, b"f new\n");
        let long_name = "n".repeat(4096);
        let long = git_with_input(
            &git,
            dir,
            &["mktree"],
            format!("100644 blob {f_new}\t{long_name}\n").as_bytes(),
        );
        let commit = commit_of_templates(
            &git,
            dir,
            &base,
            &format!("100644 blob {f_new}\tf\n040000 tree {long}\tlong\n"),
        );
        let (mc, mb) = (
            ls_tree_byte_count(&git, dir, &commit),
            ls_tree_byte_count(&git, dir, &base),
        );
        let records =
            ls_tree_record_count(&git, dir, &commit) + ls_tree_record_count(&git, dir, &base);
        assert!(
            records < MAX_LISTING_RECORDS,
            "{records} records: the record limit must not be what fires"
        );
        let validate = |bytes: usize| {
            validate_templates_commit(&git, dir, &commit, &base, |engine| {
                engine.with_listing_byte_limit(bytes)
            })
        };
        let refused_as_too_large = |err: &str, route: &str| {
            assert!(err.contains(TOO_LARGE), "{err}");
            assert!(err.contains(route), "{err}");
            assert!(!err.contains("unvalidated file committed"), "{err}");
            assert!(!err.contains(&long_name), "the refusal holds the long name");
        };

        let err = validate(mc + mb).expect_err("the long name is an unvalidated addition");
        assert!(err.contains("unvalidated file committed in HEAD"), "{err}");
        assert!(!err.contains(TOO_LARGE), "{err}");

        let err = validate(mc + mb - 1).expect_err("one byte under both listings must refuse");
        refused_as_too_large(&err, "the tree of the fetched head");

        let err = validate(mc - 1).expect_err("one byte under the commit's listing must refuse");
        refused_as_too_large(&err, "the tree of the commit to publish");
    }

    /// A fresh repository and the root tree `templates/bomb`, where bomb is `bomb_tree` of
    /// that repository (built by the closure from the repository and a leaf blob).
    fn root_with_bomb(
        git: &GitFixture,
        bomb_tree: impl FnOnce(&Path, &str) -> String,
    ) -> (tempfile::TempDir, String) {
        let repo = tempdir().unwrap();
        let dir = repo.path();
        git.run(dir, &["init", "-b", "main"]);
        let leaf = write_blob(git, dir, b"x\n");
        let bomb = bomb_tree(dir, &leaf);
        let mktree = |listing: String| git_with_input(git, dir, &["mktree"], listing.as_bytes());
        let templates = mktree(format!("040000 tree {bomb}\tbomb\n"));
        let root = mktree(format!("040000 tree {templates}\ttemplates\n"));
        (repo, root)
    }

    /// The verified listing of the tree `root` through an engine with the fixture gitconfig
    /// and the production limits.
    fn production_listing(
        git: &GitFixture,
        dir: &Path,
        root: &str,
    ) -> Result<Vec<u8>, ListingDefect> {
        let engine = SyncEngine::new().with_extra_env(git.gitconfig_env());
        engine.verified_tree_listing(dir, root, &mut HashMap::new(), &mut engine.listing_budget())
    }

    /// The smallest depth whose shared-subtree tree, with the two wrappers, lists more than
    /// MAX_LISTING_RECORDS records (3 * 2^depth).
    fn depth_over_the_production_record_limit() -> u32 {
        (0..usize::BITS)
            .find(|depth| 3usize << depth > MAX_LISTING_RECORDS)
            .unwrap()
    }

    /// The production record limit is wired: a shared-subtree tree of the smallest depth
    /// whose listing passes MAX_LISTING_RECORDS (17 for 1 << 18) is refused as too large.
    #[test]
    fn test_verified_tree_listing_refuses_tree_over_the_production_record_limit() {
        let git = GitFixture::new();
        let depth = depth_over_the_production_record_limit();
        let (repo, root) = root_with_bomb(&git, |dir, leaf| {
            shared_subtree_tree(&git, dir, leaf, depth)
        });
        let records = ls_tree_record_count(&git, repo.path(), &root);
        assert_eq!(records, 3 << depth);
        assert!(records > MAX_LISTING_RECORDS, "{records}");
        assert_eq!(
            production_listing(&git, repo.path(), &root).map(|listing| listing.len()),
            Err(ListingDefect::TooLarge)
        );
    }

    /// Control for the production record limit: one level less (196 608 records, under the
    /// limit) lists, byte for byte as git lists it.
    #[test]
    fn test_verified_tree_listing_lists_tree_under_the_production_record_limit() {
        let git = GitFixture::new();
        let depth = depth_over_the_production_record_limit() - 1;
        let (repo, root) = root_with_bomb(&git, |dir, leaf| {
            shared_subtree_tree(&git, dir, leaf, depth)
        });
        let built = production_listing(&git, repo.path(), &root)
            .map_err(|defect| format!("{defect:?}"))
            .expect("a tree under the production limit lists");
        let records = built.iter().filter(|byte| **byte == 0).count();
        assert_eq!(records, 3 << depth);
        assert!(records <= MAX_LISTING_RECORDS, "{records}");
        let listed = raw_ls_tree(&git, repo.path(), &root);
        if built != listed {
            let first = built
                .iter()
                .zip(&listed)
                .position(|(left, right)| left != right)
                .unwrap_or(built.len().min(listed.len()));
            panic!(
                "the built listing ({} bytes) differs from git's ({} bytes) at offset {first}",
                built.len(),
                listed.len()
            );
        }
    }

    /// The production byte limit is wired: a shared-subtree tree whose entries at every
    /// level carry 1024-byte names, at the smallest depth whose `ls-tree -r -t -z
    /// --full-tree` byte count (read from git) passes MAX_LISTING_BYTES, is refused as too
    /// large, while its record count is under a quarter of MAX_LISTING_RECORDS, so only the
    /// byte limit can fire.
    #[test]
    fn test_verified_tree_listing_refuses_tree_over_the_production_byte_limit() {
        let git = GitFixture::new();
        let repo = tempdir().unwrap();
        let dir = repo.path();
        git.run(dir, &["init", "-b", "main"]);
        let mktree = |listing: String| git_with_input(&git, dir, &["mktree"], listing.as_bytes());
        let (leaf_name, a, b) = ("f".repeat(1024), "a".repeat(1024), "b".repeat(1024));
        let leaf = write_blob(&git, dir, b"x\n");
        let mut bomb = mktree(format!("100644 blob {leaf}\t{leaf_name}\n"));
        let root = loop {
            bomb = mktree(format!(
                "040000 tree {bomb}\t{a}\n040000 tree {bomb}\t{b}\n"
            ));
            let templates = mktree(format!("040000 tree {bomb}\tbomb\n"));
            let root = mktree(format!("040000 tree {templates}\ttemplates\n"));
            if ls_tree_byte_count(&git, dir, &root) > MAX_LISTING_BYTES {
                break root;
            }
        };
        let records = ls_tree_record_count(&git, dir, &root);
        assert!(records < MAX_LISTING_RECORDS / 4, "{records}");
        assert_eq!(
            production_listing(&git, dir, &root).map(|listing| listing.len()),
            Err(ListingDefect::TooLarge)
        );
    }

    /// The refusal text of a walk past the publication gate's deadline; a literal of its
    /// own, so a changed production text fails the rows.
    const PAST_DEADLINE: &str = "could not be listed within the publication gate's time limit";

    /// `templates_repo` with `f` = `f base\n` and a commit on it changing f to `f new\n`;
    /// returns the repository, the base and the commit.
    fn commit_changing_f(git: &GitFixture) -> (tempfile::TempDir, String, String) {
        let (repo, base) = templates_repo(git, &[("f", "f base\n")]);
        let f_new = write_blob(git, repo.path(), b"f new\n");
        let commit = commit_of_templates(
            git,
            repo.path(),
            &base,
            &format!("100644 blob {f_new}\tf\n"),
        );
        (repo, base, commit)
    }

    /// The committed gate's walk stops at one deadline. With a walk time of 1 ms the commit's
    /// listing is refused past the deadline: the root read alone is three git runs, each
    /// well over 1 ms, so the next tree's check is past it whatever the load. The refusal is
    /// neither the record limit's nor a failed read's (the reads keep their own 30 s limit).
    /// Control, first: the same call with the default walk time passes.
    #[test]
    fn test_committed_gate_refuses_commit_listing_past_the_deadline() {
        let git = GitFixture::new();
        let (repo, base, commit) = commit_changing_f(&git);
        let dir = repo.path();
        assert_eq!(
            validate_templates_commit(&git, dir, &commit, &base, |engine| engine),
            Ok(())
        );

        let started = std::time::Instant::now();
        let res = validate_templates_commit(&git, dir, &commit, &base, |engine| {
            engine.with_listing_time(Duration::from_millis(1))
        });
        let elapsed = started.elapsed();
        let err = match res {
            Err(err) => err,
            other => panic!("a 1 ms walk must refuse, got {other:?}"),
        };
        assert!(err.contains(PAST_DEADLINE), "{err}");
        assert!(err.contains("the tree of the commit to publish"), "{err}");
        assert!(!err.contains(TOO_LARGE), "{err}");
        assert!(!err.contains("failed to list the tree of"), "{err}");
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    /// The empty-status route's listing stops at the same deadline. Control, first: the
    /// head holding the record passes with the default walk time; with 1 ms it is refused
    /// past the deadline, not as a listing that cannot be read.
    #[test]
    #[cfg(unix)]
    fn test_fetched_head_holds_refuses_listing_past_the_deadline() {
        let git = GitFixture::new();
        let (repo, file) = gate_repo(&git);
        let dir = repo.path();
        fs::write(&file, HOOKED_TEMPLATE).unwrap();
        let validated = ValidatedBlobs::from_files(std::slice::from_ref(&file));
        let staged = std::slice::from_ref(&file);
        let base = git.output(dir, &["rev-parse", "HEAD"]);
        let root = root_listing_validated_blob(&git, dir);
        let holding = git_with_input(
            &git,
            dir,
            &["commit-tree", &root, "-p", &base],
            b"holds the record\n",
        );
        git.run(dir, &["update-ref", "refs/heads/main", &holding]);
        let holds = |engine: SyncEngine| {
            engine
                .with_extra_env(git.gitconfig_env())
                .require_fetched_head_holds(&checkout_target(dir, &holding), staged, &validated)
        };

        assert_eq!(holds(SyncEngine::new()), Ok(()));
        let started = std::time::Instant::now();
        let res = holds(SyncEngine::new().with_listing_time(Duration::from_millis(1)));
        let elapsed = started.elapsed();
        let err = match res {
            Err(err) => err,
            other => panic!("a 1 ms walk must refuse, got {other:?}"),
        };
        assert!(err.contains(PAST_DEADLINE), "{err}");
        assert!(err.contains(EMPTY_STATUS_REFUSAL), "{err}");
        assert!(err.contains("the fetched head's tree"), "{err}");
        assert!(!err.contains("cannot be listed"), "{err}");
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    /// The fetched head's listing reuses the commit listing's deadline, not a new one. With
    /// a walk time of 15 s and a test hook sleeping 16 s right after the commit's listing,
    /// the fetched head's listing starts past the deadline and is refused. A budget made
    /// afresh for it would give it a new 15 s and pass. Control, first: the same walk time
    /// with no hook passes, in under a third of it.
    #[test]
    fn test_committed_gate_carries_the_deadline_into_the_fetched_head_listing() {
        const WALK: Duration = Duration::from_secs(15);
        const HOOK_SLEEP: Duration = Duration::from_secs(16);
        // The refused call returns this soon after the hook's sleep ends.
        const AFTER_SLEEP: Duration = Duration::from_secs(5);
        let git = GitFixture::new();
        let (repo, base, commit) = commit_changing_f(&git);
        let dir = repo.path();

        let started = std::time::Instant::now();
        let control = validate_templates_commit(&git, dir, &commit, &base, |engine| {
            engine.with_listing_time(WALK)
        });
        let control_elapsed = started.elapsed();
        assert_eq!(control, Ok(()));
        println!("control elapsed: {control_elapsed:?}");
        assert!(
            control_elapsed < WALK / 3,
            "the control took {control_elapsed:?}, so the margin under {WALK:?} is under 3x"
        );

        let started = std::time::Instant::now();
        let res = validate_templates_commit(&git, dir, &commit, &base, |engine| {
            engine
                .with_listing_time(WALK)
                .with_after_tree_listing(|| std::thread::sleep(HOOK_SLEEP))
        });
        let elapsed = started.elapsed();
        let err = match res {
            Err(err) => err,
            other => panic!("the fetched head's listing starts past the deadline, got {other:?}"),
        };
        assert!(err.contains(PAST_DEADLINE), "{err}");
        assert!(err.contains("the tree of the fetched head"), "{err}");
        assert!(!err.contains("the tree of the commit to publish"), "{err}");
        assert!(elapsed < HOOK_SLEEP + AFTER_SLEEP, "took {elapsed:?}");
    }

    /// A commit repeating one allowlisted path 64 times at the validated blob is refused
    /// for the repeat before the change loop reads any of its blobs. Control, first: the
    /// path listed once passes, and the counter reads 2 (the change loop's read and the
    /// record check's), so the counter counts real reads.
    #[test]
    fn test_committed_gate_refuses_repeated_path_before_reading_its_blobs() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let git = GitFixture::new();
        let (repo, base) = templates_repo(&git, &[("f", "f base\n")]);
        let dir = repo.path();
        let f_new = write_blob(&git, dir, b"f new\n");
        let entry = format!("100644 blob {f_new}\tf\n");
        let validate = |commit: &str| {
            let reads = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&reads);
            let res = validate_templates_commit(&git, dir, commit, &base, move |engine| {
                engine.with_blob_read_counter(Arc::clone(&counter))
            });
            (res, reads.load(Ordering::SeqCst))
        };

        let once = commit_of_templates(&git, dir, &base, &entry);
        assert_eq!(validate(&once), (Ok(()), 2));

        let repeated = commit_of_templates(&git, dir, &base, &entry.repeat(64));
        assert_eq!(
            ls_tree_record_count(&git, dir, &repeated),
            65,
            "templates and 64 entries named f"
        );
        let (res, reads) = validate(&repeated);
        let err = res.expect_err("a repeated path must be refused");
        assert!(
            err.contains("the tree of the commit to publish repeats a path"),
            "{err}"
        );
        assert_eq!(reads, 0, "blobs read before the repeat was refused");
    }

    /// One base record `f` at x and one commit record `f` at y (40-hex ids), as listings.
    fn one_changed_record() -> (Vec<u8>, Vec<u8>, String, String) {
        let (x, y) = ("a".repeat(40), "b".repeat(40));
        let record = |id: &str| format!("100644 blob {id}\tf\0").into_bytes();
        (record(&x), record(&y), x, y)
    }

    /// listing_diff checks its deadline in the loop over the commit's records: a deadline
    /// already come refuses the change; control: a deadline an hour away gives the `M`.
    #[test]
    fn test_listing_diff_refuses_changes_past_its_deadline() {
        let (base, commit, x, y) = one_changed_record();
        assert_eq!(
            listing_diff(&base, &commit, std::time::Instant::now()),
            Err(DiffDefect::PastDeadline)
        );
        let far = std::time::Instant::now() + Duration::from_secs(3600);
        assert_eq!(
            listing_diff(&base, &commit, far),
            Ok(diff_record_bytes(
                b"100644",
                b"100644",
                x.as_bytes(),
                y.as_bytes(),
                b'M',
                b"f"
            ))
        );
    }

    /// listing_diff checks its deadline in the deletion loop too: with an empty commit only
    /// that loop runs, and a deadline already come refuses it; control: a deadline an hour
    /// away gives the `D`.
    #[test]
    fn test_listing_diff_refuses_deletions_past_its_deadline() {
        let (base, _, x, _) = one_changed_record();
        assert_eq!(
            listing_diff(&base, b"", std::time::Instant::now()),
            Err(DiffDefect::PastDeadline)
        );
        let far = std::time::Instant::now() + Duration::from_secs(3600);
        let zero = "0".repeat(40);
        assert_eq!(
            listing_diff(&base, b"", far),
            Ok(diff_record_bytes(
                b"100644",
                b"000000",
                x.as_bytes(),
                zero.as_bytes(),
                b'D',
                b"f"
            ))
        );
    }

    /// The walk's deadline is the engine's local time limit from when the budget is made:
    /// process::LOCAL_TIMEOUT by default, and what with_limits sets otherwise.
    #[test]
    fn test_listing_budget_deadline_follows_the_local_time_limit() {
        let seven = Duration::from_secs(7);
        let engines = [
            (SyncEngine::new(), process::LOCAL_TIMEOUT),
            (
                SyncEngine::new().with_limits(ProcessLimits {
                    local: seven,
                    network: process::NETWORK_TIMEOUT,
                }),
                seven,
            ),
        ];
        for (engine, local) in engines {
            let before = std::time::Instant::now();
            let budget = engine.listing_budget();
            let after = std::time::Instant::now();
            assert!(
                before + local <= budget.deadline && budget.deadline <= after + local,
                "deadline outside [before + {local:?}, after + {local:?}]"
            );
        }
    }
}
