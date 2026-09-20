# Feature Specification: Configuration Compatibility

**Feature Branch**: `011-config-compatibility`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a compatibility specification for git.rs configuration. Cover: system configuration, global configuration, local repository configuration, worktree configuration, command-line configuration, environment overrides, includes, conditional includes, sections/subsections, multi-valued configuration, boolean/integer/path/enum parsing, configuration precedence, environment variables, .gitconfig compatibility, ~/.gitconfig compatibility, git config command behavior. The configuration implementation must be usable independently by all other git.rs components. Specify precedence rules and compatibility tests against standard Git."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Read one merged configuration from every layer (Priority: P1)

A user with settings spread across system, global (`~/.gitconfig`), repository-local, worktree-specific, command-line (`-c`), and environment overrides runs any command and sees the same effective value standard Git sees, with higher-precedence layers winning deterministically.

**Why this priority**: Every component (identity, refs, index, transport, status) reads configuration. Wrong precedence silently changes behavior everywhere at once; nothing else can be verified until the merged view agrees.

**Independent Test**: Can be fully tested by building layered fixtures (each layer setting the same keys to distinct values, plus `GIT_CONFIG_COUNT`/`-c` overrides) and comparing effective `git config --get` results and observed command behavior between implementations.

**Acceptance Scenarios**:

1. **Given** the same key set differently in system, global, and local files, **When** the user queries the effective value, **Then** both implementations return the local value (highest file precedence among the three).
2. **Given** a `-c` override and a `GIT_CONFIG_COUNT` override for the same key, **When** the user queries the effective value, **Then** both implementations return the `-c` value, matching standard Git's override ordering.
3. **Given** a worktree-specific setting, **When** the user runs inside that worktree vs the main worktree, **Then** both implementations apply the setting only inside that worktree.

---

### User Story 2 - Parse every value the way Git does (Priority: P1)

A user stores booleans, integers with unit suffixes, paths with `~` expansion, enumerated options, multi-valued keys, sections and subsections (including case rules), and reads them back with identical typed results and identical failure behavior for malformed values.

**Why this priority**: Misparsed values flip features on/off (`true` vs `maybe`), mis-size buffers (`1g` vs `1`), or point at wrong files. Typed parsing is the contract between the file format and every consumer.

**Independent Test**: Can be fully tested with a value-matrix fixture file (every bool spelling, integer suffix, path form, subsection case variant, multi-valued key) comparing typed query results and error verdicts between implementations.

**Acceptance Scenarios**:

1. **Given** boolean spellings (`true/false`, `yes/no`, `on/off`, `1/0`, empty-means-true), **When** the user reads them as booleans, **Then** both implementations agree on true/false/invalid for every spelling.
2. **Given** integer values with suffixes (`10k`, `2m`, `1g`) and out-of-range input, **When** the user reads them as integers, **Then** both implementations compute the same numbers and reject the same inputs.
3. **Given** a multi-valued key, **When** the user reads single vs all values, **Then** both implementations return the same last-wins single value and the same ordered full list.

---

### User Story 3 - Share includes across files conditionally (Priority: P2)

A user splits configuration into shared files with `[include]` and `[includeIf]` (branch, remote-URL, worktree, and file-existence conditions), and every command resolves the same final value set as standard Git, including cycle and missing-file handling.

**Why this priority**: Includes are how organizations distribute standard settings. Divergent include resolution means managed machines behave differently under each implementation.

**Independent Test**: Can be fully tested with include fixtures (relative/absolute/`~` paths, globs, each `includeIf` condition true and false, cycles, missing files) comparing effective values and diagnostics.

**Acceptance Scenarios**:

1. **Given** an `[include]` chain spanning three files, **When** the user queries a key from the deepest file, **Then** both implementations return the same value with the same file-origin attribution.
2. **Given** an `[includeIf "gitdir:..."]` condition matching (and not matching) the current repository, **When** the user queries keys from the conditional file, **Then** both implementations include (or exclude) them identically.
3. **Given** an include cycle, **When** configuration loads, **Then** both implementations fail with a cycle diagnostic naming the file rather than hanging or silently dropping entries.

