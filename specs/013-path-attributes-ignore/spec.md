# Feature Specification: Path Attributes and Ignore Handling

**Feature Branch**: `013-path-attributes-ignore`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the specification for git.rs path attributes and ignore handling. Cover: .gitignore, .git/info/exclude, global excludes, .gitattributes, pattern matching, negation, directory patterns, recursive patterns, pathspec behavior, attributes, text/binary classification, diff attributes, merge attributes, filter attributes, export attributes. Define a reusable matching engine and its integration with status, add, diff, checkout, merge, archive, and other commands."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Ignored files stay out of the way (Priority: P1)

A user keeps build outputs, editor backups, and machine-local files untracked by listing patterns in a `.gitignore` file, the repository-local `.git/info/exclude` file, or a global excludes file. Untracked-but-ignored paths disappear from `status`, are refused by `add` unless forced, and are reported by `check-ignore` exactly as C git reports them.

**Why this priority**: Ignore handling guards every worktree-facing command. Wrong ignore decisions either leak generated files into commits or silently drop user files. This is the minimum viable slice: one shared ignore decision reused by all commands.

**Independent Test**: Can be fully tested by building fixture worktrees with patterns in each source (nested `.gitignore` files, `info/exclude`, global excludes) plus collisions (negations, directory patterns, `**`), then comparing `status`, `add`, `ls-files`, `clean`, and `check-ignore` stdout/stderr/exit codes against C git.

**Acceptance Scenarios**:

1. **Given** patterns spread across `.gitignore`, `.git/info/exclude`, and the global excludes file, **When** the user runs `status` or `ls-files --others --exclude-standard`, **Then** ignored untracked paths are hidden and tracked paths are unaffected, identically to C git.
2. **Given** an ignored path, **When** the user runs `add <path>` without force, **Then** the add is refused with C git's diagnostic; with `-f` the path stages normally.
3. **Given** any path, **When** the user runs `check-ignore [-v] [-q] [--non-matching] [--stdin] [-z] <path>...`, **Then** output (including verbose `source:line:pattern` records), exit codes (0 match, 1 no match), and quiet mode match C git byte-for-byte.

---

### User Story 2 - Attributes classify every path the same way (Priority: P2)

A user marks paths with `text`, `binary`, `diff`, `merge`, `eol`, and custom attributes in `.gitattributes`, `.git/info/attributes`, or global/system attribute files. Every command that asks "what attributes does this path have" (`check-attr`, and internally `diff`, `merge`, `checkout`, `add`) gets C-identical answers, including macro expansion and per-attribute last-match-wins.

**Why this priority**: Attribute lookup is the single choke point behind text/binary decisions, diff drivers, merge drivers, and line-ending conversion. One shared lookup keeps all consumers consistent.

**Independent Test**: Can be fully tested with fixture repositories whose attribute files conflict across levels (worktree vs. index fallback, info vs. per-directory vs. global vs. system, macros, `!`-reset) by comparing `check-attr [-a] [--cached] [--source] [--stdin] [-z]` output against C git for every path.

**Acceptance Scenarios**:

1. **Given** conflicting attribute assignments across `info/attributes`, nested `.gitattributes`, global, and system files, **When** the user checks any path, **Then** the per-attribute winner follows C precedence (info highest, then nearer directory wins, then global, then system lowest; later line wins within one file).
2. **Given** a macro definition (`[attr]name ...`) and uses of the macro name, **When** attributes resolve, **Then** setting the macro applies its expansion, and `!attr` resets an attribute to Unspecified, matching C git.
3. **Given** a path matching a directory pattern, **When** attributes resolve for files inside that directory, **Then** the directory match does not leak onto its contents (unlike ignore), unless a `path/**` form is used.

---

### User Story 3 - Pathspecs select the right files in every command (Priority: P2)

A user limits `status`, `add`, `diff`, `checkout`, `reset`, `log`, `grep`, and `archive` to a subset of paths using literal paths, globs, directory prefixes, `:(...)` magic (`top`, `literal`, `icase`, `glob`, `exclude`, `attr`), and `--` separation, and gets the same file set C git would select.

**Why this priority**: Pathspecs are the second half of "which files does this command touch". Mis-scoped pathspecs cause commands to stage, restore, or show the wrong files.

