# Feature Specification: Revision Parsing and Walking

**Feature Branch**: `009-revision-parsing-walking`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a detailed specification for git.rs revision parsing and revision walking. Git revision syntax is compatibility-critical. Cover: HEAD, branches, tags, remote refs, abbreviated object IDs, full object IDs, parent notation, ^ and ~ syntax, ranges, A..B, A...B, reflog selectors, @{...} syntax, date-based selectors, ancestry operators, object type disambiguation, pathspec/revision ambiguity, revision expressions used by log, diff, show, rev-list, checkout, merge, reset, etc. Define parsing rules, resolution order, ambiguity handling, error behavior, and tests. The implementation should be reusable across all commands requiring revision resolution."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Resolve any single revision the way C git does (Priority: P1)

A user runs any command that takes a revision (`log`, `diff`, `show`, `rev-list`, `checkout`, `reset`, and others) and spells the target the way they always have: `HEAD`, a branch name, a tag, a remote-tracking name, a full object ID, a short unique prefix, or a revision with `~` / `^` / `^{type}` / `:<path>` suffixes. The command resolves to exactly the same object C git would resolve to, or fails with the same error C git would print.

**Why this priority**: Every command in scope depends on this. If single-revision resolution diverges by even one object or one error message, all downstream commands diverge. This is the minimum viable slice: one shared resolution behavior used by all commands.

**Independent Test**: Can be fully tested by resolving a fixed matrix of revision spellings (HEAD, `@`, branch, tag, remote ref, full ID, abbreviated ID, `HEAD~2`, `HEAD^2`, `tag^0`, `rev^{commit}`, `rev^{tree}`, `rev^{tag}`, `rev^{}`, `rev^{/text}`, `:/text`, `HEAD:path`, `:0:path`, `describe` output) against a fixture repository with C git as oracle, and confirming identical resolved objects and identical stdout/stderr/exit codes.

**Acceptance Scenarios**:

1. **Given** a repository with branches, tags, and remote-tracking refs, **When** the user resolves `HEAD`, `@`, `main`, `heads/main`, `refs/heads/main`, `v1.0`, `origin/main`, or `refs/remotes/origin/main`, **Then** the result matches C git's six-rule ref disambiguation order exactly.
2. **Given** a repository with objects, **When** the user supplies a full-length hexadecimal object ID or a short unique prefix (minimum 4 hex characters), **Then** a full ID resolves when the object exists, a unique prefix resolves to its single match, and an ambiguous prefix fails with C git's "short object ID is ambiguous" diagnostic.
3. **Given** a commit with multiple parents and a tag pointing at it, **When** the user writes `A~3`, `A^2`, `A^^`, `A^0`, `tag^{commit}`, or `tag^{}`, **Then** first-parent ancestry, nth-parent selection, tag-to-commit dereference, and recursive tag peeling all match C git, including failure when a requested parent does not exist.
4. **Given** any command accepting a revision plus an optional object-type expectation (e.g. a command needing a commit vs. a tree), **When** the revision names an object of the wrong type, **Then** the behavior (dereference, peel, or error) matches C git's `^{commit}` / `^{tree}` / `^{tag}` / `^{object}` rules.

---

### User Story 2 - Express commit sets with ranges and exclusions (Priority: P2)

A user asks history-traversing commands (`log`, `rev-list`) for "what is in B but not in A" (`A..B`, `^A B`), "what changed on either side" (`A...B`), or parent-set shorthands (`^@`, `^!`, `^-`), and gets exactly the commit set C git would list, in an order C git would produce for the requested ordering flags.

**Why this priority**: Ranges are the second most-used revision surface and the primary input to the revision walker. They build directly on Story 1 (each side of a range is a single revision) and feed every log/history view.

**Independent Test**: Can be fully tested by running range expressions (`A..B`, `A...B`, `^A B`, `A^@`, `A^!`, `A^-`, omitted-side defaults like `..B` / `A..`, `--not`, `--all/--branches/--tags/--remotes/--glob`) over linear, branched, and octopus-merge fixture histories and comparing the resulting commit sets and orderings byte-for-byte against C git.

