# Feature Specification: References and Branch Management

**Feature Branch**: `008-refs-branch-management`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the specification for references and branch management in git.rs. Cover: refs/heads, refs/tags, refs/remotes, HEAD, symbolic references, packed-refs, reflogs, reference transactions, atomic reference updates, locking, concurrent modification, reference deletion, branch creation/deletion, upstream tracking, remote-tracking branches, detached HEAD, reflog expiration, reference discovery. Define the API boundary between the storage layer and higher-level branch commands. Include concurrency, corruption, crash-safety, and interoperability requirements."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Create, move, and delete branches and tags safely (Priority: P1)

A user creates a branch at a commit, moves it forward, and deletes it when done (likewise lightweight tags), and every operation either completes fully with the same ref value, log entry, output, and exit code as standard Git, or fails cleanly leaving the previous value untouched.

**Why this priority**: Branch and tag mutation is the most frequent ref operation. Unsafe updates (half-written refs, lost concurrent updates, deleted checked-out branches) are data-loss defects; everything else builds on this guarantee.

**Independent Test**: Can be fully tested by running create/set/delete sequences (new branch, fast-forward move, forced move, delete, delete-checked-out-branch refusal, invalid names, D/F conflicts) with each implementation on identical stores and comparing ref values, outputs, diagnostics, and exit codes.

**Acceptance Scenarios**:

1. **Given** a valid commit, **When** the user creates `refs/heads/<name>` pointing at it, **Then** both implementations store the same value, report success the same way, and the new ref resolves identically in both.
2. **Given** a ref update with an expected old value, **When** the stored value matches (or differs), **Then** both implementations apply (or reject) the update identically, so a concurrent mover can never silently overwrite the other.
3. **Given** a request to delete the currently checked-out branch, **When** the user attempts it, **Then** both implementations refuse with the same diagnostic and exit code and the branch still exists.

---

### User Story 2 - Follow HEAD through branches and detached states (Priority: P1)

A user switches branches, detaches HEAD at a commit for inspection, and returns to a branch (including starting from an unborn branch with no commits yet), observing the same HEAD bytes, resolution results, and error behavior in both implementations.

**Why this priority**: HEAD determines what "current branch" means for checkout, commit, and status. Wrong attached/detached/unborn handling misattributes commits or claims a clean state that is not there.

**Independent Test**: Can be fully tested with attached, detached, unborn, missing, and malformed HEAD fixtures by comparing HEAD file bytes, resolution outcomes, and diagnostics between implementations.

**Acceptance Scenarios**:

1. **Given** `HEAD` pointing at `refs/heads/main`, **When** the user resolves `HEAD`, **Then** both implementations return the same commit ID and report the same symbolic target.
2. **Given** a detached `HEAD` holding a raw ID, **When** the user resolves it, **Then** both implementations return that ID directly and report detached state distinctly from attached state.
3. **Given** an unborn branch (HEAD points at a ref that does not exist yet), **When** the user queries state, **Then** both implementations report the branch name with no ID rather than a missing-object error.

---

### User Story 3 - Discover and list every reference (Priority: P1)

A user (or script) lists branches, tags, and remote-tracking refs, filters by namespace or pattern, and resolves any listed name back to the same object, with identical names, order, dereferenced tag lines, and exit codes in both implementations.

**Why this priority**: Discovery is the read contract behind checkout, log, fetch display, and all ref automation. Missing refs, wrong ordering, or loose/packed disagreement makes identical stores look different.

**Independent Test**: Can be fully tested on fixture stores (loose-only, packed-only, mixed with loose-overrides-packed, peeled tags, per-worktree HEAD, hundreds of refs) by comparing full and pattern-filtered listings plus resolve-round-trips.

**Acceptance Scenarios**:

1. **Given** refs under `refs/heads`, `refs/tags`, and `refs/remotes`, **When** the user lists each namespace, **Then** both implementations print the same names in the same byte-sorted order with the same values.
2. **Given** a ref present in both loose and packed storage, **When** the user resolves or lists it, **Then** the loose value wins in both implementations.
3. **Given** an annotated tag, **When** the user lists with dereferencing, **Then** both implementations emit the same peeled `^{}` continuation line.

