//! Git ignore and attributes engine
//!
//! This crate implements the gitignore and gitattributes pattern matching
//! and lookup functionality, matching C Git's behavior exactly.

pub mod wildmatch;
pub mod ignore;
pub mod attributes;

pub use wildmatch::{wildmatch, WM_MATCH, WM_NOMATCH, flags as wildmatch_flags};
pub use ignore::{IgnoreEngine, IgnoreMatch, IgnorePattern, PatternFlags, parse_gitignore};
pub use attributes::{AttributesEngine, AttrCheck, AttrValue};