**Acceptance Scenarios**:

1. **Given** two commits A and B, **When** the user requests `A..B`, `^A B`, `..B` (= `HEAD..B`), or `A..` (= `A..HEAD`), **Then** the result is commits reachable from B excluding anything reachable from A, with an empty side defaulting to `HEAD` and `..` alone yielding the empty set.
2. **Given** two commits A and B, **When** the user requests `A...B`, **Then** the result is the symmetric difference (reachable from either but not both, i.e. both sides minus all merge bases), with an empty side defaulting to `HEAD`.
3. **Given** a merge commit M, **When** the user writes `M^@` (all parents), `M^!` (M alone), or `M^-` / `M^-2` (M minus its nth parent), **Then** the expansions match C git (`M^@` = all parents listed, `M^!` = M plus `^` on every parent, `M^-n` = `M^n..M`).

---

### User Story 3 - Reach history through reflog and time selectors (Priority: P3)

A user recovers previous positions with reflog selectors (`@{1}`, `main@{2}`, `@{u}`, `@{push}`, `@{-1}`) or asks "what did this ref point at yesterday" (`main@{yesterday}`, `HEAD@{5 minutes ago}`), and gets the same answer C git gives, including the same failure when no reflog exists.

**Why this priority**: Reflog/time selectors are recovery-critical ("where was my branch before the reset") but strictly layered on top of Stories 1-2: they resolve to a single revision first, then participate in walks and ranges like any other revision.

**Independent Test**: Can be fully tested by scripting ref movements (commits, resets, checkouts) to build known reflogs, then resolving ordinal (`@{n}`), `@{upstream}` / `@{push}` (including triangular workflows), `@{yesterday}`-style date lookups, and `@{-n}` checkout-history lookups, comparing each against C git including error cases (missing reflog, out-of-range ordinal, unresolvable upstream).

**Acceptance Scenarios**:

1. **Given** a ref with a recorded reflog, **When** the user resolves `<ref>@{n}` or `@{n}` (current branch), **Then** the nth prior value is returned exactly as C git returns it, and an out-of-range `n` or a ref without a reflog fails exactly as C git fails.
2. **Given** a branch with upstream/push configuration (including triangular pull-vs-push setups), **When** the user resolves `<branch>@{upstream}` / `@{u}` or `<branch>@{push}`, **Then** the corresponding remote-tracking ref is returned, honoring case-insensitive spelling and failing when no upstream/push destination is configured.
3. **Given** a ref with a reflog containing timestamps, **When** the user resolves `<ref>@{<date>}` with any date form the system's date parser accepts (yesterday, relative, ISO, RFC 2822, epoch), **Then** the ref's value at that prior time is returned exactly as C git computes it.

---

### User Story 4 - Never confuse a revision with a file path (Priority: P2)

A user runs a command that accepts both revisions and file paths (`log`, `diff`, `checkout`, `reset`, `show`) in a repository where a branch, tag, or abbreviation collides with a filename, and the `--` separator plus C git's ambiguity diagnostics keep the outcome predictable and identical to C git.

**Why this priority**: Ambiguity mishandling silently operates on the wrong thing (wrong commit, or a path treated as a commit). This story makes the failure mode safe and C-identical across every command in scope.

**Independent Test**: Can be fully tested with fixture repositories containing deliberate collisions (branch named like a file, tag matching a directory, short ID that is also a valid path) by invoking commands with and without `--` and comparing stdout/stderr/exit codes against C git.

**Acceptance Scenarios**:

1. **Given** an argument that could be both a revision and a path, **When** the user runs a command without `--`, **Then** revision interpretation wins exactly where C git prefers revisions, and the ambiguity diagnostic (prompting `--` separation) appears exactly where C git prints it.
2. **Given** arguments after a `--` separator, **When** the user runs a history or diff command, **Then** everything after `--` is treated strictly as paths (never as revisions), matching C git.