---

### User Story 4 - Audit ref movements through reflogs (Priority: P2)

A user reviews where a branch pointed yesterday (reflog entries with old/new values, actor, timestamp, message), and old entries expire on schedule, identically in both implementations.

**Why this priority**: Reflogs are the safety net for recovering from bad resets and forced moves. Missing entries or wrong expiry silently removes the recovery path.

**Independent Test**: Can be fully tested by performing identical ref update sequences with both implementations and comparing reflog files entry-for-entry, then running expiry and comparing surviving entries.

**Acceptance Scenarios**:

1. **Given** a branch update, **When** it completes, **Then** both implementations append the same reflog entry (old value, new value, actor, timestamp, message) to the same log path.
2. **Given** a reflog with recent and ancient entries, **When** expiration runs with the same policy, **Then** both implementations keep and drop the same entries (reachable-current values kept regardless of age per policy).
3. **Given** a deleted ref, **When** the user reads its history, **Then** both implementations agree on whether the log survives deletion and for how long.

---

### User Story 5 - Update many refs as one atomic unit (Priority: P2)

A user (or higher-level command such as a fetch/checkout flow) submits several ref updates together with expected old values, and either all of them land or none do — never a half-applied batch — with identical outcomes in both implementations.

**Why this priority**: Multi-ref batches are how complex operations stay consistent (e.g. updating a branch plus HEAD, or many remote-tracking refs at once). Partial application leaves the store in a state no single command would have produced.

**Independent Test**: Can be fully tested by submitting batches (all-valid, one bad name, one stale old-value, mixed create/delete) to identical stores and comparing final ref values, per-update verdicts, and exit codes.

**Acceptance Scenarios**:

1. **Given** a batch where every update verifies, **When** it is submitted, **Then** both implementations apply all updates and report success identically.
2. **Given** a batch where one update has a stale expected old value, **When** it is submitted, **Then** both implementations reject the whole batch, change nothing, and name the failing ref identically.

---

### User Story 6 - Track branches against their upstreams (Priority: P2)

A user links a local branch to its upstream (a local or remote-tracking ref), sees ahead/behind counts and status display derived from that link, and the link survives branch moves and renames exactly as in standard Git.

**Why this priority**: Upstream configuration drives status, push/pull defaults, and branch display. Divergent link storage or counting makes the same repository report different sync states.

**Independent Test**: Can be fully tested by setting, listing, and clearing upstream links (local upstream, remote-tracking upstream, missing upstream, deleted upstream) and comparing stored configuration, resolved upstream values, and ahead/behind counts.

**Acceptance Scenarios**:

1. **Given** a local branch and a target ref, **When** the user sets the upstream, **Then** both implementations store the same link and report the same upstream for that branch.
2. **Given** a branch with an upstream, **When** either side advances, **Then** both implementations compute the same ahead/behind counts.
3. **Given** a renamed branch, **When** the rename completes, **Then** both implementations agree on whether the upstream link followed the rename or was cleared.

---

### User Story 7 - Survive concurrency, crashes, and corruption (Priority: P2)

A user running concurrent ref writers, killing a writer mid-update, or opening a store with damaged refs/logs gets the same verdicts and the same intact store as under standard Git — never a half-written ref, lost update, or silently accepted corruption.

**Why this priority**: Refs are shared mutable state with no server. Locking plus atomic-rename discipline is the only protection; agreement here is a safety boundary, not polish.

**Independent Test**: Can be fully tested with lock-contention fixtures (two writers, stale locks, read-only directories), kill-during-write simulations, and a corruption corpus (malformed loose refs, bad packed-refs lines, truncated reflogs), comparing diagnostics, exit codes, and post-operation store bytes.

**Acceptance Scenarios**:

1. **Given** a ref locked by another writer, **When** the user updates it, **Then** both implementations fail with a lock diagnostic naming the ref, leave the prior value intact, and exit non-zero identically.
2. **Given** a writer terminated mid-update, **When** the user next reads the ref, **Then** they see either the complete old value or the complete new value — never a truncation — with stale-lock cleanup behaving like standard Git.
3. **Given** a corrupt ref or log entry, **When** the user reads or verifies it, **Then** both implementations report the corruption naming the ref and cause, and refuse to resolve it as if healthy.

