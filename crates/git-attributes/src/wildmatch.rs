//! Wildmatch pattern matching - ported from C Git's wildmatch.c
//!
//! Provides shell-style pattern matching for `?`, `\`, `[]`, and `*` characters.
//! This is the same engine used by gitignore and gitattributes.

/// Flags for wildmatch()
pub mod flags {
    /// Case-insensitive matching
    pub const CASEFOLD: u32 = 1;
    /// In pathname mode, `*` does not match `/`, but `**` does
    pub const PATHNAME: u32 = 2;
}

// Return values
const WM_ABORT_ALL: i32 = -1;
const WM_ABORT_TO_STARSTAR: i32 = -2;

/// Pattern did not match
pub const WM_NOMATCH: i32 = 1;
/// Pattern matched
pub const WM_MATCH: i32 = 0;

/// Match pattern `p` against `text` with the given flags.
///
/// Returns `WM_MATCH` (0) if the pattern matches, `WM_NOMATCH` (1) if it doesn't.
pub fn wildmatch(pattern: &str, text: &str, flags: u32) -> i32 {
    let pat_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    let res = do_wild(pat_bytes, text_bytes, 0, 0, flags);
    if res == WM_MATCH {
        WM_MATCH
    } else {
        WM_NOMATCH
    }
}

