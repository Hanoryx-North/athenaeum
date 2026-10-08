//! A three-way line merge, for the one place a sync cannot decide by itself.
//!
//! Two devices edited the same file. If they touched different lines the edits combine; if they
//! touched the same ones, the answer is not for software to pick. The merge reports which, and
//! never produces a result that quietly drops either side.

/// Past this many cells in the comparison table a merge gives up and reports a conflict. A
/// conflict is the safe answer: the person decides, and nothing is lost.
const MAX_TABLE_CELLS: usize = 4_000_000;

/// The outcome of merging two edits of one base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Both sets of edits combine without overlap.
    Clean(String),
    /// At least one region was edited differently on both sides.
    Conflicted,
}

/// Merge `ours` and `theirs`, two edits of `base`.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Merged {
    match run(base, ours, theirs, None) {
        Some((text, false)) => Merged::Clean(text),
        _ => Merged::Conflicted,
    }
}

/// Merge like [`merge3`], but write every overlapping region with git-style conflict markers
/// instead of giving up. For a person to edit by hand; the markers are the point.
pub fn merge3_with_markers(base: &str, ours: &str, theirs: &str, labels: (&str, &str)) -> String {
    match run(base, ours, theirs, Some(labels)) {
        Some((text, _)) => text,
        None => whole_file_markers(ours, theirs, labels),
    }
}

/// True when a file still contains a conflict marker line written by [`merge3_with_markers`].
pub fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line.starts_with("<<<<<<< ") || line == "=======" || line.starts_with(">>>>>>> ")
    })
}

fn whole_file_markers(ours: &str, theirs: &str, labels: (&str, &str)) -> String {
    let mut out = format!("<<<<<<< {}\n{}", labels.0, ours);
    if !ours.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("=======\n");
    out.push_str(theirs);
    if !theirs.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(">>>>>>> {}\n", labels.1));
    out
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Returns the merged text and whether any region conflicted, or `None` when the comparison was
/// too large to attempt.
fn run(
    base: &str,
    ours: &str,
    theirs: &str,
    markers: Option<(&str, &str)>,
) -> Option<(String, bool)> {
    let b = lines(base);
    let o = lines(ours);
    let t = lines(theirs);
    let to_ours = matches(&b, &o)?;
    let to_theirs = matches(&b, &t)?;

    let mut out = String::new();
    let mut conflicted = false;
    let (mut bi, mut oi, mut ti) = (0, 0, 0);

    let mut gap = |b_range: &[&str], o_range: &[&str], t_range: &[&str], out: &mut String| {
        if o_range == b_range {
            t_range.iter().for_each(|line| out.push_str(line));
        } else if t_range == b_range || o_range == t_range {
            o_range.iter().for_each(|line| out.push_str(line));
        } else {
            conflicted = true;
            if let Some((ours_label, theirs_label)) = markers {
                out.push_str(&format!("<<<<<<< {ours_label}\n"));
                push_terminated(out, o_range);
                out.push_str("=======\n");
                push_terminated(out, t_range);
                out.push_str(&format!(">>>>>>> {theirs_label}\n"));
            }
        }
    };

    for (index, line) in b.iter().enumerate() {
        let (Some(o_at), Some(t_at)) = (to_ours[index], to_theirs[index]) else {
            continue;
        };
        gap(&b[bi..index], &o[oi..o_at], &t[ti..t_at], &mut out);
        out.push_str(line);
        bi = index + 1;
        oi = o_at + 1;
        ti = t_at + 1;
    }
    gap(&b[bi..], &o[oi..], &t[ti..], &mut out);
    Some((out, conflicted))
}

fn push_terminated(out: &mut String, range: &[&str]) {
    range.iter().for_each(|line| out.push_str(line));
    if range.last().is_some_and(|line| !line.ends_with('\n')) {
        out.push('\n');
    }
}

/// For each line of `a`, the index of the line of `b` it is matched to in a longest common
/// subsequence, or `None`.
fn matches(a: &[&str], b: &[&str]) -> Option<Vec<Option<usize>>> {
    let mut mapping = vec![None; a.len()];

    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    for (index, slot) in mapping.iter_mut().enumerate().take(prefix) {
        *slot = Some(index);
    }
    let max_suffix = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    for k in 0..suffix {
        mapping[a.len() - 1 - k] = Some(b.len() - 1 - k);
    }

    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];
    if a_mid.is_empty() || b_mid.is_empty() {
        return Some(mapping);
    }
    if (a_mid.len() + 1).saturating_mul(b_mid.len() + 1) > MAX_TABLE_CELLS {
        return None;
    }

    let width = b_mid.len() + 1;
    let mut table = vec![0_u32; (a_mid.len() + 1) * width];
    for i in (0..a_mid.len()).rev() {
        for j in (0..b_mid.len()).rev() {
            table[i * width + j] = if a_mid[i] == b_mid[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a_mid.len() && j < b_mid.len() {
        if a_mid[i] == b_mid[j] {
            mapping[prefix + i] = Some(prefix + j);
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Some(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "one\ntwo\nthree\nfour\nfive\n";

    #[test]
    fn edits_to_different_lines_combine() {
        let ours = "ONE\ntwo\nthree\nfour\nfive\n";
        let theirs = "one\ntwo\nthree\nfour\nFIVE\n";
        assert_eq!(
            merge3(BASE, ours, theirs),
            Merged::Clean("ONE\ntwo\nthree\nfour\nFIVE\n".into())
        );
    }

    #[test]
    fn edits_to_the_same_line_conflict() {
        let ours = "one\nTWO\nthree\nfour\nfive\n";
        let theirs = "one\n2\nthree\nfour\nfive\n";
        assert_eq!(merge3(BASE, ours, theirs), Merged::Conflicted);
    }

    #[test]
    fn identical_edits_are_not_a_conflict() {
        let both = "one\nTWO\nthree\nfour\nfive\n";
        assert_eq!(merge3(BASE, both, both), Merged::Clean(both.into()));
    }

    #[test]
    fn insertions_at_both_ends_combine() {
        let ours = "zero\none\ntwo\nthree\nfour\nfive\n";
        let theirs = "one\ntwo\nthree\nfour\nfive\nsix\n";
        assert_eq!(
            merge3(BASE, ours, theirs),
            Merged::Clean("zero\none\ntwo\nthree\nfour\nfive\nsix\n".into())
        );
    }

    #[test]
    fn a_deletion_on_one_side_applies() {
        let ours = "one\nthree\nfour\nfive\n";
        assert_eq!(
            merge3(BASE, ours, BASE),
            Merged::Clean("one\nthree\nfour\nfive\n".into())
        );
    }

    #[test]
    fn markers_keep_both_sides_of_a_conflict() {
        let ours = "one\nTWO\nthree\nfour\nfive\n";
        let theirs = "one\n2\nthree\nfour\nfive\n";
        let text = merge3_with_markers(BASE, ours, theirs, ("local", "remote"));
        assert!(text.contains("<<<<<<< local\nTWO\n=======\n2\n>>>>>>> remote\n"), "{text}");
        assert!(has_conflict_markers(&text));
        assert!(!has_conflict_markers(BASE));
    }

    #[test]
    fn two_files_that_were_both_created_conflict_unless_equal() {
        assert_eq!(merge3("", "a\n", "b\n"), Merged::Conflicted);
        assert_eq!(merge3("", "a\n", "a\n"), Merged::Clean("a\n".into()));
    }
}
