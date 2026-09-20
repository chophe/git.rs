# Feature Specification: Git Index Implementation

**Feature Branch**: `004-git-index-implementation`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the specification for git.rs's Git index implementation. The index must be treated as a compatibility-critical binary file format, not merely as an internal Rust data structure. Specify: index file format, index versions, cache entries, paths, file modes, stat information, timestamps, inode/device information, file sizes, object IDs, stage numbers, conflict stages, extensions, checksums, index locking, atomic updates, index corruption handling, index refresh, skip-worktree, assume-unchanged, intent-to-add, sparse-index behavior if applicable, compatibility with standard Git. Cover behavior needed by: git add, git status, git diff, git checkout, git restore, git reset, git commit, merge operations. Define interoperability tests where an index produced by git.rs is consumed by Git and vice versa."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Stage and commit files with a byte-compatible index (Priority: P1)

A user stages file changes and commits them. The index records each path with its mode, object ID, and stat data so that the commit contains exactly what was staged, and an index written by one implementation is accepted without modification by the other.

**Why this priority**: Staging is the index's core job. Without byte-compatible read/write of entries, no command that consumes the index interoperates.

**Independent Test**: Can be fully tested by staging identical work trees (new, modified, deleted, renamed, executable-bit-only changes) with each implementation and comparing the resulting index bytes, entry listings, and commit trees.

**Acceptance Scenarios**:

1. **Given** a work tree with new and modified files, **When** the user stages everything, **Then** the index contains one stage-0 entry per path with the correct mode and the object ID of the file content, and the other implementation lists identical entries.
2. **Given** an index written by one implementation, **When** the other implementation reads it, **Then** it loads all entries, modes, IDs, and flags without error or silent data loss.
3. **Given** staged content, **When** the user commits, **Then** the commit tree matches the tree built from the index by the other implementation for the same index state.

---

### User Story 2 - See accurate worktree status and diffs (Priority: P1)

A user runs status and diff operations and sees exactly which files are staged, modified, deleted, or untracked, matching standard Git, including fast paths where unchanged files are not re-hashed.

**Why this priority**: Status and diff are the most frequently used index consumers. Incorrect freshness comparison (stat vs content) produces false modifications or misses real ones.

**Independent Test**: Can be fully tested by building fixture work trees (clean, dirty, timestamp-only touch, size change, mode change, deleted, untracked, ignored) and comparing status/diff name-status output between implementations.

**Acceptance Scenarios**:

1. **Given** a clean work tree matching the index, **When** the user checks status, **Then** both implementations report clean and neither re-hashes file contents unnecessarily beyond what standard Git does for refresh.
2. **Given** a file whose content changed, **When** the user checks status or diff, **Then** both implementations report it as modified with identical paths and identical staged-vs-unstaged classification.
3. **Given** a file whose timestamp changed but content did not (including the same-second-as-index-write case), **When** the user checks status, **Then** both implementations agree on clean vs modified after content verification, with no persistent false-dirty state.

---

### User Story 3 - Checkout, restore, and reset move files and index together (Priority: P2)

A user switches branches, restores paths, or resets staged content, and the work tree files plus index entries end up in the same state under either implementation.

**Why this priority**: These commands write both the index and the work tree. Divergence here causes lost files, wrong modes, or phantom staged changes.

**Independent Test**: Can be fully tested by running branch switches, path restores (staged and worktree variants), and soft/mixed/hard resets on fixtures and comparing resulting index entries plus work-tree file bytes and modes.

**Acceptance Scenarios**:

1. **Given** a branch switch that adds, removes, and modifies files, **When** the user checks out the branch, **Then** work-tree files, modes, and index entries are identical regardless of which implementation performed the checkout.
2. **Given** staged changes, **When** the user unstages paths (restore --staged / reset), **Then** the index returns to the prior committed state for those paths while work-tree files are preserved, identically in both implementations.
3. **Given** a hard reset request, **When** it completes, **Then** both index and work tree match the target tree exactly, including removal of files that do not belong to it.

---

### User Story 4 - Resolve merge conflicts through index stages (Priority: P2)

A user merges, sees conflicts recorded as higher-stage entries, and resolves them by staging the merged result, with conflict state visible identically in both implementations.

**Why this priority**: Conflict stages (1/2/3) are the merge contract. If stages are misread or miswritten, conflicts appear resolved when they are not, or unmerged paths are committed silently.

