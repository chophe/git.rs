//! The revision-walking option machine: a subset of `revision.c`'s
//! `rev_info` covering ordering, parent-count filters, content filters,
//! exclusion (`--not`, `^rev`, ranges), and count/limit options.

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering as CmpOrdering;

use git_hash::Oid;
use git_object::Commit;

/// Commit ordering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Commit-date order with parents-before-children (C's default).
    #[default]
    Default,
    /// Strict topological order (Kahn's algorithm, date tie-break).
    Topo,
    /// Commit-date order (parents allowed to interleave on clock skew).
    Date,
}

/// Options controlling a revision walk.
#[derive(Debug, Clone, Default)]
pub struct RevOptions {
    pub order: Order,
    pub reverse: bool,
    pub max_count: Option<usize>,
    pub skip: usize,
    /// `--first-parent`: follow only the first parent of merges.
    pub first_parent: bool,
    /// `--merges` / `--no-merges` / `--min-parents` / `--max-parents`.
    pub min_parents: usize,
    pub max_parents: Option<usize>,
    /// `--author=` / `--committer=` / `--grep=` header/message filters.
    pub authors: Vec<String>,
    pub committers: Vec<String>,
    pub greps: Vec<String>,
    pub invert_grep: bool,
    pub ignore_case: bool,
}

struct QueuedCommit {
    seq: usize,
    oid: Oid,
    commit_date: i64,
}

