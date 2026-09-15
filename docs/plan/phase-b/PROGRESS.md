# Phase B Implementation Log

Running log of implemented Phase B items ("Workflow Core / usable repo", see
[conversion-plan.md](../conversion-plan.md)). Each entry records what was
implemented, where it landed, and how it was verified. Newest entries at the
bottom.

| Date | Item | Status |
|---|---|---|
| 2026-09-14 | B1 `git init` | DONE |
| 2026-09-15 | B2 cache-tree (`TREE`) index extension | DONE (partial: REUC / v3-v4 pending) |
| 2026-09-15 | B4 `write-tree` / `read-tree` | PARTIAL (one-way + cache-tree) |

## Details

### B1 — `git init` — DONE

Implemented per [01-git-init.md](01-git-init.md) in
`crates/git-command/src/init.rs` (~1600 lines), plus support in
`git-config` (case-insensitive sections/keys, `ConfigSet::append`) and
`git-command/Cargo.toml`.

- Argument parsing with C-parse-options-compatible diagnostics (unknown /
  ambiguous options, `takes no value`, bundled shorts, `--no-*`).
- `--bare`, `--separate-git-dir`, `--template[=]`, `--shared[=perm]`,
  `-b`/`--initial-branch`, `--object-format`, `--ref-format`, `-q`, `--help`.
- C-compatible git-dir resolution: `--bare` sets `GIT_DIR` to the cwd (or the
  operand directory), matching `builtin/init-db.c`'s
  `setenv(GIT_DIR_ENVIRONMENT, cwd, argc > 0)`; `GIT_DIR`/`GIT_WORK_TREE`
  overrides; `guess_repository_type` for the bare default.
- Template copying (`copy_templates_1` semantics: skip dotfiles, never
  overwrite, format vintage check), `$GIT_TEMPLATE_DIR` /
  `init.templatedir` / compiled-default resolution.
- Config surgery preserving untouched content: `core.repositoryformatversion`,
  `filemode`/`symlinks`/`ignorecase`/`precomposeunicode` probes, `bare`,
  `logallrefupdates`, `worktree`, `extensions.objectformat` /
  `extensions.refstorage`, `core.sharedrepository` + `receive.denyNonFastforwards`.
- `Initialized empty` / `Reinitialized existing` reporting with the shared
  marker, canonical path and trailing slash; `HEAD` + `init.defaultBranch`
  advice; `--initial-branch` validation.

Verification: `phaseB01_crosswise.rs` (registered `phaseB01-crosswise`, 8
tests) — hermetic-env byte parity with C git for stdout/stderr/exit and the
resulting `.git` layout, HEAD and config, across plain/`--bare`/
`--separate-git-dir`/`-b`/`init.defaultBranch`/reinit/`--shared=group`/
`--template` scenarios; C git accepts Rust-created repos in both directions.
Scoreboard: 24/24 suites green (no regression).

Known gaps: `--object-format=sha256` writes the config extension but the
object layer is SHA-1-first; `--ref-format=reftable` records the extension but
the reftable backend is not yet wired in (Phase 7).

### B2 — cache-tree (`TREE`) index extension — DONE (partial)

Implemented in `crates/git-index/src/cache_tree.rs` (new) plus `Index`
integration:

- `CacheTree { name, entry_count, oid, subtrees }` with `serialize`/`parse`
  matching `cache-tree.c:write_one`/`read_one` exactly (name NUL, `"%d %d\n"`,
  optional raw oid, recursive subtrees).
- `Index` gained `cache_tree: Option<CacheTree>`; `Index::parse` decodes the
  `TREE` extension and skips unknown extensions; `Index::to_bytes` emits the
  `TREE` extension when present.
- Unit tests: nested round-trip, invalid (`-1`, no oid) node, trailing-garbage
  rejection, extension round-trip, unreadable-extension ignored.

Still pending from B2: REUC extension, index versions 3/4 (extended flags /
path compression), split and sparse index.

### B4 — `git write-tree` / `git read-tree` — PARTIAL

Implemented in `crates/git-command/src/{write_tree,read_tree,treeobj}.rs`:

- `write-tree [--missing-ok] [--prefix=<prefix>/]`: builds the tree hierarchy
  from the index (`treeobj`), writes the tree objects, prints the root oid, and
  refreshes the index cache-tree + rewrites the index (like C). Unmerged
  (stage > 0) entries print `<path>: unmerged (<oid>)` per entry then
  `fatal: git-write-tree: error building trees` (exit 128). Missing prefix is
  `fatal: git-write-tree: prefix <p> not found`. Empty/missing index yields the
  empty tree.
- `read-tree` one-way: `<tree-ish>...` (union), `--empty`, `--reset`, `-i`,
  `-n`/`--dry-run`, `-v`, `--index-output=<file>`; flattens the tree(s) into
  stage-0 entries with zeroed stat data and primes the cache-tree. Missing /
  non-tree objects: `fatal: failed to unpack tree object <oid>`. No arguments
  emits C's deprecation warning and empties the index.
- `treeobj::build` writes trees (write-tree) or computes oids only
  (read-tree) and returns the matching `CacheTree`.

Verification: `phaseB04_crosswise.rs` (registered `phaseB04-crosswise`, 6
tests) — `write-tree` stdout **and the post-write index bytes** are identical
to C; `read-tree` index bytes are byte-identical (including the `TREE`
extension) for both `GIT_INDEX_FILE` and `--index-output`; `--prefix`,
unmerged, bad-object and blob-not-tree errors match byte-for-byte. Scoreboard:
25/25 suites green.

Deferred (documented in FOLLOWUPS): `read-tree -m` (two/three-way), `-u`
(worktree update), `--prefix`; these depend on `unpack-trees` (B7).
