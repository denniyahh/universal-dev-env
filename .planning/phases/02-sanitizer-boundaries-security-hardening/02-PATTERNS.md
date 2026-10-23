# Phase 2: Sanitizer, Boundaries & Security Hardening - Pattern Map

**Mapped:** 2026-10-23
**Files analyzed:** 11 (4 new, 7 modified)
**Analogs found:** 9 / 11 (the vendored rule file and its licence notice have no analog)

All analog paths below are git-tracked (checked with `git ls-files` on branch
`gsd/phase-02-sanitizer-boundaries-security-hardening`). Line numbers were read on this branch at
commit `7723d4a`; re-check them if earlier plans in the phase shift lines.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/sanitizer/entropy.rs` (NEW) | utility (pure scorer) | transform | `src/sync/merge.rs` (pure submodule + in-file tests); constants style from `src/process.rs` | role-match |
| `src/sanitizer/gitleaks.rs` (NEW) | utility (rule loader + matcher) | transform / config load | `src/manifest/mod.rs` (serde + `toml::from_str`) + `src/process.rs:49-51` (`LazyLock` regex) | role-match (composite) |
| `src/sanitizer/rules/gitleaks.toml` (NEW, vendored verbatim) | config (data) | file-I/O (compile-time embed) | none | no analog |
| `src/sanitizer/rules/LICENSE-gitleaks` (NEW) | config (notice) | — | none | no analog |
| `src/sanitizer/mod.rs` (MODIFY) | service (facade) | transform | itself (`check_secrets` L61-71, `parameterize_paths` L76-112, tests L159-343) | exact |
| `src/sync/mod.rs` — `push_updates` staging scan + render-back check (MODIFY) | service | request-response (pre-validation pass) | itself L1446-1475; render pattern L1063-1083 | exact |
| `src/sync/mod.rs` — `stage_publication` / `validate_committed_tree` gates (MODIFY) | service (gate) | batch over git blobs | itself L1720-1745, L2019-2041 | exact |
| `src/sync/mod.rs` — shared `should_render` predicate (MODIFY, extraction) | utility | transform | inline copies at L642-644 and L1065-1067 | exact |
| `src/main.rs` — init materialization + `kept_ancestor` (MODIFY) | controller (CLI) | file-I/O | `render_and_write` L1658-1677, `rendered_template` L1680-1696 | exact |
| `src/template/mod.rs` (MODIFY, optional home of `should_render`) | utility | transform | `TemplateRenderer::render_str` L160-169 | exact |
| `tests/e2e_lifecycle.rs` — init renders static placeholders (MODIFY, new test) | test (e2e) | request-response (CLI) | `test_init_keeps_committed_gitignore_when_template_is_absent` L3636-3666 | exact |
| `docs/ARCHITECTURE.md:80` (MODIFY, optional) | docs | — | — | n/a |

Proposed test additions inside `src/sync/mod.rs` `mod tests` (L2342) are mapped in the sync section below.

---

## Pattern Assignments

### `src/sanitizer/entropy.rs` (NEW — utility, transform)

**Analog:** `src/sync/merge.rs` (512 lines; a pure-function submodule of a parent module, with its own `#[cfg(test)] mod tests` at L150).

**Module header + doc-comment style** (`src/sync/merge.rs:1-25`): one `//!` line naming the
requirement/decision ID, then `///` docs that state input contracts and failure semantics.
```rust
//! Three-way text merge for managed files (D-02), backed by diffy.

use std::path::Path;
...
/// Merges `local_lf` and `upstream_lf` against their common `ancestor_lf`.
///
/// All three inputs must be LF-normalized text. An absent file must never be
/// passed as an empty string: ...
pub fn merge_texts(ancestor_lf: &str, local_lf: &str, upstream_lf: &str) -> MergeOutcome {
```
Copy as: `//! R11 entropy screen for credential-shaped tokens (SEC-01, D-04, D-05).`

**Named-constant style** (`src/process.rs:11-22`, also `src/sync/staging.rs:12-25`): every
threshold is a documented `const`, no magic numbers (operator code-style rule).
```rust
/// Time limit for local git queries (config reads, index updates, ls-files).
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Interval between child and reader polls.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
```
Apply to `ENTROPY_FACTOR = 0.80`, `ENTROPY_LEN_CAP = 64`, `SWITCH_FACTOR = 0.75`,
`MIN_TOKEN_LEN = 20`, class sizes `26/26/10`, and the marker `ude:allow-entropy`.