**Independent Test**: Can be fully tested with fixture trees (nested dirs, tricky names with spaces/globs/case variants, attribute-tagged files) by running each command with literal, glob, magic, exclusion, and `--`-separated pathspecs and comparing selected file sets and outputs against C git.

**Acceptance Scenarios**:

1. **Given** a pathspec after `--` or in a path-accepting position, **When** the user runs any command in scope, **Then** the matched file set (including directory-prefix recursion and `:(exclude)` / `:!` subtraction) matches C git.
2. **Given** magic selectors such as `:(top)`, `:(literal)`, `:(icase)`, `:(glob)`, and `:(attr:...)`, **When** the user scopes a command from a subdirectory, **Then** anchoring, case folding, and attribute filtering behave as C git documents.
3. **Given** an argument that could be a revision or a path, **When** the user omits `--`, **Then** C git's disambiguation and "use `--` to separate" guidance apply unchanged.

---

### User Story 4 - Conversions and archive honors attributes end to end (Priority: P3)

A user relies on `text`/`eol` conversion on `add`/`checkout`, `filter` (clean/smudge) and `ident` expansion, `diff`/`merge` driver selection (including `-diff`/`-merge` binary treatment and `text=auto` detection), and `export-ignore`/`export-subst` on `archive`, and observes C-identical file bytes and archive contents.

**Why this priority**: These are the observable effects of attributes. They layer strictly on top of Stories 2–3 (lookup first, then act), and each maps to a concrete user-visible behavior.

**Independent Test**: Can be fully tested with CRLF/mixed fixtures, custom clean/smudge scripts, `$Id$` fixtures, binary fixtures with `-diff`/`-merge`, and archives, comparing worktree bytes, index bytes, diff output, merge outcomes, and archive listings against C git.

**Acceptance Scenarios**:

1. **Given** `text`, `text=auto`, `-text`, and `eol=crlf|lf` assignments plus `core.autocrlf`/`core.eol` settings, **When** the user adds then checks out files, **Then** index normalization (LF) and worktree conversion match C git, and binary detection (`text=auto` on NUL-containing files) suppresses conversion.
2. **Given** `filter=<driver>` with configured clean/smudge commands and `ident` assignments, **When** the user checks files in and out, **Then** smudge runs on checkout and clean runs on add, with missing-driver behavior identical to C git.
3. **Given** `diff=<driver>` / `-diff`, `merge=<driver>` / `-merge`, and `textconv` settings, **When** the user diffs or merges, **Then** binary files show C's "Binary files differ" treatment, custom drivers are invoked with C's arguments and environment, and fallbacks match C git.
4. **Given** `export-ignore` and `export-subst` assignments, **When** the user creates an archive, **Then** marked paths are omitted and placeholders (`$Format:...$`) expand exactly as C git expands them.

---

### Edge Cases

