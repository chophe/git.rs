# Feature Specification: Git Plumbing Commands

**Feature Branch**: `006-plumbing-commands`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a detailed specification for Git plumbing commands in git.rs. Focus on the low-level commands that form the foundation of Git: hash-object, cat-file, update-index, write-tree, read-tree, commit-tree, update-ref, symbolic-ref, rev-parse, for-each-ref, show-ref, ls-tree, ls-files, verify-pack, rev-list, count-objects, fsck, pack-objects, index-pack, unpack-objects, unpack-file, mktag, verify-commit, verify-tag, other relevant plumbing commands discovered during repository analysis. Specify exact behavioral and data-format compatibility requirements. The resulting design must allow porcelain commands to be implemented on top of stable plumbing primitives."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Hash and inspect raw objects (Priority: P1)

A user creating automation (editors, importers, test harnesses) hashes arbitrary bytes into blob/tree/commit/tag objects with `hash-object`, and reads any object back with `cat-file` (type, size, content, existence check, batch modes), observing byte-identical IDs, output bytes, diagnostics, and exit codes to standard Git.

**Why this priority**: Object hashing and inspection are the lowest layer. Every other plumbing command consumes objects produced here; divergence breaks all crosswise interoperation.

**Independent Test**: Can be fully tested by hashing identical inputs (empty, binary, large, NUL-containing, non-UTF8) with and without writing, then reading results back in both directions (objects written by either implementation read by the other), comparing IDs, stdout bytes, stderr text, and exit codes.

**Acceptance Scenarios**:

1. **Given** arbitrary file bytes, **When** the user runs `hash-object` with the same type flag, **Then** both implementations print the same hex ID and, with write enabled, store a mutually readable object.
2. **Given** an object stored by either implementation, **When** the user runs `cat-file -t/-s/-e/-p` on its ID, **Then** both implementations report the same type, size, existence verdict, and pretty-printed content.
3. **Given** a missing or corrupt object, **When** the user runs `cat-file`, **Then** both implementations fail with the same diagnostic class and non-zero exit rather than printing fabricated content.
4. **Given** a list of IDs on stdin, **When** the user runs `cat-file --batch` / `--batch-check`, **Then** both implementations stream the same per-object records including `missing` handling.

---

### User Story 2 - Build trees from the index (Priority: P1)

A user staging content with `update-index`, listing staged state with `ls-files`, writing trees with `write-tree`, materializing trees with `read-tree`, and inspecting trees with `ls-tree` gets identical index bytes, tree IDs, file listings, and error behavior from either implementation.

**Why this priority**: The index-to-tree pipeline is how staged content becomes history. It is the direct foundation for `add`, `commit`, `checkout`, and `reset` porcelain.

**Independent Test**: Can be fully tested by staging fixture work trees (new, modified, deleted, mode-only, symlink, submodule, conflicted, special names) with each implementation and comparing index bytes, `ls-files` output, `write-tree` IDs, `ls-tree` listings, and `read-tree` end states.

**Acceptance Scenarios**:

1. **Given** a work tree with staged changes, **When** the user runs `update-index` then `write-tree`, **Then** both implementations produce byte-identical index files and the same tree ID.
2. **Given** a tree ID, **When** the user runs `read-tree` into an index, **Then** both implementations leave the same index entries and report the same errors for bad trees, bad prefixes, or unmerged states.
3. **Given** any index/tree state, **When** the user runs `ls-files` / `ls-tree` with the same flags, **Then** both implementations print the same path/mode/ID listings in the same order.

---

### User Story 3 - Create commits and tags from explicit parts (Priority: P1)

A user builds commits with `commit-tree` (tree + parents + message + author/committer identity), creates tag objects with `mktag`, extracts file blobs with `unpack-file`, and validates signatures with `verify-commit` / `verify-tag`, with commit/tag IDs and verification verdicts identical to standard Git.

**Why this priority**: Commits and tags are the units of history. Porcelain `commit` and `tag` are thin wrappers over these primitives plus ref updates; any ID or validation divergence forks history.