---

### User Story 4 - Manage settings with the config command (Priority: P2)

A user runs `git config` to read (`--get`, `--get-all`, `--list`, `--get-regexp`), write (`--add`, `--replace-all`, `--unset`), and scope (`--system`, `--global`, `--local`, `--worktree`, `--file`) settings, observing identical file edits, output formats, and exit codes in both implementations.

**Why this priority**: The config command is the only supported writer. If reads and writes disagree on format or scope routing, hand-edited and command-written files diverge and round-trips break.

**Independent Test**: Can be fully tested by running read/write/scope command sequences against fixture homes and repositories with both implementations and comparing resulting file bytes, stdout, stderr, and exit codes.

**Acceptance Scenarios**:

1. **Given** a multi-valued key, **When** the user runs `--get` vs `--get-all` vs `--list`, **Then** both implementations print the same values in the same order with the same exit codes (including non-zero for missing keys where standard Git exits non-zero).
2. **Given** `--add` then `--replace-all` then `--unset` on a scoped file, **When** each step completes, **Then** both implementations leave byte-comparable file content and the same remaining values.
3. **Given** `--global` vs `--local` writes, **When** the command completes, **Then** each implementation edits exactly the file standard Git edits for that scope and no other.

---

### User Story 5 - Consume configuration from any component identically (Priority: P2)

A developer of any git.rs component (identity, refs, index, status, transport) asks for a setting and gets the same value, origin file, and scope as every other component asking for the same setting in the same context — one shared view, never per-component drift.

**Why this priority**: This is the independence requirement: configuration must be a single shared capability. Per-component re-parsing that disagrees (different precedence, different bool rules) produces bugs that only appear in one command.

**Independent Test**: Can be fully tested by querying the same layered fixture through every consumer path (command behavior plus direct reads) and asserting a single agreed value set with consistent origin attribution.

**Acceptance Scenarios**:

1. **Given** a layered fixture, **When** different commands read the same key during their flows, **Then** all of them act on the same effective value.
2. **Given** a value queried with origin information, **When** any component reports where it came from, **Then** all components name the same winning file and scope.

---

### User Story 6 - Survive hostile and broken configuration safely (Priority: P3)

A user with a corrupt, unreadable, or permission-restricted config file (or a hostile one with huge values, deep includes, non-UTF8 bytes) gets the same diagnostics and the same safe fallback as standard Git — never a crash, hang, or silently wrong value.

**Why this priority**: Configuration loads before almost every command, including in scripts running as other users. A crash-or-wrong-value here is a reliability and trust defect across the whole tool.

**Independent Test**: Can be fully tested with a corruption corpus (bad syntax, unterminated quotes/sections, huge files, deep include chains, non-UTF8 bytes, unreadable files, directories in place of files) comparing diagnostics, exit codes, and fallback behavior.

**Acceptance Scenarios**:

1. **Given** a syntactically corrupt config file, **When** any command loads it, **Then** both implementations report the corruption naming the file (and line where standard Git names it) with a non-zero exit.
2. **Given** an unreadable-due-to-permissions file, **When** a command needs it, **Then** both implementations behave like standard Git (same error vs same ignore-per-scope behavior) rather than crashing.
3. **Given** non-UTF8 bytes in a config file, **When** it loads, **Then** both implementations handle them identically — never by silently dropping the file content or substituting a different value.

---

### Edge Cases

