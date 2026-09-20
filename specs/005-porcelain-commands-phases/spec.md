# Feature Specification: Porcelain Commands Phases

**Feature Branch**: `005-porcelain-commands-phases`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a phased specification for implementing Git's porcelain commands in git.rs. Classify commands into: (1) foundational, (2) repository inspection, (3) staging/worktree, (4) history, (5) branch/reference, (6) merge/rebase, (7) remote/transport, (8) patch/diff, (9) maintenance, (10) advanced/plumbing commands. For each command define: command name, subcommands, important options, arguments, expected stdout, expected stderr, exit codes, configuration interaction, environment variables, filesystem effects, repository effects, compatibility requirements, minimum viable implementation, edge cases, test strategy. Prioritize behavioral compatibility over simply providing a similar API. Do not assume every Git command must initially be implemented. Define explicit phases and compatibility levels."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Complete a basic edit-stage-commit-inspect loop (Priority: P1)

A user initializes a repository, stages files, commits, and inspects status, differences, and history. Every command in this loop behaves like standard Git in output, errors, and exit codes.

**Why this priority**: This is the minimum credible Git. It exercises foundational, staging, inspection, and history commands end to end and unblocks all later phases.

**Independent Test**: Can be fully tested by running scripted init/add/commit/status/diff/log/show sequences under both implementations on identical fixtures and comparing stdout, stderr, exit codes, index bytes, and work-tree state.

**Acceptance Scenarios**:

1. **Given** an empty directory, **When** the user initializes, stages files, and commits, **Then** the resulting HEAD tree, index, and log output match standard Git byte-for-byte for the same inputs.
2. **Given** modified, deleted, and untracked files, **When** the user checks status and diff, **Then** classifications, paths, and exit codes (including diff `--quiet`/`--exit-code` semantics) match standard Git.
3. **Given** an existing history, **When** the user views log and shows objects, **Then** ordering, formatting defaults, and revision-argument errors match standard Git.

---

### User Story 2 - Branch, switch, and reconcile diverged work (Priority: P2)

A user creates branches and tags, switches between them, and merges or rebases diverged lines, resolving conflicts through index stages with identical outcomes under either implementation.

**Why this priority**: Branching and merging are the collaboration core. They build directly on the basic loop and gate remote workflows.

**Independent Test**: Can be fully tested with branch/tag fixtures (create, list, verify, delete, switch, detach) and merge/rebase fixtures (fast-forward, clean merge, conflicting merge, abort/continue) comparing refs, index stages, work trees, and diagnostics.

**Acceptance Scenarios**:

1. **Given** branches and tags, **When** the user lists, shows, creates, or deletes them, **Then** ref updates, listing output, and error cases (invalid names, non-existent refs, unborn HEAD) match standard Git.
2. **Given** a feature branch, **When** the user switches to it, **Then** work-tree files, modes, index entries, and HEAD state are identical regardless of which implementation performed the switch.
3. **Given** diverged branches, **When** the user merges or rebases, **Then** clean cases produce identical trees and conflicting cases record identical stage-1/2/3 entries, refuse commits while unmerged with equivalent diagnostics, and converge after resolution.

---

### User Story 3 - Exchange history with other repositories (Priority: P2)

A user clones, fetches from, and pushes to another repository over local paths, with identical ref updates, object transfer, and error behavior in both implementations.

**Why this priority**: Remotes make repositories collaborative. Local-transport parity must come before any network protocol work.

**Independent Test**: Can be fully tested with local-path clone/fetch/push/pull fixtures (new branches,Updates, deletions, shallow boundaries where in scope, dry runs) comparing refs, objects, and output.

**Acceptance Scenarios**:

1. **Given** a source repository, **When** the user clones it via a local path, **Then** the resulting refs, HEAD, work tree, and remote configuration match a standard-Git clone of the same source.
2. **Given** new upstream commits, **When** the user fetches and merges (or pulls), **Then** fetched refs/objects and the merged work tree equal standard Git's results.
3. **Given** local commits, **When** the user pushes, **Then** the destination refs update identically (including rejections for non-fast-forward with equivalent diagnostics) under either implementation.

---