**Independent Test**: Can be fully tested by creating commits/tags (single/multi-parent, empty message, non-UTF8, signed, annotated vs lightweight) with each implementation and comparing object IDs, `cat-file -p` output, `fsck` verdicts, and verify-command exit codes.

**Acceptance Scenarios**:

1. **Given** a tree ID plus parent IDs and a message, **When** the user runs `commit-tree`, **Then** both implementations print the same commit ID and store mutually readable commits.
2. **Given** a tag payload on stdin, **When** the user runs `mktag`, **Then** both implementations print the same tag ID for valid input and reject invalid input with the same diagnostic class.
3. **Given** a signed or unsigned commit/tag, **When** the user runs `verify-commit` / `verify-tag`, **Then** both implementations report the same validity verdict and exit code.

---

### User Story 4 - Manage refs as stable named pointers (Priority: P1)

A user scripting branch/tag automation updates refs with `update-ref` (create, set, delete, stdin transactions), reads symbolic refs with `symbolic-ref`, lists refs with `show-ref` / `for-each-ref`, and resolves arbitrary revisions with `rev-parse`, with identical ref values, listing order, format output, and error behavior.

**Why this priority**: Refs name every history tip. Porcelain branch, tag, checkout, and push flows compose exactly these primitives; unstable ref output or transaction semantics breaks all of them.

**Independent Test**: Can be fully tested on fixture ref stores (loose-only, packed-only, mixed, symref chains, detached HEAD, dangling, invalid names, D/F conflicts) by running each ref command with both implementations and comparing ref values, stdout bytes, stderr, and exit codes.

**Acceptance Scenarios**:

1. **Given** a valid ref update (create/set/delete/verify-value), **When** the user runs `update-ref`, **Then** both implementations leave the same ref value, reflog entry, and locking behavior, and reject invalid names/values identically.
2. **Given** a ref store, **When** the user runs `show-ref` / `for-each-ref` with the same pattern and format, **Then** both implementations print the same refs in the same order with the same dereferenced `^{}` lines.
3. **Given** any revision expression, **When** the user runs `rev-parse --verify` (and query flags), **Then** both implementations resolve to the same ID or fail with the same diagnostic and exit-code class.

---

### User Story 5 - Walk and validate history (Priority: P2)

A user enumerating history with `rev-list` (ranges, limits, counts, object lists) and checking repository health with `fsck` and `count-objects` sees identical commit sets, traversal membership, dangling/unreachable reports, count/size fields, and exit codes.

**Why this priority**: History walking and integrity checking are the read path for `log`, `gc` decisions, and corruption triage. Membership or diagnostic divergence silently changes what users see as "their history".

**Independent Test**: Can be fully tested on linear, merge-heavy, disconnected, shallow/grafted, and corrupted fixtures by comparing `rev-list` stdout sets, `fsck` reports, and `count-objects [-v]` output byte-for-byte.

**Acceptance Scenarios**:

1. **Given** a commit graph, **When** the user runs `rev-list` with the same range/limit flags, **Then** both implementations print the same object set with the same ordering guarantees and the same `--count` value.
2. **Given** a healthy or damaged repository, **When** the user runs `fsck`, **Then** both implementations report the same missing/corrupt/dangling/unreachable classification with the same exit code.
3. **Given** any object store, **When** the user runs `count-objects [-v]`, **Then** both implementations report the same counts and size/garbage fields with the same units.

---

### User Story 6 - Pack, move, and verify object bulk transport (Priority: P2)

A user archiving history with `pack-objects`, indexing packs with `index-pack`, unpacking with `unpack-objects`, verifying with `verify-pack`, and extracting single blobs with `unpack-file` gets mutually readable packs/indexes/objects and identical verification reports from either implementation.

**Why this priority**: Packs are the bulk-transfer and scale format. Porcelain fetch/push/clone and `gc`/`repack` compose these primitives; non-interoperable packs partition repositories.