**Independent Test**: Can be fully tested with merge fixtures (clean merge, modify/modify conflict, add/add conflict, delete/modify conflict, rename-involved merge) comparing unmerged-entry listings and post-resolution state.

**Acceptance Scenarios**:

1. **Given** a conflicting merge, **When** the merge stops with conflicts, **Then** both implementations list the same unmerged paths with the same stage-1/2/3 entries, modes, and object IDs.
2. **Given** unmerged paths, **When** the user attempts to commit, **Then** both implementations refuse with an unmerged-paths diagnostic and non-zero exit.
3. **Given** a resolved conflict staged by the user, **When** the resolution is committed, **Then** the resulting tree is identical regardless of which implementation recorded the resolution.

---

### User Story 5 - Survive locking, crashes, and corruption without data loss (Priority: P2)

A user running commands while another process holds the index lock, or opening a damaged index, gets the same diagnostics and the same intact index as under standard Git — never a half-written or silently repaired index.

**Why this priority**: The index is a single shared file with no server. Locking and atomic-update discipline is the only protection against corruption; error agreement here is a safety boundary.

**Independent Test**: Can be fully tested with lock-contention fixtures, kill-during-write simulations, and a corruption corpus (truncated, bit-flipped, bad-checksum indexes), comparing diagnostics, exit codes, and post-operation index bytes.

**Acceptance Scenarios**:

1. **Given** a locked index, **When** the user runs a command that writes the index, **Then** it fails with a lock diagnostic naming the lock file, leaves the original index untouched, and exits non-zero like standard Git.
2. **Given** a process terminated mid-write, **When** the user next reads the index, **Then** they see either the complete old index or the complete new index — never a truncated mix — with stale lock cleanup behaving like standard Git.
3. **Given** a corrupt index, **When** the user runs a read command, **Then** the system reports corruption (naming the file and cause) and refuses to fabricate entries; recovery requires explicit removal or checkout-index-equivalent rebuild, matching standard Git.

---

### User Story 6 - Honor special flags and extensions across implementations (Priority: P3)

A user working with assume-unchanged, skip-worktree, intent-to-add, or cached-tree and sparse-directories state sees the same status/diff/checkout behavior in both implementations, with extension data preserved across round-trips.

**Why this priority**: These flags change the meaning of entries. Dropping a flag on read or write silently changes status output and checkout behavior for sparse and special workflows.

**Independent Test**: Can be fully tested by setting each flag/extension with one implementation and exercising status, diff, checkout, and index read/write with the other, comparing flags, outputs, and extension bytes.

**Acceptance Scenarios**:

1. **Given** entries marked assume-unchanged or skip-worktree, **When** the work-tree file changes, **Then** both implementations suppress the modification in status/diff identically, and checkout preserves the flag.
2. **Given** an intent-to-add entry, **When** the user checks status and diff, **Then** both implementations report the path as new/added with empty content semantics, and refuse to include it in a tree until real content is staged.
3. **Given** an index carrying cached-tree and sparse-directory data, **When** the other implementation reads and rewrites it without touching those paths, **Then** the extension data round-trips byte-identical and status output is unchanged.

---

### Edge Cases