**Core algorithm:** no in-repo analog. Use RESEARCH.md "R11 core (probe-verified)" verbatim as the
starting point, plus Pitfall 3 (locked interpretation: char-count entropy, `>=` compare, ASCII
token runs, whole-token hex/UUID/SRI exemption, ssh body exempt only after a key type on the same
line, marker is a per-line substring).

**Test-module pattern** (`src/sync/merge.rs:150-175`): `use super::*;`, one behaviour per test,
paired `_negative_control` test.
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_overlapping_changes_conflict_negative_control() {
```

**Seeded sampling:** proptest is a dev-dependency (`Cargo.toml:26`), used today only through the
`proptest!` macro (`src/sync/mod.rs:2750-2773`). `TestRunner::deterministic()` + `new_tree` has
**no in-repo analog**; use RESEARCH.md "Don't Hand-Roll" row and Context7 citation.

---

### `src/sanitizer/gitleaks.rs` (NEW — utility, config load + transform)

**Analog A (serde + TOML):** `src/manifest/mod.rs`

Imports and struct derive (L1-37):
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UniversalConfig {
    pub generic: bool,
    #[serde(default)]
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    #[serde(rename = "type")]
    pub project_type: String,
```
Parse entry point returning the typed error, not swallowing it (L116-119):
```rust
impl UdeManifest {
    pub fn parse_str(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }
```
For gitleaks: `Deserialize` only (no `Serialize` needed); use `#[serde(default)]` for optional
arrays (`keywords`, `allowlists`, `stopwords`), `#[serde(rename = "secretGroup")]`,
`#[serde(rename = "regexTarget")]` for the camelCase keys. The loader must return
`Result<RuleSet, String>` with the rule id in each error (Pitfall 1/2: never `filter_map` errors).

**Analog B (compile-once static):** `src/process.rs:7, 47-51`
```rust
use std::sync::LazyLock;

/// `scheme://userinfo@`: the userinfo runs from `://` to the first `@` and contains
/// no `/`, whitespace or quote, so an `@` later in a path is not mistaken for it.
static URL_USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([A-Za-z][A-Za-z0-9+.\-]*://)[^/\s'"@]+@"#).expect("valid userinfo pattern")
});
```
Copy as `static RULES: LazyLock<RuleSet> = LazyLock::new(|| load(include_str!("rules/gitleaks.toml")).expect("vendored gitleaks rules compile"));`.
The `.expect(...)` with a reason string is the established form for an invariant guarded by a test
(`rules_load_all` asserts 222 rules, 0 errors). `include_str!` has **no existing use** in the repo.

**Why static, with evidence:** `Sanitizer::new()` is constructed in `SyncEngine::new()`
(`src/sync/mod.rs:402`), in `push_boundary` (`src/main.rs:1366`, called per status hint) and in the
pre-API scan (`src/main.rs:1074`). Compiling rules in `new()` would recompile on every one.

**Regex dialect:** no in-repo use of `regex::bytes`. Use RESEARCH.md "Go-compatible brace escape +
ASCII byte regex" (replace its `unwrap_or('\0')` per the planner note there).

**SHA-256 pin test:** reuse the hashing idiom at `src/sync/mod.rs:9` and `:456-460`:
```rust
use sha2::{Digest, Sha256};
...
        let mut hasher = Sha256::new();
```
Pin value: `e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf` (RESEARCH.md).

---

### `src/sanitizer/rules/gitleaks.toml` + `LICENSE-gitleaks` (NEW — vendored data)

No analog. Constraints from RESEARCH.md that the plan must carry:
- File name must be exactly `gitleaks.toml` (Pitfall 6): the repo's CI runs
  `"$(mise which gitleaks)" git --redact` (`.github/workflows/supply-chain.yml:32`) and the
  pre-commit hook runs `gitleaks git --pre-commit --staged --redact` (`scripts/hooks/pre-commit:18`),
  both with gitleaks' default config, whose global path allowlist matches `gitleaks\.toml`.
- Not to be confused with the shipped baseline `templates/universal/generic/security/gitleaks.toml`
  (a 20-line `useDefault = true` extension with a `templates/.*` path allowlist — the reason for D-01).

---

### `src/sanitizer/mod.rs` (MODIFY — service facade, transform)

**Analog:** itself.

**Current imports and struct** (L1-13) — `Regex` (Unicode, `&str`) per instance:
```rust
use regex::Regex;
use std::path::{Component, Path};