**Independent Test**: Can be fully tested by generating packs (undeltified, deltified, thin where applicable, multi-pack, empty, single-object) with each implementation and cross-verifying (opposite-side `verify-pack`, `index-pack --verify`, object reads through deltas).

**Acceptance Scenarios**:

1. **Given** a set of objects, **When** the user runs `pack-objects`, **Then** packs written by either implementation verify clean under the other and decode to identical objects.
2. **Given** a pack file, **When** the user runs `index-pack` / `verify-pack`, **Then** both implementations accept or reject the same files with the same diagnostics and produce functionally equivalent indexes.
3. **Given** a pack or single blob, **When** the user runs `unpack-objects` / `unpack-file`, **Then** both implementations restore the same loose objects / work-tree file bytes.

---

### User Story 7 - Compose porcelain from stable plumbing (Priority: P2)

A porcelain author (or shell script) implements `commit`, `branch`, `tag`, `checkout`, `log`-class flows using only the plumbing specified here, without depending on human-oriented output, and the composed flow behaves identically regardless of which implementation provides the plumbing.

**Why this priority**: This is the design gate: plumbing output formats used by scripts must be stable and machine-parseable, and porcelain must not need unstable internals.

**Independent Test**: Can be fully tested by writing reference shell flows (stage → tree → commit → ref update → walk → verify) that invoke only plumbing commands, running each flow against both implementations, and comparing end-state stores plus every intermediate machine-readable capture.

**Acceptance Scenarios**:

1. **Given** a scripted flow using only plumbing commands, **When** run against either implementation, **Then** all intermediate IDs/listings and the final repository state are identical.
2. **Given** porcelain-format output vs plumbing-format output for the same data, **When** the porcelain format changes, **Then** the plumbing format used by scripts remains unchanged (stability contract).

---

### Edge Cases

- Empty inputs: empty blob, empty tree, commit with empty message, tag with empty message, empty pack (zero objects), empty index, empty ref store.
- Boundary objects: zero-length vs one-byte blobs, NUL bytes and non-UTF8 bytes in blob content, paths, messages, and identities.
- Special tree entries: modes `100644`/`100755`/`120000`/`040000`/`160000`, submodule commits, symlinks, names with spaces/newlines/quotes/backslashes, non-UTF8 names, deep nesting.
- Revision corners: full vs abbreviated IDs (unique, ambiguous, too-short, no-match), `HEAD` attached/detached/unborn/missing/malformed, `@{...}`, ranges `A..B`/`A...B`, `<rev>:<path>`, `~`/`^` peels, reflog-dependent forms where applicable.
- Ref corners: invalid names per `check-ref-format` rules, `D/F` file-vs-directory conflicts, loose-overrides-packed, missing `packed-refs`, unsorted/malformed packed lines, symref loops and depth overflow, dangling refs, locked refs and concurrent writers.
- Index/tree corners: unmerged stages blocking `write-tree`, `read-tree -m` conflicts, `--prefix` subtrees, intent-to-add/skip-worktree/assume-unchanged entries, version 3/4 and unknown-extension indexes.
- Pack corners: zero-object packs, 64-bit offsets, large counts, truncated records, bad trailer checksums, delta chains with missing bases or cycles, thin packs at layer boundaries.
- Signature corners: unsigned objects, well-formed but untrusted signatures, malformed signatures, missing keys — verification verdicts agree without network access.
- I/O corners: `--stdin` with empty input, NUL-separated `-z` input, pathspec magic, broken pipes, read-only stores, disk-full mid-write (no half-written objects/refs/indexes visible).
- Environment corners: `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`/`GIT_OBJECT_DIRECTORY`/`GIT_ALTERNATE_OBJECT_DIRECTORIES` overrides, `-C` directory changes, bare repositories, work-tree-less operation for object/ref/pack commands.

## Requirements *(mandatory)*

### Functional Requirements

Cross-cutting compatibility (applies to every command below):

