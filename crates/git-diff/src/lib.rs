//! Diff: tree comparison and unified-diff rendering.

pub mod myers;
pub mod tree;
pub mod unified;

pub use myers::{diff as diff_lines, split_lines, Op};
pub use tree::{compare_trees, Change};
pub use unified::{diff_blobs, render_unified, CONTEXT};