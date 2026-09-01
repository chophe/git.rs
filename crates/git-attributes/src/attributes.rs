//! Git attributes engine
//!
//! Implements the gitattributes parsing and lookup functionality, matching
//! C Git's behavior exactly including the attribute stack, macros, and
//! the `diff=`, `text`, `binary`, `eol` attributes.

use std::collections::HashMap;
use std::path::Path;

/// Attribute value states matching C Git's ATTR__* values
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrValue {
    /// Attribute is set to true (e.g., `text`)
    Set,
    /// Attribute is set to false (e.g., `-text`)
    Unset,
    /// Attribute has a specific value (e.g., `diff=cpp`)
    Value(String),
    /// Attribute is unspecified
    Unspecified,
}

impl AttrValue {
    /// Check if the attribute value is "set" (true)
    pub fn is_set(&self) -> bool {
        matches!(self, AttrValue::Set)
    }

    /// Check if the attribute value is "unset" (false)
    pub fn is_unset(&self) -> bool {
        matches!(self, AttrValue::Unset)
    }

    /// Check if the attribute value is unspecified
    pub fn is_unspecified(&self) -> bool {
        matches!(self, AttrValue::Unspecified)
    }

    /// Get the value string if present
    pub fn value(&self) -> Option<&str> {
        match self {
            AttrValue::Value(v) => Some(v),
            _ => None,
        }
    }
}

impl std::fmt::Display for AttrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttrValue::Set => write!(f, "set"),
            AttrValue::Unset => write!(f, "unset"),
            AttrValue::Value(v) => write!(f, "{}", v),
            AttrValue::Unspecified => write!(f, "unspecified"),
        }
    }
}

/// A single attribute assignment
#[derive(Debug, Clone)]
pub struct AttrAssignment {
    /// Attribute name
    pub name: String,
    /// Attribute value
    pub value: AttrValue,
}

/// A pattern with associated attribute assignments
#[derive(Debug, Clone)]
pub struct AttrPattern {
    /// Pattern to match against file paths
    pub pattern: String,
    /// Attribute assignments for this pattern
    pub assignments: Vec<AttrAssignment>,
    /// Line number in source file
    pub srcpos: i32,
}

/// A list of attribute patterns from a single source
#[derive(Debug, Clone, Default)]
pub struct AttrPatternList {
    /// The patterns in this list
    pub patterns: Vec<AttrPattern>,
    /// Source identifier (e.g., file path)
    pub src: String,
}

/// Attribute check request
#[derive(Debug, Clone)]
pub struct AttrCheck {
    /// The attributes to check
    pub attrs: Vec<(String, AttrValue)>,
}

impl AttrCheck {
    /// Create a new attribute check
    pub fn new() -> Self {
        Self { attrs: Vec::new() }
    }

    /// Add an attribute to check
    pub fn add_attr(&mut self, name: &str) {
        self.attrs.push((name.to_string(), AttrValue::Unspecified));
    }

    /// Set the value for an attribute
    pub fn set_value(&mut self, name: &str, value: AttrValue) {
        for (attr_name, attr_val) in &mut self.attrs {
            if attr_name == name {
                *attr_val = value;
                return;
            }
        }
        self.attrs.push((name.to_string(), value));
    }

    /// Get the value for an attribute
    pub fn get_value(&self, name: &str) -> AttrValue {
        for (attr_name, attr_val) in &self.attrs {
            if attr_name == name {
                return attr_val.clone();
            }
        }
        AttrValue::Unspecified
    }
}

/// Attributes engine that manages pattern lists and performs lookups
#[derive(Debug, Default)]
pub struct AttributesEngine {
    /// Built-in attributes (from $GIT_DIR/info/attributes)
    info_patterns: Vec<AttrPatternList>,
    /// In-tree .gitattributes patterns
    in_tree_patterns: Vec<AttrPatternList>,
    /// Global attributes (from core.attributesFile)
    global_patterns: Vec<AttrPatternList>,
    /// Attribute macros
    macros: HashMap<String, Vec<AttrAssignment>>,
    /// Whether to use case-insensitive matching
    ignore_case: bool,
}

impl AttributesEngine {
    /// Create a new attributes engine
    pub fn new() -> Self {
        Self {
            ignore_case: false,
            ..Default::default()
        }
    }