- **FR-001**: Every plumbing command MUST exit with standard Git exit-code classes: `0` success; `1` for false/negative verdicts (e.g. failed verification, non-matching comparison) and generic errors; `128` fatal (bad object, corrupt store, lock failure); `129` usage (bad flags/syntax). Where standard Git uses a specific class, this implementation MUST use the same class.
- **FR-002**: For identical inputs and stores, stdout bytes, stderr diagnostic class (message identifying the object/ref/revision and cause), and exit code MUST be identical to standard Git (C git `v2.55.0` behavior; where C source and the `t/` suite disagree, the `t/` suite wins).
- **FR-003**: Machine-readable plumbing output formats (ref listings, object listings, traversal output, count fields, verify reports) MUST be stable: porcelain and scripts MUST be implementable against them without parsing human-oriented text, and future porcelain-format changes MUST NOT alter these plumbing formats.
- **FR-004**: Every command MUST honor repository discovery and environment overrides (`-C`, `--git-dir`, `--work-tree`, `--bare`, `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`) with the same resolution and error behavior as standard Git, and MUST operate work-tree-less wherever standard Git does.
- **FR-005**: Every object-touching command MUST respect the repository hash algorithm (SHA-1 default, SHA-256 where configured): ID lengths, empty-object constants, path fanout, and pack/index trailer widths follow the selected algorithm, and cross-algorithm input is an error, not a silent reinterpretation.
- **FR-006**: All writers (objects, refs, index entries, packs) MUST be atomic (temp file + fsync + rename discipline) so readers never observe half-written state; failed operations MUST leave the prior store intact.

Object primitives:

- **FR-007 (`hash-object`)**: MUST support hashing file operands and stdin (`--stdin`), type selection (`-t blob|tree|commit|tag`, default blob), write-to-store (`-w`), and literal hashing semantics compatible with standard Git; MUST print the resulting hex ID on stdout; MUST reject unknown types and missing files with usage/fatal classes matching standard Git; MUST produce IDs and stored bytes readable by standard Git and vice versa.
- **FR-008 (`cat-file`)**: MUST support `-t` (type), `-s` (size), `-e` (existence verdict, no output), `-p` (pretty-print by type), direct content streaming, `--batch` and `--batch-check` (stdin-driven records with `missing` handling for absent objects); MUST resolve full/abbreviated IDs and ref expressions through the same resolver as `rev-parse --verify`; MUST report missing vs corrupt distinctly with matching diagnostics.
- **FR-009 (`mktag`)**: MUST read a tag payload from stdin, strictly validate `object`/`type`/`tag`/`tagger` headers and target existence/kind, store the tag object, and print its ID; MUST reject malformed payloads, unknown target types, and missing targets with fatal diagnostics matching standard Git.
- **FR-010 (`mktree`)**: MUST read `mode SP type SP oid TAB name` lines from stdin, validate modes/types/IDs/names and entry ordering per tree-format rules, store the tree, and print its ID; MUST reject malformed lines, invalid modes, and duplicate/unsorted entries with matching diagnostics. (Discovered during analysis: already routed in the port; included here to pin its contract.)
- **FR-011 (`unpack-file`)**: MUST extract a blob object to a temporary work-tree-adjacent file and print the resulting path; MUST refuse non-blob objects with a fatal diagnostic matching standard Git; MUST NOT modify the index or refs.
- **FR-012 (`verify-commit`, `verify-tag`)**: MUST verify the embedded signature payload of a commit/tag object and report validity with matching stdout verdicts and exit-code classes (valid 0, invalid/unsigned non-zero); MUST NOT require network access; MUST treat malformed signature headers as verification failures with matching diagnostics, never as crashes.

Index and tree pipeline:

- **FR-013 (`update-index`)**: MUST support staging paths (`--add`, `--remove`, `--force-remove`, explicit pathspec operands), and at minimum the refresh/cacheinfo paths needed by porcelain `add`; MUST produce index bytes byte-identical to standard Git for the same inputs; MUST refuse to stage paths outside the work tree and report pathspec misses with matching diagnostics.
- **FR-014 (`write-tree`)**: MUST build a tree object from stage-0 index entries (honoring `--prefix` subtree writes and `--missing-ok` semantics where standard Git defines them), MUST refuse with an unmerged-entries diagnostic when stages 1–3 are present, and MUST print the resulting tree ID; resulting tree bytes MUST be byte-identical to standard Git for the same index.
- **FR-015 (`read-tree`)**: MUST support reading a tree into the index (`--empty`, one-way `--reset`-family single-tree read, `--index-output` alternate index, `-n` dry-run semantics), MUST reject two/three-way merge reads it does not implement with a clear unimplemented diagnostic only where standard Git behavior is explicitly deferred and documented — otherwise MUST implement merge reads (`-m`) with identical end-state index entries; MUST leave the work tree untouched (work-tree updates belong to checkout-class commands).
- **FR-016 (`ls-files`)**: MUST list index entries with `--stage`, `--others/--cached/--deleted/--modified` selection, `-z` NUL termination, and `--exclude-standard` ignore interplay matching standard Git ordering and quoting; MUST exit non-zero only where standard Git does (e.g. unmerged-aware `--error-unmatch` paths).
- **FR-017 (`ls-tree`)**: MUST list tree objects with `-r` recursion, `-t` tree inclusion, `-d` directories-only, `-z` termination, `--name-only`/`--name-status`-class selectors, and `--full-tree` semantics, printing `mode SP type SP oid TAB name` lines in standard Git order with byte-identical formatting.

Commit, revision, and history primitives:

- **FR-018 (`commit-tree`)**: MUST accept a tree ID plus zero or more `-p` parents, read the message from stdin/file/`-m`, apply author/committer identity from the environment and `-c` overrides with standard Git formatting (including timezone handling), store the commit, and print its ID; MUST resolve tree/parent arguments through the shared revision resolver (abbreviations, `~`/`^`, `<rev>:<path>`); MUST reject missing trees/parents with fatal diagnostics.
- **FR-019 (`rev-parse`)**: MUST resolve single revisions (`--verify`), echo store locations (`--git-dir`, `--show-toplevel`, `--is-inside-work-tree`, `--is-bare-repository`, `--absolute-git-dir` family), and support `--short`/abbreviation output, `--symbolic-full-name`/`--abbrev-ref`, and range forms (`A..B`, `A...B`) with output, ordering, and exit-code parity; ambiguous/short/unknown revisions MUST produce matching diagnostics.
- **FR-020 (`rev-list`)**: MUST walk history from explicit heads/ranges with `--count`, `-n/--max-count`, `--objects`, `--all`, `--topo-order`/`--date-order`, `--first-parent`, and path-limiting semantics matching standard Git membership; output order for unordered walks MUST contain exactly the same set (ordered walks MUST match order); `--count` values MUST agree.
- **FR-021 (`for-each-ref`)**: MUST iterate refs matching patterns with `--format` placeholders (at minimum `%(objectname)`, `%(objecttype)`, `%(refname)`, `%(refname:short)`, `%(upstream`-family where documented), `--sort` keys, and `--count` limits, printing byte-identical lines in the same order as standard Git.
- **FR-022 (`show-ref`)**: MUST list refs as `<oid> <refname>` in byte-sorted refname order with `--head`, `--tags`, `--heads`, `-d` dereference (`^{}` peeled lines), `--verify` single-ref mode, and pattern filtering, matching standard Git output and exit codes (including non-zero when patterns match nothing where standard Git does so).
- **FR-023 (`symbolic-ref`)**: MUST read (`symbolic-ref HEAD`/`<name>`), create/update (`<name> <target>`), and delete (`--delete`) symbolic refs with strict refname validation, matching file bytes (`ref: <target>\n`), stdout echoes, and diagnostics of standard Git, including quiet mode (`-q`) exit-code-only behavior.
- **FR-024 (`update-ref`)**: MUST support single-ref create/set/delete with old-value verification (`<ref> <new> [<old>]`), `-d` deletion, `--no-deref` direct-symref update, message/reflog (`-m`), and `--stdin` multi-ref transactions with all-or-nothing atomicity; MUST implement ref locking with matching lock-failure diagnostics and MUST enforce `check-ref-format` name rules and `D/F` conflict handling identically.

