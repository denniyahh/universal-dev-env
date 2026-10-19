//! Three-way text merge for managed files (D-02), backed by diffy.

use std::path::Path;

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

/// Rewrites diffy's hard-coded conflict labels to LOCAL / ANCESTOR / UPSTREAM.
pub fn relabel_conflict_markers(text: &str) -> String {
    text.to_string()
}

/// One version of a conflicted file, as shown in its `.conflict` sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide<'a> {
    /// LF-normalized text content.
    Text(&'a str),
    /// The file does not exist on this side.
    Absent,
    /// Non-text content, identified by its fingerprint.
    Binary { fingerprint: &'a str },
}

/// Renders the body of the `.conflict` sidecar for `rel`.
pub fn render_conflict_sidecar(
    _rel: &Path,
    _ancestor: ConflictSide<'_>,
    _local: ConflictSide<'_>,
    _upstream: ConflictSide<'_>,
) -> String {
    String::new()
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

    /// Every occurrence of a 7-character conflict marker, and how many of them do
    /// not start a line (glued onto the end of the previous line).
    fn marker_positions(text: &str) -> (usize, usize) {
        let mut total = 0;
        let mut glued = 0;
        for marker in ["<<<<<<<", "|||||||", "=======", ">>>>>>>"] {
            for (offset, _) in text.match_indices(marker) {
                total += 1;
                if offset != 0 && !text[..offset].ends_with('\n') {
                    glued += 1;
                }
            }
        }
        (total, glued)
    }

    #[test]
    fn test_adjacent_edits_conflict() {
        assert!(
            matches!(
                merge_texts("a=1\nb=1\n", "a=2\nb=1\n", "a=1\nb=2\n"),
                MergeOutcome::Conflict(_)
            ),
            "edits on adjacent lines must conflict"
        );
        // Negative control: one unchanged line between the edits merges cleanly.
        assert_eq!(
            merge_texts("a=1\nx\nb=1\n", "a=2\nx\nb=1\n", "a=1\nx\nb=2\n"),
            MergeOutcome::Clean("a=2\nx\nb=2\n".to_string())
        );
    }

    #[test]
    fn test_relabel_conflict_markers() {
        let marked = "<<<<<<< ours\nl\n||||||| original\no\n=======\nu\n>>>>>>> theirs\n";
        assert_eq!(
            relabel_conflict_markers(marked),
            "<<<<<<< LOCAL\nl\n||||||| ANCESTOR\no\n=======\nu\n>>>>>>> UPSTREAM\n"
        );
        // Negative control: lines that only resemble a marker are left untouched.
        for line in [
            "<<<<<<< oursX\n",
            "  <<<<<<< ours\n",
            "||||||| original \n",
            ">>>>>>> theirs!\n",
            "<<<<<<< ours",
        ] {
            assert_eq!(relabel_conflict_markers(line), line);
        }
    }

    #[test]
    fn test_merge_conflict_labels() {
        let MergeOutcome::Conflict(text) = merge_texts("a=1\n", "a=2\n", "a=3\n") else {
            panic!("expected a conflict");
        };
        for marker in [
            "<<<<<<< LOCAL\n",
            "||||||| ANCESTOR\n",
            "=======\n",
            ">>>>>>> UPSTREAM\n",
        ] {
            assert!(text.contains(marker), "missing {marker:?} in {text:?}");
        }
        for label in ["ours", "original", "theirs"] {
            assert!(!text.contains(label), "diffy label {label:?} in {text:?}");
        }
    }

    #[test]
    fn test_merge_crlf() {
        assert_eq!(
            merge_texts(
                "a=1\r\nb=1\r\nc=1\r\nd=1\r\n",
                "a=2\r\nb=1\r\nc=1\r\nd=1\r\n",
                "a=1\r\nb=1\r\nc=1\r\nd=2\r\n",
            ),
            MergeOutcome::Clean("a=2\r\nb=1\r\nc=1\r\nd=2\r\n".to_string())
        );
        // Negative control: the same edits in LF produce no CR.
        let MergeOutcome::Clean(lf) = merge_texts(
            "a=1\nb=1\nc=1\nd=1\n",
            "a=2\nb=1\nc=1\nd=1\n",
            "a=1\nb=1\nc=1\nd=2\n",
        ) else {
            panic!("expected a clean LF merge");
        };
        assert_eq!(lf, "a=2\nb=1\nc=1\nd=2\n");
        assert!(!lf.contains('\r'));
    }

    #[test]
    fn test_merge_no_trailing_newline_markers_start_line() {
        let (ancestor, local, upstream) = ("a=1\nb=1", "a=1\nb=2", "a=1\nb=3");
        let outcome = merge_texts(ancestor, local, upstream);
        assert_eq!(
            outcome,
            MergeOutcome::Conflict(
                "a=1\n<<<<<<< LOCAL\nb=2\n||||||| ANCESTOR\nb=1\n=======\nb=3\n>>>>>>> UPSTREAM\n"
                    .to_string()
            )
        );
        let MergeOutcome::Conflict(text) = outcome else {
            unreachable!()
        };
        assert_eq!(marker_positions(&text), (4, 0));

        // Negative control: diffy's default incomplete-hunk style glues markers onto
        // the last line of each side, and the helper sees it.
        let glued = MergeOptions::new()
            .merge(ancestor, local, upstream)
            .expect_err("the fixture must conflict");
        assert_eq!(
            glued,
            "a=1\n<<<<<<< ours\nb=2||||||| original\nb=1=======\nb=3>>>>>>> theirs\n"
        );
        assert_eq!(marker_positions(&glued), (4, 3));
    }

    const HEADER: &str = "# UDE sync conflict: .gitconfig\n\
        # Resolve with: ude sync resolve .gitconfig\n\
        # LOCAL = your working copy, ANCESTOR = last synced version, UPSTREAM = central template\n";

    #[test]
    fn test_render_conflict_sidecar_text() {
        let rel = Path::new(".gitconfig");
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Text("a=1\nb=1\n"),
            ConflictSide::Text("a=2\nb=1\n"),
            ConflictSide::Text("a=1\nb=2\n"),
        );
        let MergeOutcome::Conflict(body) = merge_texts("a=1\nb=1\n", "a=2\nb=1\n", "a=1\nb=2\n")
        else {
            panic!("expected a conflict");
        };
        assert_eq!(rendered, format!("{HEADER}\n{body}"));
        assert!(rendered.contains(".gitconfig"));
        assert!(rendered.contains("ude sync resolve .gitconfig"));
        let mut rest = rendered.as_str();
        for marker in [
            "<<<<<<< LOCAL\n",
            "||||||| ANCESTOR\n",
            "=======\n",
            ">>>>>>> UPSTREAM\n",
        ] {
            let at = rest
                .find(marker)
                .unwrap_or_else(|| panic!("{marker:?} missing or out of order"));
            rest = &rest[at + marker.len()..];
        }
        // Negative control: an all-text render never carries diffy's labels.
        assert!(!rendered.contains("ours"), "{rendered}");
        assert!(!rendered.contains("theirs"), "{rendered}");
        assert!(!rendered.contains("{{"), "{rendered}");

        // Three text sides that diffy merges cleanly still show all three versions.
        assert_eq!(
            render_conflict_sidecar(
                rel,
                ConflictSide::Text("a\n"),
                ConflictSide::Text("a\n"),
                ConflictSide::Text("b\n"),
            ),
            format!(
                "{HEADER}\n<<<<<<< LOCAL\na\n||||||| ANCESTOR\na\n=======\nb\n>>>>>>> UPSTREAM\n"
            )
        );
    }

    #[test]
    fn test_render_conflict_sidecar_deleted_upstream() {
        let rel = Path::new(".gitconfig");
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Text("a=1\n"),
            ConflictSide::Text("a=2"),
            ConflictSide::Absent,
        );
        assert_eq!(
            rendered,
            format!(
                "{HEADER}# UPSTREAM: deleted upstream\n\n\
                 <<<<<<< LOCAL\na=2\n||||||| ANCESTOR\na=1\n=======\n>>>>>>> UPSTREAM\n"
            )
        );
        assert!(
            rendered.contains("=======\n>>>>>>> UPSTREAM\n"),
            "UPSTREAM section must be empty"
        );
        assert_eq!(marker_positions(&rendered), (4, 0));

        // Symmetric: a local deletion against an upstream edit.
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Text("a=1\n"),
            ConflictSide::Absent,
            ConflictSide::Text("a=3\n"),
        );
        assert_eq!(
            rendered,
            format!(
                "{HEADER}# LOCAL: deleted locally\n\n\
                 <<<<<<< LOCAL\n||||||| ANCESTOR\na=1\n=======\na=3\n>>>>>>> UPSTREAM\n"
            )
        );

        // Never synced: no ancestor, and an empty text side stays an empty section.
        assert_eq!(
            render_conflict_sidecar(
                rel,
                ConflictSide::Absent,
                ConflictSide::Text(""),
                ConflictSide::Text("u"),
            ),
            format!(
                "{HEADER}# ANCESTOR: none (never synced)\n\n\
                 <<<<<<< LOCAL\n||||||| ANCESTOR\n=======\nu\n>>>>>>> UPSTREAM\n"
            )
        );
    }

    #[test]
    fn test_render_conflict_sidecar_binary() {
        let rel = Path::new(".gitconfig");
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Text("a=1\n"),
            ConflictSide::Binary {
                fingerprint: "fp-local",
            },
            ConflictSide::Binary {
                fingerprint: "fp-upstream",
            },
        );
        assert_eq!(
            rendered,
            format!(
                "{HEADER}# LOCAL fingerprint: fp-local\n# UPSTREAM fingerprint: fp-upstream\n\n\
                 # Binary content: no text diff available.\n"
            )
        );
        assert!(!rendered.contains("<<<<<<<"));

        // One binary side is enough to suppress the text body; the other notes stay.
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Binary {
                fingerprint: "fp-ancestor",
            },
            ConflictSide::Text("a=2\n"),
            ConflictSide::Absent,
        );
        assert_eq!(
            rendered,
            format!(
                "{HEADER}# ANCESTOR fingerprint: fp-ancestor\n# UPSTREAM: deleted upstream\n\n\
                 # Binary content: no text diff available.\n"
            )
        );
        let rendered = render_conflict_sidecar(
            rel,
            ConflictSide::Text("a=1\n"),
            ConflictSide::Text("a=2\n"),
            ConflictSide::Binary {
                fingerprint: "fp-upstream",
            },
        );
        assert!(rendered.contains("# UPSTREAM fingerprint: fp-upstream\n"));
        assert!(!rendered.contains("<<<<<<<"), "{rendered}");
    }

    const DIFFY_MARKERS: [&str; 3] = ["<<<<<<< ours", "||||||| original", ">>>>>>> theirs"];

    proptest::proptest! {
        #[test]
        fn test_relabel_prop(
            lines in proptest::collection::vec(
                proptest::strategy::Strategy::prop_filter(
                    r"[^\n]{0,24}",
                    "exact diffy marker line",
                    |line: &String| !DIFFY_MARKERS.contains(&line.as_str()),
                ),
                0..=12,
            ),
            index in proptest::prelude::any::<proptest::sample::Index>(),
        ) {
            let text = format!("{}\n", lines.join("\n"));
            proptest::prop_assert_eq!(relabel_conflict_markers(&text), text.clone());

            // Negative control: an exact diffy marker line inserted anywhere is relabeled,
            // and only that line changes.
            let at = index.index(lines.len() + 1);
            let mut marked = lines.clone();
            marked.insert(at, DIFFY_MARKERS[0].to_string());
            let relabeled = relabel_conflict_markers(&format!("{}\n", marked.join("\n")));
            let out: Vec<&str> = relabeled.split('\n').collect();
            proptest::prop_assert_eq!(out.len(), marked.len() + 1);
            for (i, line) in marked.iter().enumerate() {
                if i == at {
                    proptest::prop_assert_eq!(out[i], "<<<<<<< LOCAL");
                } else {
                    proptest::prop_assert_eq!(out[i], line.as_str());
                }
            }
        }
    }
}