- Empty index (zero entries): reads as valid, writes with header + zero count + checksum; status shows all tracked files deleted, commit of empty index yields the empty tree.
- Single-entry and maximum-path-length entries (overlong paths beyond filesystem limits reported as errors, not truncated).
- Non-UTF8 path bytes: preserved byte-exact, never mangled by display encoding; sorted by raw bytes.
- Paths with spaces, newlines, quotes, backslashes, and leading dots/dashes listed and quoted exactly like standard Git.
- Same-second (racily-clean) files: timestamp equals index mtime granularity; content must be verified by hash, and the refreshed stat recorded on write-back.
- Empty files, executable-bit-only changes, symlink entries (mode 120000 storing the link target as blob content), submodule entries (mode 160000 referencing a commit ID with no work-tree content comparison).
- Intent-to-add entries with zero stat data; assume-unchanged vs skip-worktree precedence where both could apply.
- Unmerged entries (stages 1–3) coexisting with stage-0 entries for other paths; duplicate path+stage pairs rejected.
- Case-insensitive filesystems: case-colliding paths behave like standard Git (conflict/error rather than silent overwrite) where applicable.
- Index version 3 (extended flags) and version 4 (path compression) files produced by standard Git: read correctly or rejected with a clear diagnostic — never misparsed as version 2.
- Unknown or malformed extensions: unknown-but-documented-skippable extensions skipped; malformed extension lengths or truncated data reported as corruption.
- Checksum mismatch, truncated header, entry-count lie (header count vs actual entries), entry padding errors, path-length overflow in version 4 encoding.
- Lock file already present, stale lock from a dead process, read-only directory, disk-full during write, missing index file (treated as empty, not an error, for commands that create it).
- Split-index and untracked-cache extensions if encountered: behavior must be explicit (supported with identical semantics, or rejected with a clear message) rather than silently dropped on rewrite.
- Sparse index with collapsed directory entries: paths under a collapsed directory resolve identically; expanding vs preserving collapse on write must match the documented policy.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST read and write the index file header (`DIRC` magic, version, 32-bit big-endian entry count) and MUST treat magic or length violations as corruption, not as an empty index.
- **FR-002**: The system MUST support version 2 as the canonical read/write version, producing bytes byte-identical to standard Git for identical entry sets.
- **FR-003**: The system MUST read version 3 (extended flags) correctly, preserving extended-flag bits, and MUST either write version 3 byte-identically when extended flags require it or document the downgrade policy without silent flag loss.
- **FR-004**: The system MUST read version 4 (prefix-compressed paths) correctly, and MUST either write version 4 byte-identically when chosen or document when it normalizes to version 2, never emitting a version it cannot parse back.
- **FR-005**: The system MUST store one sorted entry per (path, stage) pair, sorted by path bytes with stage as tiebreaker, and MUST reject duplicate (path, stage) pairs.
- **FR-006**: The system MUST store paths as raw bytes (preserving non-UTF8), enforce the documented maximum path handling of standard Git, and MUST quote/escape paths in text output exactly like standard Git.
- **FR-007**: The system MUST store and enforce file modes limited to the standard set (regular `100644`, executable `100755`, symlink `120000`, submodule `160000`, directory `040000` where standard Git uses it) and MUST reject unknown modes as corruption on read.
- **FR-008**: The system MUST store per-entry stat data (creation/change timestamps with seconds + nanoseconds, device, inode, user/group IDs, file size) and MUST write back refreshed stat data after work-tree verification.
- **FR-009**: The system MUST compare work-tree files using the same freshness rules as standard Git (size, timestamps with nanosecond granularity where available, inode/device change detection) and MUST fall back to content hashing for racily-clean or otherwise suspect entries.
- **FR-010**: The system MUST store object IDs using the repository hash algorithm's width (20 bytes SHA-1 / 32 bytes SHA-256) and MUST select the width from the repository, never assuming SHA-1.
- **FR-011**: The system MUST record stage numbers 0 (normal) and 1/2/3 (base/ours/theirs conflict stages), MUST list unmerged paths distinctly, and MUST refuse tree-writing operations while unmerged entries exist (except operations defined to operate on unmerged state).
- **FR-012**: The system MUST read and write entry flags (assume-valid for assume-unchanged, extended skip-worktree bit, intent-to-add bit) and MUST apply their status/diff/checkout semantics identically to standard Git.
- **FR-013**: The system MUST read and write the `TREE` (cached-tree) extension and MUST invalidate affected cached-tree nodes when covered entries change, recomputing or marking invalid exactly where standard Git does.
- **FR-014**: The system MUST read and write the `REUC` (resolve-undo) extension, preserving conflict-resolution history across operations that keep it, and clearing it exactly where standard Git clears it.
- **FR-015**: The system MUST preserve extensions it does not natively interpret (round-trip their bytes unchanged on read-modify-write) when standard Git defines them as preservable, and MUST reject-or-document any extension it cannot safely preserve instead of silently dropping it.
- **FR-016**: The system MUST verify the trailing checksum over all preceding bytes on every read and MUST write a correct trailing checksum (matching algorithm width) on every write.
- **FR-017**: The system MUST update the index atomically: write to a lock file in the same directory, flush to stable storage, then rename over the index; readers MUST never observe a half-written index.
- **FR-018**: The system MUST implement lock-file semantics matching standard Git: failure to acquire the lock fails the command with a lock diagnostic and non-zero exit, leaves the original index intact, and stale-lock handling matches standard Git.
- **FR-019**: The system MUST report a corrupt index (bad magic, version, count, entry, extension, or checksum) with a diagnostic naming the index file and cause, a non-zero exit, and MUST NOT fabricate entries or auto-repair silently.
- **FR-020**: The system MUST implement refresh semantics (stat revalidation against the work tree, content-hash verification where required, updating in-memory and on-disk stat data) with results identical to standard Git's refresh.
- **FR-021**: The system MUST treat a missing index file as an empty index for commands that create it, and MUST return the standard "not in index" / "path does not exist" class diagnostics for commands requiring entries.
- **FR-022**: The system MUST stage file content (add): hash current work-tree bytes as the correct object type, insert or replace the stage-0 entry with fresh stat data, and remove entries for deleted paths exactly like standard Git.
- **FR-023**: The system MUST implement status classification (staged vs unstaged vs untracked, renamed/copied detection inputs where applicable) from index + HEAD + work tree with output identical to standard Git for the same state.
- **FR-024**: The system MUST implement diff inputs from the index (index-vs-HEAD and worktree-vs-index comparisons) yielding identical file lists, modes, and content hashes as standard Git.
- **FR-025**: The system MUST implement checkout/restore-from-index (write index entries to work-tree files with correct bytes, modes, and refreshed stat) and restore-to-index (copy HEAD or work-tree state into the index) with identical end states to standard Git.
- **FR-026**: The system MUST implement reset variants (soft: move HEAD only; mixed: reset index to tree keeping work tree; hard: reset index and work tree to tree) with identical index and work-tree outcomes to standard Git.
- **FR-027**: The system MUST build commit trees exclusively from stage-0 entries (plus cached-tree acceleration), with tree entry order, modes, and object IDs identical to standard Git for the same index.
- **FR-028**: The system MUST record merge conflicts as stage-1/2/3 entry sets, MUST expose them in status/diff listings as unmerged, and MUST clear them to a single stage-0 entry on resolution exactly like standard Git.
- **FR-029**: The system MUST implement sparse-index behavior explicitly: read indexes containing collapsed (sparse-directory) entries, apply status/diff/checkout semantics that skip collapsed contents identically to standard Git, and preserve collapse on rewrite unless the operation definitionally expands it.
- **FR-030**: The system MUST interoperate crosswise: any index file written by one implementation MUST be fully readable, verifiable, and usable by the other for add/status/diff/checkout/reset/commit/merge flows, verified by byte comparison and behavior comparison suites.
- **FR-031**: The system MUST match standard Git exit-code classes for index operations (0 success; non-zero for lock failure, corruption, unmerged-blocked commit, pathspec misses; usage errors 129 where standard Git uses them) with equivalent stderr diagnostics.