/// Result of a sanitization check
#[derive(Debug, PartialEq, Eq)]
pub enum SanitizationResult {
    Clean(String),
    SecretDetected { reason: String, pattern: String },
}

pub struct Sanitizer {
    secret_patterns: Vec<(&'static str, Regex)>,
}
```
Add `pub mod entropy; pub mod gitleaks;` at the top (sibling-module pattern: `src/sync/mod.rs:1-3`
declares `pub mod merge; pub mod staging; pub mod state;`).

**Current scan loop to wrap** (L60-71) — keep the signature so `src/main.rs:1077` and
`src/sync/mod.rs` callers compile; reason text must name rule + line, never the value:
```rust
    pub fn check_secrets(&self, content: &str) -> SanitizationResult {
        for (name, regex) in &self.secret_patterns {
            if regex.is_match(content) {
                return SanitizationResult::SecretDetected {
                    reason: format!("Forbidden secret pattern matched: {name}"),
                    pattern: name.to_string(),
                };
            }
        }
        SanitizationResult::Clean(content.to_string())
    }
```
Existing tests assert on `pattern` equality (`"GitHub Personal Access Token"`, `"Private Key"`,
`"Bearer Token"`, `"Generic API Key"`, `"GitLab Personal Access Token"`) — L172, 188, 203, 221, 235,
246, 275. Ordering "UDE named rules first" keeps those names winning over gitleaks ids.

**Current parameterizer to replace** (L76-112) — the boundary class `[/\\"'\s:]` and the
fixpoint `loop` (L85-94, L102-109) both go away under RESEARCH Pattern 3:
```rust
            let pattern = format!(
                r#"(^|[^A-Za-z0-9_.\-]){}([/\\"'\s:]|$)"#,
                regex::escape(xdg_config)
            );
            if let Ok(re) = Regex::new(&pattern) {
                loop {
                    let next = re
                        .replace_all(&result, "${1}{{ user.xdg_config }}$2")
```
Keep: `xdg_config` first and only when `!xdg_config.is_empty() && xdg_config != user_home`
(L79); the two emitted strings must equal `PARAMETERIZED_PLACEHOLDERS`
(`src/sync/mod.rs:2264`: `["{{ user.home }}", "{{ user.xdg_config }}"]`).

**Tests that must stay green** (regression anchors for the new predicate):
- `test_path_parameterization_boundary_safety` L281-305 (`/home/alice2`, `/backup/home/alice` untouched; xdg before home).
- `test_path_parameterization_repeated_adjacent_paths` L307-321 (`"/home/alice:/home/alice"` → both replaced — the case the fixpoint loop existed for).

**Runtime-built fixture pattern** (L163-169) — copy for every new positive:
```rust
        // Dynamically assembled fixtures so host git pre-commit hooks (gitleaks) don't trigger on test source
        let token_prefix = "ghp_";
        let token_body = "111122223333444455556666777788889999";
        let dirty = format!("export GITHUB_TOKEN={token_prefix}{token_body}");
```
**Literal to convert** (Wave 0 gap): L230-231 hold `b_jwt` as one literal string — the single
repo false positive under R11. Build it at runtime (e.g. join of parts or generated).

**Negative-control pattern** (L252-264):
```rust
    #[test]
    fn test_secret_detection_negative_control() {
        let sanitizer = Sanitizer::new();
        let benign = "export APP_ENV=production\nexport PORT=8080\n# Public documentation link: https://api.github.com";
        match sanitizer.check_secrets(benign) {
            SanitizationResult::Clean(content) => {
                assert_eq!(content, benign);
            }
            SanitizationResult::SecretDetected { reason, .. } => {
                panic!("False positive on benign content: {reason}");
            }
```

**Corpus control (templates/ + template/ → 0 hits):** walk under `env!("CARGO_MANIFEST_DIR")`.
Analog: `copy_repo_templates` in `tests/e2e_lifecycle.rs:1909-1923` (`walkdir::WalkDir`, already a
normal dependency at `Cargo.toml` so usable from unit tests):
```rust
fn copy_repo_templates(dest: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    for entry in walkdir::WalkDir::new(&source) {
        let entry = entry.expect("walk templates");
```
Current tracked count under `templates/` + `template/`: **33** files (`git ls-files templates template | wc -l`),
matching RESEARCH's "files ≥ 33" floor.

---

### `src/sync/mod.rs` — `push_updates` (MODIFY — service, pre-validation pass)

**Analog:** itself, L1321-1488.

**Signature and the context it builds today** (L1321-1335). `dummy_ctx` comes from the real host
home, not the `user_home` / `xdg_config` parameters — the render-back check must override them
(RESEARCH Pattern 4):
```rust
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
```
Override pattern for the context fields (test analog `src/sync/mod.rs:6166-6171`):
```rust
        let mut ctx = crate::template::TemplateContext::detect_for_project(
            local_dir.path(), "test-proj", "rust",
        );
        ctx.user.home = user_home.to_string();
```
Both nested (`ctx.user.home`, `ctx.user.xdg_config`) and flat (`ctx.user_home`, `ctx.xdg_config`)
fields exist (`src/template/mod.rs:8-20`); set all four.

**The staging scan block to restructure** (L1446-1475). Today the scan runs on raw `content`
before parameterization, and the non-UTF-8 `else` branch is never scanned (L1473-1475):
```rust
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
```
Target shape: compute `published` bytes in both branches, then one `scan_bytes(&published)` →
`PushResult::SecretBlocked { path, reason }`; then (UTF-8 only) render-back check against
`raw_local_str` (already `normalize_eol(content)`). Pass 1 is atomic pre-validation: any `return`
here happens before `StagedTransaction::new` (L1493), so nothing is written — keep refusals in Pass 1.

**Gate numbering comments** ("Gate 1" L1372, "Gate 2" L1392, "Gate 3" L1448, "Gate 4" L1459): a new
render-back gate should continue the series ("Gate 5: reversibility").

**Render-with-error pattern for the check** (pull path, L1063-1083) — the render error is a
refusal, not swallowed:
```rust
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
```
Do **not** copy the scan-path variant (L645-649), which swallows errors via
`.unwrap_or_else(|_| raw_string.clone())` — that is the proxy the check must avoid.

**Result variant for a reversibility refusal:** `PushResult` (L111-149) has `SecretBlocked`,
`IoFailed`, `ScanFailed`, `GitRemoteFailed`. `main.rs:1319-1326` maps `SecretBlocked` to exit 1
and `IoFailed` to exit 2 (L1335-1342). RESEARCH A5 wants exit 1 → either reuse `SecretBlocked`
with a reason, or add a variant plus a `main.rs` arm next to L1319. Planner's choice; whichever,
add the main.rs arm in the same plan.

---

### `src/sync/mod.rs` — publication gates (MODIFY — gate, batch over blobs)

**Analog:** itself. Both call sites have the same three-line shape; replace the lossy decode +
`check_secrets` with `scan_bytes(&blob_res.stdout)`, keeping the error strings (existing tests
match on them).

`stage_publication` (L1736-1745) — on a hit, `reset_index` then `Err`:
```rust
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
```
`validate_committed_tree` (L2033-2041):
```rust
            let blob_content = String::from_utf8_lossy(&blob_res.stdout);
            if let SanitizationResult::SecretDetected { reason, .. } =
                self.sanitizer.check_secrets(&blob_content)
            {
                return Err(format!(
                    "Post-commit publication gate violation: secret detected in committed HEAD blob {}: {reason}",
                    p.display()
                ));
            }
```
Rollback is already wired at the callers and needs no change — PR mode L1796-1798
(`rollback_commit` + `abandon_sync_branch`), direct mode L1872-1873 (`rollback_commit`). The gate
error surfaces as `PushRemoteError::Failed` → exit 2 (`src/main.rs:1290-1295`), RESEARCH A6.

Note: `use crate::sanitizer::{SanitizationResult, Sanitizer};` (L8) — if all three sync call sites
move to `scan_bytes`, drop `SanitizationResult` from this import only if nothing else uses it
(clippy `-D warnings` will flag an unused import).

---

### `src/sync/mod.rs` — `should_render` predicate (MODIFY, extraction)

Two inline copies today; extract one function and call it from scan (L642-644), pull (L1065-1067),
the new push render-back check, `main.rs` init and `kept_ancestor`:
```rust
                            let should_render = p.extension().and_then(|e| e.to_str())
                                == Some("jinja")
                                || raw_string.contains("{{");
```
Free-function analog in the same file: `template_has_logic` (L2262-2290) — `pub fn` at module
level with a `///` doc that names the decision ID, used by both sync and `main.rs`:
```rust
/// Whether `template` is a `.jinja` file with template constructs (other than the
/// parameterized path placeholders) that publishing rendered text would erase (CR-02).
/// An absent template has none.
pub fn template_has_logic(template: &Path) -> Result<bool, String> {
    if template.extension().is_none_or(|ext| ext != "jinja") {
```
Placement: `pub fn` in `src/sync/mod.rs` (alongside `template_has_logic`, `map_to_upstream`)
or in `src/template/mod.rs`. `main.rs` already calls `sync::map_to_upstream` and
`sync::refuse_symlink_under`, so either is reachable.

---

### `src/sync/mod.rs` tests (MODIFY — new integration tests)

**Hermetic git fixture** (L2379-2451) — every git-touching test uses it; the engine gets the fixture
env through `with_extra_env`:
```rust
    struct GitFixture {
        _home: tempfile::TempDir,
        gitconfig: PathBuf,
    }
    ...
        fn cmd(&self, dir: &Path) -> Command {
            let mut cmd = Command::new("git");
            cmd.current_dir(dir);
            for name in process::GIT_LOCAL_ENV_VARS {
                cmd.env_remove(name);
            }
            cmd.env("GIT_CONFIG_GLOBAL", &self.gitconfig)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
                .stdin(Stdio::null());
```

| Proposed test | Copy from | Lines | What to change |
|---|---|---|---|
| `gate_blocks_gitleaks_only_category` | `test_publication_gate_scans_staged_blob_rejecting_clean_filter_secrets` | 6245-6320 | Clean-filter script `inject_secret.sh` (L6264-6277) emits a runtime-built gitleaks-only token (e.g. Slack bot) instead of `ghp_`; keep the index-empty negative control (L6310-6311) and `push_failure` helper (L6534-6539). |
| `committed_gate_rolls_back_gitleaks_only_category` | `test_post_commit_verification_catches_hook_injected_secrets_and_rolls_back` | 6965-7027 | Clean commit passes (L6985-6993), injected commit fails with `"secret detected in committed HEAD blob"` (L7003-7012), `rollback_commit` then HEAD blob lacks the token (L7014-7026). |
| `push_blocks_secret_in_non_utf8_file` | `test_push_updates_blocked_on_secret` + `test_binary_non_utf8_files_scanned_and_tracked_by_hash` | 5497-5523, 7160-7207 | Write `[0xFF, 0xFE, ...]` + ASCII runtime token to `scripts/hooks/pre-push`; expect `PushResult::SecretBlocked`. Keep the 7161 test green (its bytes alone are clean). |
| `push_refuses_unrenderable_parameterized_file` | `test_push_updates_blocked_on_secret` | 5497-5523 | Local hook containing `${#FIRST_LINE}` (real shape: `templates/projects/generic/hooks/commit-msg:42-43`) plus a home path → refusal; control: same hook without the home path → `Success`. |
| SEC-03/04 round trip (must stay green) | `test_defect3_path_parameterization_roundtrip_static_template` | 6126-6184 | Unchanged; also `test_push_updates_clean_with_parameterization` L5464-5495. |

Direct `push_updates` test call shape (L5508-5515):
```rust
        let res = engine.push_updates(
            local_dir.path(),
            upstream_dir.path(),
            "/home/denniyahh",
            "/home/denniyahh/.config",
            false,
            true,
        );
```

---

### `src/main.rs` — init materialization (MODIFY — CLI controller, file-I/O)

**Analog:** `render_and_write` (L1658-1677) / `rendered_template` (L1680-1696) in the same file.

Today the six publishable non-jinja files go through `copy_template_file` (`fs::copy`, L1641-1656):
- `.gitconfig` ← `projects/generic/gitconfig.repo` (call L323-331)
- `.gitignore` ← `projects/generic/gitignore.repo` (call L363-368)
- `scripts/hooks/{commit-msg,pre-commit,pre-push}` (call L378-382, then `set_executable`)
- `scripts/check-in-container.sh` (call L391-395, then `set_executable`)
- scratch scripts (call L410-414) — local-only boundary, never published; init still copies them.

```rust
fn copy_template_file(root: &Path, src: &Path, dst: &Path) -> Result<(), String> {
    sync::refuse_symlink_under(root, src)?;
    if src.exists() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| { ... })?;
        }
        fs::copy(src, dst)
            .map_err(|e| format!("Failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }
    Ok(())
}
```
Pattern to copy for the rendered branch (L1680-1696) — note it uses `read_to_string`, which fails
on non-UTF-8; the new path must fall back to a byte copy for non-UTF-8 or no-`{{` content so
`${#…}` hooks stay byte-identical:
```rust
    sync::refuse_symlink_under(root, src)?;
    if !src.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(src)
        .map_err(|e| format!("Failed to read template {}: {e}", src.display()))?;
    renderer
        .render_str(&content, ctx)
        .map(Some)
        .map_err(|e| format!("Failed to render template {}: {e}", src.display()))
```
`renderer` and `ctx` are already in scope at the call sites (used by `render_and_write` at
L420-426). Error convention at each call site: `eprintln!("  {} Initialization failed: {e}", "✗".red()); std::process::exit(EXIT_HARD_FAILURE);`.

**`kept_ancestor` must use the same predicate** (L1700-1721). Today only `.jinja` is rendered
(L1712); a non-jinja template with `{{` would record raw bytes as ancestor while init writes
rendered text, so the file would read as locally modified:
```rust
    if src.extension().is_some_and(|ext| ext == "jinja") {
        return Ok(rendered_template(renderer, root, &src, ctx)?.map(String::into_bytes));
    }
    sync::refuse_symlink_under(root, &src)?;
    match fs::read(&src) {
        Ok(bytes) => Ok(Some(bytes)),
```

**Pre-API scan (unchanged)** L1073-1086 — inherits the new scanner through `check_secrets`;
exits with literal `1`:
```rust
                            let pre_sanitizer = sanitizer::Sanitizer::new();
                            if let sanitizer::SanitizationResult::SecretDetected {
                                reason, ..
                            } = pre_sanitizer.check_secrets(&local_txt)
```

**Exit-code constants** (L16-19): `EXIT_DRIFT = 1`, `EXIT_HARD_FAILURE = 2`. The
`SecretBlocked` arm (L1319-1326) uses a bare `std::process::exit(1)`.

---

### `src/template/mod.rs` (MODIFY only if `should_render` lands here)

**Renderer** (L152-169) — the single render path; `set_keep_trailing_newline(true)` matters for
byte-exact round trips:
```rust
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_keep_trailing_newline(true);
        Self { env }
    }

    /// Renders a template string given a context
    pub fn render_str(&self, template_str: &str, ctx: &TemplateContext) -> Result<String, String> {
```
**Hand-built context for tests** (L179-195) — use for the SEC-04 proptest
`prop_parameterize_render_roundtrip` so no test spawns git via `detect_for_project`:
```rust
        let ctx = TemplateContext {
            user: UserContext {
                home: "/home/testuser".to_string(),
                xdg_config: "/home/testuser/.config".to_string(),
                xdg_data: "/home/testuser/.local/share".to_string(),
            },
            user_home: "/home/testuser".to_string(),
            xdg_config: "/home/testuser/.config".to_string(),
            ...
            extra: HashMap::new(),
        };
```
Degenerate-home evidence: fallback to `"."` at L44-48 (and `src/main.rs:712-715`).

---

### `tests/e2e_lifecycle.rs` — `init_renders_placeholders_in_static_templates` (MODIFY, new test)

**Analog:** `test_init_keeps_committed_gitignore_when_template_is_absent` (L3636-3666) with helpers
`project_with_local_templates` (L2110-2116), `sync_cli` (L1208-1244), `reinit_rust` (L2118-2123),
`outputs` (L1246-1254).

```rust
#[test]
fn test_init_keeps_committed_gitignore_when_template_is_absent() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    fs::remove_file(project.join(GITIGNORE_TEMPLATE)).unwrap();
    ...
    reinit_rust(&ude, &project);
```
How the template edit reaches init: `project_with_local_templates` copies the repo `templates/`
into `<project>/templates`, and `find_templates_dir` (`src/main.rs:1615-1631`) tries the relative
`templates` first. Edit `project.join(GITCONFIG_TEMPLATE)` (const at L1924) to contain
`{{ user.home }}` before `reinit_rust`. The fake HOME is `root.join("fake_home")` (L1221), so assert
`.gitconfig` contains that path and no `{{`. Control: a hook template with `${#…}` and no `{{`
is byte-identical after init. Use `ude_command`/`sync_cli` only (they drop `TYPESAFE_API_KEY`, L29-39).

---

## Shared Patterns

### Secret fixtures built at runtime
**Source:** `src/sanitizer/mod.rs:166-169`, `src/sync/mod.rs:5504-5506`, `:6265-6270`, `:6996-6998`
**Apply to:** every positive in entropy, gitleaks category table, gate and staging tests.
Split prefix and body into separate `let`s and `format!` them; for R11 sampling, generate with a
seeded runner. Never commit a whole token as one literal (CI and pre-commit run gitleaks).

### Never print the matched value
**Source:** `check_secrets` reason (`src/sanitizer/mod.rs:64-67`) names only the rule; gate errors
(`src/sync/mod.rs:1741-1744`, `2037-2040`) interpolate `{reason}` and the path only.
**Apply to:** `scan_bytes` `Finding` (rule + line) and every new refusal string. Test assertions
should also check the value is absent from the error text (negative control).

### Fail closed, errors as `Result<_, String>`
**Source:** `template_has_logic` (`src/sync/mod.rs:2269-2282`) returns `Err(format!(...))` naming
the path; gate helpers reset the index before returning `Err` (`:1729-1733`).
**Apply to:** rule loader, render-back check, init render path.

### Exit codes
**Source:** `src/main.rs:16-19` (`EXIT_DRIFT = 1`, `EXIT_HARD_FAILURE = 2`), `SecretBlocked` → 1
(L1319-1326), gate `PushRemoteError::Failed` → 2 (L1290-1295).
**Apply to:** any new `PushResult` variant must get its own arm with an explicit code.

### Compile-once statics
**Source:** `src/process.rs:7, 49-51` (`LazyLock<Regex>` + `.expect("…")`).
**Apply to:** gitleaks `RuleSet`, the 6 UDE named rules (now `regex::bytes`), R11 shape regexes.

### Hermetic tests
**Source:** `GitFixture` (`src/sync/mod.rs:2385-2451`; `tests/e2e_lifecycle.rs:87-115`),
`sync_cli` env setup (`tests/e2e_lifecycle.rs:1228-1243`), `tempfile::tempdir()` everywhere.
**Apply to:** all new integration tests. No `std::env::set_var`; env only on spawned `Command`s.

### proptest form
**Source:** `src/sync/mod.rs:2750-2773` (`proptest::proptest! { #[test] fn …(x in strategy) { … } }`
with fully qualified `proptest::prop_assert_eq!` and an in-property negative control).
**Apply to:** `prop_no_partial_substring_corruption`, `prop_parameterize_render_roundtrip`.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/sanitizer/rules/gitleaks.toml` | config (vendored data) | compile-time embed | No vendored third-party data and no `include_str!` in the repo yet. Use RESEARCH.md Package Legitimacy Audit (URL, SHA-256, MIT). |
| `src/sanitizer/rules/LICENSE-gitleaks` | notice | — | First vendored third-party artifact. |
| R11 algorithm and Go-brace escape (inside new files) | algorithm | transform | No entropy or regex-dialect code exists; use RESEARCH.md Code Examples. |
| Seeded `TestRunner::deterministic()` sampling | test helper | — | proptest is used only through the `proptest!` macro today. |

## Metadata

**Analog search scope:** `src/` (all modules), `tests/e2e_lifecycle.rs`, `Cargo.toml`,
`.github/workflows/supply-chain.yml`, `scripts/hooks/pre-commit`, `templates/`, `docs/ARCHITECTURE.md`.
**Files scanned:** 14 read or excerpted.
**Pattern extraction date:** 2026-10-23

**What this map does not establish:**
- Line numbers are from commit `7723d4a`; any plan that edits `src/sync/mod.rs` first shifts the later ranges.
- I did not run any code or tests. "Must stay green" lists are the tests that cover the touched code, found by reading; I did not check that they are the only ones.
- The init fix's interaction with existing e2e init tests (for example `test_first_init_on_fresh_clone_keeps_committed_edits`, L3462) was not traced. The planner should run the full e2e suite after the `kept_ancestor` change.
- `docs/ARCHITECTURE.md:76-82` lists `{{ xdg.config }}`, `{{ xdg.data }}` and `{{ git.email }}`, none of which `parameterize_paths` emits. RESEARCH recommends fixing the doc rather than adding aliases. That edit is optional and was not mapped further.