Pack, store-health, and verification primitives:

- **FR-025 (`verify-pack`)**: MUST verify pack `.pack`/`.idx` pairs with `-v` verbose object listings (`oid type size size-in-pack offset` lines in index order) and `-s` statistics output, reporting corrupt packs/indexes with matching diagnostics and exit codes; MUST accept packs written by either implementation.
- **FR-026 (`pack-objects`)**: MUST write `PACK` v2 packs (header, per-object records, trailing checksum) plus matching `.idx` behavior expected by `verify-pack`/`index-pack --verify`; delta encoding (window/depth, ofs-delta vs ref-delta selection) MUST decode to identical objects under both implementations; packs written here MUST pass standard Git verification.
- **FR-027 (`index-pack`)**: MUST build (or `--verify`) a `.idx` for a `.pack` (including `--stdin` pack streams), validating magic/version/count, offsets, CRCs, and pack/index checksum agreement; output index bytes MUST be functionally equivalent (accepted by opposite-side verification) and diagnostics MUST match.
- **FR-028 (`unpack-objects`)**: MUST restore loose objects from a pack stream (`-n` dry-run, `-r` recover-damaged handling where standard Git defines it), reporting corrupt streams with matching diagnostics and never fabricating objects.
- **FR-029 (`count-objects`)**: MUST report loose/pack counts and, with `-v`, the full field set (`count`, `size`, `in-pack`, `packs`, `size-pack`, `prune-packable`, `garbage`, `size-garbage`) with identical definitions, units (1024-byte blocks for sizes), and human-readable (`-H`) formatting.
- **FR-030 (`fsck`)**: MUST walk reachability from all refs/HEAD/index, reporting missing, corrupt, dangling, and unreachable objects with the standard message catalog classes and exit-code parity (clean 0, problems non-zero); `--full`, `--no-dangling`, `--unreachable`, `--connectivity-only` selection semantics MUST match; MUST NOT auto-repair.

Discovered related plumbing (pinned by this spec to prevent silent drift):

- **FR-031 (`check-ref-format`)**: Refname validation used by every ref writer MUST agree with `git check-ref-format` accept/reject verdicts and exit codes for all edge names.
- **FR-032 (`show-index`)**: Pack-index dump output (`<offset> <oid> <crc>` lines in pack order) MUST be byte-identical to standard Git for the same `.idx`.
- **FR-033 (`pack-refs`)**: Repacking refs into `packed-refs` (all vs `--all` semantics, peeled-tag handling, loose pruning) MUST leave a ref store that lists identically under both implementations.
- **FR-034 (`stripspace`)**: Whitespace cleanup of messages (trailing space, blank collapsing, comment stripping) used by commit/tag message paths MUST match standard Git so IDs derived from cleaned messages agree.
- **FR-035 (`merge-base`, `diff-tree`/`diff-files`/`diff-index` read paths)**: Reachability and tree-comparison inputs consumed by plumbing walkers MUST agree with standard Git membership (-octopus/independent/ancestor verdicts, tree-diff file sets); where full merge-ort porcelain is out of scope, the plumbing read paths used by scripts MUST still match.
- **FR-036 (registration)**: Every ported plumbing command MUST be registered in the command dispatcher and the `t/`-suite shim router with a byte-identical differential (crosswise) suite covering stdout/stderr/exit-code parity.

### Key Entities