- What happens when a negation (`!keep`) sits under an excluded parent directory (re-inclusion is impossible; excluded directories are not listed)?
- How are blank lines, `#` comments, escaped `\#` / `\!`, trailing unescaped spaces (ignored) vs. backslash-quoted trailing spaces, and a trailing lone backslash handled?
- How do `*` / `?` / `[...]` behave with `/` (never match a slash), and how do the three `**` forms (leading `**/`, trailing `/**`, middle `/​**/`) differ from plain `*`?
- What happens when a pattern has a leading `/`, a middle `/`, a trailing `/`, or no slash at all (anchored vs. floating vs. directory-only)?
- How do patterns from `info/exclude` and global excludes anchor (as if at repository root) versus per-directory `.gitignore` anchoring?
- What happens to already-tracked files matching new ignore patterns (they stay tracked)?
- How does `core.ignorecase` affect matching, and how are non-UTF-8 path bytes matched?
- What happens when `.gitattributes` is missing from the worktree but present in the index (index fallback), and during checkout (index first, worktree fallback)?
- How do attribute value forms differ: `attr` (Set), `-attr` (Unset), `attr=value` (Value), `!attr` (Unspecified reset)?
- What happens when a macro name collides with a real attribute or the reserved `builtin_*` namespace?
- How do `text=auto` binary detection, `-text`/`-diff`/`-merge` binary treatment, and `eol` interact with `core.autocrlf`?
- What happens with `filter` when the configured driver is missing, and with `ident` on binary files?
- How do `export-ignore` on a directory vs. a file, and `export-subst` outside archives (no expansion), behave?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST read ignore patterns from all four sources: command-line options (where the command supports them), per-directory `.gitignore` files, `$GIT_DIR/info/exclude` (common dir), and `core.excludesFile` (default `$XDG_CONFIG_HOME/git/ignore`, else `$HOME/.config/git/ignore`).
- **FR-002**: The system MUST apply ignore precedence highest-to-lowest: command line, then `.gitignore` in the path's own directory down from deeper to shallower parents, then `info/exclude`, then `core.excludesFile`; within one precedence level the last matching pattern decides.
- **FR-003**: The system MUST implement negation: a `!`-prefixed pattern re-includes files excluded by earlier patterns, except re-inclusion under an excluded parent directory MUST have no effect (excluded directories are not descended into).
- **FR-004**: The system MUST implement anchoring: patterns with a leading or middle `/` anchor at the `.gitignore` file's directory (leading `/` redundant with a middle `/`); slash-free patterns float and match the basename at any level below; `info/exclude` and global patterns anchor at the repository root.
- **FR-005**: The system MUST implement directory-only patterns: a trailing `/` matches directories (and everything under them) but never plain files or symlinks.
- **FR-006**: The system MUST implement wildcards where `*` and `?` never match `/`, `[...]` ranges match one non-slash character, backslash escapes any character, and the three `**` forms work (leading `**/` = all directories, trailing `/**` = everything inside, `/​**/` = zero or more directories); other `**` runs act as plain `*`.
- **FR-007**: The system MUST treat blank lines as no-ops, `#` as comments (with `\#` escape), ignore unquoted trailing spaces while honoring backslash-quoted ones, and support `\!` for literal leading `!`.
- **FR-008**: The system MUST leave already-tracked files unaffected by ignore patterns (ignore applies to untracked paths only), and MUST NOT follow symlinks when locating worktree `.gitignore` files.
- **FR-009**: The system MUST read attribute assignments from `$GIT_DIR/info/attributes` (highest precedence), same-directory `.gitattributes` down through parents to the worktree root (nearer wins), then the `core.attributesFile` file (default XDG location), then the system file (lowest); a later line wins per attribute within one file.
- **FR-010**: The system MUST support the four attribute states: Set (`name`), Unset (`-name`), Value (`name=value`), Unspecified (no match, or `!name` reset), with per-attribute (not per-line) overriding.
- **FR-011**: The system MUST forbid negative patterns in attribute files, MUST NOT let directory matches recurse into contents (trailing-slash `path/` form is inert; `path/**` required), and MUST use index content as fallback when the worktree `.gitattributes` is missing (index first, worktree fallback during checkout).
- **FR-012**: The system MUST expand `[attr]` macros (including built-in `binary` = `-diff -merge -text`) when the macro name is set, and MUST ignore user definitions under the reserved `builtin_*` namespace with a C-identical warning.
- **FR-013**: The system MUST classify text/binary from `text` (`Set` = convert, `Unset` = never, `text=auto` = heuristic with binary detection, Unspecified = `core.autocrlf` decides) combined with `eol` (`crlf`/`lf`) and config, normalizing to LF in the index and converting on checkout exactly as C git does.
- **FR-014**: The system MUST apply `filter=<driver>` clean (check-in) and smudge (checkout) programs with C's invocation, environment, and missing-driver behavior, plus `ident` `$Id$` expansion for marked text files.
- **FR-015**: The system MUST honor `diff` (`-diff` = binary treatment with "Binary files differ"; `diff=<driver>` = named driver with `command`/`textconv`/`funcname`/`wordRegex` behavior) and `merge` (`-merge` = no textual merge; `merge=<driver>` = named driver; `Set` = built-in text merge) in diff display and merge execution.
- **FR-016**: The system MUST honor `export-ignore` (omit path from archives, directory form omits subtrees) and `export-subst` (expand `$Format:...$` placeholders only in archives) during archive creation.
- **FR-017**: The system MUST implement pathspec matching (literal, glob, directory prefix, `:(top)`, `:(literal)`, `:(icase)`, `:(glob)`, `:(exclude)` / `:!` subtraction, `:(attr:...)`) with `--` separation, subdirectory anchoring, and `core.ignorecase` handling identical to C git for every command in scope.
- **FR-018**: The system MUST integrate one shared ignore decision into `status` (hide/​`--ignored`), `add` (refuse unless `-f`, `--dry-run` reporting), `ls-files` (`--others`/`--ignored`/`--exclude-standard`), `clean` (skip ignored unless `-x`, keep unless `-X`), `grep`, and worktree traversal; and one shared attribute lookup into `check-attr`, `diff`, `merge`, `checkout`/`switch`, `add`/`commit`, and `archive`.
- **FR-019**: The system MUST reproduce `check-ignore` (exit 0/1, `-v` source:line:pattern records, `-q`, `--non-matching`, `--stdin`, `-z`, `--no-index`) and `check-attr` (`-a`/`--all`, `--cached`, `--source`, `--stdin`, `-z`, `unspecified` rendering) byte-for-byte with C git.
- **FR-020**: The system MUST verify every behavior in FR-001–FR-019 against C git byte-for-byte (decision, output text, exit code, and resulting file/index bytes) on fixtures covering nesting, negation, `**`, non-UTF-8 names, case collisions, CRLF/mixed content, binary content, custom drivers, and archives, plus property tests over generated pattern/path matrices.

