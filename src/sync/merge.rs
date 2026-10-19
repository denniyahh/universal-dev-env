//! Three-way text merge for managed files (D-02), backed by diffy.

use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};

/// Result of a three-way text merge.
#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Local and upstream changes combine without overlap; carries the merged text.
    Clean(String),
    /// Local and upstream changes overlap or touch adjacent lines; carries the text
    /// with diff3 conflict markers, each marker starting its own line.
    Conflict(String),
}

/// Merges `local_lf` and `upstream_lf` against their common `ancestor_lf`.
///
/// All three inputs must be LF-normalized text. An absent file must never be
/// passed as an empty string: diffy treats `""` as real content, so a deletion on
/// both sides would read as a clean merge to an empty file.
///
/// Edits on adjacent lines conflict (as in git); edits separated by at least one
/// unchanged line merge cleanly, and identical edits on both sides merge cleanly.
pub fn merge_texts(ancestor_lf: &str, local_lf: &str, upstream_lf: &str) -> MergeOutcome {
    let mut opts = MergeOptions::new();
    opts.set_conflict_style(ConflictStyle::Diff3)
        .set_incomplete_hunk_style(IncompleteHunkStyle::Git);
    // diffy calls the sides "ours" and "theirs": ours = local, theirs = upstream.
    match opts.merge(ancestor_lf, local_lf, upstream_lf) {
        Ok(merged) => MergeOutcome::Clean(merged),
        Err(marked) => MergeOutcome::Conflict(marked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_orthogonal_changes_clean() {
        let base = "a=1\nx\nb=1\n";
        let local = "a=2\nx\nb=1\n";
        let upstream = "a=1\nx\nb=2\n";

        assert_eq!(
            merge_texts(base, local, upstream),
            MergeOutcome::Clean("a=2\nx\nb=2\n".to_string())
        );
    }

    #[test]
    fn test_merge_overlapping_changes_conflict_negative_control() {
        let base = "a=1\nb=1\n";
        let local = "a=2\nb=1\n";
        let upstream = "a=3\nb=1\n";

        assert!(
            matches!(
                merge_texts(base, local, upstream),
                MergeOutcome::Conflict(_)
            ),
            "Expected conflict for overlapping changes"
        );
    }

    #[test]
    fn test_merge_identical_changes_clean() {
        let base = "a=1\nb=1\n";
        let local = "a=2\nb=1\n";
        let upstream = "a=2\nb=1\n";

        assert_eq!(
            merge_texts(base, local, upstream),
            MergeOutcome::Clean("a=2\nb=1\n".to_string())
        );
    }

    #[test]
    fn test_merge_disjoint_append() {
        let base = "line1\n";
        let local = "line1\nlocal_extra\n";
        let upstream = "upstream_top\nline1\n";

        assert_eq!(
            merge_texts(base, local, upstream),
            MergeOutcome::Clean("upstream_top\nline1\nlocal_extra\n".to_string())
        );
    }
}