- Empty config file; file with only comments/blank lines; missing file per scope (absent global is not an error; absent local outside a repository is).
- Section header without closing bracket; key outside any section; key with no value and no `=`; `key value` (whitespace-separated) form.
- Case rules: section and key names case-insensitive (folded), subsection names case-sensitive and preserved.
- Subsection quoting: `[remote "origin"]` vs `[remote origin]`; escaped quotes/backslashes inside subsection names; empty subsection.
- Inline comments after values (`#`/`;` only when preceded by whitespace and outside quotes); `#`/`;` inside quoted values preserved; full-line comments with leading whitespace.
- Quoted values with `\n`, `\t`, `\"`, `\\` escapes; unterminated quotes; multiline values via trailing backslash continuation; blank continuation lines.
- Multi-valued keys: interleaved with single-valued reads; `--get` returning last; `--get-all` order; unset of one vs all occurrences.
- Integer suffixes `k`/`m`/`g` (case-insensitive), negative values, overflow beyond 64-bit, trailing garbage after suffix.
- Boolean edge spellings: empty value means true; mixed case (`True`, `ON`); `maybe` and other invalid spellings rejected as type errors, not false.
- Path values: `~/`, bare `~`, `$HOME`/`${HOME}` expansion, relative-to-file vs relative-to-worktree resolution per key family, non-existent paths accepted (no existence check).
- Include paths: relative resolved against the including file's directory; `~` expansion; globs in conditional includes; `GIT_CONFIG_NOSYSTEM` skipping system include processing consistently.
- `includeIf` conditions: `gitdir:`, `gitdir/i:`, `onbranch:`, `remotename:`, `hasconfig:remote.*.url:` families; trailing-slash directory match semantics; `**` patterns; condition with no matching file.
- Environment: `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` (missing key index is fatal), `GIT_CONFIG_PARAMETERS`/`GIT_CONFIG_NOSYSTEM` where standard Git honors them, `XDG_CONFIG_HOME` vs `~/.config` fallback, `HOME` unset.
- Scopes: `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` overrides pointing at alternate files; `--file` with `-` (stdin) where supported; worktree config `config.worktree` presence vs absence; bare repositories (no worktree scope).
- Locking: concurrent `git config` writers to the same file; stale locks; read-only files; disk-full mid-write (prior content intact).
- `git config` output: `--list --show-origin` origin prefixes; `--null` separators; `--get-regexp` pattern matching and case handling; `--type=bool|int|path|expiry-date` coercion errors with matching messages and exit code `128` where standard Git uses it.

## Requirements *(mandatory)*

### Functional Requirements

Scopes and files:

- **FR-001**: The system MUST load configuration from the standard scope files: system (platform-specific path, skippable via `GIT_CONFIG_NOSYSTEM`), global (`~/.gitconfig` plus XDG `config/git/config` fallback honoring `XDG_CONFIG_HOME` and `HOME`), local (`<common-dir>/config`), and worktree (`<common-dir>/config.worktree` applied only inside its worktree), each missing file meaning "no entries from this scope", never an error by itself.
- **FR-002**: The `git config` command MUST route reads/writes to scopes exactly like standard Git: `--system`, `--global`, `--local`, `--worktree`, `--file <path>` select the file; the default write target is local (worktree-aware where standard Git is); writes MUST edit exactly the selected file and no other.
- **FR-003**: Scope files MUST be `.gitconfig`-compatible: files written by either implementation (or hand-edited per documented syntax) MUST parse identically in the other, including comments, blank lines, quoting, and entry order preservation on read-modify-write cycles.

Precedence (normative order, lowest to highest):

1. system file, 2. global files (`~/.gitconfig`, then XDG), 3. local file, 4. worktree file, 5. `GIT_CONFIG_COUNT`-family environment entries in index order, 6. `-c name=value` command-line overrides in command-line order.
   Within one file, later entries win for single-value reads; includes splice at the point of the `[include]` directive.