impl PartialEq for QueuedCommit {
    fn eq(&self, other: &Self) -> bool {
        self.commit_date == other.commit_date && self.seq == other.seq
    }
}
impl Eq for QueuedCommit {}
impl PartialOrd for QueuedCommit {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedCommit {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Max-heap by commit date; ties broken by insertion order (later
        // insertion first, like C's FIFO for equal dates).
        self.commit_date
            .cmp(&other.commit_date)
            .then(other.seq.cmp(&self.seq).reverse())
    }
}

/// Extract the commit timestamp from a raw commit's `committer` header.
fn commit_date(commit: &Commit) -> i64 {
    commit
        .committer
        .as_deref()
        .and_then(|raw| {
            raw.rsplit(' ')
                .find(|t| t.bytes().all(|b| b.is_ascii_digit()) && !t.is_empty())
                .and_then(|t| t.parse::<i64>().ok())
        })
        .unwrap_or(0)
}

fn matches_any(haystack: &str, needles: &[String], ignore_case: bool) -> bool {
    needles.iter().any(|n| {
        if ignore_case {
            haystack.to_lowercase().contains(&n.to_lowercase())
        } else {
            haystack.contains(n.as_str())
        }
    })
}

/// Walk the commits reachable from `tips`, excluding anything reachable
/// from `hidden`, applying the filters and ordering in `opts`.
/// Returns the oids in output order.
pub fn walk_commits(
    loader: &mut dyn FnMut(&Oid) -> Option<Commit>,
    tips: &[Oid],
    hidden: &[Oid],
    opts: &RevOptions,
) -> Vec<Oid> {
    // Date-ordered priority-queue walk (C git's limit_list): seeded with
    // the tips, FIFO tie-breaks for equal dates, parents pushed on emit.
    let mut seen: HashSet<Oid> = HashSet::new();
    let mut heap: BinaryHeap<QueuedCommit> = BinaryHeap::new();
    let mut seq = 0usize;
    let mut queue_init: VecDeque<Oid> = VecDeque::new();
    for t in tips {
        if seen.insert(t.clone()) {
            queue_init.push_back(t.clone());
        }
    }
    for h in hidden {
        seen.insert(h.clone());
    }
    for t in queue_init {
        if let Some(c) = loader(&t) {
            let date = commit_date(&c);
            heap.push(QueuedCommit { seq, oid: t, commit_date: date });
            seq += 1;
        }
    }

    let mut commits: HashMap<Oid, Commit> = HashMap::new();
    let mut ordered: Vec<Oid> = Vec::new();
    while let Some(q) = heap.pop() {
        if commits.contains_key(&q.oid) {
            continue;
        }
        let Some(commit) = loader(&q.oid) else { continue };
        let parents: Vec<Oid> = if opts.first_parent {
            commit.parents.iter().take(1).cloned().collect()
        } else {
            commit.parents.clone()
        };
        for p in parents {
            if seen.insert(p.clone()) {
                if let Some(pc) = loader(&p) {
                    let date = commit_date(&pc);
                    heap.push(QueuedCommit { seq, oid: p, commit_date: date });
                    seq += 1;
                }
            }
        }
        ordered.push(q.oid.clone());
        commits.insert(q.oid, commit);
    }

    // Remove the closure of the hidden commits (loaded independently of
    // the walk, which stops at hidden tips).
    if !hidden.is_empty() {
        let mut hide_queue: VecDeque<Oid> = hidden.iter().cloned().collect();
        let mut hidden_set: HashSet<Oid> = HashSet::new();
        while let Some(oid) = hide_queue.pop_front() {
            if !hidden_set.insert(oid.clone()) {
                continue;
            }
            if let Some(c) = loader(&oid) {
                for p in c.parents {
                    hide_queue.push_back(p);
                }
            }
        }
        for h in &hidden_set {
            commits.remove(h);
        }
        ordered.retain(|o| !hidden_set.contains(o));
    }

    // Apply parent-count and content filters (order-preserving).
    let _selected: Vec<Oid> = ordered.clone();
    ordered.retain(|oid| {
        let commit = &commits[oid];
        let nparents = commit.parents.len();
        if nparents < opts.min_parents {
            return false;
        }
        if let Some(max) = opts.max_parents {
            if nparents > max {
                return false;
            }
        }
        if let Some(author) = &commit.author {
            if !opts.authors.is_empty() && !matches_any(author, &opts.authors, opts.ignore_case) {
                return false;
            }
        }
        if let Some(committer) = &commit.committer {
            if !opts.committers.is_empty()
                && !matches_any(committer, &opts.committers, opts.ignore_case)
            {
                return false;
            }
        }
        if !opts.greps.is_empty() {
            let msg = String::from_utf8_lossy(&commit.message);
            if matches_any(&msg, &opts.greps, opts.ignore_case) == opts.invert_grep {
                return false;
            }
        }
        true
    });
    // Order.
    let mut ordered = if opts.order == Order::Topo {
        topo_order(&ordered, &commits)
    } else {
        ordered
    };

    if opts.reverse {
        ordered.reverse();
    }
    if opts.skip > 0 {
        ordered = ordered.into_iter().skip(opts.skip).collect();
    }
    if let Some(n) = opts.max_count {
        ordered.truncate(n);
    }
    ordered
}

/// Kahn's algorithm over the selected subgraph; ties break by commit date
/// (newer first), then by the date-order sequence.
fn topo_order(selected: &[Oid], commits: &HashMap<Oid, Commit>) -> Vec<Oid> {
    let set: HashSet<&Oid> = selected.iter().collect();
    // In-degree = number of selected children per commit.
    let mut children: HashMap<Oid, Vec<Oid>> = HashMap::new();
    for oid in selected {
        if let Some(c) = commits.get(oid) {
            for p in &c.parents {
                if set.contains(p) {
                    children.entry(p.clone()).or_default().push(oid.clone());
                }
            }
        }
    }
    // In-degree = number of children still unemitted.
    let mut indeg: HashMap<Oid, usize> = selected
        .iter()
        .map(|o| (o.clone(), children.get(o).map(|c| c.len()).unwrap_or(0)))
        .collect();
    let _ = &children;

    let mut heap: BinaryHeap<QueuedCommit> = BinaryHeap::new();
    for (i, oid) in selected.iter().enumerate() {
        if indeg.get(oid).copied().unwrap_or(0) == 0 {
            let date = commit_date(&commits[oid]);
            heap.push(QueuedCommit { seq: i, oid: oid.clone(), commit_date: date });
        }
    }
    let mut out = Vec::with_capacity(selected.len());
    while let Some(q) = heap.pop() {
        out.push(q.oid.clone());
        // Emitting a commit makes its parents' in-degrees (children counts)
        // decrease; a parent becomes ready when all its children are out.
        if let Some(c) = commits.get(&q.oid) {
            for p in &c.parents {
                if let Some(e) = indeg.get_mut(p) {
                    *e -= 1;
                    if *e == 0 {
                        let date = commits.get(p).map(commit_date).unwrap_or(0);
                        heap.push(QueuedCommit {
                            seq: usize::MAX,
                            oid: p.clone(),
                            commit_date: date,
                        });
                    }
                }
            }
        }
    }
    out
}

use std::collections::VecDeque;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mk(parents: Vec<Oid>, date: i64) -> (Oid, Commit, Oid) {
        let tree = git_hash::HashAlgorithm::Sha1.empty_tree().clone();
        let mut content = format!("tree {tree}\n");
        for p in &parents {
            content.push_str(&format!("parent {p}\n"));
        }
        content.push_str(&format!("author A <a@b> {date} +0000\ncommitter C <c@d> {date} +0000\n\nm\n"));
        let obj = git_object::Object::from_data(git_object::ObjectKind::Commit, content.into_bytes());
        let oid = obj.compute_id(git_hash::HashAlgorithm::Sha1);
        let commit = git_object::parse_commit(&obj.data, git_hash::HashAlgorithm::Sha1).unwrap();
        (oid, commit, tree)
    }

