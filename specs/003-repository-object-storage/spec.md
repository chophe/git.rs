# Feature Specification: Repository Storage and Object Model

**Feature Branch**: `003-repository-object-storage`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a detailed specification for git.rs repository storage and Git's object model. Focus on behavioral compatibility with Git. Specify: repository discovery, .git directory and worktree layouts, bare repositories, HEAD, refs, symbolic refs, packed refs, loose objects, object IDs, SHA-1 compatibility, SHA-256 repository support where applicable, blob objects, tree objects, commit objects, tag objects, object headers, object serialization, hashing, zlib compression, object existence and lookup, object corruption handling, alternate object databases, quarantine/object isolation where relevant, object database maintenance, packfiles, index files for packfiles, delta objects, reachability, object traversal. For every feature specify: expected behavior, input/output format, compatibility requirements, error behavior, security/corruption considerations, interoperability requirements with standard Git, required tests. Use the existing repository implementation as context, but validate assumptions against Git's documented behavior and existing tests."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Open and work inside any repository layout (Priority: P1)

A user runs a repository command (for example, looking up an object or resolving a ref) from any directory inside a work tree, inside the `.git` directory itself, or inside a bare repository, and the system finds and opens the correct repository with the correct work tree, common directory, and hash algorithm.

**Why this priority**: Nothing else works without discovery. Every command depends on finding the repository, distinguishing bare from non-bare, honoring environment overrides, and selecting the right object format. Incompatibilities here break all crosswise interoperation.

**Independent Test**: Can be fully tested by creating fixtures (plain, bare, linked worktree, `gitdir:` pointer file, `commondir` redirect, environment-variable overrides) with standard Git and opening each of them from multiple working directories, comparing discovered paths, bare flag, work-tree presence, and reported errors byte-for-byte.

**Acceptance Scenarios**:

1. **Given** a standard work tree with a `.git` directory, **When** the user runs a command from the top level or any nested subdirectory, **Then** the system locates the same `.git` directory, reports the top-level work tree, and uses the repository config.
2. **Given** a bare repository, **When** the user runs a command with the working directory at the repository root, **Then** the system reports bare status, reports no work tree, and reads objects and refs from the root.
3. **Given** a `GIT_DIR` override pointing at a valid repository, **When** the user runs a command from an unrelated directory, **Then** the system uses exactly that directory and echoes it back in directory-reporting operations.
4. **Given** a `GIT_DIR` override pointing at a non-repository, **When** the user runs a command, **Then** the system fails with a "not a git repository" diagnostic and a non-zero exit.
5. **Given** a linked work tree whose `.git` is a `gitdir:` pointer file, **When** the user runs a command inside it, **Then** the system follows the pointer, resolves the shared common directory via the `commondir` file, and reads shared refs/objects from the common directory.

---

### User Story 2 - Store and retrieve versioned content by ID (Priority: P1)

A user stores file contents, directory listings, commits, and tags, and later retrieves exactly the same bytes by object ID. Hashing, serialization, compression, and on-disk loose-object layout are byte-compatible with standard Git so repositories can be shared freely.

**Why this priority**: The object model is the core contract. Blobs, trees, commits, tags, headers, hashing, and zlib must round-trip identically or nothing interoperates.

**Independent Test**: Can be fully tested by writing each object kind with both implementations on identical inputs and asserting identical object IDs, identical on-disk bytes, and successful cross-reading (`cat-file`-style reads in both directions plus `fsck`-style validation).

**Acceptance Scenarios**:

1. **Given** arbitrary file bytes, **When** the user stores them as a blob, **Then** the returned ID equals the standard Git ID for the same bytes and the stored file is readable by standard Git.
2. **Given** an object written by standard Git (loose), **When** the user reads it by full or unambiguous abbreviated ID, **Then** the system returns the exact type and bytes standard Git returns.
3. **Given** a tree, commit, or tag with canonical fields, **When** the user stores and re-reads it, **Then** the bytes, parsed fields, and ID are identical to standard Git's.
4. **Given** a corrupt object (bad header, size mismatch, bad zlib, hash mismatch), **When** the user reads it, **Then** the system reports corruption naming the object and refuses to return fabricated content.

---

### User Story 3 - Resolve branches, tags, and HEAD exactly like Git (Priority: P1)

A user reads and follows `HEAD`, lightweight and annotated refs, symbolic refs, and packed refs, including detached `HEAD` and unborn branches, and observes the same resolution, listing order, and error behavior as standard Git.

**Why this priority**: Refs are how users name history. `HEAD` semantics, symref chains, packed-refs fallback, and refname validation directly determine checkout, log, and status behavior.

**Independent Test**: Can be fully tested by resolving and listing refs in fixture repositories (loose-only, packed-only, mixed with loose-overrides-packed, symref chains, detached `HEAD`, unborn `HEAD`, invalid names) and comparing resolved IDs, symbolic targets, listing order, and diagnostics.

**Acceptance Scenarios**:

1. **Given** `HEAD` pointing at `refs/heads/main`, **When** the user resolves `HEAD`, **Then** the system follows the chain and returns the same commit ID standard Git returns.
2. **Given** a detached `HEAD` containing a raw ID, **When** the user resolves `HEAD`, **Then** the system returns that ID directly.
3. **Given** a ref present in both loose and packed storage, **When** the user resolves it, **Then** the loose value wins, matching standard Git.
4. **Given** an invalid refname, **When** the user creates or resolves it, **Then** the system rejects it with the same validity rules as `git check-ref-format`.

---

### User Story 4 - Scale history with packs, deltas, and traversal (Priority: P2)

A user works with repositories whose history is packed (including delta-compressed packs with index files), walks reachable history, and checks connectivity, observing identical object visibility and walk results as standard Git.

**Why this priority**: Real repositories are packed. Pack layout, index lookup, delta resolution, reachability, and traversal determine whether log, fetch-sized reads, and integrity checks agree.

**Independent Test**: Can be fully tested by packing fixtures with standard Git (plain, delta, multi-pack) and asserting byte-compatible pack/index acceptance, identical object reads through deltas, and identical reachable/dangling reports and walk orders.

**Acceptance Scenarios**:

1. **Given** a repository with packfiles created by standard Git, **When** the user reads any packed object by ID, **Then** the returned type and bytes match standard Git, including objects stored as deltas.
2. **Given** packfiles created by this system, **When** standard Git verifies them, **Then** verification succeeds and standard Git reads every object.
3. **Given** a commit graph with merges, **When** the user walks history from given starting points, **Then** each reachable commit appears exactly once and unreachable/corrupt-missing objects are reported with matching diagnostics and exit codes.
4. **Given** an abbreviated ID, **When** the user looks it up, **Then** ambiguity (no match, unique match, multiple matches) behaves exactly like standard Git.