### User Story 4 - Share, apply, and maintain patches and repositories (Priority: P3)

A user exchanges changes as patches, applies mailed or generated patches, cleans untracked files safely, and runs maintenance (integrity checks, object counts, garbage collection accounting) with matching behavior.

**Why this priority**: Patch and maintenance flows are high-value but depend on diff, apply, and object-store correctness from earlier phases.

**Independent Test**: Can be fully tested with format-patch/email-apply round-trips, apply --check/stat fixtures, clean dry-run/force fixtures, and fsck/count-objects/gc-accounting fixtures comparing outputs and filesystem effects.

**Acceptance Scenarios**:

1. **Given** a commit range, **When** the user formats patches, **Then** patch files (headers, diffs, numbering) apply cleanly under both implementations and `git am`-equivalent application yields identical trees.
2. **Given** a patch file, **When** the user checks and applies it, **Then** success, context-mismatch failure, and whitespace-error diagnostics match standard Git, with identical work-tree and index effects.
3. **Given** a repository needing checks or cleanup, **When** the user runs integrity/count/clean operations, **Then** reports, exit codes, and file deletions match standard Git (including clean's refuse-without-force safety).

---

### Edge Cases

- Outside any repository: `fatal: not a git repository` class diagnostic with the correct exit code for commands requiring one; `init`/`clone` explicitly exempt.
- Bare repositories: work-tree commands refuse with the standard work-tree diagnostic; plumbing and ref commands work.
- Unborn HEAD (no commits yet): log shows the standard empty/usage diagnostic; status shows all files as new; commit creates the root commit; branch lists are empty.
- Detached HEAD: switch/restore/merge flows report detached state identically; commits advance the detached HEAD without moving any branch.
- Empty arguments, unknown options, and ambiguous revisions exit 129 (usage) where standard Git does, with `usage:` prefix text.
- Pathspecs matching nothing: add/rm/checkout-class commands report `pathspec '<x>' did not match` with the standard exit; `--` separator handling and magic-signature pathspecs behave identically.
- Non-UTF8 paths, quoted output, `-z` NUL-terminated modes, and `--porcelain`/`--short` machine-readable formats are byte-identical.
- Pager, color (`--no-color`/`--color=never` respected; no color codes when stdout is not a terminal or `--no-pager`), and `--quiet`/`--verbose`/`--dry-run`/`--force` semantics match.
- Lock contention (`.git/index.lock`, ref locks) fails with lock diagnostics and leaves state untouched; crash-during-write never leaves half-written index or refs.
- Case-colliding paths, submodules (mode 160000), symlinks (mode 120000), and executable-bit-only changes behave identically, including on platforms without symlink/executable support where standard Git degrades gracefully.
- Shallow, partial-clone, and grafted repositories: commands outside declared scope fail with an explicit unsupported diagnostic rather than silently wrong results.
- Network transports (ssh/https/file-daemon) beyond local paths are out of scope for the initial phases and MUST be reported as unsupported, not half-implemented.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Every command in scope MUST define its per-command record (name, subcommands, important options, arguments, stdout, stderr, exit codes, config keys, environment variables, filesystem effects, repository effects, compatibility level, MVI boundary, edge cases, test strategy) per the schema in this spec; records live with the phase plan, not tribal knowledge.
- **FR-002**: Commands MUST be classified into exactly the ten requested categories (foundational; inspection; staging/worktree; history; branch/reference; merge/rebase; remote/transport; patch/diff; maintenance; advanced/plumbing) with the membership listed in this spec.
- **FR-003**: The system MUST implement commands in explicit phases (Phase A foundational + basic staging/inspection/history first; later phases branch, merge, remote-local, patch/maintenance; network remotes and exotic commands last or explicitly deferred) and MUST NOT claim a command before its phase gates pass.
- **FR-004**: Each command MUST declare a compatibility level: L1 minimum-viable subset, L2 byte-identical core behavior, or L3 full parity; unlisted options MUST either behave identically or be rejected with `unknown option` usage errors (exit 129), never silently ignored when they change semantics.
- **FR-005**: Behavioral compatibility MUST take precedence over API similarity: stdout/stderr bytes, exit codes, file/mode effects, ref updates, and error wording for covered cases MUST match standard Git; extra conveniences MUST NOT alter covered behavior.
- **FR-006**: Unimplemented commands and options MUST fall through to a clear diagnostic (`unknown command` exit 1; `unknown option` exit 129; out-of-scope transport exit 128 with an unsupported message), and the dispatcher registry MUST stay synchronized with the supported set.
- **FR-007**: The system MUST honor global context for every command: `-C`, `-c name=value`, `--git-dir`, `--work-tree`, `--common-dir`, `--bare`, `--literal-pathspecs`, `--no-pager`, plus `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n`, `GIT_CEILING_DIRECTORIES`, `GIT_PAGER`/`GIT_EDITOR` interactions where applicable.
- **FR-008**: Repository effects (HEAD, refs, index, objects) produced by any covered command MUST be readable and verifiable by standard Git and vice versa (crosswise rule); command suites MUST assert both directions where state is exchanged.
- **FR-009**: Differential testing MUST compare stdout, stderr, and exit codes against standard Git on shared fixtures for every covered option; intentional deviations MUST be listed in the per-command record with justification and a tracking reference.
- **FR-010**: Phase gates MUST require: workspace tests green, differential suites green for the phase's commands, no regression in previously gated commands, and the `t/` scoreboard not regressing through the command dispatcher.
- **FR-011**: Foundational commands (init, version/help dispatch, hash-object, cat-file, rev-parse, config interaction surface) MUST reach at least L2 before later phases begin, since all other commands depend on discovery, hashing, and revision resolution.
- **FR-012**: Staging/worktree commands MUST preserve index byte-compatibility (cross-reference the index specification): add/rm/mv/status/diff/checkout/restore/reset/clean/ls-files/update-index refresh semantics and flag handling identical to standard Git for covered options.
- **FR-013**: History commands MUST agree on revision walking, ordering, formatting defaults, and decoration for covered options; ambiguous/short-ID and unknown-revision diagnostics MUST match.
- **FR-014**: Branch/reference commands MUST enforce refname validity, reflog behavior where covered, detached-HEAD semantics, and symbolic-ref handling identically; destructive operations MUST require the same force/confirmation flags.
- **FR-015**: Merge/rebase commands MUST record conflicts as index stages 1/2/3 with resolve-undo bookkeeping where covered, MUST block commits while unmerged with equivalent diagnostics, and MUST support abort/quit/continue flows for covered subcommands.
- **FR-016**: Remote/transport commands MUST implement local-path transfer first with identical refspec, prune, force, and dry-run semantics; network protocols MUST remain explicitly out of scope until the local gate passes, with clear unsupported diagnostics.
- **FR-017**: Patch/diff commands MUST produce and consume patch formats (unified diff, extended headers, rename detection inputs where covered, mail patches) that round-trip with standard Git in both directions.
- **FR-018**: Maintenance commands MUST never destroy reachable data: prune/expire/repack-class operations verify before removing, and integrity commands classify corrupt vs dangling vs missing identically.
- **FR-019**: Documentation for each command MUST state its MVI boundary: which subcommands/options are covered, which are rejected, and which are deferred — so users can predict behavior without reading source.
- **FR-020**: The phase plan MUST list deferred commands explicitly (not silently omitted) with the condition for promoting each into scope.

### Key Entities *(include if feature involves data)*

- **Porcelain Command**: A user-facing command (e.g. status, checkout, merge) with stable output and diagnostics; specified by its per-command record.
- **Plumbing Command**: A low-level command (e.g. hash-object, update-index, merge-file) exposing primitives other commands build on; same record schema, stricter byte rules.
- **Per-Command Record**: The sixteen-field contract (name, subcommands, options, arguments, stdout, stderr, exit codes, config, environment, filesystem effects, repository effects, compatibility requirements, MVI, edge cases, test strategy).
- **Compatibility Level**: L1 (viable subset), L2 (byte-identical core), L3 (full parity). Declared per command/option.
- **Phase**: An ordered implementation stage with entry criteria, command set, and exit gates; later phases build on earlier ones.
- **MVI Boundary**: The exact subcommand/option subset a command supports at a given level; everything outside it is rejected or deferred explicitly.
- **Differential Suite**: A fixture-driven test comparing stdout/stderr/exit-code plus filesystem/repository effects between implementations.
- **Crosswise Check**: A state-exchange test proving artifacts from either implementation work in the other.
- **Dispatcher Registry**: The single routing table mapping command names to implementations with fallback for unimplemented commands.

## Command Classification and Phase Plan

### Compatibility levels (apply to every record)

- **L1 minimum-viable**: Covered paths behave identically; uncovered options are rejected (129), not guessed. Enough for scripted use of the documented subset.
- **L2 byte-identical core**: Default output, common options, error wording, and exit codes match on the full core matrix; only exotic flags remain deferred.
- **L3 full parity**: Entire option surface including edge semantics (whitespace, renames, decorations, editor/pager flows) matches; remaining gaps are bugs, not scope.

### Phases (ordered; each gates the next)

- **Phase A — Foundation + basic loop (first)**: init, version/help dispatch, hash-object, cat-file, rev-parse, config surface; add, status, diff (basic), log (basic), show (basic). Target L2 for the loop. Gates: FR-010–FR-013 on the basic matrix.
- **Phase B — Staging/worktree completion**: rm, mv, clean, ls-files, update-index (covered subset), write-tree/read-tree, checkout, switch, restore, reset. Target L2. Gates: index byte-compat, checkout/reset convergence suites.
- **Phase C — History completion + branch/reference**: rev-list, diff-tree, ls-tree, mktree, commit-tree, merge-base; branch, switch, tag, show-ref/for-each-ref, update-ref/symbolic-ref, reflog-covered subset. Target L2 core. Gates: walk-order, ref-validity, detached-HEAD suites.
- **Phase D — Merge/rebase**: merge (fast-forward + true merge + conflict recording), merge-file, cherry-pick/revert (covered subset), rebase (basic linear onto with abort/continue), stash (covered subset). Target L1→L2 per command record. Gates: conflict-stage matrix, abort/continue convergence.
- **Phase E — Remote local-transport + patch/maintenance**: clone/fetch/push/pull over local paths, ls-remote (local); format-patch/am, apply; fsck, count-objects, gc-accounting subset, clean safety. Target L1→L2. Gates: local-transfer suites, patch round-trips, no-data-loss maintenance gates.
- **Phase F — Deferred/parity**: network transports, shallow/partial-clone depth handling, exotic diff/rename heuristics, remaining subcommands/flags to L3. Explicitly out of initial scope; each item promotes only with its own record and gates.

### Category membership and per-command MVI

Notation per command: important options → MVI boundary. Global contract fields (stdout/stderr/exit/config/env/effects) follow the uniform rules below unless the record overrides them.

**1. Foundational**: `init` (options: `-q`, `--bare`, `--template`, `-b` initial branch → MVI: plain + bare + initial-branch; deferred: separate-git-dir, shared modes), `version` (MVI: report pinned version), `help`/`usage` (MVI: usage text + exit 129 on misuse), global `-C/-c/--git-dir/--work-tree/--common-dir/--bare/--no-pager/--literal-pathspecs` (MVI: all honored per FR-007).

**2. Repository inspection**: `rev-parse` (`--git-dir`, `--show-toplevel`, `--is-inside-work-tree`, `--verify`, `--abbrev-ref`, `--show-ref-format` subset → MVI: discovery flags + verify), `show-ref`/`for-each-ref` (pattern, `--heads/--tags` → MVI: list + filter), `count-objects` (`-v` → MVI: counts incl. human-readable variant), `fsck` (dangling/unreachable reporting → MVI: reachable scan + classification), `verify-pack`/`index-pack`/`multi-pack-index`/`commit-graph` (verify paths → MVI: verify + diagnostics).

**3. Staging/worktree**: `add` (`-A`, `-u`, `.`, pathspec, `-n/--dry-run`, `-f`, `--chmod` subset → MVI: stage/modified/deleted + dry-run), `rm` (`--cached`, `-r`, `-f`, `--dry-run` → MVI: index+worktree removal incl. recursive), `mv` (rename + directory moves → MVI: index + filesystem rename), `status` (`--short`, `--porcelain`, `-z`, `-u`, `--ignored` subset → MVI: default + short/porcelain + NUL), `checkout`/`switch`/`restore` (branch switch, paths restore `--staged/--worktree/--source`, `--detach`, `-f`, `--dry-run` → MVI: switch + staged/worktree restore + detach), `reset` (`--soft/--mixed/--hard`, paths → MVI: all three modes), `clean` (`-n/-d/-f/-x` subset → MVI: dry-run + force + directory handling with safety refusal), `ls-files` (`--stage`, `--others`, `-z` subset → MVI: cached/others/stage listing), `update-index` (`--refresh`, `--assume-unchanged/--skip-worktree` flags subset → MVI: refresh + flag set/clear).

**4. History**: `log` (`--oneline`, `-n`, `-- <path>`, `--decorate` subset → MVI: walk + oneline + limits + path filtering), `rev-list` (range args, `--count`, `--objects` subset → MVI: ranges + count), `show` (revision + path subset → MVI: commit/file display), `diff` (`--stat`, `--name-only/status`, `--quiet/--exit-code`, `--cached`, `-z` subset → MVI: worktree/index/HEAD comparisons + quiet/exit-code), `diff-tree`/`ls-tree`/`mktree`/`commit-tree`/`write-tree`/`read-tree` (format + tree construction → MVI: canonical formats + tree round-trips).

**5. Branch/reference**: `branch` (create/list/delete `-d/-D/-f/-m`, `--show-current` → MVI: full CRUD + current), `tag` (lightweight + annotated create/list/delete/verify subset → MVI: create + list + delete), `update-ref`/`symbolic-ref` (create/update/delete/verify → MVI: atomic updates + symref set/read), `merge-base` (`--is-ancestor`, independent-base subset → MVI: base + ancestry test), reflog-covered subset (read/expire where declared → MVI: read path only unless record says more).

**6. Merge/rebase**: `merge` (`--ff-only`, `--no-ff`, `--abort`, `--quit`, `--continue`, `-m` → MVI: fast-forward + true merge + conflict recording + abort/continue), `merge-file` (ours/base/theirs three-way → MVI: full three-way with conflict markers), `cherry-pick`/`revert` (`-n`, `--abort/--continue` subset → MVI: single-commit apply + skip/abort), `rebase` (basic linear `--onto`, `--abort/--continue/--skip` → MVI: linear replay + abort/continue; interactive deferred), `stash` (`push`, `pop`, `list`, `drop` subset → MVI: push/pop/list).

**7. Remote/transport**: `clone` (local path, `--bare`, `--branch`, `--depth` policy → MVI: local-path full clone + bare), `fetch`/`push`/`pull` (refspec, `--force`, `--prune`, `--dry-run`, `--tags` subset → MVI: local-path transfer + force/prune/dry-run), `ls-remote`/`remote` (list/add/remove/show subset → MVI: local listing + remote config CRUD). Network URLs: explicit unsupported diagnostic (128) until Phase F.

**8. Patch/diff**: `diff` (patch generation incl. `--src-prefix` subset → MVI per category 4), `format-patch` (`-o`, numbering, `--stdout` subset → MVI: range-to-mail patches), `am` (apply mail patches incl. `--abort/--continue/--skip` → MVI: apply + three-way fallback subset), `apply` (`--check`, `--stat`, `-p`, `--whitespace` subset → MVI: check/stat/apply with context-mismatch errors).

**9. Maintenance**: `fsck` (per category 2), `count-objects`, `gc`/`prune`/`repack` accounting subset (MVI: expire-unreachable + repack-verify-then-swap, never deleting reachable objects), `clean` safety (per category 3), `worktree` (`list`, `add`, `remove` subset → MVI: list + add/remove with `commondir` correctness).

**10. Advanced/plumbing**: `hash-object` (`-w`, `-t`, `--stdin` → MVI: full), `cat-file` (`-p/-t/-s/-e` → MVI: full), `update-index`, `write-tree`/`read-tree`/`mktree`/`commit-tree`, `rev-list`, `diff-tree`, `merge-file`, `apply`, `check-ignore`/`check-attr` (MVI: ignore/attr precedence + output formats), `commit-graph`/`multi-pack-index` verify paths. New plumbing surface is added only to serve a porcelain phase above.

### Uniform per-command contract (defaults; records override)

- **stdout**: Primary results only (ref lists, commit output, diff text, file lists) in standard Git default format; machine-readable variants (`--porcelain`, `--short`, `-z`) byte-identical where covered.
- **stderr**: Diagnostics and progress; `fatal:`/`error:`/`warning:` prefixes matching standard Git families; usage text starting `usage:` on 129.
- **Exit codes**: 0 success; 1 general/difference-found (diff `--quiet`/`--exit-code`); 2 documented diff-form error classes where standard Git uses them; 128 fatal (lock, corruption, unmerged-blocked, transport rejection); 129 usage (bad option, bad revision syntax class). Each record pins its codes.
- **Configuration**: Only declared keys affect behavior (e.g. `user.name`/`user.email` for commits, `core.bare`, `core.ignorecase`, `color.*`, `status.*`, `diff.*`, `merge.*`, `pull.rebase` policy, `remote.*`/`branch.*` for transport); undeclared keys MUST NOT change covered behavior.
- **Environment**: FR-007 set honored uniformly; `GIT_EDITOR`/`GIT_PAGER` only where the record declares interactive/paged behavior; `GIT_INDEX_FILE` redirects index effects; object-directory/alternates vars extend reads, never writes.
- **Filesystem effects**: Exactly the work-tree additions/modifications/deletions/mode changes the operation defines, plus temp/lock files removed on success and failure; no stray files.
- **Repository effects**: Exactly the HEAD/ref/index/object updates defined (atomic via temp+rename); failed operations leave prior state intact.
- **Compatibility requirements**: Covered paths byte-identical (output, bytes, modes, ref values, tree IDs); uncovered paths rejected, never approximated.
- **Test strategy** (every record): differential stdout/stderr/exit-code suite on shared fixtures; filesystem + repository effect comparison (index bytes, work-tree bytes/modes, ref values, object IDs); crosswise state-exchange checks; corruption/lock/contention negatives; `t/` scoreboard coverage through the dispatcher where applicable.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user completing init → add → commit → status → diff → log → show with either implementation observes identical outputs, effects, and exit codes across the Phase A fixture matrix (zero unexplained differences).
- **SC-002**: Branch, switch, tag, and ref operations converge: after the same sequence under either implementation, refs, HEAD state, index, and work tree are identical across the branch matrix.
- **SC-003**: Merge and rebase flows agree: clean operations yield identical trees; conflicting operations record identical unmerged stages, block commits equivalently, and converge after resolution.
- **SC-004**: Local-path clone/fetch/push/pull transfer identical objects and refs with identical diagnostics (including force/prune/dry-run/rejection cases) in both directions.
- **SC-005**: Patch round-trips succeed: patches formatted by either implementation apply under the other yielding identical trees, and apply --check/stat agree on success and failure fixtures.
- **SC-006**: Maintenance operations report identically and never lose reachable data; clean refuses without force and deletes exactly the standard set with force.
- **SC-007**: Every command documents its MVI boundary and compatibility level, and no covered invocation ends in `unknown command/option` while no uncovered invocation silently succeeds with different semantics.

## Assumptions

- Standard Git behavior (on-disk formats plus the `t/` suite) is the oracle; where documentation and the suite disagree, the suite wins.
- The dispatcher registry stays synchronized with the supported set: newly covered commands are registered, and everything else falls through to the system implementation or a clear diagnostic during the transition.
- The index, object-store, and ref specifications are companion contracts; this spec defers to them for byte layouts and states only command-observable behavior here.
- Pinned-version parity: default outputs match the pinned standard Git version; version-dependent formatting differences are recorded per command, not treated as failures.
- Network transports, shallow/partial-clone semantics, submodules, LFS, and GUI/editor integrations are out of scope for Phases A–E unless a per-command record explicitly includes them.
- Performance targets are out of scope for this spec; correctness and behavioral compatibility gate first, with parity performance addressed at planning time.