    #[test]
    fn topo_prefers_newest_ready_lineage() {
        let (c1_oid, c1, _) = mk(vec![], 1);
        let (c2_oid, c2, _) = mk(vec![c1_oid.clone()], 2);
        let (c3_oid, c3, _) = mk(vec![c2_oid.clone()], 3);
        let (side_oid, side, _) = mk(vec![c2_oid.clone()], 5);
        let (merge_oid, merge, _) = mk(vec![c3_oid.clone(), side_oid.clone()], 6);
        let store: HashMap<Oid, Commit> = [
            (c1_oid.clone(), c1),
            (c2_oid.clone(), c2),
            (c3_oid.clone(), c3),
            (side_oid.clone(), side),
            (merge_oid.clone(), merge),
        ]
        .into_iter()
        .collect();
        let mut loader = |oid: &Oid| store.get(oid).cloned();
        let opts = RevOptions { order: Order::Topo, ..Default::default() };
        let out = walk_commits(&mut loader, &[merge_oid.clone()], &[], &opts);
        assert_eq!(
            out,
            vec![merge_oid, side_oid, c3_oid, c2_oid, c1_oid],
            "topo order should match C git's date-desc priority among ready commits"
        );
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use crate::rev_info::walk_commits;
    use proptest::prelude::*;
    use std::collections::HashMap;

    proptest! {
        /// Topo output must be a valid topological order of the DAG:
        /// every child appears before each of its parents.
        #[test]
        fn topo_output_is_topological(dates in proptest::collection::vec(0i64..1000, 1..12)) {
            let algo = git_hash::HashAlgorithm::Sha1;
            let tree = algo.empty_tree().clone();
            // Chain + a side branch: commit i's parents.
            let mut commits: HashMap<Oid, Commit> = HashMap::new();
            let mut oids: Vec<Oid> = Vec::new();
            for (i, d) in dates.iter().enumerate() {
                let parents = match i {
                    0 => vec![],
                    _ => vec![oids[i - 1].clone()],
                };
                let mut content = format!("tree {tree}\n");
                for p in &parents {
                    content.push_str(&format!("parent {p}\n"));
                }
                content.push_str(&format!("author A <a@b> {d} +0000\ncommitter C <c@d> {d} +0000\n\nm{i}\n"));
                let obj = git_object::Object::from_data(git_object::ObjectKind::Commit, content.into_bytes());
                let oid = obj.compute_id(algo);
                let commit = git_object::parse_commit(&obj.data, algo).unwrap();
                oids.push(oid.clone());
                commits.insert(oid, commit);
            }
            let mut loader = |oid: &Oid| commits.get(oid).cloned();
            let tip = oids.last().unwrap().clone();
            let opts = RevOptions { order: Order::Topo, ..Default::default() };
            let out = walk_commits(&mut loader, &[tip], &[], &opts);
            // Position map: child before parent.
            let pos: HashMap<&Oid, usize> = out.iter().enumerate().map(|(i, o)| (o, i)).collect();
            for i in 1..oids.len() {
                let child_pos = pos[&oids[i]];
                let parent_pos = pos[&oids[i - 1]];
                prop_assert!(child_pos < parent_pos, "child {i} must precede its parent");
            }
        }
    }
}
