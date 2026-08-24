//! Tree-to-tree comparison producing per-path changes.

use git_hash::Oid;
use git_object::{compare_entry_names, parse_tree, Object, TreeEntry};

/// A single changed path between two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub old_oid: Option<Oid>,
    pub new_oid: Option<Oid>,
    /// `A` added, `D` deleted, `M` modified, `T` type-change.
    pub status: char,
}

/// An object loader used to descend into subtrees.
pub type Loader<'a> = dyn FnMut(&Oid) -> Option<Object> + 'a;

/// Compare two trees, returning the list of changes.
///
/// With `recursive`, subtree changes are expanded to their contained files;
/// otherwise a changed subtree is reported as a single entry.
pub fn compare_trees(
    old: &[TreeEntry],
    new: &[TreeEntry],
    prefix: &str,
    recursive: bool,
    loader: &mut Loader<'_>,
) -> Vec<Change> {
    let mut changes = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);

    while i < old.len() || j < new.len() {
        let o = old.get(i);
        let n = new.get(j);
        match (o, n) {
            (Some(o), Some(n)) => {
                let ord = compare_entry_names(&o.name, o.is_dir(), &n.name, n.is_dir());
                if ord == std::cmp::Ordering::Less {
                    collect_delete(&mut changes, o, prefix, recursive, loader);
                    i += 1;
                } else if ord == std::cmp::Ordering::Greater {
                    collect_add(&mut changes, n, prefix, recursive, loader);
                    j += 1;
                } else {
                    // Same name.
                    if o.is_dir() && n.is_dir() && recursive {
                        let mut sub = compare_trees(
                            &load_tree(o, loader),
                            &load_tree(n, loader),
                            &join(prefix, &o.name),
                            recursive,
                            loader,
                        );
                        changes.append(&mut sub);
                    } else if o.oid != n.oid || o.mode != n.mode {
                        if o.is_dir() != n.is_dir() {
                            changes.push(Change {
                                path: join(prefix, &o.name),
                                old_mode: Some(o.mode.clone()),
                                new_mode: Some(n.mode.clone()),
                                old_oid: Some(o.oid),
                                new_oid: Some(n.oid),
                                status: 'T',
                            });
                        } else {
                            changes.push(Change {
                                path: join(prefix, &o.name),
                                old_mode: Some(o.mode.clone()),
                                new_mode: Some(n.mode.clone()),
                                old_oid: Some(o.oid),
                                new_oid: Some(n.oid),
                                status: 'M',
                            });
                        }
                    }
                    i += 1;
                    j += 1;
                }
            }
            (Some(o), None) => {
                collect_delete(&mut changes, o, prefix, recursive, loader);
                i += 1;
            }
            (None, Some(n)) => {
                collect_add(&mut changes, n, prefix, recursive, loader);
                j += 1;
            }
            (None, None) => unreachable!(),
        }
    }
    changes
}

fn collect_add(
    changes: &mut Vec<Change>,
    e: &TreeEntry,
    prefix: &str,
    recursive: bool,
    loader: &mut Loader<'_>,
) {
    if e.is_dir() && recursive {
        let sub = load_tree(e, loader);
        for entry in &sub {
            collect_add(changes, entry, &join(prefix, &e.name), recursive, loader);
        }
    } else {
        changes.push(Change {
            path: join(prefix, &e.name),
            old_mode: None,
            new_mode: Some(e.mode.clone()),
            old_oid: None,
            new_oid: Some(e.oid),
            status: 'A',
        });
    }
}

fn collect_delete(
    changes: &mut Vec<Change>,
    e: &TreeEntry,
    prefix: &str,
    recursive: bool,
    loader: &mut Loader<'_>,
) {
    if e.is_dir() && recursive {
        let sub = load_tree(e, loader);
        for entry in &sub {
            collect_delete(changes, entry, &join(prefix, &e.name), recursive, loader);
        }
    } else {
        changes.push(Change {
            path: join(prefix, &e.name),
            old_mode: Some(e.mode.clone()),
            new_mode: None,
            old_oid: Some(e.oid),
            new_oid: None,
            status: 'D',
        });
    }
}

fn load_tree(e: &TreeEntry, loader: &mut Loader<'_>) -> Vec<TreeEntry> {
    loader(&e.oid)
        .and_then(|obj| parse_tree(&obj.data, e.oid.algorithm()).ok())
        .unwrap_or_default()
}

fn join(prefix: &str, name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name).into_owned();
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;
    use git_object::ObjectKind;

    fn blob(oid: &Oid) -> Object {
        Object::from_data(ObjectKind::Blob, oid.as_slice().to_vec())
    }

    fn entry(mode: &str, name: &str, oid: Oid) -> TreeEntry {
        TreeEntry {
            mode: mode.to_string(),
            name: name.as_bytes().to_vec(),
            oid,
        }
    }

    #[test]
    fn detects_add_modify_delete() {
        let algo = HashAlgorithm::Sha1;
        let a = *algo.empty_blob();
        let b = *algo.empty_tree();
        let old = vec![entry("100644", "keep.txt", a), entry("100644", "gone.txt", b)];
        let new = vec![entry("100644", "keep.txt", a), entry("100644", "new.txt", b)];

        let mut loader = |oid: &Oid| -> Option<Object> { Some(blob(oid)) };
        let changes = compare_trees(&old, &new, "", false, &mut loader);
        assert_eq!(changes.len(), 2);
        let d = changes.iter().find(|c| c.path == "gone.txt").unwrap();
        assert_eq!(d.status, 'D');
        assert_eq!(d.new_oid, None);
        let add = changes.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(add.status, 'A');
        assert_eq!(add.old_oid, None);
    }
}