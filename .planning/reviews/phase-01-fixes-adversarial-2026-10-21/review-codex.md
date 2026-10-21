## CONFIRMED

All five were reproduced by execution against this snapshot’s build.

- **[High] `src/main.rs:251`, `src/main.rs:1515` — Symlink refusal ends before the write begins.** The comment promises “Init writes only inside the project,” but `render_and_write` later calls plain `fs::write`. Reproduction: block the AGENTS template read with a FIFO after preflight; insert `.agents/AGENTS.md → outside_file`; release the read. Init overwrites the external file and exits **0**. Without the symlink insertion, the external file remains unchanged. This demonstrates exploitation with concurrent filesystem access, not exploitation of a static symlink.

- **[High] `src/main.rs:539` — An unreadable committed file silently loses its protection against publication.** `if let Ok(bytes) = fs::read(...)` discards every read error. Reproduction: commit `.gitconfig`, remove its template, make the local file unreadable, run init, restore readability, then push. Init succeeds without recording a tombstone; push creates the supposedly forbidden upstream template. Control: the readable file receives a tombstone and is not published. Fail initialization instead of silently omitting this record.

- **[High] `src/main.rs:530`, `src/sync/mod.rs:493`, `src/sync/mod.rs:1137` — “Pull never deletes it” is false after template reappearance.** Reproduction: initialize a committed `.gitconfig` without a template; restore an identical template; pull; delete that template; pull again. The final pull **deletes the kept local file**. Equality produces `InSync`, recording an ancestor clears the tombstone (`src/sync/state.rs:247`), and subsequent deletion propagates normally. Control: with upstream continuously absent, pull preserves the file. The promised permanence is not implemented.

- **[Medium] `src/main.rs:496` — Re-init stages uncommitted hook content, despite describing this operation as a mode update.** The command uses `"update-index", "--add", "--chmod=+x"`. Reproduction: initialize and commit, edit `scripts/hooks/pre-push` without staging, then re-init. The edit appears in the index afterward; beforehand the cached diff was empty. This is a **pre-existing scope leak left unresolved by the fixes**: preserving working-tree bytes does not preserve the operator’s staging boundary.

- **[Medium] `src/main.rs:222`, `src/main.rs:310` — First init recreates an uncommitted deletion of a committed managed file.** `kept_committed` requires successful filesystem metadata, excluding precisely the committed paths deleted locally. Reproduction: commit `.gitconfig`, delete it without staging, then initialize with no prior state. Init restores the template copy. Control: an ordinary uncommitted edit survives. This is another **residual surrounding-code defect**, not a newly introduced regression.

## SUSPECTED

- **[High; not executed] `src/main.rs:948`, `src/main.rs:962` — TypeSafe retains the same symlink check/use race.** Metadata inspection and `read_to_string` are separate operations. Replacing AGENTS.md between them could transmit the symlink target before push rejects it. The static-symlink test does not establish safety against concurrent replacement.

## TEST GAPS

- **`tests/e2e_lifecycle.rs:3154`** — `git diff --name-only` excludes staged changes. The assertion “init changed committed files” cannot support index preservation; inspect `git diff --cached` too.
- **`tests/e2e_lifecycle.rs:3256`** — The uncommitted `.gitignore` test never enters the new `committed_keep` tombstone branch. It cannot detect reverting that fix.
- **`tests/e2e_lifecycle.rs:3183`** — Templates remain absent throughout the tombstone test. It misses reappearance, automatic adoption, and subsequent deletion.
- Seven existing targeted tests passed under an isolated Git test parent. These results cover their exercised cases only; Windows behavior was not verified.

No snapshot files were edited. Probe directories and processes were cleaned up; build artifacts remain in the requested shared Cargo cache.
