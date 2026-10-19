use similar::{Algorithm, DiffOp, capture_diff_slices};

#[derive(Debug, PartialEq, Eq)]
pub enum MergeResult {
    Clean(String),
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit<'a> {
    base_start: usize,
    base_end: usize,
    replacement: Vec<&'a str>,
}

fn extract_edits<'a>(ops: Vec<DiffOp>, modified_lines: &'a [&'a str]) -> Vec<Edit<'a>> {
    let mut edits = Vec::new();
    for op in ops {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                edits.push(Edit {
                    base_start: old_index,
                    base_end: old_index + old_len,
                    replacement: Vec::new(),
                });
            }
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => {
                let replacement = modified_lines[new_index..new_index + new_len].to_vec();
                edits.push(Edit {
                    base_start: old_index,
                    base_end: old_index,
                    replacement,
                });
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let replacement = modified_lines[new_index..new_index + new_len].to_vec();
                edits.push(Edit {
                    base_start: old_index,
                    base_end: old_index + old_len,
                    replacement,
                });
            }
        }
    }
    edits
}

fn edits_overlap(a: &Edit, b: &Edit) -> bool {
    let a_is_insert = a.base_start == a.base_end;
    let b_is_insert = b.base_start == b.base_end;

    if a_is_insert && b_is_insert {
        a.base_start == b.base_start
    } else if a_is_insert {
        a.base_start > b.base_start && a.base_start < b.base_end
    } else if b_is_insert {
        b.base_start > a.base_start && b.base_start < a.base_end
    } else {
        a.base_start < b.base_end && b.base_start < a.base_end
    }
}

/// Performs a 3-way line merge between ancestor (base), local, and upstream text.
/// If changes from local and upstream are orthogonal or identical, returns MergeResult::Clean.
/// If changes collide on overlapping lines, returns MergeResult::Conflict.
pub fn merge_3way_text(base: &str, local: &str, upstream: &str) -> MergeResult {
    if local == upstream {
        return MergeResult::Clean(local.to_string());
    }
    if local == base {
        return MergeResult::Clean(upstream.to_string());
    }
    if upstream == base {
        return MergeResult::Clean(local.to_string());
    }

    let base_lines: Vec<&str> = base.lines().collect();
    let local_lines: Vec<&str> = local.lines().collect();
    let upstream_lines: Vec<&str> = upstream.lines().collect();

    let local_ops = capture_diff_slices(Algorithm::Myers, &base_lines, &local_lines);
    let upstream_ops = capture_diff_slices(Algorithm::Myers, &base_lines, &upstream_lines);

    let local_edits = extract_edits(local_ops, &local_lines);
    let upstream_edits = extract_edits(upstream_ops, &upstream_lines);

    // Check for collisions between any local edit and upstream edit
    for l in &local_edits {
        for u in &upstream_edits {
            if edits_overlap(l, u) {
                // If they overlap but are identical edits, they don't conflict
                if l.base_start == u.base_start
                    && l.base_end == u.base_end
                    && l.replacement == u.replacement
                {
                    continue;
                }
                return MergeResult::Conflict;
            }
        }
    }

    // Combine all edits tagged with their source
    enum TaggedEdit<'a> {
        Local(Edit<'a>),
        Upstream(Edit<'a>),
        Both(Edit<'a>),
    }

    let mut combined: Vec<TaggedEdit> = Vec::new();

    let mut u_used = vec![false; upstream_edits.len()];
    for l in local_edits {
        let mut matched = false;
        for (u_idx, u) in upstream_edits.iter().enumerate() {
            if l.base_start == u.base_start
                && l.base_end == u.base_end
                && l.replacement == u.replacement
            {
                combined.push(TaggedEdit::Both(l.clone()));
                u_used[u_idx] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            combined.push(TaggedEdit::Local(l));
        }
    }

    for (u_idx, u) in upstream_edits.into_iter().enumerate() {
        if !u_used[u_idx] {
            combined.push(TaggedEdit::Upstream(u));
        }
    }

    // Sort edits by base_start. If base_start is equal (e.g. insertion followed by replace),
    // stable sort ensures order.
    combined.sort_by_key(|e| match e {
        TaggedEdit::Local(ed) | TaggedEdit::Upstream(ed) | TaggedEdit::Both(ed) => {
            (ed.base_start, ed.base_end)
        }
    });

    let mut result_lines: Vec<&str> = Vec::new();
    let mut current_base_idx = 0;

    for te in combined {
        let edit = match &te {
            TaggedEdit::Local(e) | TaggedEdit::Upstream(e) | TaggedEdit::Both(e) => e,
        };

        // Emit untouched base lines up to edit start
        if edit.base_start > current_base_idx {
            result_lines.extend_from_slice(&base_lines[current_base_idx..edit.base_start]);
        }

        // Emit replacement lines
        result_lines.extend_from_slice(&edit.replacement);

        // Advance base index
        current_base_idx = current_base_idx.max(edit.base_end);
    }

    // Emit remaining base lines
    if current_base_idx < base_lines.len() {
        result_lines.extend_from_slice(&base_lines[current_base_idx..]);
    }

    let mut merged = result_lines.join("\n");
    if (base.ends_with('\n') || local.ends_with('\n') || upstream.ends_with('\n'))
        && !merged.is_empty()
        && !merged.ends_with('\n')
    {
        merged.push('\n');
    }

    MergeResult::Clean(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_orthogonal_changes_clean() {
        let base = "a=1\nx\nb=1\n";
        let local = "a=2\nx\nb=1\n";
        let upstream = "a=1\nx\nb=2\n";

        match merge_3way_text(base, local, upstream) {
            MergeResult::Clean(merged) => {
                assert_eq!(merged, "a=2\nx\nb=2\n");
            }
            MergeResult::Conflict => panic!("Expected clean merge for orthogonal changes"),
        }
    }

    #[test]
    fn test_merge_overlapping_changes_conflict_negative_control() {
        let base = "a=1\nb=1\n";
        let local = "a=2\nb=1\n";
        let upstream = "a=3\nb=1\n";

        match merge_3way_text(base, local, upstream) {
            MergeResult::Clean(_) => panic!("Expected conflict for overlapping changes"),
            MergeResult::Conflict => {}
        }
    }

    #[test]
    fn test_merge_identical_changes_clean() {
        let base = "a=1\nb=1\n";
        let local = "a=2\nb=1\n";
        let upstream = "a=2\nb=1\n";

        match merge_3way_text(base, local, upstream) {
            MergeResult::Clean(merged) => {
                assert_eq!(merged, "a=2\nb=1\n");
            }
            MergeResult::Conflict => panic!("Expected clean merge for identical changes"),
        }
    }

    #[test]
    fn test_merge_disjoint_append() {
        let base = "line1\n";
        let local = "line1\nlocal_extra\n";
        let upstream = "upstream_top\nline1\n";

        match merge_3way_text(base, local, upstream) {
            MergeResult::Clean(merged) => {
                assert_eq!(merged, "upstream_top\nline1\nlocal_extra\n");
            }
            MergeResult::Conflict => panic!("Expected clean merge for disjoint insertions"),
        }
    }
}
