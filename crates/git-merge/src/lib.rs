//! Merge bases (commit reachability) and 3-way line merging.
//!
//! `merge_bases` computes the set of merge bases between two commits; `merge3`
//! performs a diff3-style 3-way content merge producing conflict markers.

use std::collections::HashSet;

use git_hash::Oid;

/// A loader that returns the parent oids of a commit.
pub type CommitLoader<'a> = dyn FnMut(&Oid) -> Vec<Oid> + 'a;

/// All commits reachable from `start` (including itself).
pub fn reachable(start: &Oid, loader: &mut CommitLoader<'_>) -> HashSet<Oid> {
    let mut seen = HashSet::new();
    let mut stack = vec![*start];
    while let Some(o) = stack.pop() {
        if seen.insert(o) {
            stack.extend(loader(&o));
        }
    }
    seen
}

/// The merge bases of `a` and `b`: common ancestors that are not themselves
/// reachable from another common ancestor.
pub fn merge_bases(a: &Oid, b: &Oid, loader: &mut CommitLoader<'_>) -> Vec<Oid> {
    let ra = reachable(a, loader);
    let rb = reachable(b, loader);
    let common: Vec<Oid> = ra.intersection(&rb).cloned().collect();

    let mut result = Vec::new();
    for c in &common {
        // c is a merge base unless it is reachable from another common
        // ancestor (i.e. c is dominated by a "closer" common ancestor).
        let dominated = common.iter().any(|d| d != c && reachable(d, loader).contains(c));
        if !dominated {
            result.push(*c);
        }
    }
    result
}

/// A region of the base replaced by `lines`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub lines: Vec<Vec<u8>>,
}

/// Compute the base-relative changes transforming `base` into `new`.
pub fn diff_changes(base: &[&[u8]], new: &[&[u8]]) -> Vec<Change> {
    let ops = git_diff::diff_lines(base, new);
    let mut changes = Vec::new();
    let (mut bi, mut ni) = (0usize, 0usize);
    let mut start: Option<usize> = None;
    let mut added: Vec<Vec<u8>> = Vec::new();
    for op in ops {
        match op {
            git_diff::Op::Keep => {
                if let Some(s) = start.take() {
                    changes.push(Change { start: s, end: bi, lines: std::mem::take(&mut added) });
                }
                bi += 1;
                ni += 1;
            }
            git_diff::Op::Delete => {
                if start.is_none() {
                    start = Some(bi);
                }
                bi += 1;
            }
            git_diff::Op::Insert => {
                added.push(new[ni].to_vec());
                ni += 1;
            }
        }
    }
    if let Some(s) = start.take() {
        changes.push(Change { start: s, end: bi, lines: added });
    }
    changes
}

/// The result of a 3-way merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub lines: Vec<Vec<u8>>,
    pub conflict: bool,
}

/// Merge the two change sets onto `base`, producing conflict markers when both
/// sides changed the same region differently.
pub fn merge3(
    base: &[&[u8]],
    ours: &[Change],
    theirs: &[Change],
    ours_label: &str,
    theirs_label: &str,
) -> MergeResult {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut bi = 0usize;
    let (mut oi, mut ti) = (0usize, 0usize);
    let mut conflict = false;

    loop {
        let oc = ours.get(oi).cloned();
        let tc = theirs.get(ti).cloned();
        match (oc, tc) {
            (Some(o), Some(t)) if o.start == bi && t.start == bi => {
                if o == t {
                    out.extend(o.lines.iter().cloned());
                    bi = o.end;
                    oi += 1;
                    ti += 1;
                } else {
                    conflict = true;
                    push_conflict(&mut out, &o.lines, &t.lines, ours_label, theirs_label);
                    bi = o.end.max(t.end);
                    oi += 1;
                    ti += 1;
                }
            }
            (Some(o), _) if o.start == bi => {
                out.extend(o.lines.iter().cloned());
                bi = o.end;
                oi += 1;
            }
            (_, Some(t)) if t.start == bi => {
                out.extend(t.lines.iter().cloned());
                bi = t.end;
                ti += 1;
            }
            _ => {
                if bi >= base.len() {
                    break;
                }
                out.push(base[bi].to_vec());
                bi += 1;
            }
        }
    }
    MergeResult { lines: out, conflict }
}

fn push_conflict(
    out: &mut Vec<Vec<u8>>,
    ours_lines: &[Vec<u8>],
    theirs_lines: &[Vec<u8>],
    ours_label: &str,
    theirs_label: &str,
) {
    out.push(format!("<<<<<<< {ours_label}\n").into_bytes());
    out.extend(ours_lines.iter().cloned());
    out.push(b"=======\n".to_vec());
    out.extend(theirs_lines.iter().cloned());
    out.push(format!(">>>>>>> {theirs_label}\n").into_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&[u8]> {
        s.split_inclusive('\n').map(|l| l.as_bytes()).collect()
    }

    #[test]
    fn merge_base_of_two_commits() {
        // Build a graph by hand.
        let mut store: std::collections::HashMap<Oid, Vec<Oid>> = Default::default();
        let o = |n: u8| Oid::new(git_hash::HashAlgorithm::Sha1, &[n; 20]);
        // base is root; a and b both from base; m merges a+b.
        store.insert(o(1), vec![]);
        store.insert(o(2), vec![o(1)]);
        store.insert(o(3), vec![o(1)]);
        store.insert(o(4), vec![o(2), o(3)]);
        let mut loader = |id: &Oid| store.get(id).cloned().unwrap_or_default();
        assert_eq!(merge_bases(&o(4), &o(4), &mut loader), vec![o(4)]);
        assert_eq!(merge_bases(&o(2), &o(3), &mut loader), vec![o(1)]);
        // Base of the merge and one parent is the parent.
        assert_eq!(merge_bases(&o(4), &o(2), &mut loader), vec![o(2)]);
    }

    #[test]
    fn clean_merge_applies_both_changes() {
        let base = lines("a\nb\nc\nd\ne\nf\n");
        let ours = lines("a\nB\nc\nd\ne\nf\n");
        let theirs = lines("a\nb\nc\nd\nE\nf\n");
        let oc = diff_changes(&base, &ours);
        let tc = diff_changes(&base, &theirs);
        let r = merge3(&base, &oc, &tc, "ours", "theirs");
        assert!(!r.conflict);
        let joined: Vec<u8> = r.lines.concat();
        assert_eq!(joined, b"a\nB\nc\nd\nE\nf\n");
    }

    #[test]
    fn conflicting_change_produces_markers() {
        let base = lines("a\nb\nc\n");
        let ours = lines("a\nX\nc\n");
        let theirs = lines("a\nY\nc\n");
        let oc = diff_changes(&base, &ours);
        let tc = diff_changes(&base, &theirs);
        let r = merge3(&base, &oc, &tc, "ours.txt", "theirs.txt");
        assert!(r.conflict);
        let joined: Vec<u8> = r.lines.concat();
        assert_eq!(
            joined,
            b"a\n<<<<<<< ours.txt\nX\n=======\nY\n>>>>>>> theirs.txt\nc\n"
        );
    }

    #[test]
    fn one_sided_change_applies() {
        let base = lines("a\nb\nc\nd\n");
        let ours = lines("a\nb\nZ\nd\n");
        let theirs = lines("a\nb\nc\nd\n");
        let oc = diff_changes(&base, &ours);
        let tc = diff_changes(&base, &theirs);
        let r = merge3(&base, &oc, &tc, "ours", "theirs");
        assert!(!r.conflict);
        let joined: Vec<u8> = r.lines.concat();
        assert_eq!(joined, b"a\nb\nZ\nd\n");
    }
}