---

### User Story 5 - Survive corruption, isolation, and maintenance safely (Priority: P2)

A user encountering corrupt data, alternate object stores, in-flight quarantined objects, or housekeeping (prune/repack/garbage accounting) sees safe behavior: corruption is reported not hidden, alternates are read-only fallbacks, quarantined objects stay isolated until accepted, and maintenance never loses reachable data.

**Why this priority**: Integrity is a trust boundary. Silent corruption acceptance, writing into shared alternates, leaking quarantined objects, or deleting reachable objects are data-loss defects.

**Independent Test**: Can be fully tested with corrupted fixtures (truncated, bit-flipped, mistyped), alternate-store fixtures, simulated quarantine directories, and prune/expire scenarios, comparing diagnostics, exit codes, object visibility, and post-maintenance integrity reports.

**Acceptance Scenarios**:

1. **Given** a corrupt loose or packed object, **When** the user reads or verifies it, **Then** the system reports which object is corrupt and why, with the same exit-code class as standard Git.
2. **Given** an `info/alternates` entry, **When** the user reads an object present only in the alternate, **Then** the read succeeds but writes never modify the alternate.
3. **Given** quarantined incoming objects, **When** the user performs normal reads, **Then** quarantined objects remain invisible until the owning operation accepts them.
4. **Given** a prune/expire operation, **When** it completes, **Then** all reachable objects remain readable and all remaining ones still verify.

---

### User Story 6 - Operate in SHA-256 repositories (Priority: P3)

A user working in a SHA-256 (`objectFormat=sha256`) repository stores, reads, and verifies objects with 64-hex IDs and the SHA-256 empty tree/blob constants, interoperating with standard Git SHA-256 repositories.

**Why this priority**: SHA-256 is the forward-compatibility path. The scope here is repository-local correctness and interop, not cross-algorithm translation.

**Independent Test**: Can be fully tested by initializing SHA-256 fixtures with standard Git and asserting identical IDs, paths, empty-object constants, and verification outcomes in both directions.

**Acceptance Scenarios**:

1. **Given** a SHA-256 repository, **When** the user stores a blob, **Then** the ID is the 64-hex SHA-256 of the canonical serialization and standard Git reads it.
2. **Given** a SHA-1 and a SHA-256 repository side by side, **When** the user opens each, **Then** each uses its own algorithm, ID length, and path fanout without cross-contamination.

---

### Edge Cases

- Discovery from inside `.git/` itself, from a bare repo subdirectory, and from outside any repository (ceiling / filesystem-boundary behavior matches standard Git, including `GIT_CEILING_DIRECTORIES` and discovery-across-filesystem stops).
- `.git` is a file (`gitdir: <path>`) with relative vs absolute target, trailing whitespace/newline handling, and missing-target errors.
- `commondir` redirect present vs absent; `GIT_COMMON_DIR` override.
- Unborn `HEAD` (points at nonexistent ref): resolution reports unborn state rather than a hard error where standard Git does.
- Symref loops and depth overflow: resolution fails safely instead of recursing forever.
- `packed-refs` missing, empty, header-only, unsorted, or containing peeled `^{}` lines and comments.
- Refname edge rules: `..`, control chars, `~^:?*[`, leading/trailing slash or dot, `@{`, consecutive slashes, per `git check-ref-format`.
- Empty blob / empty tree IDs per algorithm; zero-length commit messages; top-level tree with zero entries vs missing tree.
- Tree entries with special names (spaces, newlines, quotes, non-UTF8 bytes, submodules at mode `160000`, symlinks at `120000`).
- Object size field with leading zeros, overflow, negative, or trailing garbage.
- zlib streams that are truncated, over-long (trailing bytes), or use unexpected compression levels.
- Abbreviated IDs shorter than 4 hex chars, ambiguous prefixes, and prefixes that match both loose and packed objects.
- Packfiles with zero objects, maximum-count headers, 64-bit offsets, corrupted trailer checksum, or delta chains referencing missing bases or forming cycles.
- Alternates entries that are comments, blank lines, relative paths, recursive alternates, or point at missing directories.
- Quarantine directory present but empty, missing, or containing objects that collide with accepted ones.
- Concurrent writer scenarios: reader never observes a half-written loose object, ref, or `packed-refs` (atomic temp-file + rename discipline).
- Case-insensitive filesystems and non-UTF8 paths behave like standard Git (no silent ref aliasing).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST discover repositories by walking upward from the working directory for a `.git` entry, honoring `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_CEILING_DIRECTORIES`, and filesystem-boundary stops exactly like standard Git.
- **FR-002**: The system MUST support both `.git` directories and `.git` pointer files (`gitdir: <path>`), including relative targets resolved against the pointer's location.
- **FR-003**: The system MUST resolve the common directory via the `commondir` file and `GIT_COMMON_DIR`, defaulting to the git directory itself when absent.
- **FR-004**: The system MUST distinguish bare from non-bare repositories using config (`core.bare`) and layout, reporting no work tree for bare repositories.
- **FR-005**: The system MUST select the hash algorithm from config (`extensions.objectFormat`), defaulting to SHA-1 and supporting SHA-256 repositories.
- **FR-006**: The system MUST read `HEAD` as either a symbolic ref (`ref: <target>`) or a detached raw object ID, and MUST report unborn `HEAD` distinctly from corrupt `HEAD`.
- **FR-007**: The system MUST resolve symbolic refs recursively with a bounded depth, detecting loops and overflow as errors rather than recursing unboundedly.
- **FR-008**: The system MUST validate refnames with the same rules as `git check-ref-format` and reject invalid names on create, update, and lookup paths.
- **FR-009**: The system MUST implement loose refs (one file per ref containing `<hex>\n` or `ref: <target>`) and `packed-refs` (header, sorted `<oid> <refname>` lines, optional `^{}` peeled lines), with loose values overriding packed ones.
- **FR-010**: The system MUST list refs in byte-sorted refname order with loose-overrides-packed merge semantics.
- **FR-011**: The system MUST update refs atomically (lock file + rename) and MUST never leave half-written ref files visible to readers.
- **FR-012**: The system MUST store loose objects at `objects/xx/<rest>` with zlib-compressed `<type> <size>\0<content>` payloads, creating fanout directories as needed.
- **FR-013**: The system MUST write loose objects atomically (temp file + fsync + rename) with read-only-safe permissions, so concurrent readers never see partial objects.
- **FR-014**: The system MUST parse and enforce object headers strictly: type MUST be exactly one of `blob`, `tree`, `commit`, `tag`; size MUST be a canonical decimal integer matching the payload length.
- **FR-015**: The system MUST serialize blobs as raw content bytes, trees as sorted `mode name NUL oid` entry sequences, commits with `tree`/`parent`* + author/committer/message structure, and tags with `object`/`type`/`tag`/`tagger` + message structure, byte-identical to standard Git.
- **FR-016**: The system MUST sort tree entries with standard Git ordering (byte order with directory-name trailing-slash comparison) and preserve it on write.
- **FR-017**: The system MUST compute object IDs as the selected algorithm's hash over the exact serialized `header + content` bytes.
- **FR-018**: The system MUST apply collision-detecting SHA-1 semantics: known collision-attack inputs MUST be rejected with a collision diagnostic, not silently accepted.
- **FR-019**: The system MUST compress loose objects with zlib and decompress strictly, rejecting truncated, over-long, or checksum-invalid streams as corruption.
- **FR-020**: The system MUST resolve full IDs, unambiguous abbreviations (minimum 4 hex chars), and-ref/HEAD expressions to unique objects, reporting not-found vs ambiguous distinctly.
- **FR-021**: The system MUST search objects in order: primary loose store, then packfiles (newest-first or index order matching standard Git), then alternate stores; writes MUST go only to the primary store.
- **FR-022**: The system MUST report missing objects, unknown types, and corrupt payloads with distinct diagnostics and standard-Git-compatible exit-code classes (success 0, not-found/missing non-zero, usage errors 129 where applicable).
- **FR-023**: The system MUST read `objects/info/alternates` (skipping blanks and `#` comments, resolving relative paths against the primary objects directory) plus the alternate-directories environment variable, treating alternates as read-only.
- **FR-024**: The system MUST isolate quarantined incoming objects from normal reads until the owning operation accepts them, and MUST clean them up on abort.
- **FR-025**: The system MUST read packfiles with `PACK` magic, version 2, object count, per-object type/size encoding, and trailing checksum, supporting undeltified, offset-delta, and reference-delta objects.
- **FR-026**: The system MUST read version-2 pack index files (magic, fanout, sorted ID table, CRC32, 31-bit + 64-bit offsets, pack + index checksums) and MUST reject index/pack checksum mismatches as corruption.
- **FR-027**: The system MUST resolve delta chains recursively (base size + result size headers, copy/insert instructions), enforcing depth/cycle/size limits and reporting missing bases as corruption, not truncation.
- **FR-028**: The system MUST determine reachability from starting points through commit parents, tree entries, and tag targets without revisiting objects, and MUST classify unreachable objects as dangling vs corrupt distinctly.
- **FR-029**: The system MUST traverse history in commit, topological, and date orders without duplication or infinite loops on merges, matching standard Git walk membership.
- **FR-030**: The system MUST perform maintenance accounting (loose/pack counts and sizes, garbage detection for unrecognized files, prune of unreachable-loose-expired objects, repack consolidation) without deleting reachable objects.
- **FR-031**: The system MUST support SHA-256 repositories end-to-end (64-hex IDs, 32-byte raw IDs, SHA-256 empty blob/tree constants, 32-byte tree-entry OIDs, 32-byte pack trailer) for all features above.
- **FR-032**: The system MUST interoperate crosswise with standard Git: artifacts written by either implementation MUST be readable and verifiable by the other (`fsck`-style verification, pack/index verification, ref listing agreement).