---

### Edge Cases

- What happens when a short object ID prefix matches zero, one, or several objects (including across loose vs. packed storage)?
- How does the system handle a `~<n>` / `^<n>` peel applied to a non-commit (blob/tree), to a root commit with no parents, or with `n = 0`?
- What happens when `^{<type>}` dereference cannot reach the requested type (e.g. `blob^{commit}`), and how does `^{object}` (existence check) vs. `^{tag}` (must be a tag) vs. `^{}` (peel tags only) differ?
- How are `:/text` (message search from any ref) vs. `<rev>^{/text}` (youngest match reachable from `<rev>`) scoped, including `:/!` negative match and `:/!!` literal-`!` forms?
- What happens when a range side names a non-commit (tag to blob, bare tree, blob path) — is it peeled, rejected, or walked?
- How does `A...B` behave when there are zero, one, or several (criss-cross) merge bases?
- What happens when `@{n}` is out of range, the ref has no reflog, or the `@{date}` predates all reflog entries?
- How are `@{-<n>}` (nth previously checked-out branch) and bare `@` (shortcut for `HEAD`) resolved when there is no checkout history or no `HEAD`?
- What happens when `@{upstream}` / `@{push}` has no configured tracking branch, or the push destination is a URL shared by several remotes?
- How does `<rev>:<path>` treat `./` and `../` prefixes (relative to working directory, normalized to repository root), empty paths, `:<n>:<path>` stage selectors outside a merge, and non-UTF-8 path bytes?
- How do multiple ranges compose (`A..B C..D` is one connected set, not two ranges — e.g. linear `---A---B---o---o---C---D` yields only `D`)?
- What happens when revision-looking text appears after `--` (must be a path, never a revision)?
- How do usage errors (exit 129) vs. fatal revision errors (exit 128) vs. "unknown command" (exit 1) map for each command in scope?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST resolve `HEAD` and `@` (bare `@` = `HEAD`) to the current commit, including detached-`HEAD` and unborn-`HEAD` behavior identical to C git.
- **FR-002**: The system MUST resolve branch names, tag names, and remote-tracking names using C git's six-rule disambiguation order: (1) `$GIT_DIR/<refname>` (HEAD, FETCH_HEAD, ORIG_HEAD, MERGE_HEAD, REBASE_HEAD, REVERT_HEAD, CHERRY_PICK_HEAD, BISECT_HEAD, AUTO_MERGE), then (2) `refs/<refname>`, (3) `refs/tags/<refname>`, (4) `refs/heads/<refname>`, (5) `refs/remotes/<refname>`, (6) `refs/remotes/<refname>/HEAD`.
- **FR-003**: The system MUST resolve full-length hexadecimal object IDs (40 hex chars for SHA-1; hash-length parameterized for other algorithms) when the object exists.
- **FR-004**: The system MUST resolve abbreviated object IDs of at least 4 hex characters when the prefix is unique within the repository (searching loose and packed objects), fail when it matches nothing, and fail with C git's "short object ID is ambiguous" diagnostic when it matches more than one object.
- **FR-005**: The system MUST support first-parent ancestry `~[<n>]` (default 1; `<rev>~3` = three first-parent steps) and nth-parent `^[<n>]` (default 1; `<rev>^` = first parent, `<rev>^2` = second parent of a merge), failing when the requested parent does not exist or the object is not a commit.
- **FR-006**: The system MUST support `<rev>^0` (the commit itself, used for tag-to-commit handling) with C-identical semantics.
- **FR-007**: The system MUST support object-type dereference `^{<type>}` for types `commit`, `tree`, `tag`, `object` (recursive dereference until the type is reached; `^{object}` only asserts existence; `^{tag}` requires a tag object), and `^{}` (recursively peel tags until a non-tag).
- **FR-008**: The system MUST support commit-message search `:/<text>` (youngest match reachable from any ref, regex over the message, `:/!` negative, `:/!!` literal `!`, `:/^foo` anchored) and `<rev>^{/<text>}` (youngest match reachable from `<rev>`).
- **FR-009**: The system MUST resolve `<rev>:<path>` to the blob/tree at that path in the tree of `<rev>` (commits dereferenced to trees), with `./` and `../` interpreted relative to the working directory and normalized to the repository root.
- **FR-010**: The system MUST resolve `:[<n>:]<path>` index-stage syntax (missing stage = stage 0; stages 1/2/3 = base/ours/theirs during a merge).
- **FR-011**: The system MUST resolve output of `describe` (`<tag>-<n>-g<abbrev>`) to the described commit.
- **FR-012**: The system MUST evaluate exclusion `^<rev>` and the `--not` flag as "exclude this commit and its ancestors" in history-traversing commands.
- **FR-013**: The system MUST evaluate two-dot ranges `A..B` as `^A B` (reachable from B excluding reachable from A), defaulting an omitted side to `HEAD` (`A..` = `A..HEAD`, `..B` = `HEAD..B`, bare `..` = empty set).
- **FR-014**: The system MUST evaluate three-dot ranges `A...B` as the symmetric difference (reachable from either but not both, i.e. both sides minus all merge bases), defaulting an omitted side to `HEAD`.
- **FR-015**: The system MUST expand `r^@` (all parents of r), `r^!` (r alone: r plus `^` on every parent), and `r^- [<n>]` (= `r^<n>..r`, default n=1) exactly as C git does.
- **FR-016**: The system MUST compose multiple range/exclusion arguments into a single connected commit set (e.g. `A..B C..D` = reachable from B or D but from neither A nor C), never as independent ranges except in commands explicitly documented to take two ranges.
- **FR-017**: The system MUST resolve reflog ordinals `<ref>@{<n>}` and bare `@{<n>}` (current branch) to the nth prior ref value, requiring an existing reflog and failing C-identically otherwise.
- **FR-018**: The system MUST resolve date-based selectors `[<ref>]@{<date>}` to the ref's value at that prior time using the same date forms as the system's date parser (relative, ISO, RFC 2822, epoch), failing C-identically when the ref has no reflog.
- **FR-019**: The system MUST resolve `[<branch>]@{upstream}` / `@{u}` and `[<branch>]@{push}` (case-insensitive spelling) via branch tracking configuration, including triangular workflows where pull and push destinations differ, failing C-identically when unconfigured.
- **FR-020**: The system MUST resolve `@{-<n>}` to the nth previously checked-out branch/commit.
- **FR-021**: The system MUST implement `--` separation: arguments after `--` are always pathspec, never revisions; without `--`, an argument that is neither a known revision nor a valid path fails with C git's "ambiguous argument ... unknown revision or path not in the working tree ... Use '--' to separate paths from revisions" diagnostic.
- **FR-022**: The system MUST reproduce C git's exit-code contract: usage errors exit 129, fatal revision errors exit 128 with byte-identical stderr text (including the ambiguous-argument guidance and the short-ID ambiguity prefix line), and unknown subcommands exit 1.
- **FR-023**: The system MUST expose one shared resolution and range-expansion behavior reused by all commands in scope (`log`, `diff`, `show`, `rev-list`, `checkout`, `switch`, `restore`, `merge`, `reset`, `rev-parse`, `merge-base`, `diff-tree`, `commit-tree`, `read-tree`, `cat-file`, and any future revision-accepting command) so no command implements its own divergent dialect.
- **FR-024**: The system MUST implement revision walking (reachability from tips minus exclusions) with C-identical ordering modes (default commit-date order, `--topo-order`, `--date-order`, `--reverse`, `--first-parent`, `--ancestry-path`), limits (`--max-count`, `--skip`, `-n`), parent filters (`--merges`, `--no-merges`, `--min-parents`, `--max-parents`), content filters (`--author`, `--committer`, `--grep` with `--invert-grep` / `-i`), ref-selection flags (`--all`, `--branches`, `--tags`, `--remotes`, `--glob`), path limiting (`-- <path>`), and `--no-walk` / `--objects` / `--count` / `--parents` output modes.
- **FR-025**: The system MUST verify every behavior in FR-001–FR-024 against C git byte-for-byte (resolved object, commit set and order, stdout, stderr, exit code) on fixture repositories covering linear history, merges (including octopus), tags (lightweight and annotated), remote-tracking refs, packed vs. loose objects, and reflog-bearing refs, plus property tests over generated histories.

