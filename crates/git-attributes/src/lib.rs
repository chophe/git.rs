//! Git ignore and attributes engine
//!
//! This crate implements the gitignore and gitattributes pattern matching
//! and lookup functionality, matching C Git's behavior exactly.

pub mod wildmatch;

pub use wildmatch::{wildmatch, flags};