---

### Edge Cases

- Unborn HEAD (points at nonexistent branch); missing HEAD; HEAD with trailing whitespace; HEAD containing NUL or overlong content.
- Symref chains (multi-hop), self-loops, two-cycles, depth overflow; symref pointing at a missing ref; symref pointing at a non-ref path.
- Invalid names: `..`, control characters, space, `~^:?*[`, backslash, leading/trailing slash or dot-component, `@{`, `.lock` suffix, lone `@`, names outside `refs/` for branch-class operations.
- D/F conflicts: ref `refs/heads/a` vs `refs/heads/a/b` coexisting; file occupying a directory prefix and vice versa.
- Loose file with empty content, missing trailing newline, uppercase hex, wrong-length hex, `ref:` with extra whitespace.
- `packed-refs`: missing, empty, header-only, unsorted entries, duplicate names, malformed lines, `^{}` peel line without a preceding tag line, overlong lines, stale entries shadowed by loose refs.
- Deletion corners: deleting a nonexistent ref (no-op vs error per command family), deleting a ref with a stale expected old value, deleting the current branch, deleting a ref that is also packed (loose removal must reveal or hide per packed-refs update rules).
- Transaction corners: empty batch, batch touching the same ref twice, create-vs-update existence preconditions, zero-ID (`0{40/64}`) as delete sentinel, per-update messages vs batch message.
- Reflog corners: update with empty message, actor identity missing, timestamp at epoch/far-future, log directory missing (auto-created vs error), reflog for a ref that never had one, expire with `--all`, `--expire=never`, per-namespace policies (heads vs remotes vs stash-class).
- Remote-tracking corners: `refs/remotes/<remote>/<branch>` hierarchy, remote HEAD symref (`refs/remotes/<remote>/HEAD`), upstream pointing at a deleted remote-tracking ref, case where fetch prunes stale remote-tracking refs.
- Worktree corners: per-worktree HEAD vs shared refs, `commondir` indirection, `GIT_COMMON_DIR` override, bare repositories (no work tree, HEAD at root).
- Hash-width corners: 40-hex vs 64-hex values in a SHA-1 vs SHA-256 repository; zero IDs of the wrong width; mixed-width stores rejected, never reinterpreted.
- Concurrency corners: lock file already present, stale lock from a dead process, lock directory unwritable, two processes racing the same ref, batch racing single updates.

## Requirements *(mandatory)*

### Functional Requirements

Namespaces and discovery:

- **FR-001**: The system MUST store local branches under `refs/heads/`, tags under `refs/tags/`, and remote-tracking refs under `refs/remotes/<remote>/`, with loose files (one per ref) and `packed-refs` as the two files-backend/populated stores, and MUST resolve names through the same namespace search rules as standard Git (short names like `main` resolve per the standard six-rule precedence where applicable).
- **FR-002**: Reference discovery MUST merge loose refs with packed refs using loose-overrides-packed semantics, list results in byte-sorted refname order, and MUST return identical name sets, values, ordering, and pattern-filtering behavior as standard Git for the same store (including empty and header-only `packed-refs`, which mean "no packed refs", not an error).
- **FR-003**: Refname validation MUST accept exactly what `git check-ref-format` accepts for ref creation, update, and lookup paths, including all edge rules (`..`, control chars, `~^:?*[`, backslash, `@{`, `.lock`, leading/trailing slashes and dots, reflog-prohibited forms), and MUST reject invalid names with a fatal diagnostic naming the ref and a non-zero exit.

HEAD, symbolic refs, detached state:

- **FR-004**: `HEAD` MUST be stored as either `ref: <target>\n` (attached, target usually `refs/heads/<branch>` and possibly unborn) or `<hex>\n` (detached), with exact file bytes matching standard Git, and MUST report attached vs detached vs unborn distinctly (unborn names the branch with no ID; it MUST NOT surface as a missing-object error).
- **FR-005**: Symbolic refs (including `HEAD` and `refs/remotes/<remote>/HEAD`) MUST resolve by following `ref:` chains up to the same bounded depth as standard Git, detecting loops and overflow as resolution failures (no unbounded recursion, no hang), and MUST distinguish "symref target missing" from "symref malformed".
- **FR-006**: Detaching (writing a raw ID into `HEAD`) and re-attaching (writing `ref:` back) MUST produce HEAD bytes, reflog entries, and command outputs identical to standard Git, and MUST refuse to detach at a missing or corrupt object with a matching diagnostic.

Packed refs:

- **FR-007**: The system MUST read `packed-refs` with header tolerance (`# pack-refs with:` capabilities, comments, blank lines), `<oid> <refname>` entries, and optional `^{<peeled>}` continuation lines for annotated tags, and MUST treat malformed lines as packed-refs corruption naming the file and line cause.
- **FR-008**: The system MUST write `packed-refs` (repack-all operation) with sorted entries, correct peeled lines, and the standard capability header, pruning loose refs covered by the pack exactly where standard Git prunes them, so that listings before and after repacking are identical and the file is readable by standard Git and vice versa.

Reflogs:

- **FR-009**: Every ref create/update/delete performed with logging enabled MUST append a reflog record (`old-id SP new-id SP actor-ident SP timestamp SP timezone TAB message`) to `logs/<ref>` (with `logs/HEAD` updated wherever standard Git updates it), creating log directories as needed and using message/identity formatting identical to standard Git.
- **FR-010**: Reflog reads MUST return entries newest-last in file order with the same parsing strictness as standard Git (malformed lines are corruption naming the log, not skipped silently), and MUST agree on what "no reflog" means per ref (absent file vs empty file behavior identical).
- **FR-011**: Reflog expiration MUST apply the same policy inputs as standard Git (per-namespace expire times for reachable vs unreachable entries, `--expire`, `--expire-unreachable`, `--all`, dry-run reporting) and MUST keep every entry guarding a currently-reachable value regardless of age while dropping the same aged-out entries, verified by entry-for-entry comparison.

Transactions, locking, atomicity:

- **FR-012**: Single-ref updates MUST be compare-and-swap: `update(<ref>, <new>, <expected-old>)` applies only when the stored value equals `<expected-old>` (with defined sentinels for "must exist", "must not exist", and "no expectation"), and MUST report stale-expectation failures naming the ref with expected vs actual values and a non-zero exit.
- **FR-013**: Multi-ref batches (including `--stdin` transaction input) MUST be all-or-nothing: verification of every update precedes any write, and any failure (bad name, stale old value, lock conflict, D/F conflict) aborts the batch with zero refs changed; per-update verdicts and the batch exit code MUST match standard Git.
- **FR-014**: Locking MUST follow the standard lock-file protocol: one `<ref>.lock` per ref under update, lock acquisition failure fails the owning update with a lock diagnostic naming the ref, locks release on success (via atomic rename) and on abort (via removal), and stale locks from dead processes are handled like standard Git (reported, never silently broken while live).
- **FR-015**: All ref writes (loose files, `packed-refs` rewrites, reflog appends, HEAD writes) MUST be atomic — complete new content via temp file plus flush plus rename — so concurrent readers observe only old-or-new states, and a crash at any point leaves either the prior store or the complete new store, never a truncation or mix.
- **FR-016**: Concurrent modification MUST be detected, not masked: a writer whose expected-old value changed under it fails (per FR-012/FR-013) rather than overwriting; lock-hold failures MUST NOT be retried into silent success — the command MUST surface the conflict with the same exit-code class as standard Git.

Branches, deletion, upstreams, remote-tracking:

- **FR-017**: Branch creation MUST set `refs/heads/<name>` to the resolved start point (defaulting to HEAD where standard Git defaults), MUST create reflogs per policy, MUST reject invalid names, existing names (without force/move semantics of the owning command), and missing start points with matching diagnostics.
- **FR-018**: Branch deletion MUST remove the loose ref (and handle packed-refs presence per FR-008 rules), MUST refuse the currently checked-out branch and unmerged-deletion without force exactly where standard Git refuses, and MUST report deleted vs missing refs with the same messages and exit codes.
- **FR-019**: Branch rename/move MUST relocate the ref value, move or recreate its reflog per standard Git rules, relocate or clear the upstream link per FR-021, and MUST handle renames onto existing names and across D/F conflicts identically.
- **FR-020**: Remote-tracking refs (`refs/remotes/<remote>/*`) MUST follow namespace rules (created/updated/pruned by fetch-class flows and plumbing, listed under remote namespaces, never treated as local branches for checkout-branch guards), and pruning MUST remove exactly the stale set standard Git removes for the same fetch state.
- **FR-021**: Upstream links (`branch.<name>.remote` + `branch.<name>.merge`) MUST be stored, listed, renamed-with-branch, and cleared exactly like standard Git for local and remote-tracking targets (including `@{upstream}`/`@{u}` resolution), and MUST report missing/deleted upstreams identically rather than resolving to a wrong ref.
- **FR-022**: Ahead/behind computation backing status/branch display for upstream-linked branches MUST agree with standard Git counts for identical graphs (merge-base-based, first-parent-independent where standard Git is), without requiring network access.

Corruption, crash-safety, interoperability:

- **FR-023**: Corrupt state (malformed loose ref, bad `packed-refs` line, truncated reflog, symref loop/overflow, wrong-width hex, dangling ref whose object is missing) MUST be reported with diagnostics naming the ref/file and cause, MUST fail resolution/listing/verification paths that depend on it, and MUST never be auto-repaired or resolved to a fabricated value.
- **FR-024**: Crash-safety: after a kill at any write point (loose write, packed-refs rewrite, reflog append, batch commit), the store MUST satisfy: readers see old-or-new only, no `.lock` residue blocks future writers beyond standard stale-lock behavior, and reflog and ref values remain mutually consistent (no log entry for a value that never landed, beyond what standard Git itself permits).
- **FR-025**: Crosswise interoperability: every ref store state written by one implementation (loose refs, symrefs, HEAD states, `packed-refs` incl. peeled tags, reflogs) MUST be listed, resolved, walked, and verified by the other with zero differences, checked by byte comparison of control files plus behavioral comparison of listings/resolutions.
- **FR-026**: Exit-code classes MUST match standard Git everywhere: `0` success; `1` generic error / negative verification; `128` fatal (bad ref, corrupt store, lock failure, non-symbolic-ref where symbolic required); `129` usage (bad flags/syntax) — and stderr diagnostics MUST identify the ref and cause with the same specificity.

### Storage-layer / branch-command API boundary

The boundary below is a behavioral contract (what each layer owns and what crosses it), not a code-level interface. Conformance is judged by observable behavior: either side of the boundary may change internals freely as long as every FR above still holds.

- **FR-027 (storage layer owns)**: The storage layer owns ref bytes on disk and guarantees: resolve (with symref following + depth bound), ordered list with loose-overrides-packed merge, compare-and-swap single update, all-or-nothing batch, lock acquisition/release per FR-014, packed-refs read/write, reflog append/read/expire primitives, and corruption detection per FR-023. It MUST NOT decide user-facing policy (branch deletion guards, upstream semantics, display formats) — it only enforces name validity, expected-value matching, and locking, and reports machine-checkable verdicts (applied / stale / bad-name / locked / corrupt / missing).
- **FR-028 (branch commands own)**: Higher-level branch commands own policy and presentation: which start point a new branch defaults to, force-vs-refuse rules for move/delete (checked-out, unmerged), upstream link create/rename/clear, remote-tracking namespace routing, output formats and quiet/verbose variants, and error-message wording. They MUST implement that policy exclusively through storage-layer operations (never by writing ref files directly), passing explicit expected-old values so concurrent races surface as stale-expectation failures rather than silent overwrites.
- **FR-029 (data exchanged across the boundary)**: Only these facts cross the boundary: refname strings, new IDs (or delete intent), expected-old IDs with existence sentinels, actor identity + timestamp + message for reflog records, per-update verdicts, and ordered ref listings. Display strings, configuration keys, and revision-expression parsing MUST NOT leak into storage; lock files, temp paths, and file layouts MUST NOT leak out to branch commands.
- **FR-030 (porcelain insulation)**: No branch/tag/checkout/status behavior may depend on storage internals (file paths, lock names, pack layout). All such consumers MUST observe identical results when the storage layer changes its internals (e.g. repacking refs, pruning loose files) as long as logical ref values are unchanged — verified by repack-then-compare suites.

