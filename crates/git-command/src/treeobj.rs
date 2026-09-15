//! Shared tree assembly from index paths (used by `write-tree` and
//! `read-tree` to build both tree objects and the cache-tree extension).

use std::collections::BTreeMap;

use git_hash::{HashAlgorithm, Oid};
use git_index::CacheTree;
use git_object::{serialize_tree, Object, ObjectKind, TreeEntry};
use git_odb::{LooseStore, OdbError};

/// A node in the tree hierarchy being assembled from index paths.
pub enum TreeNode {
    Blob { mode: u32, oid: Oid },
    Dir(BTreeMap<String, TreeNode>),
}

/// Insert `path` (already split into components) into the hierarchy.
pub fn insert_path(map: &mut BTreeMap<String, TreeNode>, comps: &[&str], mode: u32, oid: Oid) {
    if comps.is_empty() {
        return;
    }
    if comps.len() == 1 {
        map.insert(comps[0].to_string(), TreeNode::Blob { mode, oid });
        return;
    }
    let entry = map
        .entry(comps[0].to_string())
        .or_insert_with(|| TreeNode::Dir(BTreeMap::new()));
    if let TreeNode::Dir(sub) = entry {
        insert_path(sub, &comps[1..], mode, oid);
    }
}

/// Recursively build the tree object(s) for `map` and the matching cache-tree
/// node. `name` is this node's path component (`""` for the root). When
/// `store` is provided the tree objects are written to the object database.
pub fn build(
    map: &BTreeMap<String, TreeNode>,
    name: Vec<u8>,
    algo: HashAlgorithm,
    store: Option<&LooseStore>,
) -> Result<(Oid, CacheTree), OdbError> {
    let mut entries: Vec<TreeEntry> = Vec::with_capacity(map.len());
    let mut subtrees: Vec<CacheTree> = Vec::new();
    let mut entry_count: i32 = 0;
    for (child, node) in map {
        match node {
            TreeNode::Blob { mode, oid } => {
                entries.push(TreeEntry { mode: format!("{mode:o}"), name: child.as_bytes().to_vec(), oid: *oid });
                entry_count += 1;
            }
            TreeNode::Dir(sub) => {
                let (sub_oid, sub_ct) = build(sub, child.as_bytes().to_vec(), algo, store)?;
                entries.push(TreeEntry { mode: "40000".to_string(), name: child.as_bytes().to_vec(), oid: sub_oid });
                entry_count += sub_ct.entry_count;
                subtrees.push(sub_ct);
            }
        }
    }
    let data = serialize_tree(&entries, algo).map_err(|e| OdbError::Corrupt(e.to_string()))?;
    let obj = Object::from_data(ObjectKind::Tree, data);
    let oid = obj.compute_id(algo);
    if let Some(store) = store {
        store.write(&obj)?;
    }
    Ok((oid, CacheTree::new(name, entry_count, oid, subtrees)))
}

/// Assemble a cache-tree directly from index entries (computing the tree
/// object ids without writing them).
pub fn cache_tree_from_entries(
    entries: &[(u32, Oid, String)],
    algo: HashAlgorithm,
) -> Result<CacheTree, OdbError> {
    let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();
    for (mode, oid, path) in entries {
        let comps: Vec<&str> = path.split('/').collect();
        insert_path(&mut root, &comps, *mode, *oid);
    }
    let (_, ct) = build(&root, Vec::new(), algo, None)?;
    Ok(ct)
}
