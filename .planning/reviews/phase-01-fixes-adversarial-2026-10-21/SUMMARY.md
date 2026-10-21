# Adversarial external review — phase 01 post-review fixes (bc0cce5^..b2bbe33, src/ + tests/)

Lanes: codex gpt-6.1-sol (high), agy gemini-3.8-flash-high, pi deepseek-flash. None dropped.
Lane execution claims are NOT trusted as-is: pi built pre-fix binaries into the shared CARGO_TARGET_DIR.
Every finding below was re-run by the host against a clean HEAD build AND a pre-fix (bc0cce5^) build (probes.rs).

| # | Finding | HEAD | pre-fix | Class | Lanes |
|---|---------|------|---------|-------|-------|
| A | Unreadable committed file with no template: tombstone skipped (`if let Ok(fs::read)`), push publishes it | defect | published (readable too) | hole in 539a3d7 | codex, agy, pi |
| E | Stray/unusable `.git` above project (empty dir, copied worktree) aborts init exit 1 | defect | init ok | REGRESSION from 8e7c6dd | pi |
| C | Uncommitted managed file, template absent: init keeps it, records it as ancestor, pull deletes it | data loss | same | pre-existing | agy |
| B | Uncommitted .gitignore, template absent: status "Locally Modified", push publishes it | defect | same | pre-existing (todo "never publish kept files") | agy, pi |
| D | Monorepo subdir: core.hooksPath/include.path never set, success line printed | defect | same | pre-existing, exposed by 8e7c6dd | agy, pi |
| F | Symlinked project root: writes into target, state.json not saved, exit 0 | defect | same | pre-existing | agy |
| G | Kept file whose identical template reappears then is deleted: pull deletes the file | by design; init message "sync leaves it unmanaged" overstated | same | wording | codex |
| H | Unreadable .gitignore refusal happens after .gitconfig written (partial init) | exit 2 | exit 1 | pre-existing partial write | pi |

Not host-verified: TOCTOU between symlink pre-flight and writes (codex reports a FIFO repro; agy/pi suspected);
re-init `update-index --add` stages uncommitted hook edits; first init recreates an uncommitted deletion of a committed
file; save_to_dir failure prints warning then "sync leaves it unmanaged", exit 0.

Test gaps (agreed by host): uncommitted-.gitignore test never runs push; unreadable-.gitignore test does not assert
no partial write; monorepo test does not check hook registration; no tombstone→template-reappears test.

## Disposition (operator: "proceed as suggested", 2026-10-21)

- E fixed in bd08717 (stray parent .git warned and ignored; project's own .git still must work).
- A and C fixed in b5c11a3 (unreadable committed file stops init with exit 2; user's own file with no template left Unmanaged).
- B appended to todo "Never publish files that init kept over the shared template".
- D, F, H logged as todos dated 2026-10-21. G (wording) and the unverified items are not tracked separately.
