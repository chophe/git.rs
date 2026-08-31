//! Builtin userdiff driver patterns and matching logic.

use regex::bytes::{Regex, RegexBuilder};

pub struct UserdiffDriver {
    pub name: &'static str,
    pub funcname: Option<&'static str>,
    pub is_icase: bool,
}

/// The built-in userdiff drivers, matching C git's `builtin_drivers` exactly.
/// Each entry has a name and optionally a multi-line pattern representing funcname regexes.
/// In C git, these pattern strings can contain multiple regexes separated by `\n`.
/// If a line starts with `!`, it's a negative pattern.
pub const BUILTIN_DRIVERS: &[UserdiffDriver] = &[
    UserdiffDriver {
        name: "ada",
        funcname: Some(
            "!^(.*[ \t])?(is[ \t]+new|renames|is[ \t]+separate)([ \t].*)?$\n\
             !^[ \t]*with[ \t].*$\n\
             ^[ \t]*((procedure|function)[ \t]+.*)$\n\
             ^[ \t]*((package|protected|task)[ \t]+.*)$",
        ),
        is_icase: true,
    },
    UserdiffDriver {
        name: "bash",
        funcname: Some(
            "^[ \t]*(((([a-zA-Z_][a-zA-Z0-9_]*[ \t]*\\([ \t]*\\))|(function[ \t]+[a-zA-Z_][a-zA-Z0-9_]*(([ \t]*\\([ \t]*\\))|([ \t]+)))).*$)"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "bibtex",
        funcname: Some("(@[a-zA-Z]{1,}[ \t]*\\{{0,1}[ \t]*[^ \t\"@',\\#}{~%]*).*$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "cpp",
        funcname: Some(
            "!^[ \t]*[A-Za-z_][A-Za-z_0-9]*:[[:space:]]*($|/[/*])\n\
             ^((::[[:space:]]*)?[A-Za-z_].*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "csharp",
        funcname: Some(
            "!(^|[ \t]+)(do|while|for|foreach|if|else|new|default|return|switch|case|throw|catch|using|lock|fixed)([ \t(]+|$)\n\
             ^[ \t]*((([][[:alnum:]@_.]+)(<[][[:alnum:]@_, \t<>]+>)?)+([ \t]+([][[:alnum:]@_.](<[][[:alnum:]@_, \t<>]+>)?)+)+[ \t]*\\([^;]*)$\n\
             ^[ \t]*(([][[:alnum:]@_.](<[][[:alnum:]@_, \t<>]+>)?)+([ \t]+([][[:alnum:]@_.](<[][[:alnum:]@_, \t<>]+>)?)+)+[^;=:,()]*)$\n\
             ^[ \t]*(((static|public|internal|private|protected|new|unsafe|sealed|abstract|partial)[ \t]+)*(class|enum|interface|struct|record)[ \t]+.*)$\n\
             ^[ \t]*(namespace[ \t]+.*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "css",
        funcname: Some(
            "![:;][[:space:]]*$\n\
             ^[:[@.#]?[_a-z0-9].*$"
        ),
        is_icase: true,
    },
    UserdiffDriver {
        name: "dts",
        funcname: Some(
            "!;\n\
             !=\n\
             ^[ \t]*((/[ \t]*\\{|&?[a-zA-Z_]).*)"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "elixir",
        funcname: Some("^[ \t]*((def(macro|module|impl|protocol|p)?|test)[ \t].*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "fortran",
        funcname: Some(
            "!^([C*]|[ \t]*!)\n\
             !^[ \t]*MODULE[ \t]+PROCEDURE[ \t]\n\
             ^[ \t]*((END[ \t]+)?(PROGRAM|MODULE|BLOCK[ \t]+DATA|([^!'\" \t]+[ \t]+)*(SUBROUTINE|FUNCTION))[ \t]+[A-Z].*)$"
        ),
        is_icase: true,
    },
    UserdiffDriver {
        name: "fountain",
        funcname: Some("^((\\.[^.]|(int|ext|est|int\\.?/ext|i/e)[. ]).*)$"),
        is_icase: true,
    },
    UserdiffDriver {
        name: "golang",
        funcname: Some(
            "^[ \t]*(func[ \t]*.*(\\{[ \t]*)?)\n\
             ^[ \t]*(type[ \t].*(struct|interface)[ \t]*(\\{[ \t]*)?)"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "html",
        funcname: Some("^[ \t]*(<[Hh][1-6]([ \t].*)?>.*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "ini",
        funcname: Some("^[ \t]*\\[[^]]+\\]"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "java",
        funcname: Some(
            "!^[ \t]*(catch|do|for|if|instanceof|new|return|switch|throw|while)\n\
             ^[ \t]*(([a-z-]+[ \t]+)*(class|enum|interface|record)[ \t]+.*)$\n\
             ^[ \t]*(([A-Za-z_<>&][][?&<>.,A-Za-z_0-9]*[ \t]+)+[A-Za-z_][A-Za-z_0-9]*[ \t]*\\([^;]*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "kotlin",
        funcname: Some("^[ \t]*(([a-z]+[ \t]+)*(fun|class|interface)[ \t]+.*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "markdown",
        funcname: Some("^ {0,3}#{1,6}[ \t].*"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "matlab",
        funcname: Some("^[[:space:]]*((classdef|function)[[:space:]].*)$|^(%%%?|##)[[:space:]].*$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "objc",
        funcname: Some(
            "!^[ \t]*(do|for|if|else|return|switch|while)\n\
             ^[ \t]*([-+][ \t]*\\([ \t]*[A-Za-z_][A-Za-z_0-9* \t]*\\)[ \t]*[A-Za-z_].*)$\n\
             ^[ \t]*(([A-Za-z_][A-Za-z_0-9]*[ \t]+)+[A-Za-z_][A-Za-z_0-9]*[ \t]*\\([^;]*)$\n\
             ^(@(implementation|interface|protocol)[ \t].*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "pascal",
        funcname: Some(
            "^(((class[ \t]+)?(procedure|function)|constructor|destructor|interface|implementation|initialization|finalization)[ \t]*.*)$\n\
             ^(.*=[ \t]*(class|record).*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "perl",
        funcname: Some(
            "^package .*\n\
             ^sub [[:alnum:]_':]+[ \t]*(\\([^)]*\\)[ \t]*)?(:[^;#]*)?(\\{[ \t]*)?(#.*)?$\n\
             ^(BEGIN|END|INIT|CHECK|UNITCHECK|AUTOLOAD|DESTROY)[ \t]*(\\{[ \t]*)?(#.*)?$\n\
             ^=head[0-9] .*"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "php",
        funcname: Some(
            "^[\t ]*(((public|protected|private|static|abstract|final)[\t ]+)*function.*)$\n\
             ^[\t ]*((((final|abstract)[\t ]+)?class|enum|interface|trait).*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "python",
        funcname: Some("^[ \t]*((class|(async[ \t]+)?def)[ \t].*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "r",
        funcname: Some("^[ \t]*([a-zA-z][a-zA-Z0-9_.]*[ \t]*(<-|=)[ \t]*function.*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "ruby",
        funcname: Some("^[ \t]*((class|module|def)[ \t].*)$"),
        is_icase: false,
    },
    UserdiffDriver {
        name: "rust",
        funcname: Some(
            "^[\t ]*((pub(\\([^\\)]+\\))?[\t ]+)?((async|const|unsafe|extern([\t ]+\"[^\"]+\"))[\t ]+)?(struct|enum|union|mod|trait|fn|impl|macro_rules!)[< \t]+[^;]*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "scheme",
        funcname: Some(
            "^(\\(.*)$\n\
             ^[\t ]*(\\(((define|def(struct|syntax|class|method|rules|record|proto|alias)?)[-*/ \t]|(library|module|struct|class)[*+ \t]).*)$\n\
             ^  ?(\\([Dd][Ee][Ff].*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "swift",
        funcname: Some(
            "^[ \t]*((@[A-Za-z_][A-Za-z0-9_]*(\\([^()]*\\))?[ \t]+)*([a-z]+[ \t]+)*(func|init|deinit|subscript|class|struct|enum|protocol|extension|actor)[ \t(?!<].*)$"
        ),
        is_icase: false,
    },
    UserdiffDriver {
        name: "tex",
        funcname: Some("^(\\\\((sub)*section|chapter|part)\\*{0,1}\\{.*)$"),
        is_icase: false,
    },
];

pub struct CompiledDriver {
    pub name: &'static str,
    patterns: Vec<(Regex, bool)>, // (regex, negate)
}

impl CompiledDriver {
    pub fn compile(driver: &UserdiffDriver) -> Self {
        let mut patterns = Vec::new();
        if let Some(funcname) = driver.funcname {
            for line in funcname.lines() {
                let (pat, negate) = if line.starts_with('!') {
                    (&line[1..], true)
                } else {
                    (line, false)
                };
                let mut builder = RegexBuilder::new(pat);
                builder.case_insensitive(driver.is_icase);
                // In C git, patterns are compared on lines with their newline removed.
                // We will match against &[u8] representing a single line (sans trailing \n/\r).
                if let Ok(re) = builder.build() {
                    patterns.push((re, negate));
                }
            }
        }
        CompiledDriver {
            name: driver.name,
            patterns,
        }
    }

    /// Matches a line and returns the matched substring if it matches a positive pattern
    /// and doesn't match a negative pattern.
    pub fn find_match<'a>(&self, line: &'a [u8]) -> Option<&'a [u8]> {
        for (re, negate) in &self.patterns {
            if let Some(m) = re.captures(line) {
                if *negate {
                    return None;
                }
                // C git tries to match the first capture group (pmatch[1]), or falls back to full match (pmatch[0]).
                let captured = if m.len() > 1 {
                    m.get(1).map(|c| c.as_bytes())
                } else {
                    m.get(0).map(|c| c.as_bytes())
                };
                if let Some(mut bytes) = captured {
                    // Strip trailing spaces, matching C git's ff_regexp:
                    // while (result > 0 && (isspace(line[result - 1]))) result--;
                    while !bytes.is_empty() && bytes[bytes.len() - 1].is_ascii_whitespace() {
                        bytes = &bytes[..bytes.len() - 1];
                    }
                    return Some(bytes);
                }
                break;
            }
        }
        None
    }
}

/// Fallback standard function header match `def_ff`:
/// Checks if the first byte of `rec` is alphabetical, `_` or `$`.
/// If so, strips trailing whitespace and returns the slice.
pub fn find_default_match(line: &[u8]) -> Option<&[u8]> {
    if line.is_empty() {
        return None;
    }
    let first = line[0];
    if first.is_ascii_alphabetic() || first == b'_' || first == b'$' {
        let mut len = line.len();
        while len > 0 && line[len - 1].is_ascii_whitespace() {
            len -= 1;
        }
        Some(&line[..len])
    } else {
        None
    }
}