### Key Entities

- **Revision Expression**: A user-supplied spelling of an object or commit set (single spelling, peel/dereference suffix, message search, path suffix, reflog selector, or range composition). Has a textual form and a resolution result or a C-identical error.
- **Resolved Object**: The outcome of single-revision resolution — an object identifier plus its type (commit/tag/tree/blob) — before any walk or range expansion. Type mismatches trigger dereference or error per the `^{type}` rules.
- **Revision Range**: A commit set defined by include tips minus exclusions (`A..B`, `A...B`, `^r`, `--not`, `^@`/`^!`/`^-` expansions). Composes into one connected set with merge-base subtraction where specified.
- **Reflog Selector**: An ordinal, date, upstream/push, or checkout-history suffix (`@{n}`, `@{date}`, `@{u}`, `@{push}`, `@{-n}`) that maps a ref plus history to one prior value. Requires backing ref history; otherwise it is a resolution error.
- **Walk Specification**: The combination of a Revision Range with ordering, limit, filter, ref-selection, and path-limiting options that defines which commits a history command emits and in what order.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users resolving any single-revision spelling from the matrix (HEAD, `@`, branch, tag, remote ref, full ID, unique short ID, `~`/`^` peels, `^{type}`/`^{}`/`^{/text}`, `:/text`, `<rev>:<path>`, `:<stage>:<path>`, describe output) get the same object as C git in 100% of fixture cases.
- **SC-002**: Users requesting two-dot, three-dot, exclusion, and parent-set ranges get the same commit set as C git in 100% of fixture cases, including omitted-side defaults and multi-range composition.
- **SC-003**: Users recovering positions via reflog ordinals, dates, upstream/push, and `@{-n}` get the same answer as C git in 100% of fixture cases, including identical failures when history is missing.
- **SC-004**: Users hitting ambiguous, unknown, or type-mismatched revisions see stderr text and exit codes indistinguishable from C git in 100% of fixture cases (verified by blind comparison of outputs).
- **SC-005**: Users get consistent results across commands: the same revision spelling resolves identically whether passed to log, diff, show, rev-list, checkout, reset, or merge (zero per-command divergences in the fixture matrix).
- **SC-006**: 95% of users complete a revision task (e.g. "show what changed since the fork", "restore a pre-reset branch position") on the first attempt without consulting compatibility notes, measured by acceptance-scenario walkthroughs.

## Assumptions

- The C git revision documentation (`SPECIFYING REVISIONS` / `SPECIFYING RANGES`) is the behavior oracle; where prose and the test suite disagree, the test suite wins.
- Object-ID lengths are parameterized by hash algorithm (40 hex for SHA-1); abbreviated IDs require a minimum of 4 hex characters and repository-wide uniqueness.
- Reflog lookups reflect local ref history only, not commit times; time-based commit filtering (`--since`/`--until`) is out of scope for this feature.
- Date forms inside `@{<date>}` reuse the system's existing date parser (relative, ISO-8601, RFC 2822, epoch forms with explicit timezones).
- Upstream/push resolution reads branch tracking configuration (`branch.<name>.remote`, `branch.<name>.merge`, push defaults); remote-URL-only push destinations follow C git's single-remote-matching rule.
- Shell quoting/word-splitting concerns are the caller's responsibility; the specification covers the raw revision syntax as seen by git.
- Out of scope: `--since`/`--until` commit-time filters beyond `@{date}` lookup, shallow/graft/replace interactions except as walk exclusions already support, and server-side (fetch/push negotiation) revision expansion.