### Key Entities *(include if feature involves data)*

- **Index File**: The `.git/index` binary file: header (`DIRC` + version + entry count), sorted cache entries, extensions, trailing checksum. The single source of staged state.
- **Cache Entry**: One (path, stage) record carrying stat data, mode, object ID, and flag bits (assume-valid, extended, stage, name length). The unit of staging and conflict representation.
- **Stat Data**: Per-entry filesystem snapshot: change/creation timestamps (seconds + nanoseconds), device, inode, user/group IDs, size. Basis of freshness comparison and refresh.
- **Path**: Raw-byte work-tree-relative file path; sort key of the index; may be non-UTF8 and require quoting in display.
- **File Mode**: Entry type + permissions, restricted to the standard set (regular, executable, symlink, submodule). Determines work-tree materialization.
- **Object ID**: Content address of the staged blob (or submodule commit) using the repository hash width; the link between index and object store.
- **Stage Number**: 0 for resolved entries; 1 (base), 2 (ours), 3 (theirs) for conflict entries. Same path may appear up to three times during conflicts.
- **Entry Flags**: assume-valid (assume-unchanged), skip-worktree, intent-to-add, extended-flag presence. Modifiers of status/diff/checkout semantics.
- **Extension**: Trailing typed block (four-letter signature + length + payload), e.g. cached tree, resolve-undo, sparse-directory data, end-of-index marker. Preserved or interpreted per type.
- **Cached Tree (TREE)**: Extension caching tree object IDs for index subtrees to accelerate tree writing; invalidated on entry change.
- **Resolve-Undo (REUC)**: Extension recording pre-merge entry states to support conflict abort/redo flows.
- **Checksum**: Trailing hash over all preceding index bytes; integrity gate for every read.
- **Index Lock**: The `.git/index.lock` file providing mutual exclusion and atomic-commit-via-rename for writers.
- **Refreshed Entry**: An entry whose stat data was revalidated against the work tree (rehashing where required) so subsequent comparisons are fast and correct.
- **Sparse Directory Entry**: Collapsed representation of a fully-skipped directory in a sparse index; expands only on operations that require its contents.