- **Blob**: Raw content bytes with a `blob` header; hashed, compressed, and addressed by ID; the unit handled by `hash-object`, `cat-file`, `unpack-file`.
- **Tree**: Sorted `mode name NUL oid` entry list; built by `mktree`/`write-tree`, listed by `ls-tree`, materialized by `read-tree`.
- **Commit**: `tree` + `parent`* + author/committer/message structure; created by `commit-tree`, walked by `rev-list`, validated by `verify-commit`/`fsck`.
- **Tag object**: `object`/`type`/`tag`/`tagger`/message structure; created by `mktag`, validated by `verify-tag`, dereferenced with `^{}` peeling.
- **Index**: Staged (path, stage) entry set; written by `update-index`/`read-tree`, read by `ls-files`/`write-tree`.
- **Ref**: Name-to-ID mapping (loose file, `packed-refs` line, or symref); mutated by `update-ref`/`symbolic-ref`, listed by `show-ref`/`for-each-ref`, resolved by `rev-parse`.
- **Symbolic ref**: `ref: <target>` pointer (notably `HEAD`); resolved recursively with bounded depth.
- **Revision expression**: Text (`HEAD`, branch, tag, hex prefix, `A..B`, `<rev>:<path>`, `~`/`^`) resolved to an ID by `rev-parse`/`rev-list` inputs.
- **Packfile / pack index**: Bulk object container (`PACK` v2) plus random-access index; written by `pack-objects`, indexed by `index-pack`, verified by `verify-pack`, expanded by `unpack-objects`.
- **Verification verdict**: Machine-readable validity outcome (valid/invalid/missing/corrupt) with a defined exit-code class, produced by `cat-file -e`, `verify-commit`/`verify-tag`, `verify-pack`, `index-pack --verify`, `fsck`.
- **Plumbing output contract**: The stable, script-parseable subset of command output (IDs, listings, counts, verdicts) that porcelain builds upon; distinct from human-oriented porcelain formatting.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A scripted flow (stage files → write tree → create commit → update ref → walk history → verify store) built only from plumbing commands completes in under 5 minutes on a 1000-commit fixture and leaves byte-comparable end states (same commit IDs, same ref values, same verify reports) regardless of which implementation provides the plumbing.
- **SC-002**: 100% of the in-scope plumbing commands produce byte-identical stdout/stderr/exit codes to standard Git across a shared differential corpus of at least 200 cases (healthy, empty, corrupt, and edge-case inputs per command family).
- **SC-003**: Artifacts cross over cleanly in both directions on first attempt for at least 95% of fixtures: objects, trees, commits, tags, packs, indexes written by one implementation verify and read under the other without repair steps.
- **SC-004**: 90% of operators complete a plumbing-composed task (e.g. "hash content, build a tree, commit it, point a branch at it, list it back") on first attempt using only plumbing documentation, without dropping to porcelain or reading source code.
- **SC-005**: Zero silent data-loss events across the corruption corpus: every corrupt/missing/dangling fixture is reported (not hidden) with a matching exit-code class, and no failed write leaves a half-written object, ref, index, or pack visible to readers.
- **SC-006**: The committed `t/`-suite scoreboard shows no regression on any plumbing-gated test script after landing, and every newly ported command adds its differential suite plus shim registration.

## Assumptions

- Standard Git (C git at the vendored version; `t/` suite as oracle on disagreements) defines correct behavior, including exit-code classes (usage `129`, fatal `128`, general error `1`) and output formats.
- SHA-1 is the default algorithm matrix; SHA-256 coverage applies to width-sensitive paths (IDs, fanout paths, trailers); cross-algorithm translation is out of scope.
- Identity/date handling is UTC-based per the port's documented deviation; signed-verification scope is verdict parity without keyring/network features.
- Delta-encoding choices (window/depth/base selection) may differ internally as long as packs verify crosswise and decode to identical objects.
- Network/transport (`fetch`/`push`/`clone`, smart HTTP/SSH), interactive porcelain (`add -p`, editors, hooks, `gc` automation), and non-Git VCS bridges are out of scope; only the plumbing read/write paths they would consume are pinned here.
- Performance targets are set at planning time; byte-compatibility and atomicity gate before optimization.