fn do_wild(pattern: &[u8], text: &[u8], mut p_idx: usize, mut t_idx: usize, flags: u32) -> i32 {
    let casefold = (flags & flags::CASEFOLD) != 0;
    let pathname = (flags & flags::PATHNAME) != 0;

    while p_idx < pattern.len() {
        let p_ch = pattern[p_idx];
        let t_ch = if t_idx < text.len() {
            text[t_idx]
        } else {
            // text exhausted
            if p_ch != b'*' {
                return WM_ABORT_ALL;
            }
            0
        };

        let t_ch = if casefold && t_ch.is_ascii_uppercase() {
            t_ch.to_ascii_lowercase()
        } else {
            t_ch
        };
        let mut p_ch = if casefold && p_ch.is_ascii_uppercase() {
            p_ch.to_ascii_lowercase()
        } else {
            p_ch
        };

        match p_ch {
            b'\\' => {
                // Literal match with following character
                p_idx += 1;
                if p_idx >= pattern.len() {
                    return WM_ABORT_ALL;
                }
                let p_ch = pattern[p_idx];
                let t_ch = if t_idx < text.len() { text[t_idx] } else { return WM_ABORT_ALL; };
                if t_ch != p_ch {
                    return WM_NOMATCH;
                }
                t_idx += 1;
                p_idx += 1;
            }
            b'?' => {
                // Match anything but '/' in pathname mode
                if pathname && t_ch == b'/' {
                    return WM_NOMATCH;
                }
                if t_idx >= text.len() {
                    return WM_ABORT_ALL;
                }
                t_idx += 1;
                p_idx += 1;
            }
            b'*' => {
                // Count consecutive asterisks
                let prev_p_idx = p_idx;
                p_idx += 1;
                let mut star_count = 1;
                while p_idx < pattern.len() && pattern[p_idx] == b'*' {
                    p_idx += 1;
                    star_count += 1;
                }

                let match_slash = if !pathname {
                    true
                } else if star_count >= 2
                    && (p_idx >= pattern.len()
                        || pattern[p_idx] == b'/'
                        || (pattern[p_idx] == b'\\'
                            && p_idx + 1 < pattern.len()
                            && pattern[p_idx + 1] == b'/'))
                {
                    // ** at end of pattern, or followed by /, matches slashes
                    // Try to match nothing at first
                    if p_idx < pattern.len()
                        && pattern[p_idx] == b'/'
                        && do_wild(pattern, text, p_idx + 1, t_idx, flags) == WM_MATCH
                    {
                        return WM_MATCH;
                    }
                    true
                } else if star_count >= 2
                    && (prev_p_idx == 0
                        || pattern[prev_p_idx - 1] == b'/')
                {
                    // ** at start of pattern or preceded by /, but not at end and not followed by /
                    // This is the "leading **" case - matches in all directories
                    // Try to match nothing at first
                    if p_idx < pattern.len()
                        && pattern[p_idx] == b'/'
                        && do_wild(pattern, text, p_idx + 1, t_idx, flags) == WM_MATCH
                    {
                        return WM_MATCH;
                    }
                    true
                } else {
                    false
                };

                if p_idx >= pattern.len() {
                    // Trailing "**" matches everything
                    if !match_slash {
                        // Trailing "*" matches only if no more slashes
                        if text[t_idx..].contains(&b'/') {
                            return WM_ABORT_TO_STARSTAR;
                        }
                    }
                    return WM_MATCH;
                } else if !match_slash && p_idx < pattern.len() && pattern[p_idx] == b'/' {
                    // One asterisk followed by slash matches the next directory
                    match text[t_idx..].iter().position(|&c| c == b'/') {
                        Some(pos) => {
                            t_idx += pos;
                            p_idx += 1; // consume slash
                            // Continue matching the rest
                            return do_wild(pattern, text, p_idx, t_idx, flags);
                        }
                        None => return WM_ABORT_ALL,
                    }
                } else {
                    // Try matching the rest of the pattern at each position
                    loop {
                        if t_idx >= text.len() {
                            break;
                        }

                        // Optimization: advance faster when asterisk followed by literal
                        if p_idx < pattern.len() && !is_glob_special(pattern[p_idx]) {
                            let next_p_ch = if casefold && pattern[p_idx].is_ascii_uppercase() {
                                pattern[p_idx].to_ascii_lowercase()
                            } else {
                                pattern[p_idx]
                            };
                            loop {
                                if t_idx >= text.len() {
                                    break;
                                }
                                let cur_t_ch = if casefold && text[t_idx].is_ascii_uppercase() {
                                    text[t_idx].to_ascii_lowercase()
                                } else {
                                    text[t_idx]
                                };
                                if !match_slash && text[t_idx] == b'/' {
                                    break;
                                }
                                if cur_t_ch == next_p_ch {
                                    break;
                                }
                                t_idx += 1;
                            }
                            let cur_t_ch = if t_idx < text.len() {
                                let c = text[t_idx];
                                if casefold && c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c }
                            } else {
                                0
                            };
                            if t_idx >= text.len() || cur_t_ch != next_p_ch {
                                if match_slash {
                                    return WM_ABORT_ALL;
                                } else {
                                    return WM_ABORT_TO_STARSTAR;
                                }
                            }
                        }

                        let matched = do_wild(pattern, text, p_idx, t_idx, flags);
                        if matched != WM_NOMATCH {
                            if !match_slash || matched != WM_ABORT_TO_STARSTAR {
                                return matched;
                            }
                        } else if !match_slash && t_idx < text.len() && text[t_idx] == b'/' {
                            return WM_ABORT_TO_STARSTAR;
                        }

                        if t_idx >= text.len() {
                            break;
                        }
                        t_idx += 1;
                    }
                    return WM_ABORT_ALL;
                }
            }
            b'[' => {
                // Character class
                if t_idx >= text.len() {
                    return WM_ABORT_ALL;
                }
                let t_ch = text[t_idx];
                p_idx += 1;
                if p_idx >= pattern.len() {
                    return WM_ABORT_ALL;
                }

                let mut negated = false;
                if pattern[p_idx] == b'!' || pattern[p_idx] == b'^' {
                    negated = true;
                    p_idx += 1;
                    if p_idx >= pattern.len() {
                        return WM_ABORT_ALL;
                    }
                }

                let mut prev_ch = 0u8;
                let mut matched = false;
                loop {
                    if p_idx >= pattern.len() {
                        return WM_ABORT_ALL;
                    }
                    p_ch = pattern[p_idx];
                    if p_idx + 1 < pattern.len() && pattern[p_idx] == b']' {
                        // ] at start of class is literal
                        if p_idx == 1 || (p_idx == 2 && negated) {
                            matched = t_ch == b']';
                            prev_ch = b']';
                            p_idx += 1;
                            continue;
                        }
                        break; // End of class
                    }
                    if p_ch == b']' {
                        break;
                    }

                    if p_ch == b'\\' {
                        p_idx += 1;
                        if p_idx >= pattern.len() {
                            return WM_ABORT_ALL;
                        }
                        p_ch = pattern[p_idx];
                        if t_ch == p_ch {
                            matched = true;
                        }
                    } else if p_ch == b'-'
                        && prev_ch != 0
                        && p_idx + 1 < pattern.len()
                        && pattern[p_idx + 1] != b']'
                    {
                        p_idx += 1;
                        if p_idx >= pattern.len() {
                            return WM_ABORT_ALL;
                        }
                        p_ch = pattern[p_idx];
                        if p_ch == b'\\' {
                            p_idx += 1;
                            if p_idx >= pattern.len() {
                                return WM_ABORT_ALL;
                            }
                            p_ch = pattern[p_idx];
                        }
                        if t_ch >= prev_ch && t_ch <= p_ch {
                            matched = true;
                        }
                    } else {
                        if t_ch == p_ch {
                            matched = true;
                        }
                    }
                    prev_ch = p_ch;
                    p_idx += 1;
                }

                if matched == negated {
                    return WM_NOMATCH;
                }
                t_idx += 1;
                p_idx += 1; // Skip past ]
            }
            _ => {
                // Literal match
                if t_idx >= text.len() {
                    return WM_ABORT_ALL;
                }
                if t_ch != p_ch {
                    return WM_NOMATCH;
                }
                t_idx += 1;
                p_idx += 1;
            }
        }
    }

    // Pattern exhausted - match if text also exhausted
    if t_idx >= text.len() {
        WM_MATCH
    } else {
        WM_NOMATCH
    }
}