## Detailed Behavioral Specification

Each item states expected behavior, input/output format, compatibility requirements, error behavior, and required tests.

### 1. File format and header

- **Expected**: Single file: `DIRC`, 32-bit big-endian version, 32-bit big-endian entry count, entries, extensions, trailing checksum. No trailing bytes after checksum.
- **Format**: Exact binary layout above; all multi-byte integers big-endian; entry count equals the number of parsed entries.
- **Compatibility**: Header bytes produced for the same entry set MUST be byte-identical to standard Git for the chosen version.
- **Error**: Bad magic, short header, or count mismatch is corruption (FR-019). Extra bytes after checksum are corruption.
- **Tests**: Golden-byte tests for empty/single/multi-entry indexes; header-fuzz tests (bad magic, truncated, count lie).

### 2. Versions 2, 3, 4

- **Expected**: Version 2 baseline (flags + 12-bit name length). Version 3 adds extended-flags word when the extended bit is set. Version 4 replaces paths with prefix-compression (varint prefix length + suffix) and a Healy-style name encoding.
- **Compatibility**: Read all three; canonical write is version 2 unless extended flags or version-4 input require otherwise per documented policy (FR-002–FR-004).
- **Error**: Unknown version number is a clear unsupported/corrupt diagnostic, never parsed as version 2.
- **Tests**: Crosswise fixtures in each version (written by standard Git, read here and vice versa); version-4 compression round-trips with long shared prefixes.

### 3. Cache entries, paths, modes

- **Expected**: Fixed-size stat/mode/ID/flags fields plus variable path, padded to 8-byte alignment (versions 2–3) or compression encoding (version 4). Sorted by path bytes, stage tiebreak.
- **Compatibility**: Field order, widths, padding, name-length truncation rules, and sort order MUST match standard Git byte-for-byte.
- **Error**: Bad padding, embedded NUL in path, invalid mode, or unsorted/duplicate entries reported as corruption on strict read paths.
- **Tests**: Golden vectors (special names, non-UTF8, max lengths, all legal modes, padding boundaries 1–8); sort-order tests; fuzz tests for truncation/mode violations.

### 4. Stat information, timestamps, inode/device, sizes

- **Expected**: 32-bit seconds + 32-bit nanoseconds for change and modification times; 32-bit device, inode, mode, user/group IDs, size. Size is the work-tree file size at stage time (intent-to-add uses defined zero/empty semantics).
- **Compatibility**: Widths, order, and granularity MUST match; nanosecond fields preserved where the platform provides them.
- **Error**: Stat comparison never trusts a match on size or timestamp alone for racily-clean entries — content is hashed (FR-009).
- **Tests**: Fixtures with touched timestamps, same-second writes, inode recycling (delete + recreate), size-only and mode-only changes; refreshed-stat write-back comparison.

### 5. Object IDs and hash-width agility

- **Expected**: Raw ID bytes inline per entry at repository hash width; entry length and checksum width follow the algorithm.
- **Compatibility**: SHA-1 (20-byte) and SHA-256 (32-byte) indexes MUST both read/write with correct widths; IDs equal the stored object's ID.
- **Error**: Wrong-width ID data is corruption; cross-algorithm index reuse is an error, not silent reinterpretation.
- **Tests**: SHA-1 and SHA-256 index fixtures crosswise in both directions; width-confusion negative tests.

### 6. Stages and conflict representation

- **Expected**: Stage encoded in flag bits; normal state has only stage 0; conflicts carry stages 1–3 for the same path with independent modes/IDs/stats.
- **Compatibility**: Stage-bit layout, unmerged listing format, and tree-write refusal while unmerged MUST match standard Git.
- **Error**: Commit/tree-write with unmerged entries fails with unmerged-paths diagnostic; duplicate (path, stage) is corruption.
- **Tests**: Merge-conflict fixture matrix (modify/modify, add/add, delete/modify, directory/file) comparing stage listings and refusal behavior.