### Key Entities *(include if feature involves data)*

- **Repository**: Discovered root (git directory + common directory + optional work tree), bare flag, selected hash algorithm, merged config. Identity for all storage paths.
- **Git Directory**: The `.git` directory (or bare root) holding `HEAD`, `config`, `objects/`, `refs/`, `packed-refs`, `index`, `logs/`, `info/`, `hooks/`, `worktrees/`.
- **Work Tree**: Checked-out files associated with a non-bare repository; main work tree plus linked work trees via `gitdir:` pointers.
- **Common Directory**: Shared storage for linked work trees (refs, objects, config); located via `commondir` indirection.
- **HEAD**: Either `ref: <branch>` (attached, possibly unborn) or a raw object ID (detached).
- **Ref**: A name (`refs/heads/*`, `refs/tags/*`, `refs/remotes/*`, others) mapping to an object ID; stored loose, packed, or both.
- **Symbolic Ref**: A ref whose content is `ref: <target>`; resolved by recursive following.
- **Packed-Refs File**: Sorted flat file of `<oid> <refname>` lines with optional peeled `^{}` continuation lines and capability header.
- **Object ID**: Hex (40-char SHA-1 / 64-char SHA-256) and raw (20/32-byte) identifier; the hash of the canonical serialization. Includes null ID and abbreviated-prefix forms.
- **Blob**: Raw file-content bytes wrapped with a `blob` header.
- **Tree**: Sorted list of entries; each entry carries mode (`100644`, `100755`, `120000`, `040000`, `160000`), filename bytes, and child object ID.
- **Commit**: `tree` line, zero or more `parent` lines, author/committer identity + timestamp + zone lines, optional headers (encoding, signatures, mergetag), blank line, message bytes.
- **Tag**: `object` + `type` + `tag` + `tagger` lines, blank line, message bytes; annotated tags point at any object kind with peeled refs.
- **Loose Object**: One zlib-compressed `header + content` file at the fanout path.
- **Packfile**: `PACK` v2 container of undeltified and deltified objects plus trailing checksum.
- **Pack Index**: v2 random-access index (fanout, sorted IDs, CRCs, offsets, checksums) for a packfile.
- **Delta**: Offset-delta or reference-delta record plus instruction stream resolving against a base object.
- **Alternate Store**: Additional read-only object directory listed in `info/alternates` or the environment.
- **Quarantine Directory**: Temporary incoming-object area invisible to normal reads until promoted.
- **Reachability Closure**: The set of objects reachable from given roots through parents, trees, and tag targets.

## Detailed Behavioral Specification

Each item below uses the same seven lenses: expected behavior; input/output format; compatibility requirements; error behavior; security/corruption considerations; interoperability requirements; required tests.

### 1. Repository discovery

