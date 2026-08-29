//! Diff: tree comparison and unified-diff rendering.

pub mod myers;
pub mod tree;
pub mod unified;

pub use myers::{diff as diff_lines, split_lines, Op};
pub use tree::{compare_trees, Change, MAX_SCORE};
pub use unified::{diff_blobs, diff_blobs_ctx, render_unified, CONTEXT};