### 7. Extensions and checksum

- **Expected**: Each extension is a 4-byte signature + 32-bit length + payload; the checksum covers header + entries + extensions. Known extensions: `TREE`, `REUC`, link, untracked-cache, sparse-directory data, end-of-index marker.
- **Compatibility**: Extension order tolerance, unknown-extension skipping, and checksum coverage MUST match standard Git; interpreted extensions MUST have identical semantics.
- **Error**: Truncated extension, length overrun into checksum, or checksum mismatch is corruption naming the index file.
- **Tests**: Extension matrix (each known type present/absent/combined, unknown extension round-trip, corrupted length/checksum fixtures).

### 8. Locking and atomic updates

- **Expected**: Writers create `.git/index.lock`, write complete new content, flush, and rename onto `index`. Readers open `index` directly and never see partial writes.
- **Compatibility**: Lock-file path, lock-failure diagnostics, and stale-lock behavior MUST match standard Git; successful updates leave no `.lock` residue.
- **Error**: Lock-held failure exits non-zero without touching the index; I/O failure mid-write removes the lock file and preserves the old index.
- **Tests**: Lock-contention tests, kill-during-write tests (old-or-new completeness), stale-lock and read-only-directory tests.

### 9. Corruption handling

- **Expected**: Every read validates magic, version, count, entries, extensions, and checksum in order, reporting the first failure precisely.
- **Compatibility**: Diagnostics name the index file and cause with standard-Git-equivalent wording and exit codes.
- **Error**: Never auto-repair; never return partial entry lists as success.
- **Tests**: Corruption corpus (each validation layer damaged) with diagnostic and exit-code comparison.

### 10. Refresh and freshness

- **Expected**: Refresh re-stats every entry against the work tree, rehashes where stat is inconclusive (including racily-clean), updates in-memory state, and persists refreshed stat on write paths that standard Git persists.
- **Compatibility**: Which entries are rehashed vs trusted, and when stat is written back, MUST match standard Git observably (status output plus resulting index bytes).
- **Error**: Missing work-tree file marks entry deleted (not corruption); unreadable file is an I/O diagnostic distinct from corruption.
- **Tests**: Racy-clean corpus (write file in the same second as index update across timestamp granularities), refresh-then-compare-bytes tests.

### 11. assume-unchanged, skip-worktree, intent-to-add

- **Expected**: assume-valid bit suppresses work-tree comparison for status/diff but is cleared on operations that definitionally restage; skip-worktree additionally directs checkout to leave the work-tree file alone; intent-to-add entries behave as empty blobs for diff and are excluded from trees.
- **Compatibility**: Bit positions, precedence, persistence across read/write, and per-command semantics MUST match standard Git exactly.
- **Error**: Commands requiring real content (e.g. tree-write including an intent-to-add path) fail with the standard diagnostic rather than writing placeholder data.
- **Tests**: Per-flag matrices: set flag with one implementation, exercise status/diff/checkout/commit with the other; flag-persistence round-trip tests.

### 12. Sparse-index behavior

- **Expected**: Sparse indexes collapse fully-skipped directories into directory entries; status/diff/checkout treat collapsed contents as skipped without expanding; expansion happens only for operations requiring those paths.
- **Compatibility**: Collapsed-entry encoding, skipped-path semantics, and preserve-vs-expand-on-write policy MUST match standard Git for the same sparse-checkout patterns.
- **Error**: A full (non-sparse) reader encountering collapsed entries MUST either interpret them correctly or fail with a clear sparse-unsupported diagnostic — never silently expand them into wrong entries.
- **Tests**: Sparse-checkout fixtures (cone mode, selected directories): compare status/diff/checkout outputs and index round-trip bytes in both directions.

### 13. Command behaviors (add / status / diff / checkout / restore / reset / commit / merge)