- **Expected behavior**: Walk from the working directory upward looking for `.git` (directory or `gitdir:` file); stop at ceiling directories and filesystem boundaries as configured; honor `GIT_DIR` (verbatim echo in directory reporting), `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_CEILING_DIRECTORIES`, and discovery-across-filesystem flags.
- **Input/output format**: Input: working directory + environment/flag overrides. Output: canonical git directory, common directory, work tree (or none), bare flag, algorithm, merged config; directory-reporting operations echo the user-supplied `GIT_DIR` verbatim.
- **Compatibility requirements**: Directory chosen, bare detection, `gitdir:` following, `commondir` resolution, and "not a git repository" diagnostics MUST match standard Git, including discovery from inside `.git/` and from bare subdirectories.
- **Error behavior**: Outside any repository: `fatal: not a git repository (or any of the parent directories): .git` class diagnostic, non-zero exit. Explicit bad `GIT_DIR`: `fatal: not a git repository: '<path>'`. Missing `gitdir:` target: fatal with target path named.
- **Security/corruption considerations**: Malformed `gitdir:`/`commondir` pointers MUST NOT cause directory traversal outside the intended resolution (no silent fallback to `/`); unreadable directories are errors, not empty-repo fabrications.
- **Interoperability requirements**: Repositories initialized by either implementation MUST be discoverable by the other from every nested path.
- **Required tests**: Fixture matrix (plain, bare, `gitdir:` relative/absolute, `commondir`, env overrides, ceiling dirs, outside-repo) asserting discovered paths/bare/work-tree/diagnostics byte-identical to standard Git.

### 2. `.git` directory and worktree layouts

- **Expected behavior**: Recognize and preserve the standard layout: `HEAD`, `config`, `objects/` (+`pack/`,`info/`), `refs/` (+`heads/`,`tags/`), `packed-refs`, `index`, `logs/`, `hooks/`, `info/`, `worktrees/<name>/{gitdir,commondir,HEAD,index,logs/}`; linked work trees keep per-worktree `HEAD`/`index` and share the common store.
- **Input/output format**: On-disk files/directories with documented text formats (`HEAD`, `gitdir:`, `commondir`); no reorganization or renaming of standard paths.
- **Compatibility requirements**: Layout created by initialization MUST be accepted by standard Git and vice versa; per-worktree vs common file placement MUST match (`HEAD` per worktree, refs/objects shared).
- **Error behavior**: Missing `HEAD`/`objects`/`refs` in an explicit directory: "not a git repository". Missing per-worktree files: worktree-scoped error naming the worktree.
- **Security/corruption considerations**: MUST NOT follow unexpected symlinks to rewrite shared state; MUST NOT create world-writable control files; template/hook copying MUST NOT execute anything.
- **Interoperability requirements**: `git worktree list`-equivalent state agreed; standard Git status/log inside linked worktrees works on this layout and vice versa.
- **Required tests**: Layout snapshot tests for init variants (plain, bare, linked worktree, separate-git-dir) diffed against standard Git; worktree pointer-following tests.

### 3. Bare repositories

- **Expected behavior**: A bare repository stores `HEAD`, `objects/`, `refs/`, `config` (with `core.bare=true`) at its root, has no work tree, and refuses work-tree operations with the standard diagnostic.
- **Input/output format**: Same control files as `.git` but at root; `rev-parse --is-bare-repository`-style queries report true; directory reports reflect root.
- **Compatibility requirements**: Bare detection and work-tree refusal diagnostics MUST match standard Git; cloning/fetching into bare MUST place refs identically.
- **Error behavior**: Work-tree-dependent operations: standard "this operation must be run in a work tree" class error, non-zero exit.
- **Security/corruption considerations**: MUST NOT implicitly create a work tree or write index/worktree files into a bare root.
- **Interoperability requirements**: Bare repos created by either side MUST serve as push/fetch sources for the other.
- **Required tests**: Bare fixture tests for discovery flags, refusal messages, and ref/object writes.

### 4. HEAD

- **Expected behavior**: `HEAD` is either `ref: refs/heads/<branch>\n` (attached; branch may not exist yet = unborn) or `<hex>\n` (detached). Resolution follows the symref chain to a commit ID; unborn is a distinct state (branch name known, no ID).
- **Input/output format**: File bytes exactly `ref: <target>\n` or `<hex>\n` (single line, LF-terminated). Queries report symbolic target vs raw ID distinctly.
- **Compatibility requirements**: Attached/detached/unborn reporting, and the bytes written on branch-switch/detach/unborn-init, MUST match standard Git.
- **Error behavior**: Missing `HEAD`: corrupt/missing diagnostic. Malformed content (neither `ref:` nor hex): corrupt-HEAD error naming the file. Unborn: operations needing a commit report unborn-branch diagnostics, not "bad object".
- **Security/corruption considerations**: Overlong or NUL-containing `HEAD` MUST be rejected, not truncated-and-followed.
- **Interoperability requirements**: Switching/detaching with either implementation MUST be readable by the other.
- **Required tests**: Attached, detached, unborn, missing, and malformed `HEAD` fixtures with resolution + diagnostics compared to standard Git.

### 5. Refs (files backend)

- **Expected behavior**: Loose ref = file with `<hex>\n`; hierarchical names under `refs/`; listing merges packed + loose with loose winning, sorted by refname bytes.
- **Input/output format**: File content `<lowercase-hex>\n` (40/64 chars per algorithm). Listings emit `<oid> <refname>` sorted.
- **Compatibility requirements**: Storage paths, file bytes, listing order, and loose-wins semantics MUST match; `check-ref-format` validity rules enforced identically.
- **Error behavior**: Invalid name: `fatal: '<name>' is not a valid ref name`. Missing ref on resolve: not-found (distinct from corrupt). Dangling ref (points at missing object): reported as broken on verification, resolution yields ID but object read fails as missing.
- **Security/corruption considerations**: Ref updates MUST be atomic (write temp + rename; never truncate in place); `D/F` conflicts (file vs directory at same prefix) MUST be handled without deleting unrelated refs; locking failures MUST NOT corrupt the previous value.
- **Interoperability requirements**: Branch/tag create/delete/update by either side MUST be listed and resolved identically by the other.
- **Required tests**: Create/update/delete5217? No — create/update/delete/resolve/list matrix incl. `D/F` conflicts, invalid names, dangling refs, compared to standard Git.

### 6. Symbolic refs