- **FR-004**: Effective single-value reads MUST follow the precedence order above exactly (last applicable entry wins), and MUST agree with standard Git on every layered combination, verified by the precedence matrix in Compatibility Tests.
- **FR-005**: Multi-valued reads (`--get-all`, list-all APIs) MUST return values from all applicable scopes in precedence order (lowest scope first), and single-valued reads MUST return the highest-precedence entry — never a blended or out-of-order result.
- **FR-006**: Command-line `-c name=value` (missing `=` means boolean true) and `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n` entries MUST parse names with section-first-dot/last-dot-key splitting, fold section/key case, preserve subsection case, and MUST outrank all files; a missing `GIT_CONFIG_KEY_n` MUST be fatal with a matching diagnostic, not silently skipped.

Syntax and data model:

- **FR-007**: The parser MUST support sections `[name]`, subsections `[name "sub"]` (quoted; case-sensitive, escapes honored), `key = value` and `key value` forms, full-line and trailing comments, quoted values with standard escapes, and backslash-newline continuations, with byte-level parsing behavior (comment recognition, whitespace handling, continuation splicing) identical to standard Git.
- **FR-008**: Section and key names MUST be case-folded on read, write, and lookup; subsection names MUST preserve case; fully qualified names MUST render as `section.key` and `section."subsection".key` with standard quoting on output paths (`--list`, error messages).
- **FR-009**: Malformed input (unterminated section header, unterminated quote, corrupt escaping) MUST be a load error naming the file (and line number where standard Git reports it) with a non-zero exit — never a silent skip of the offending section, and never a fallback to empty configuration.

Typed values:

- **FR-010**: Boolean reads MUST accept exactly `true/false`, `yes/no`, `on/off`, `1/0` (case-insensitive) plus empty-means-true, and MUST reject all other spellings as type errors with matching diagnostics and exit codes.
- **FR-011**: Integer reads MUST accept decimal values with `k`/`m`/`g` suffixes (case-insensitive, 1024-based), reject overflow/trailing-garbage/empty with matching diagnostics, and MUST agree on boundary values (suffix maxima, negative handling per key family).
- **FR-012**: Path reads MUST expand `~`, `~/`, and `$HOME`/`${HOME}` forms, resolve relative paths per the owning key's documented base (including-file directory vs worktree), and MUST NOT require the target to exist.
- **FR-013**: Enum reads (per-key fixed vocabularies, e.g. merge/diff/branch-policy families) MUST accept exactly the documented spellings with standard case handling and MUST reject unknown spellings with diagnostics naming the key and offending value.

Includes:

- **FR-014**: `[include] path` MUST resolve relative paths against the including file's directory, expand `~`, support multiple includes spliced in directive order, and MUST detect cycles as fatal errors naming the repeated file (no hangs, no unbounded recursion).
- **FR-015**: `[includeIf "<condition>"] path` MUST support the standard condition families (`gitdir:`, `gitdir/i:`, `onbranch:`, plus remote-URL and config-presence families where standard Git at the vendored version supports them) with identical match semantics (including trailing-slash directory matching and `**`/glob handling), evaluated against the current repository/branch/remote context.
- **FR-016**: Missing include targets MUST behave like standard Git (warn-and-continue vs fatal per include form), and MUST name the missing path in diagnostics where standard Git does.

Environment variables:

- **FR-017**: The implementation MUST honor the configuration-bearing environment (`GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n`, `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` alternate-file overrides, `HOME`/`XDG_CONFIG_HOME` for file location) with the same discovery, fallback, and error behavior as standard Git, including unset-`HOME` handling.
- **FR-018**: Files referenced through environment overrides MUST be treated with the scope semantics standard Git gives them (alternate global/system files participate in precedence at their scope's position, not as top-precedence entries).

The `git config` command:

- **FR-019**: Reads (`--get`, `--get-all`, `--get-regexp`, `--list` with `--show-origin`/`--null` variants) MUST match standard Git output bytes (order, quoting, separators), origin attribution, and exit codes — including exit `1` for "key not found" on `--get` where standard Git exits `1`.
- **FR-020**: Writes (`--add`, `--replace-all` with optional value-pattern, `--unset`/`--unset-all`, `--rename-section`, `--remove-section`) MUST produce file edits equivalent to standard Git (entry placement, comment/blank-line preservation, quoting of written values), MUST be atomic (temp file + rename; failed writes leave the prior file intact), and MUST use the same locking diagnostics on contention.
- **FR-021**: `--type=bool|int|bool-or-int|path|expiry-date|color` coercion on read and write MUST accept/reject exactly what standard Git accepts/rejects, with matching messages and exit codes (type errors exit non-zero distinctly from missing-key exits).
- **FR-022**: `--includes`/`--no-includes` MUST toggle include resolution during command operation identically, and `--default` fallback values MUST apply exactly where standard Git applies them.

Shared-component contract:

- **FR-023**: Configuration MUST be provided as a single shared capability usable independently by all other components: one load of the layered files yields one entry set that every consumer queries; per-consumer re-parsing MUST NOT diverge (same precedence, same type rules, same origin data for the same context).
- **FR-024**: Every entry MUST carry its origin (scope + file path, or command-line/environment marker), and origin queries MUST attribute the winning entry identically from every consumer.
- **FR-025**: Reads MUST be side-effect-free and safe under concurrency (no writes, no lock files, no mutation of shared state); writes happen only through the `git config` write paths with FR-020 atomicity.

Corruption, safety, compatibility:

- **FR-026**: Corrupt or unreadable configuration MUST fail closed with file-and-cause diagnostics and non-zero exits per scope rules (a corrupt file that standard Git ignores in a given context is ignored; one it loads is fatal) — never a crash, hang, truncated-value acceptance, or silent empty-config fallback where standard Git reports an error.
- **FR-027**: Non-UTF8 bytes MUST be handled exactly like standard Git (same acceptance/rejection per position), and MUST NOT cause silent content loss (such as dropping the remainder of a file).
- **FR-028**: Byte-compatibility: fixture files spanning every syntax feature MUST parse to identical entry sets in both implementations, and files written by either implementation's `git config` MUST be byte-comparable (same entries, same placement/quoting conventions) and re-parse identically in the other.

### Precedence Rules (normative)

| Priority | Source | Notes |
|----------|--------|-------|
| 1 (lowest) | System file | Skipped under `GIT_CONFIG_NOSYSTEM`; platform path |
| 2 | Global `~/.gitconfig` | `HOME`-relative; absent `HOME` handled like standard Git |
| 3 | XDG `config/git/config` | Under `XDG_CONFIG_HOME` or `~/.config` fallback |
| 4 | Local `<common-dir>/config` | Repository scope |
| 5 | Worktree `<common-dir>/config.worktree` | Only inside its worktree |
| 6 | `GIT_CONFIG_COUNT` entries | Index order `0..n`; missing key fatal |
| 7 (highest) | `-c name[=value]` | Command-line order; missing `=value` means true |

Within a file: later entries win; `[include]` splices included entries at the directive position; `[includeIf]` splices only when its condition matches. Multi-valued reads return all entries in positions 1–7 order.

### Compatibility Tests (required suites)

- **CT-001 Precedence matrix**: every non-empty subset combination of scopes 1–7 setting the same key to distinct values; assert identical effective value and identical origin attribution vs standard Git (minimum 40 combinations).
- **CT-002 Value-type matrix**: bool spellings × int suffixes/boundaries × path forms × enum vocabularies × multi-valued orderings; assert identical typed results and identical accept/reject verdicts.
- **CT-003 Syntax corpus**: sections, subsections (quoted/unquoted/escaped/empty), comments (full-line, trailing, in-quote), quotes, escapes, continuations, case folding, keys outside sections, malformed headers/quotes; assert identical entry sets or identical diagnostics.
- **CT-004 Include suite**: relative/absolute/`~` paths, multi-include ordering, globs, each `includeIf` family true and false, cycles, missing targets; assert identical effective values and diagnostics.
- **CT-005 Scope-routing suite**: reads/writes with `--system/--global/--local/--worktree/--file`, default-target selection inside and outside repositories and worktrees; assert identical target files and resulting bytes.
- **CT-006 Command-parity suite**: `--get/--get-all/--get-regexp/--list` (+`--show-origin`, `--null`, `--includes/--no-includes`, `--default`, `--type=`) and `--add/--replace-all/--unset/--unset-all/--rename-section/--remove-section`; assert identical stdout/stderr/exit codes on shared fixtures.
- **CT-007 Corruption corpus**: bad syntax, unterminated quotes/sections, huge values/files, deep include chains, non-UTF8 bytes, unreadable files, directories-as-files, concurrent writers, disk-full writes; assert identical diagnostics/exit codes and intact prior files.
- **CT-008 Crosswise round-trip**: files written by each implementation's `git config` re-parsed and re-listed by the other across CT-002–CT-004 fixtures; assert identical entry sets with zero silent drops.

### Key Entities

- **Scope**: One configuration layer (system, global, XDG, local, worktree, environment, command-line) with a defined precedence position.
- **Config file**: An INI-style file (`config`, `.gitconfig`, `config.worktree`, arbitrary `--file`) holding sections, entries, comments, and include directives.
- **Section / subsection**: `[name]` grouping and `[name "sub"]` instance grouping (e.g. per-remote/per-branch settings); section/key case-folded, subsection case-preserved.
- **Entry**: A single `key = value` record with section, optional subsection, key, value, and origin.
- **Multi-valued key**: A key with several entries whose single-read is last-wins and whose full read is ordered.
- **Include directive**: `[include] path` unconditional splice or `[includeIf "condition"] path` conditional splice of another file.
- **Origin**: The scope + file path (or command-line/environment marker) an entry came from; basis of `--show-origin` and precedence debugging.
- **Typed value**: A value interpreted as bool/int/path/enum with strict accept/reject rules.
- **Effective value**: The winning value for a key after precedence resolution across all layers.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users get the same effective setting as standard Git on 100% of the CT-001 precedence matrix (40+ layered combinations) including origin attribution.
- **SC-002**: The value-type matrix (CT-002) agrees fully: identical typed results and identical accept/reject verdicts on every bool spelling, integer boundary/suffix, path form, and enum spelling.
- **SC-003**: Include handling agrees on 100% of CT-004 fixtures (ordering, conditional true/false per family, cycles, missing targets) with matching diagnostics.
- **SC-004**: `git config` read/write suites (CT-005/CT-006) show byte-identical stdout/stderr/exit codes and byte-comparable resulting files on every case.
- **SC-005**: Zero unsafe outcomes on CT-007: every corrupt/unreadable/hostile fixture yields matching diagnostics and exit codes, failed writes always leave the prior file intact, and no case crashes or hangs.
- **SC-006**: Crosswise round-trips (CT-008) succeed with zero silent entry drops in both directions across the full syntax corpus.
- **SC-007**: The committed `t/`-suite scoreboard shows no regression on configuration-gated scripts, and 90% of operators complete everyday settings tasks (set identity, add an include, scope a value, unset a key) on first attempt with identical outcomes under either implementation.

## Assumptions

- Standard Git (C git at the vendored version; the `t/` suite wins where documentation and suite disagree) defines correct behavior, including scope paths, exit codes (`1` key-not-found, `128` type/usage-fatal class, `129` usage where applicable), and output formats.
- Platform system-config paths follow standard Git per-OS conventions; the spec matrix covers the discovery behavior, not a hardcoded path list.
- Credential/secret helper interaction (values consumed by credential flows) is out of scope; storage, parsing, precedence, and the `git config` command contract are in scope.
- Remote-URL-rewriting and conditional-include families added after the vendored version are out of scope; the families standard Git supports at the vendored version are in scope per FR-015.
- Performance targets (large-file load time, command latency) are set at planning time; parsing correctness, precedence fidelity, and write atomicity gate before optimization.