- **add**: Hashes work-tree bytes, inserts stage-0 entries with fresh stat, stages deletions/renames per pathspec; identical resulting index bytes and tree IDs.
- **status**: Classification and output (including quoted paths, staged/unstaged/untracked sections, unmerged section) identical for the same HEAD + index + work tree.
- **diff**: Index-vs-HEAD and worktree-vs-index file lists, modes, and hashes identical; intent-to-add shown as new-file with empty content; assume-unchanged/skip-worktree suppressed.
- **checkout / restore**: Work-tree materialization (bytes, modes, symlink targets, submodule placeholders), index update, stat refresh, and flag preservation identical.
- **reset**: Soft/mixed/hard end states (HEAD, index, work tree) identical, including untracked-file preservation rules.
- **commit**: Tree built from stage 0 only, cached-tree accelerated, unmerged-blocked; resulting tree and commit metadata identical for the same index.
- **merge**: Conflict recording (stages 1–3), resolve-undo bookkeeping,assume/skip flag handling during merge, and post-resolution states identical.
- **Tests**: Per-command crosswise suites on shared fixtures asserting identical index bytes, work-tree bytes/modes, and stdout/stderr/exit codes.

### 14. Interoperability test definition

- **Direction A (produced here, consumed by standard Git)**: For every fixture family (empty, basic, special names/modes, flags, conflicts, extensions, sparse, both hash widths, versions 2–4 where applicable), write the index with this implementation, then verify with standard Git: index read succeeds, entry listing matches, verification passes, and status/diff/commit-equivalent operations produce identical results.
- **Direction B (produced by standard Git, consumed here)**: For the same fixture families generated by standard Git (including version 3/4 and extension combinations this implementation would not natively write), read here and assert identical entry data, identical command behavior, and identical rewrite bytes where a rewrite policy is defined.
- **Harness**: Shared fixture repositories plus byte-comparison of index files, structured comparison of entry listings, and stdout/stderr/exit-code comparison of command equivalents. Corruption and lock-contention fixtures run in both directions with diagnostic comparison.
- **Gates**: No fixture family passes unless both directions agree; any silent flag/extension drop or checksum divergence is a failure.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Indexes staged from identical work trees by either implementation are byte-identical for the same version, and each implementation reads the other's index with zero entry differences across the full fixture matrix.
- **SC-002**: Status and diff outputs agree exactly (same paths, same classifications, same exit codes) on at least 50 shared fixtures covering clean, dirty, renamed, mode-only, deleted, untracked, ignored, conflicted, flag-marked, and sparse states.
- **SC-003**: Checkout, restore, and reset flows converge: after the same operation performed by either implementation, index bytes and work-tree bytes/modes are identical across at least 20 fixtures including branch switches and all reset modes.
- **SC-004**: Conflicting merges record identical stage-1/2/3 entries, block commits with equivalent diagnostics, and produce identical trees after resolution across the conflict matrix.
- **SC-005**: Lock contention, crash-during-write, and corruption-corpus tests show zero half-written indexes observed by readers and equivalent diagnostics/exit codes in every case.
- **SC-006**: Flag and extension round-trips preserve assume-unchanged, skip-worktree, intent-to-add, cached-tree, and sparse-directory state with zero silent drops across both directions.
- **SC-007**: A user completing stage → status → diff → commit → switch → merge with either implementation observes no behavioral difference at any step (task-completion parity verified by the crosswise suites).

## Assumptions

- Standard Git behavior (on-disk format plus `t/` suite expectations) is the oracle; where documentation and the test suite disagree, the test suite wins.
- Version 2 is the canonical write version; versions 3 and 4 MUST be readable, with write policies documented (write 3 when extended flags require it; version 4 written only when sparse/prefix-compression semantics are explicitly in scope, otherwise normalized to version 2 without data loss beyond the documented encoding change).
- The repository hash algorithm determines ID and checksum widths for the index; SHA-1 fixtures are the default matrix with SHA-256 coverage for width-sensitive paths.
- nanosecond timestamps are preserved where the platform provides them and zero-filled otherwise, matching standard Git's platform behavior.
- Split-index and untracked-cache extensions are handled by explicit policy (support with identical semantics or clear rejection), never silently dropped; the chosen policy is documented before planning ends.
- Sparse support covers cone-mode sparse-checkout with collapsed directory entries; exotic non-cone patterns follow the same read/preserve contract but may be normalized on write per documented policy.
- Bare repositories carry an index only transiently during operations that require one; normal work-tree commands assume a work tree is present.
- Performance targets (large-index read/write times, refresh rehash rates) are out of scope for this spec and addressed at planning time; correctness and byte-compatibility gate first.