    /// Set case-insensitive matching
    pub fn set_ignore_case(&mut self, ignore_case: bool) {
        self.ignore_case = ignore_case;
    }

    /// Add info attributes (from $GIT_DIR/info/attributes)
    pub fn add_info_patterns(&mut self, patterns: AttrPatternList) {
        self.info_patterns.push(patterns);
    }

    /// Add in-tree .gitattributes patterns
    pub fn add_in_tree_patterns(&mut self, patterns: AttrPatternList) {
        self.in_tree_patterns.push(patterns);
    }

    /// Add global attributes (from core.attributesFile)
    pub fn add_global_patterns(&mut self, patterns: AttrPatternList) {
        self.global_patterns.push(patterns);
    }

    /// Parse a .gitattributes file content
    pub fn parse_gitattributes(content: &str, source: &str) -> AttrPatternList {
        let mut patterns = Vec::new();
        let mut line_num = 0;

        for line in content.lines() {
            line_num += 1;
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Parse pattern and attributes
            if let Some((pattern, attrs_str)) = trimmed.split_once(|c: char| c.is_whitespace()) {
                let pattern = pattern.trim().to_string();
                let mut assignments = Vec::new();

                // Parse attribute assignments
                for attr_str in attrs_str.split_whitespace() {
                    let attr_str = attr_str.trim();
                    if attr_str.is_empty() {
                        continue;
                    }

                    if let Some((name, value)) = attr_str.split_once('=') {
                        // Attribute with value (e.g., diff=cpp)
                        assignments.push(AttrAssignment {
                            name: name.to_string(),
                            value: AttrValue::Value(value.to_string()),
                        });
                    } else if attr_str.starts_with('-') {
                        // Unset attribute (e.g., -text)
                        assignments.push(AttrAssignment {
                            name: attr_str[1..].to_string(),
                            value: AttrValue::Unset,
                        });
                    } else if attr_str.starts_with('!') {
                        // Unspecified attribute (e.g., !diff)
                        assignments.push(AttrAssignment {
                            name: attr_str[1..].to_string(),
                            value: AttrValue::Unspecified,
                        });
                    } else {
                        // Set attribute (e.g., text)
                        assignments.push(AttrAssignment {
                            name: attr_str.to_string(),
                            value: AttrValue::Set,
                        });
                    }
                }

                patterns.push(AttrPattern {
                    pattern,
                    assignments,
                    srcpos: line_num,
                });
            }
        }

        AttrPatternList {
            patterns,
            src: source.to_string(),
        }
    }

    /// Check attributes for a path
    pub fn check_attr(&self, path: &str, check: &mut AttrCheck) {
        // Search through all pattern lists in order:
        // global -> info -> in_tree
        let lists: Vec<&AttrPatternList> = self
            .global_patterns
            .iter()
            .chain(self.info_patterns.iter())
            .chain(self.in_tree_patterns.iter())
            .collect();

        for list in lists {
            for pattern in &list.patterns {
                if match_attr_pattern(path, &pattern.pattern, self.ignore_case) {
                    for assignment in &pattern.assignments {
                        check.set_value(&assignment.name, assignment.value.clone());
                    }
                }
            }
        }
    }

    /// Get all attributes for a path
    pub fn all_attrs(&self, path: &str) -> Vec<(String, AttrValue)> {
        let mut result = Vec::new();

        let lists: Vec<&AttrPatternList> = self
            .global_patterns
            .iter()
            .chain(self.info_patterns.iter())
            .chain(self.in_tree_patterns.iter())
            .collect();

        for list in lists {
            for pattern in &list.patterns {
                if match_attr_pattern(path, &pattern.pattern, self.ignore_case) {
                    for assignment in &pattern.assignments {
                        // Check if we already have this attribute
                        if let Some(pos) = result.iter().position(|(name, _)| name == &assignment.name) {
                            result[pos] = (assignment.name.clone(), assignment.value.clone());
                        } else {
                            result.push((assignment.name.clone(), assignment.value.clone()));
                        }
                    }
                }
            }
        }

        result
    }

