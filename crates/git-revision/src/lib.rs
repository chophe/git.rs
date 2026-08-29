//! Commit walking.
//!
//! A small subset of `revision.c`: a breadth/depth walk from given tip commits
//! following first parents (with optional full-parent traversal), producing
//! commit oids in insertion (date-ish) order. Used by `rev-list` and `log`.

pub mod resolve;
pub use resolve::{ResolveError, Resolver};

use std::collections::{HashSet, VecDeque};

use git_hash::Oid;
use git_object::{parse_commit, ObjectKind};

/// A reader callback that loads and parses a commit by id.
pub type CommitLoader<'a> = dyn FnMut(&Oid) -> Option<git_object::Commit> + 'a;

/// Options for a commit walk.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Traverse all parents (like `--all`); otherwise only the first parent.
    pub follow_all_parents: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            follow_all_parents: false,
        }
    }
}

/// A commit walker over the object database.
pub struct RevWalk<'a> {
    loader: &'a mut CommitLoader<'a>,
    options: WalkOptions,
}

impl<'a> RevWalk<'a> {
    pub fn new(loader: &'a mut CommitLoader<'a>, options: WalkOptions) -> RevWalk<'a> {
        RevWalk { loader, options }
    }

    /// Walk reachable commits from `tips`, returning their oids in insertion
    /// order (each commit visited once).
    pub fn walk(&mut self, tips: &[Oid]) -> Vec<Oid> {
        let mut seen: HashSet<Oid> = HashSet::new();
        let mut queue: VecDeque<Oid> = VecDeque::new();
        let mut out = Vec::new();

        for tip in tips {
            if seen.insert(*tip) {
                queue.push_back(*tip);
            }
        }

        while let Some(oid) = queue.pop_front() {
            out.push(oid);
            if let Some(commit) = (self.loader)(&oid) {
                let parents = if self.options.follow_all_parents {
                    commit.parents
                } else {
                    commit.parents.iter().take(1).cloned().collect()
                };
                for p in parents {
                    if seen.insert(p) {
                        queue.push_back(p);
                    }
                }
            }
        }
        out
    }
}

/// Parse a commit object read from an object database, validating its kind.
pub fn parse_commit_bytes(oid: &Oid, kind: ObjectKind, data: &[u8]) -> Option<git_object::Commit> {
    if kind != ObjectKind::Commit {
        return None;
    }
    parse_commit(data, oid.algorithm()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use git_hash::HashAlgorithm;
    use git_object::Object;

    /// Build a commit with the given parents and message.
    fn mk(tree: &Oid, parents: &[&Oid], msg: &str) -> (Oid, git_object::Commit) {
        let algo = tree.algorithm();
        let mut content = format!("tree {tree}\n");
        for p in parents {
            content.push_str(&format!("parent {p}\n"));
        }
        content.push_str("author A <a@b> 0 +0000\ncommitter C <c@d> 0 +0000\n\n");
        content.push_str(msg);
        let obj = Object::from_data(ObjectKind::Commit, content.into_bytes());
        (obj.compute_id(algo), git_object::commit::parse_commit(&obj.data, algo).unwrap())
    }

    #[test]
    fn walks_first_parent_chain() {
        let algo = HashAlgorithm::Sha1;
        let tree = *algo.empty_tree();
        let (o1, c1) = mk(&tree, &[], "m1\n");
        let (o2, c2) = mk(&tree, &[&o1], "m2\n");
        let (o3, c3) = mk(&tree, &[&o2], "m3\n");
        let store: HashMap<Oid, git_object::Commit> =
            [(o1, c1), (o2, c2), (o3, c3)].into_iter().collect();

        let mut loader = |oid: &Oid| store.get(oid).cloned();
        let mut rw = RevWalk::new(&mut loader, WalkOptions::default());
        assert_eq!(rw.walk(&[o3]), vec![o3, o2, o1]);
        // Already-seen commits are visited once.
        assert_eq!(rw.walk(&[o3, o2]), vec![o3, o2, o1]);
    }

    #[test]
    fn walks_all_parents_for_merges() {
        let algo = HashAlgorithm::Sha1;
        let tree = *algo.empty_tree();
        let (o1, c1) = mk(&tree, &[], "left\n");
        let (o2, c2) = mk(&tree, &[], "right\n");
        let (merge, cm) = mk(&tree, &[&o1, &o2], "merge\n");
        let store: HashMap<Oid, git_object::Commit> =
            [(o1, c1), (o2, c2), (merge, cm)].into_iter().collect();

        let mut loader = |oid: &Oid| store.get(oid).cloned();
        let mut rw = RevWalk::new(&mut loader, WalkOptions { follow_all_parents: true });
        let ids = rw.walk(&[merge]);
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&o1) && ids.contains(&o2));
    }
}
