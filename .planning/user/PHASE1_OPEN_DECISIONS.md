# Phase 1: Architectural Decisions & Options for Cross-AI Review

**Document Purpose:** Review document for Claude and Codex to evaluate implementation strategies and trade-offs for Phase 1 of Universal Development Environment (UDE) — **Sync Engine & State Hermeticity**.

---

## 1. Context & Architectural Goals

UDE provisions, audits, and bidirectionally synchronizes portable developer environments and agent instructions between local working trees and a central upstream Git repository (`ude sync pull` / `ude sync push`).

### Core Value
Reliable, hermetic, bidirectional developer environment and agent baseline synchronization with cryptographic ancestor state tracking and zero credential leakage.

### Phase 1 Problem Statement
In previous adversarial code reviews, the sync engine exhibited critical vulnerabilities and edge-case fragility:
1. **Zombie File Resurrections (Defect #4 / `SYNC-01`):** Deleting an upstream template does not cleanly propagate locally; local sync either ignores the deletion or resurrects deleted files on the next push.
2. **Non-Atomic Multi-File Materialization (Defect #7 / `SYNC-02`):** When syncing 20+ templates, a disk full error or permission fault midway leaves the workspace in a corrupt, partially-applied state.
3. **Unbounded Subprocess Invocations (Defect #17 / `SYNC-03`):** Subprocess calls to `git` and `gh` have no timeouts and can hang indefinitely on network stalls or unexpected interactive prompts.
4. **Line-Ending Normalization Drift (Defect #8 / `SYNC-04`):** CRLF/LF line-ending differences alter raw SHA-256 hashes, producing false conflict reports across POSIX and Windows checkouts.
5. **Divergent Ancestor Conflicts (Defect #9 / `SYNC-05`):** Divergent concurrent modifications between local and upstream lack structured 3-way conflict markers.

---

## 2. Open Decisions for Review

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Phase 1 Decision Matrix                           │
├──────────────────────────────┬──────────────────────────────────────────────┤
│ 1. State Ledger Storage      │ redb (ACID KV) vs Atomic JSON vs Git Plumbing│
│ 2. Multi-File Materialization│ Cargo Two-Phase Shadow vs tfio vs AtomicWrite│
│ 3. Tombstone Deletion Policy │ Embedded Dual-Absence vs Sidecar vs Prompt   │
│ 4. Subprocess Timeouts       │ Tiered (10s local / 60s net) vs Uniform (30s)│
│ 5. 3-Way Conflict Strategy   │ Sidecar `.conflict` vs Inline with `.orig`   │
└──────────────────────────────┴──────────────────────────────────────────────┘
```

---

### Decision 1: Transactional State Ledger & Tombstone Storage (`SYNC-01`, `SYNC-04`)

How should UDE track file paths, cryptographic ancestor hashes, timestamps, and deletion tombstones with crash-safe transactional guarantees?

#### Option 1A: `redb` (Embedded ACID Key-Value Store in Pure Rust)
- **Mechanism:** Replace `.ude/state.json` with an embedded copy-on-write B-Tree database (`.ude/state.redb`).
- **Features:**
  - True ACID transactions (`begin_write()`, `commit()`, automatic rollback on drop or error).
  - Native `savepoint()` and `restore_savepoint()` for nested transaction rollbacks.
  - Separate typed tables: `const ANCESTORS: TableDefinition<&str, AncestorHash>` and `const TOMBSTONES: TableDefinition<&str, (u64, AncestorHash)>`.
- **Pros:**
  - 100% pure Rust, zero C/C++ FFI dependencies, zero unsafe runtime risks.
  - Immune to JSON parsing corruption if process is killed mid-write.
  - O(log N) point lookups and range scans.
- **Cons & Tradeoffs:**
  - Binary database file; cannot be inspected via standard `cat` or `jq` without CLI tooling (`ude state show`).
  - Cannot be easily tracked inside standard Git version control if committed.

#### Option 1B: Atomic JSON via `tempfile::NamedTempFile` (Cargo / Chezmoi Pattern)
- **Mechanism:** Retain `.ude/state.json`, but all state mutations write to a sibling temporary file (`.ude/state.json.tmp-XXXX`) in the same directory, followed by an atomic OS `persist` / `rename`.
- **Pros:**
  - Transparent, human-readable, and diffable in Git.
  - Zero external database dependencies (uses `tempfile` and `serde_json`, already in `Cargo.toml`).
  - Standard battle-tested pattern used by `cargo` (for config/credentials) and `chezmoi`.
- **Cons & Tradeoffs:**
  - Entire ledger must be serialized/deserialized in memory on every read/write.
  - Multi-process concurrency requires explicit advisory file locking (`fslock` / `fs2`).

#### Option 1C: Git Native Plumbing Tree Objects (`refs/ude/ancestor`)
- **Mechanism:** Use Git itself as the content-addressable storage engine via plumbing commands (`git mktree`, `git write-tree`, `git update-ref`).
- **Pros:**
  - Native cryptographic SHA-256/SHA-1 hashing, garbage collection, and tree diffing.
  - Zero additional state files.
- **Cons & Tradeoffs:**
  - Heavy reliance on external Git subprocess execution for basic state queries.
  - Subprocess overhead on frequent status checks.

**Review Question for Claude & Codex:**
> Does the simplicity and git-transparency of Option 1B (Atomic JSON via `NamedTempFile`) outweigh the ACID guarantees of Option 1A (`redb`) for an environment configuration tool managing ~100–500 template files?

---

### Decision 2: Multi-File Transactional Staging & Rollback (`SYNC-02`)

When `ude sync pull` or `ude init` applies updates across dozens of template files, how do we guarantee all-or-nothing materialization?

#### Option 2A: Two-Phase Shadow Staging via `tempfile::TempDir` (The Cargo Pattern)
- **Mechanism:**
  1. *Phase 1 (Stage & Validate):* Create temporary staging tree under `<workspace>/.ude/staging-<uuid>/`. Render, sanitize, and validate all files, paths, and anti-symlink traversal boundaries in the staging tree.
  2. *Phase 2 (Journaled Atomic Swap):* Iterate through staged files. For each target, write via `NamedTempFile::persist` in the target's parent directory, recording the previous file state in an in-memory journal.
  3. *Rollback:* If any file operation in Phase 2 fails, the journal automatically restores overwritten files and removes newly created files.
- **Critical Linux OS Boundary Gotcha:**
  - On immutable Fedora Blue/Kinoite, `/tmp` is a RAM-backed `tmpfs` filesystem, whereas `/var/home` is Btrfs/XFS.
  - Calling `rename(2)` across different mount points returns `EXDEV` (Invalid cross-device link).
  - *Requirement:* Staging directories MUST be created within `<workspace>/.ude/` on the same filesystem partition to guarantee true atomic renames.
- **Pros:**
  - Zero new third-party dependencies; uses existing `tempfile` crate.
  - Full control over validation gates prior to touching any working tree file.

#### Option 2B: `tfio` Crate (Transactional File I/O)
- **Mechanism:** Use `tfio::Transaction::new().create_file(...).write_file(...).execute()` with `.rollback()`.
- **Pros:**
  - Declarative transaction builder pattern.
- **Cons & Tradeoffs:**
  - Modest community adoption (low download volume on crates.io).
  - Defaults to system temp directory for backups, requiring careful configuration to prevent `EXDEV` cross-device failures on immutable OS.

**Review Question for Claude & Codex:**
> Is implementing the Cargo Two-Phase pattern (Option 2A) within `<workspace>/.ude/` the most reliable approach on immutable Linux hosts, or does an off-the-shelf crate provide better guarantees?

---

### Decision 3: Tombstone Lifetime & Deletion Propagation Policy (`SYNC-01`)

How should deleted templates be tracked in the state ledger to prevent zombie file resurrections across distributed machines?

#### Option 3A: Embedded Tombstones with Dual-Absence Convergence Pruning
- **Mechanism:** When a template is removed upstream, mark its entry in the ledger with `deleted: true`, `tombstone_timestamp: <ISO>`, and `last_known_hash`.
- **Pruning Rule:** The tombstone is pruned only after a subsequent sync cycle confirms that the file is absent in both upstream and local working trees.
- **Local Deletion Rule:** If a user deletes a template file locally that still exists upstream, mark it as `DeletedLocally` and do not resurrect it on `pull` unless `--force` or `--reset` is specified.

#### Option 3B: Sidecar `.ude/tombstones.json` with 30-Day TTL
- **Mechanism:** Keep active files in `.ude/state.json` and move deletion records to a separate sidecar file. Automatically prune entries older than 30 days.

**Review Question for Claude & Codex:**
> Does dual-absence convergence pruning in Option 3A sufficiently protect against zombie resurrections for developers who only sync once a month?

---

### Decision 4: Subprocess Timeout Strategy (`SYNC-03`)

How should child process execution (`std::process::Command`) for `git` and `gh` be bounded to prevent background thread hangs?

#### Option 4A: Tiered Execution Bounds
- **Local Git Queries:** 10-second timeout (`status`, `rev-parse`, `diff-tree`, `cat-file`).
- **Remote Network Git Operations:** 60-second timeout (`clone`, `fetch`, `pull`, `push`).
- **Interactive Tool Calls (`gh`):** 60-second timeout (`gh pr create`).
- **Mechanism:** Enforce timeout via worker thread with channel timeout or `wait_timeout` crate, actively killing hung processes (`child.kill()`).

#### Option 4B: Uniform Fixed Bound (30s)
- **Mechanism:** Apply a single 30-second timeout to all subprocess invocations regardless of operation type.

**Review Question for Claude & Codex:**
> Does the tiered timeout model (Option 4A) provide the right balance between responsiveness on local queries and resilience against network latency on remote git fetches?

---

### Decision 5: Conflict Handling on 3-Way Divergence (`SYNC-05`)

When local working tree modifications and upstream template updates conflict without a shared common ancestor:

#### Option 5A: Sidecar `.conflict` File (Preserve Clean Local Tree)
- **Mechanism:** Keep the developer's local modified file untouched. Write the upstream version to `<filename>.conflict` with diff headers and exit with status code `1` (warning/drift).
- **Pros:** Does not break local build toolchains (e.g., `cargo check` won't fail due to unparsed `<<<<<<<` syntax error markers in `src/`).

#### Option 5B: Standard Inline Git Conflict Markers with `.orig` Backup
- **Mechanism:** Inject standard Myers conflict markers (`<<<<<<< LOCAL`, `=======`, `>>>>>>> UPSTREAM`) directly into the file and save the original local file as `<filename>.orig`.
- **Pros:** Standard developer workflow; resolves directly inside editors with merge tools.

**Review Question for Claude & Codex:**
> Should UDE default to sidecar `.conflict` files (Option 5A) to avoid breaking build pipelines for unattended AI agents, or use inline Git conflict markers (Option 5B)?

---

*File location: [`.planning/user/PHASE1_OPEN_DECISIONS.md`](file:///home/denniyahh/Github/universal-dev-env/.planning/user/PHASE1_OPEN_DECISIONS.md)*