### Key Entities

- **Ignore Rule**: One parsed pattern line plus its source (file, line number, base directory) and flags (negation, directory-only, anchored/floating). Decides, with precedence, whether an untracked path is ignored.
- **Attribute Assignment**: A pattern plus per-attribute states (Set/Unset/Value/Unspecified) for one path, resolved through macro expansion and cross-file precedence into the effective attribute set.
- **Attribute Macro**: A named reusable assignment bundle (`[attr]name ...`, including built-in `binary`) applied wherever the macro name is set on a path.
- **Pathspec**: A user-supplied path selector (literal, glob, magic, exclusion) plus its anchor (cwd vs. top) that defines the file set a command operates on.
- **Conversion Decision**: The effective text/binary/filter/ident outcome for a path (from attributes plus config) that determines index normalization, checkout conversion, and clean/smudge execution.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users see the same ignored-vs-visible untracked sets as C git in 100% of fixture cases across all four pattern sources, nesting, negation, and `**` forms.
- **SC-002**: Users get the same attribute answers as C git (`check-attr`, all flags) in 100% of fixture cases, including cross-file conflicts, macros, and `!`-resets.
- **SC-003**: Users scoping commands with literal, glob, magic, exclusion, and `--`-separated pathspecs get the same file sets as C git in 100% of fixture cases.
- **SC-004**: Users adding then checking out text/CRLF/binary fixtures observe byte-identical index and worktree content as C git in 100% of cases, including `text=auto` detection and missing-driver behavior.
- **SC-005**: Users creating archives from `export-ignore`/`export-subst` fixtures receive archives with the same member lists and expanded bytes as C git in 100% of cases.
- **SC-006**: 95% of users complete ignore/attribute tasks (silence build outputs, force-add an exception, scope a command to a subtree) on the first attempt without consulting compatibility notes, measured by acceptance-scenario walkthroughs.

## Assumptions

- The C git `gitignore` and `gitattributes` documentation is the behavior oracle; where prose and the test suite disagree, the test suite wins.
- Ignore applies to untracked paths only; tracked files are never ignored regardless of patterns.
- Conventions for defaults: global excludes default to `$XDG_CONFIG_HOME/git/ignore` (else `$HOME/.config/git/ignore`); global attributes default to the `core.attributesFile` XDG path; system attributes to the install prefix location.
- Wildcard matching follows fnmatch-with-pathname semantics (`*`/`?`/`[...]` never cross `/`); character-class details defer to the C test suite on ties.
- Custom diff/merge/filter drivers run as external programs with C's arguments, environment, and failure handling; driver configuration itself is out of scope.
- Shell quoting/word-splitting is the caller's responsibility; the specification covers raw patterns and pathspecs as seen by git.
- Out of scope: sparse-checkout cone interactions beyond pathspec parity, clean/smudge filter protocol extensions (long-running filters), `working-tree-encoding` conversions, and server-side attribute negotiation.