/// Check if a character is a glob special character
fn is_glob_special(c: u8) -> bool {
    matches!(c, b'?' | b'*' | b'[' | b'\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal() {
        assert_eq!(wildmatch("foo", "foo", 0), WM_MATCH);
        assert_eq!(wildmatch("foo", "bar", 0), WM_NOMATCH);
        assert_eq!(wildmatch("foo", "fo", 0), WM_NOMATCH);
        assert_eq!(wildmatch("foo", "fooo", 0), WM_NOMATCH);
    }

    #[test]
    fn test_question() {
        assert_eq!(wildmatch("?", "a", 0), WM_MATCH);
        assert_eq!(wildmatch("?", "", 0), WM_NOMATCH);
        assert_eq!(wildmatch("?o", "fo", 0), WM_MATCH);
        assert_eq!(wildmatch("?o", "foo", 0), WM_NOMATCH);
    }

    #[test]
    fn test_star() {
        assert_eq!(wildmatch("*", "", 0), WM_MATCH);
        assert_eq!(wildmatch("*", "anything", 0), WM_MATCH);
        assert_eq!(wildmatch("f*", "foo", 0), WM_MATCH);
        assert_eq!(wildmatch("f*", "bar", 0), WM_NOMATCH);
        assert_eq!(wildmatch("*o", "foo", 0), WM_MATCH);
        assert_eq!(wildmatch("f*o", "foo", 0), WM_MATCH);
    }

    #[test]
    fn test_pathname_star() {
        // In pathname mode, * does not match /
        assert_eq!(wildmatch("*", "a/b", flags::PATHNAME), WM_NOMATCH);
        assert_eq!(wildmatch("f*", "foo/bar", flags::PATHNAME), WM_NOMATCH);
        // But ** does
        assert_eq!(wildmatch("**", "a/b", flags::PATHNAME), WM_MATCH);
        assert_eq!(wildmatch("f**", "foo/bar", flags::PATHNAME), WM_MATCH);
    }

    #[test]
    fn test_character_class() {
        assert_eq!(wildmatch("[abc]", "a", 0), WM_MATCH);
        assert_eq!(wildmatch("[abc]", "b", 0), WM_MATCH);
        assert_eq!(wildmatch("[abc]", "d", 0), WM_NOMATCH);
        assert_eq!(wildmatch("[a-z]", "m", 0), WM_MATCH);
        assert_eq!(wildmatch("[a-z]", "M", 0), WM_NOMATCH);
        assert_eq!(wildmatch("[!abc]", "d", 0), WM_MATCH);
        assert_eq!(wildmatch("[!abc]", "a", 0), WM_NOMATCH);
    }

    #[test]
    fn test_casefold() {
        assert_eq!(wildmatch("FOO", "foo", flags::CASEFOLD), WM_MATCH);
        assert_eq!(wildmatch("foo", "FOO", flags::CASEFOLD), WM_MATCH);
        assert_eq!(wildmatch("FOO", "bar", flags::CASEFOLD), WM_NOMATCH);
    }

    #[test]
    fn test_escape() {
        assert_eq!(wildmatch("\\?", "?", 0), WM_MATCH);
        assert_eq!(wildmatch("\\?", "a", 0), WM_NOMATCH);
        assert_eq!(wildmatch("\\*", "*", 0), WM_MATCH);
        assert_eq!(wildmatch("\\[", "[", 0), WM_MATCH);
    }

    #[test]
    fn test_starstar() {
        // ** matches any number of path components
        assert_eq!(wildmatch("foo/**", "foo/bar", flags::PATHNAME), WM_MATCH);
        assert_eq!(wildmatch("foo/**", "foo/bar/baz", flags::PATHNAME), WM_MATCH);
        assert_eq!(wildmatch("foo/**/bar", "foo/bar", flags::PATHNAME), WM_MATCH);
        assert_eq!(wildmatch("foo/**/bar", "foo/a/b/bar", flags::PATHNAME), WM_MATCH);
        assert_eq!(wildmatch("**/bar", "foo/bar", flags::PATHNAME), WM_MATCH);
    }
}