### Key Entities

- **Ref**: A name (usually under `refs/heads/`, `refs/tags/`, `refs/remotes/`) mapping to an object ID; stored loose, packed, or both.
- **HEAD**: The pointer to current state — either `ref: <branch>` (attached, possibly unborn) or a raw ID (detached).
- **Symbolic ref**: A ref whose value is `ref: <target>` instead of an ID; resolved by chain following (includes `HEAD` and remote `HEAD` symrefs).
- **Packed-refs file**: Sorted flat file of `<oid> <refname>` lines with optional peeled `^{}` continuation lines and a capability header; shadowed by loose refs.
- **Reflog**: Append-only history `logs/<ref>` of old/new values with actor, timestamp, and message per movement; plus `logs/HEAD`.
- **Transaction**: A batch of ref updates with per-update expected-old values that commits all-or-nothing.
- **Lock**: The per-ref `<ref>.lock` mutual-exclusion marker implementing atomic compare-and-swap.
- **Branch**: A movable ref under `refs/heads/` with an optional reflog and optional upstream link.
- **Upstream link**: The configured (`remote` + `merge`) target a branch tracks, resolved to a local or remote-tracking ref for ahead/behind.
- **Remote-tracking ref**: A cached ref under `refs/remotes/<remote>/` recording where a remote's branches were last seen.
- **Detached HEAD**: A state where `HEAD` holds a raw ID instead of a branch pointer.
- **D/F conflict**: A state where one ref path is a file-prefix of another (`a` vs `a/b`), requiring ordered handling.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users complete everyday branch work (create, switch, move, set upstream, delete) on first attempt with identical results under either implementation — branch listings, HEAD state, and upstream reports agree on 100% of a 50-case everyday-workflow corpus.
- **SC-002**: Concurrent writers never lose updates silently: in a 200-run race harness (two writers racing the same ref with expected-old guards), every race resolves as either clean winner-plus-stale-loser or lock-conflict, with zero silent overwrites and zero half-written refs observed.
- **SC-003**: Crash-injection runs (100 kills across loose-write, repack, reflog-append, and batch-commit points) show zero truncated refs/logs and zero lock-residue blocks beyond standard stale-lock behavior, with readers always seeing old-or-new values.
- **SC-004**: Corruption corpus handling agrees fully: 100% of damaged fixtures (bad loose refs, malformed packed lines, truncated logs, symref loops, dangling refs) are reported with matching exit-code classes and never resolved as healthy.
- **SC-005**: Reflog entry-for-entry agreement reaches 100% on a 30-sequence movement corpus, and expiry keeps/drops the same entries in both implementations across recent/ancient/reachable/unreachable mixes.
- **SC-006**: The committed `t/`-suite scoreboard shows no regression on ref-gated scripts (update-ref, reflog, packed-refs, rev-parse, branch families), and every newly ported ref operation lands with a byte-identical differential suite.

## Assumptions

- Standard Git (C git at the vendored version; the `t/` suite wins where documentation and suite disagree) defines correct behavior, including exit-code classes (usage `129`, fatal `128`, general error `1`).
- Scope is the files backend (loose refs + `packed-refs`) with reflogs; the reftable backend from the Phase 7 plan is explicitly deferred and MUST NOT be silently half-supported — its absence is a clear unsupported diagnostic where encountered.
- Remote-tracking namespace rules and pruning semantics are in scope; the fetch/push network transport that produces those updates is out of scope.
- SHA-1 is the default algorithm matrix with SHA-256 width handling on ID paths; cross-algorithm translation is out of scope.
- Identity/date inputs for reflog records follow the port's documented UTC-based handling; expiry durations follow standard configuration keys with documented defaults.
- Performance targets (large ref-store listing times, batch throughput) are set at planning time; correctness, atomicity, and byte-compatibility gate before optimization.