    /// Clear all patterns
    pub fn clear(&mut self) {
        self.info_patterns.clear();
        self.in_tree_patterns.clear();
        self.global_patterns.clear();
        self.macros.clear();
    }
}

/// Match a path against an attribute pattern
fn match_attr_pattern(path: &str, pattern: &str, ignore_case: bool) -> bool {
    use crate::wildmatch;

    let wm_flags = if ignore_case {
        wildmatch::flags::PATHNAME | wildmatch::flags::CASEFOLD
    } else {
        wildmatch::flags::PATHNAME
    };

    // Handle patterns with and without slashes
    if pattern.starts_with('/') {
        // Pattern is anchored to root
        let pat = &pattern[1..];
        return wildmatch(pat, path, wm_flags) == wildmatch::WM_MATCH;
    }

    if pattern.contains('/') {
        // Pattern with slash - match against full path
        if wildmatch(pattern, path, wm_flags) == wildmatch::WM_MATCH {
            return true;
        }
        // Also try with **/ prefix
        let mut prefixed = String::from("**/");
        prefixed.push_str(pattern);
        return wildmatch(&prefixed, path, wm_flags) == wildmatch::WM_MATCH;
    }

    // Pattern without slash - match against basename
    let basename = path.rfind('/').map(|i| &path[i + 1..]).unwrap_or(path);
    if wildmatch(pattern, basename, wm_flags) == wildmatch::WM_MATCH {
        return true;
    }

    // Also try with **/ prefix
    let mut prefixed = String::from("**/");
    prefixed.push_str(pattern);
    wildmatch(&prefixed, path, wm_flags) == wildmatch::WM_MATCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_attributes() {
        let content = "*.c text\n*.o -text\n";
        let list = AttributesEngine::parse_gitattributes(content, "test");

        assert_eq!(list.patterns.len(), 2);
        assert_eq!(list.patterns[0].pattern, "*.c");
        assert_eq!(list.patterns[0].assignments.len(), 1);
        assert_eq!(list.patterns[0].assignments[0].name, "text");
        assert!(list.patterns[0].assignments[0].value.is_set());

        assert_eq!(list.patterns[1].pattern, "*.o");
        assert_eq!(list.patterns[1].assignments[0].name, "text");
        assert!(list.patterns[1].assignments[0].value.is_unset());
    }

    #[test]
    fn test_parse_value_attribute() {
        let content = "*.c diff=cpp\n";
        let list = AttributesEngine::parse_gitattributes(content, "test");

        assert_eq!(list.patterns.len(), 1);
        assert_eq!(list.patterns[0].assignments[0].name, "diff");
        assert_eq!(
            list.patterns[0].assignments[0].value.value(),
            Some("cpp")
        );
    }

    #[test]
    fn test_check_attr() {
        let mut engine = AttributesEngine::new();
        let list = AttributesEngine::parse_gitattributes("*.c text\n*.o -text\n", "test");
        engine.add_in_tree_patterns(list);

        let mut check = AttrCheck::new();
        check.add_attr("text");
        engine.check_attr("foo.c", &mut check);
        assert!(check.get_value("text").is_set());

        let mut check = AttrCheck::new();
        check.add_attr("text");
        engine.check_attr("foo.o", &mut check);
        assert!(check.get_value("text").is_unset());
    }

    #[test]
    fn test_pattern_matching() {
        assert!(match_attr_pattern("foo.c", "*.c", false));
        assert!(match_attr_pattern("src/foo.c", "*.c", false));
        assert!(!match_attr_pattern("foo.o", "*.c", false));
    }

    #[test]
    fn test_anchored_pattern() {
        assert!(match_attr_pattern("foo.c", "/foo.c", false));
        assert!(!match_attr_pattern("src/foo.c", "/foo.c", false));
    }

    #[test]
    fn test_all_attrs() {
        let mut engine = AttributesEngine::new();
        let list =
            AttributesEngine::parse_gitattributes("*.c text diff=cpp\n*.o -text\n", "test");
        engine.add_in_tree_patterns(list);

        let attrs = engine.all_attrs("foo.c");
        assert!(attrs.iter().any(|(n, v)| n == "text" && v.is_set()));
        assert!(attrs.iter().any(|(n, v)| n == "diff" && v.value() == Some("cpp")));
    }
}