- **Expected behavior**: Content `ref: <target>\n`; resolution follows chains (including `HEAD` and nested symrefs) up to a fixed depth (matching standard Git's limit), returning the final ID; intermediate targets reported by symbolic queries.
- **Input/output format**: Same single-line `ref:` format; symbolic-target queries return the immediate target string.
- **Compatibility requirements**: Chain following, depth limit, and loop diagnostics MUST match standard Git (`symbolic-ref`, `rev-parse --symbolic-full-name` families).
- **Error behavior**: Loop or depth overflow: symbolic-resolution failure diagnostic (no stack overflow, no hang). Dangling intermediate: resolution fails naming the missing link.
- **Security/corruption considerations**: Depth MUST be bounded; non-`ref:`/non-hex content is corruption, not a default branch.
- **Interoperability requirements**: Symrefs written by either side (including `HEAD`) resolve identically on the other.
- **Required tests**: Chain (1..N), loop (2-cycle, self-cycle), depth-overflow, dangling-link, and detached-intermediate fixtures.

### 7. Packed refs

- **Expected behavior**: `packed-refs` starts with `# pack-refs with: peeled fully-sorted sorted` (capabilities vary by writer but peeled/sorted MUST be honored when present); each entry `<oid> <refname>\n` optionally followed by `^{<peeled-oid>}\n` for tags; file sorted by refname; loose entries shadow packed ones.
- **Input/output format**: Exact text format above; trailing newline required; `#` comment/blank handling like standard Git.
- **Compatibility requirements**: Parsing (header tolerance, peeled lines, sorting assumption), loose-wins merge, and post-repack state (packed + pruned loose) MUST match.
- **Error behavior**: Malformed line: packed-refs corrupt diagnostic identifying the line; conflicting duplicate entries: first/loose-wins deterministically, never random. Missing file: treated as empty, not an error.
- **Security/corruption considerations**: Unsorted or hostile large files MUST NOT cause ambiguous resolution; overlong lines MUST be rejected.
- **Interoperability requirements**: Repack/pack-refs output from either side MUST be listed identically by the other, including peeled tags.
- **Required tests**: Header-only, empty, missing, sorted, unsorted, peeled, duplicate, malformed-line, and loose-shadow fixtures.

### 8. Loose objects

- **Expected behavior**: Path `objects/<2-hex>/<38-or-62-hex>`; content = zlib(`<type> <size>\0<content>`); fanout directories created on demand.
- **Input/output format**: Binary zlib stream with ASCII header; file permissions honor umask with no group/other write; object directories `0755`-class.
- **Compatibility requirements**: Paths, header bytes, compression acceptance (any valid zlib level), and cross-reads MUST match; temp-file naming MUST NOT collide with real object paths.
- **Error behavior**: Missing file: not-found. Bad zlib/header/size: corrupt-object diagnostic naming the ID. I/O failure mid-read: I/O error distinct from corruption.
- **Security/corruption considerations**: Writes MUST be atomic (temp in same directory + fsync + rename); readers MUST verify decompressed size equals header size and SHOULD verify ID hash on read paths used for verification; MUST NOT accept `objects/xx/` entries with wrong-length names as objects.
- **Interoperability requirements**: Every loose object written by either side MUST be listed, read, and verified by the other.
- **Required tests**: Round-trip matrix per kind/size (empty, 1-byte, large, binary), permission checks, concurrent-write atomicity, and bad-header/bad-zlib/size-mismatch fixtures.

### 9. Object IDs

- **Expected behavior**: Canonical lowercase hex (40 SHA-1 / 64 SHA-256); raw 20/32 bytes; null ID (all zeros); well-known empty blob/tree IDs per algorithm; abbreviation with minimum 4 hex chars and uniqueness requirement.
- **Input/output format**: Hex strings (strict `[0-9a-f]`, even length for raw conversions); abbreviated forms resolved against the store.
- **Compatibility requirements**: Hex casing, length validation, null/empty constants, and abbreviation/ambiguity rules MUST match standard Git.
- **Error behavior**: Bad hex: `fatal: not a valid object name <input>`. Ambiguous prefix: `error: short object ID <prefix> is ambiguous` + hint, non-zero exit. No match: `fatal: bad object <input>`.
- **Security/corruption considerations**: Uppercase or whitespace-padded IDs MUST NOT be silently normalized into different objects; short IDs below minimum MUST be rejected.
- **Interoperability requirements**: IDs computed by either side for identical content MUST be equal; abbreviations unambiguous in one MUST resolve the same in the other on the same store.
- **Required tests**: Hex validation, null/empty constants per algorithm, and abbreviation matrix (unique, ambiguous, missing, too-short) vs standard Git.

### 10. SHA-1 compatibility (incl. collision detection)

- **Expected behavior**: SHA-1 is the default algorithm; all hashing paths use collision-detecting SHA-1; known collision-attack payloads (SHAttered-class) are rejected with the standard collision diagnostic.
- **Input/output format**: 40-hex / 20-byte IDs; collision error names the colliding hex digest.
- **Compatibility requirements**: Digests MUST equal standard Git (sha1dc) outputs; collision rejection message and failure mode MUST match (`t0013-sha1dc`-class behavior).
- **Error behavior**: Collision input on hash/store: hard failure with collision message; object MUST NOT be stored or referenced.
- **Security/corruption considerations**: This is a security boundary: MUST NOT fall back to non-detecting SHA-1; MUST thread detection through hashing, loose writes, and verification reads.
- **Interoperability requirements**: Non-colliding objects MUST hash identically to standard Git; colliding fixtures MUST be rejected by both.
- **Required tests**: SHAttered-PDF-class collision fixture (rejected with expected digest), normal vectors (NIST + Git-specific), and hash-vs-standard-Git differential tests.

### 11. SHA-256 repository support

- **Expected behavior**: When `extensions.objectFormat=sha256`, the repository uses 64-hex/32-byte IDs, SHA-256 hashing, SHA-256 empty blob/tree constants, 32-byte tree-entry OIDs and pack trailers end-to-end.
- **Input/output format**: 64-hex strings; on-disk formats identical structurally with wider ID fields.
- **Compatibility requirements**: Algorithm selection, ID lengths, constants, and path fanout (still 2-char fanout with longer suffix) MUST match standard Git SHA-256 repos.
- **Error behavior**: SHA-1-length IDs in a SHA-256 repo (and vice versa): invalid-object-name errors. Mixed-algorithm object directories MUST NOT cross-resolve.
- **Security/corruption considerations**: MUST NOT truncate or zero-pad IDs across algorithms; algorithm confusion MUST be an error, not a lookup in the wrong store.
- **Interoperability requirements**: SHA-256 fixtures from standard Git MUST open, read, and verify; objects written here MUST verify under standard Git SHA-256. Cross-algorithm translation (`compatObjectFormat`) is explicitly out of scope for this feature.
- **Required tests**: SHA-256 init/read/write/verify matrix in both directions; algorithm-confusion negative tests.

### 12–15. Blob / tree / commit / tag objects

- **Expected behavior**: See FR-015/FR-016. Blobs preserve exact bytes (including NULs, no newline normalization). Trees carry sorted entries with the five legal modes. Commits carry tree + parents + author/committer lines. Tags carry object/type/tag/tagger + message; tag `type` names the target kind.
- **Input/output format**:
  - Tree entry: `<octal-mode> SP <name-bytes> NUL <raw-oid>`; modes `40000,100644,100755,120000,160000`.
  - Commit: `tree <hex>\n(parent <hex>\n)*author <name> <email> <ts> <tz>\ncommitter <...>\n[<extra-headers>\n]\n<message>`.
  - Tag: `object <hex>\ntype <kind>\ntag <name>\ntagger <ident>\n\n<message>`.
- **Compatibility requirements**: Byte layouts, mode spellings, sort order, identity-line formatting (`name <email> timestamp timezone`), and trailing-newline/message preservation MUST be identical to standard Git.
- **Error behavior**: Unknown mode, unsorted entries, missing `tree` line, bad identity line, unknown tag `type`: corrupt-object diagnostics naming the object and field; strict parsers reject, never repair-and-hash.
- **Security/corruption considerations**: NULs in names/messages, huge sizes, and deeply nested trees MUST be handled without panics/overflows; non-UTF8 names MUST round-trip as bytes.
- **Interoperability requirements**: `mktree`/`commit-tree`/`mktag`-class payloads from either side MUST parse and re-hash identically on the other.
- **Required tests**: Golden vectors per kind (empty, special names/modes, multi-parent, signed headers, non-UTF8), sort-order tests, malformed-field rejection tests, crosswise parse/serialize tests.

### 16. Object headers

- **Expected behavior**: Loose/pack-undeltified payloads begin `<type> <size>\0` where type ∈ {`blob`,`tree`,`commit`,`tag`} and size is the decimal byte length of content.
- **Input/output format**: ASCII `type SP size NUL`; size has no leading `+`/`-`/spaces; canonical writers emit no leading zeros.
- **Compatibility requirements**: Strictness MUST match standard Git: unknown type, missing NUL/space, non-decimal or overflowing size, and size/payload-length mismatch are all corruption.
- **Error behavior**: `malformed object header` / `unknown object type` / `object size mismatch`-class diagnostics naming the object.
- **Security/corruption considerations**: Size overflow MUST NOT cause truncation or heap over-allocation; declared sizes larger than available bytes MUST fail before hashing.
- **Interoperability requirements**: Headers written by either side MUST parse on the other, including boundary sizes (0, 2^32-adjacent where representable).
- **Required tests**: Header fuzz matrix (missing NUL, bad type, bad/overflowing size, mismatch) with diagnostic comparison.

### 17. Object serialization

- **Expected behavior**: Canonical byte serialization per kind (see §12–15); parsing is strict and re-serialization of a parsed object MUST reproduce the original bytes (round-trip identity) for well-formed inputs.
- **Input/output format**: Raw content bytes exactly as hashed (no canonicalization beyond the documented sort/format rules).
- **Compatibility requirements**: Byte-for-byte agreement with standard Git on golden fixtures, including unusual-but-legal inputs (empty messages, extra commit headers in defined order).
- **Error behavior**: Non-canonical but parseable inputs: parsers accept-and-preserve (never silently rewrite the ID); malformed inputs: corrupt errors.
- **Security/corruption considerations**: Parsers MUST NOT panic on arbitrary bytes (property: no-panic on all inputs); recursion/nesting MUST be bounded.
- **Interoperability requirements**: `cat-file -p`-equivalent output agreed; hash-equality on re-serialize.
- **Required tests**: Round-trip property tests, golden byte vectors, no-panic fuzz tests.

### 18. Hashing

- **Expected behavior**: ID = `HASH(header + content)` with the repository algorithm; streaming/incremental hashing equals one-shot hashing.
- **Input/output format**: Binary digest → hex/raw ID; hash-checking variants return collision errors distinctly.
- **Compatibility requirements**: Digests MUST equal standard Git for all fixtures; empty-input constants MUST match per algorithm.
- **Error behavior**: Collision-detected input: collision error (see §10), distinct from I/O errors.
- **Security/corruption considerations**: MUST hash exactly the serialized bytes (no newline translation, no truncation); MUST NOT cache IDs across algorithm changes.
- **Interoperability requirements**: `hash-object`-equivalent outputs identical for identical inputs + type.
- **Required tests**: Known-answer tests, incremental==oneshot property, crosswise hash comparisons.

### 19. zlib compression

- **Expected behavior**: Loose objects and pack undeltified data use zlib (RFC 1950) wrapping deflate; any valid compression level accepted on read; writers use the standard default level.
- **Input/output format**: zlib streams; trailing bytes after the stream end are corruption for loose objects.
- **Compatibility requirements**: Accept everything standard Git accepts; produce bytes standard Git inflates to the same payload.
- **Error behavior**: Truncated/checksum-invalid/over-long streams: corrupt-object errors naming the object.
- **Security/corruption considerations**: Decompression MUST enforce output-size caps derived from the header (no zip-bomb over-allocation); MUST NOT accept concatenated-stream smuggling.
- **Interoperability requirements**: Inflate/deflate crosswise agreement on all sizes, including empty and multi-megabyte payloads.
- **Required tests**: Level matrix, truncation/trailing-byte fixtures, bomb-adjacent size-cap tests, crosswise inflate tests.

### 20. Object existence and lookup

- **Expected behavior**: Unified lookup by full/abbreviated ID across loose + packs + alternates; ref/HEAD expressions resolved before lookup where applicable; existence checks do not fabricate missing objects.
- **Input/output format**: Query: hex/prefix/ref-expression. Outcomes: found (type+bytes), not-found, ambiguous.
- **Compatibility requirements**: Search order, abbreviation rules, and `bad object` vs `ambiguous` diagnostics/exit codes MUST match.
- **Error behavior**: Exit-code classes: 0 found; non-zero not-found/ambiguous with stderr diagnostics; usage errors (bad rev syntax) exit 129 where standard Git does.
- **Security/corruption considerations**: Lookup MUST NOT return a different object on ambiguity; MUST NOT follow alternates into attacker-controlled paths without the configured allowlist semantics.
- **Interoperability requirements**: `cat-file -e`- / `cat-file -t`-equivalent existence/type queries agree on identical stores.
- **Required tests**: Found/loose/packed/alternate/ambiguous/missing matrix vs standard Git, including exit codes and stderr.

### 21. Object corruption handling

- **Expected behavior**: Every read path validates magic/header/type/size/zlib/hash/checksum as applicable and reports the first failure precisely; verification operations scan reachable + dangling objects and summarize.
- **Input/output format**: Diagnostics name the object (`error: corrupt loose object <oid>`, `fatal: bad object <rev>`, pack corruption with pack name + offset) and use standard exit codes.
- **Compatibility requirements**: Error classes, message substrings identifying the object and cause, and exit codes MUST match standard Git families (`fsck`, `verify-pack`, `cat-file`).
- **Error behavior**: Corruption is never auto-repaired; offending objects are reported and (for verification) counted; reads fail closed.
- **Security/corruption considerations**: Bit-flips, truncation, type confusion, and size lies MUST all be detected; error paths MUST NOT leak uninitialized bytes or panic.
- **Interoperability requirements**: Corrupted fixtures MUST be flagged by both implementations; healthy fixtures MUST verify clean on both.
- **Required tests**: Corruption corpus (bit-flip, truncate, header lie, zlib damage, checksum damage, delta-base damage) with diagnostic + exit-code comparison.

### 22. Alternate object databases

- **Expected behavior**: `objects/info/alternates` (one path per line; `#` comments and blanks ignored; relative resolved against primary `objects/`) plus the alternate-directories environment variable extend the read path; alternates are strictly read-only.
- **Input/output format**: Text lines as above; borrowed objects usable for reads/traversal/verification but never garbage-collected or written through.
- **Compatibility requirements**: File parsing, relative resolution, read-fallback order, and no-write-through MUST match standard Git.
- **Error behavior**: Missing alternate directory: ignored-or-warned per standard Git behavior, never fatal for unrelated reads; recursive alternates handled with cycle protection.
- **Security/corruption considerations**: MUST NOT write into alternates; MUST NOT follow alternates outside configured roots silently; corrupt objects in alternates reported as corrupt, not skipped.
- **Interoperability requirements**: Repos borrowing from alternates created by either side resolve identically.
- **Required tests**: Relative/absolute/comment/blank/recursive/missing alternate fixtures; write-isolation tests (alternate mtime/bytes unchanged).

### 23. Quarantine / object isolation

- **Expected behavior**: Incoming objects (fetch/receive-class flows) land in a quarantine/temporary incoming directory; normal reads exclude quarantine; accept promotes (rename/link into place), abort removes.
- **Input/output format**: Internal directory (environment/owner-specified); promotion is atomic per object.
- **Compatibility requirements**: Visibility rules (invisible until accepted) and cleanup-on-abort MUST match standard Git quarantine semantics.
- **Error behavior**: Failed acceptance: quarantined objects discarded, primary store untouched; readers never observe half-promoted state.
- **Security/corruption considerations**: Untrusted incoming bytes MUST NOT become reachable until validated; quarantine MUST NOT be on the normal search path.
- **Interoperability requirements**: Accepted quarantines MUST verify under standard Git; aborted ones MUST leave identical stores.
- **Required tests**: Visibility tests (pre-accept invisible, post-accept visible, post-abort absent) and failure-injection tests.

### 24. Object database maintenance

- **Expected behavior**: Counting (loose count + disk size, pack count + size, garbage identification for unrecognized files), pruning of expired unreachable loose objects, repack consolidation, and safe garbage reporting — none of which removes reachable objects.
- **Input/output format**: Count/size reports with the same fields/units as standard Git (`count-objects -v` family: count, size, in-pack, packs, size-pack, prune-packable, garbage, size-garbage); prune/expire by time-config.
- **Compatibility requirements**: Field definitions (e.g., size accounting units), human-readable variants, and prune/expire reachability rules MUST match standard Git.
- **Error behavior**: Maintenance on corrupt stores reports corruption but MUST NOT delete the corrupt-but-reachable object as "garbage"; I/O failures abort without partial repacks.
- **Security/corruption considerations**: Expiry MUST be anchored on reachability + mtime, never on name guessing; repacks MUST verify new packs before removing old ones.
- **Interoperability requirements**: Post-maintenance stores MUST verify clean under both implementations; counts MUST agree on identical stores.
- **Required tests**: Count/size/garbage agreement tests, prune-expiry tests (reachable preserved, expired-unreachable removed), repack-verify-then-swap tests.

### 25. Packfiles

- **Expected behavior**: Read v2 packs: `PACK` + version(2) + 32-bit count, then per-object records (type ∈ {commit,tree,blob,tag,ofs-delta,ref-delta} with variable-length size), then trailing full checksum (20/32 bytes per algorithm). Multi-pack directories supported; packs are immutable once written.
- **Input/output format**: Binary format exactly as documented in `Documentation/technical/pack-format.txt`; thin-pack acceptance only where standard Git accepts it at the storage layer.
- **Compatibility requirements**: Magic/version/count validation, object-type decoding, and checksum verification MUST match; packs written here MUST pass standard verification.
- **Error behavior**: Bad magic/version/count, truncated records, bad checksums: corrupt-pack diagnostics naming pack + offset; unknown versions rejected, never guessed.
- **Security/corruption considerations**: Count/size fields MUST be bounds-checked before allocation; deep delta chains bounded; checksum verified before trusting offsets.
- **Interoperability requirements**: Packs from either side MUST be listed, verified, and read by the other (`verify-pack`-equivalent agreement).
- **Required tests**: Empty/single/multi/deltified/multi-pack fixtures, truncation/checksum/version negative tests, crosswise verify tests.

### 26. Index files for packfiles

- **Expected behavior**: Read/write v2 indexes: magic (`0xff 0x74 0x4f 0x63`) + version(2), 256-entry fanout, sorted ID table, CRC32 per object, 31-bit offsets + 64-bit overflow table, pack checksum + index checksum trailers.
- **Input/output format**: Binary format per `Documentation/technical/pack-format.txt`; writers emit OID-sorted entries with correct fanout even when pack order differs.
- **Compatibility requirements**: Fanout semantics, sort order, CRC/offset decoding, and checksum cross-checks MUST match; generated indexes MUST be accepted by standard `index-pack --verify`/`verify-pack`.
- **Error behavior**: Magic/version mismatch, fanout inconsistency, unsorted IDs, offset-out-of-range, checksum mismatch: corrupt-index diagnostics; verification exits non-zero.
- **Security/corruption considerations**: Offsets MUST be validated against actual pack size; CRC MUST be checked on verification paths; hostile fanout MUST NOT cause out-of-bounds reads.
- **Interoperability requirements**: Index↔pack pairs from either side MUST verify on the other in both directions.
- **Required tests**: Index generation + verify matrix (sorted/unsorted packs, 64-bit offsets, CRC mismatches, checksum mismatches) crosswise in both directions.

### 27. Delta objects

- **Expected behavior**: Support `OFS_DELTA` (negative base offset) and `REF_DELTA` (base ID); delta payload = base-size + result-size (base-128 varints) + copy/insert instruction stream; resolve recursively to the base, then apply.
- **Input/output format**: Binary delta records as in pack-format docs; copy instructions reference base offsets, inserts carry literal bytes.
- **Compatibility requirements**: Offset decoding, instruction semantics, size-header validation, and base-selection equivalence MUST match standard Git (`diff-delta`/`patch-delta` behavior).
- **Error behavior**: Missing base, cyclic base, size mismatch after application, or invalid instruction: corrupt-delta diagnostic naming the delta object and base; never return partially-applied output.
- **Security/corruption considerations**: Recursion depth and output size MUST be bounded (no billion-laughs via nested deltas); copy offsets/lengths validated against base size.
- **Interoperability requirements**: Deltified packs from either side MUST inflate to identical bytes on the other.
- **Required tests**: Same-type chains, cross-boundary offsets, REF vs OFS variants, missing-base/cycle/size-lie negatives, depth-limit tests, crosswise inflation tests.

### 28. Reachability

- **Expected behavior**: From seed commits/tags/refs, close over commit parents, tree entries (recursive), and tag targets; commits reference trees, trees reference blobs/trees/commits (submodules), tags reference any kind. Unreachable objects classified as dangling (well-formed but unreferenced) vs corrupt.
- **Input/output format**: Input: seed set + store. Output: reachable set; verification reports list missing/corrupt/dangling with counts.
- **Compatibility requirements**: Closure rules and dangling-vs-missing-vs-corrupt classification plus exit codes MUST match `fsck`-family behavior.
- **Error behavior**: Missing reachable object: `missing <type> <oid>`-class report, non-zero exit; corrupt reachable object: corrupt report, non-zero exit; dangling: reported but (in default mode) distinguished from fatal breakage.
- **Security/corruption considerations**: Walks MUST be cycle-safe and MUST terminate on adversarial graphs; MUST NOT load the entire closure into unbounded memory without streaming/batching.
- **Interoperability requirements**: Reachability/verification reports MUST agree between implementations on identical stores (same missing/dangling sets).
- **Required tests**: Linear, merge, submodule, tag-chain, dangling-blob/tree/commit, missing-base, and corrupt-member fixtures with report + exit-code comparison.

### 29. Object traversal

- **Expected behavior**: History walks from seeds with no-duplicate visitation, correct handling of merges (all parents), tag peeling to commits/trees where requested, and support for commit/date/topological orderings with identical membership; path-affecting filters (where present) restrict membership identically.
- **Input/output format**: Input: seeds + ordering + filters. Output: ordered object/commit sequence (each reachable member exactly once, modulo documented ordering differences).
- **Compatibility requirements**: Membership (which objects appear) MUST match standard Git exactly; orderings MUST match within the documented ordering contract (commit/topological/date).
- **Error behavior**: Bad seeds: `bad object`-class errors; empty seeds: defined empty result, not a crash; truncated walks on corruption report the corrupt member and stop closedly.
- **Security/corruption considerations**: Adversarial deep/branchy graphs MUST NOT cause stack overflow or non-termination; limits (e.g., max-count) MUST be honored exactly.
- **Interoperability requirements**: `rev-list`-family membership agreement on shared fixtures, including merges and tag peels.
- **Required tests**: Linear/merge/octopus/tag-peel/shallow-boundary walk fixtures, ordering tests, bad-seed/empty/corrupt negatives, crosswise membership tests.

## Assumptions

- Standard Git behavior is defined by the C Git version vendored in this repository (`v2.55.0`-family) plus its `Documentation/technical/` pages (`pack-format.txt`, `reftable` excluded, `bitmap-format.txt` informative only) and the `t/` test suite as the final oracle where docs and code disagree.
- The default hash algorithm is SHA-1 with collision detection enabled on all hashing paths; SHA-256 applies only inside repositories configured with `extensions.objectFormat=sha256`.
- Cross-algorithm translation (`compatObjectFormat`, `gpgsig`↔`gpgsig-sha256` rewriting, `LMAP` loose map) is out of scope; each repository uses exactly one algorithm.
- The `reftable` ref backend is out of scope; the files backend + `packed-refs` is the specified ref store. Reflogs are referenced only as resolution/maintenance inputs, not fully specified here.
- Commit-graph, multi-pack-index (MIDX), and bitmap accelerators are read-path optimizations covered only insofar as they affect object visibility and verification; full generation semantics belong to a separate performance/acceleration feature.
- Network transport, fetch negotiation, and server-side admission are out of scope; quarantine is specified only as storage-layer isolation and promotion/cleanup.
- Garbage collection scheduling (`gc.auto`, background maintenance cadence) is out of scope; only the storage invariants (what may be counted/pruned/repacked and with what safety preconditions) are specified.
- Filesystem behavior assumed: POSIX atomic rename within a directory, `fsync` durability where stated, case-sensitive paths for ref/object names (case-insensitive filesystems behave per standard Git without extra aliasing rules in this spec).
- Existing implementation code is treated as context only; where it disagrees with standard Git documented behavior or `t/` oracle outcomes, standard Git prevails and the discrepancy is a defect, except for explicitly logged intentional deviations carried as assumptions in the plan backlog.
- Performance targets are stated in Success Criteria as user-observable bounds on fixture-scale repositories, not as asymptotic or benchmark-harness mandates.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user opening repositories from any subdirectory, from inside `.git`, from bare roots, and via `gitdir:`/env-override layouts sees the same discovered location, bare flag, and work-tree result as standard Git in 100% of the fixture matrix.
- **SC-002**: Objects written by either implementation are readable and verifiable by the other in 100% of round-trip fixtures (all four kinds, empty/binary/large/special-name cases), with identical IDs.
- **SC-003**: Ref resolution and listing (attached/detached/unborn `HEAD`, symref chains, loose-overrides-packed, peeled tags) agree with standard Git in 100% of ref fixtures, including byte-sorted listing order.
- **SC-004**: Pack and index artifacts from either implementation verify clean on the other in 100% of pack fixtures (plain, deltified, multi-pack, 64-bit offsets).
- **SC-005**: Corruption fixtures (bad header/size/zlib/checksum/delta-base) are all detected with the offending object named, and healthy fixtures verify clean, matching standard Git outcomes in 100% of cases.
- **SC-006**: History walks cover exactly the reachable membership standard Git reports (no missing, no extras, no duplicates, no hangs) across linear, merge, tag-chain, and dangling-object fixtures.
- **SC-007**: Abbreviated-ID lookups agree with standard Git (found vs ambiguous vs not-found) in 100% of abbreviation fixtures with at least 4-hex minimum enforcement.
- **SC-008**: SHA-256 repositories interoperate in both directions (identical IDs, successful verification) for all core object and pack scenarios in the SHA-256 fixture set.
- **SC-009**: Maintenance operations preserve 100% of reachable objects (post-operation verification clean) while removing only expired-unreachable loose objects per the documented expiry rules.
- **SC-010**: A user completing the primary flows (open → store → resolve → walk → verify) on a mid-size fixture repository finishes end-to-end verification in a time comparable to standard Git on the same machine (within 2× wall-clock on the reference fixture set), with zero data-loss events.
