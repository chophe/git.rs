# Phase B Implementation Log

Running log of implemented Phase B items ("Workflow Core / usable repo", see
[conversion-plan.md](../conversion-plan.md)). Each entry records what was
implemented, where it landed, and how it was verified. Newest entries at the
bottom.

| Date | Item | Status |
|---|---|---|
| 2026-09-14 | B1 `git init` | DONE |
| 2026-09-15 | B2 cache-tree (`TREE`) index extension | DONE (partial: REUC / v3-v4 pending) |
| 2026-09-15 | B3 `git add` | DONE |
| 2026-09-15 | B4 `write-tree` / `read-tree` | PARTIAL (one-way + cache-tree) |
| 2026-09-15 | B5 `git commit` | DONE |

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

### B3 — `git add` — DONE

Implemented in `crates/git-command/src/add.rs` (+ `ignore_util.rs` shared with
`check-ignore`):

- Pathspec resolution (literal paths, directories, `.`, `*`/`?`/`[` globs via
  wildmatch without `WM_PATHNAME` like C, absolute paths, `../`, `--`).
- Modes: default (add+modify+delete restricted to pathspecs), `-A`/`--all`,
  `-u`/`--update`, `-n`/`--dry-run`, `-v`/`--verbose`, `-f`/`--force`.
- Ignore integration: per-directory `.gitignore`, `.git/info/exclude`,
  `core.excludesFile`; ignored directories are skipped silently; explicitly
  named ignored files produce C's warning + exit 1; globs matching only
  ignored files report "did not match".
- C-parity diagnostics: `Nothing specified, nothing added.` + hints,
  `fatal: pathspec '<p>' did not match any files`, the ignored-files block.
- Stat-accurate entries (symlinks → mode `120000` with the link target as the
  blob; exec bit per `core.filemode`).
- Cache-tree parity: replicate C's `add_to_index` early return — an entry is
  replaced/invalidated only when its stat record differs (`ie_match_stat`) or
  it is racily clean (entry mtime ≥ index mtime); only oid/mode changes are
  reported by `-v`. The result is a **byte-identical index** (including the
  invalidated `TREE` nodes) versus C git.

Verification: `phaseB03_crosswise.rs` (registered `phaseB03-crosswise`, 4
tests) — byte-identical stdout/stderr/exit/**index** for `-A`/`-u`/`./dir`/
glob/literal-file specs, ignored + `-f`, dry-run/verbose reports, no-args,
bad pathspec, and from a subdirectory. The test pins the index mtime to a
far-future value so racy handling is deterministic.

Deferred (documented in FOLLOWUPS): `-p`/`-i` interactive, `-N`
intent-to-add (needs index v3 extended flags), `--refresh`, `--chmod`,
pathspec magic (`:(...)`), negation inside an ignored directory, and
clean/smudge/CRLF filters.

### B5 — `git commit` — DONE

Implemented in `crates/git-command/src/commit.rs` (+
`git-date::Timestamp::format_git_default`):

- Messages: `-m` (repeatable, joined by blank lines), `-F <file>`/`-F -`
  (stdin), editor fallback via `core.editor`/`GIT_EDITOR`, `--amend
  --no-edit` reuse; C's default "whitespace" cleanup (strip trailing
  whitespace, trim blank edges; `#` stripping for editor mode); empty
  message aborts (`Aborting commit due to empty commit message.`) unless
  `--allow-empty-message`.
- `-a` stages tracked modifications/deletions by invoking the validated
  `add -u` path (writes blobs + refreshed index), then commits.
- Tree from the index (`treeobj`); parents from HEAD (or HEAD's parents on
  amend); author/committer idents with `--author`/`--date` and amend
  author-preservation; commit object written and verified byte-identical.
- Ref update + reflog: `.git/logs/HEAD` always, `.git/logs/refs/heads/<b>`
  only when the ref value changes (matches C's no-op-amend behavior);
  `commit (initial)` / `commit` / `commit (amend)` actions with the subject.
- Summary output: `[<branch>[ (root-commit)] <short7>] <subject>`, plus
  `Author:`/`Date:` lines when the author differs or the date is explicit,
  and the diffstat (`N files changed, X insertions(+), Y deletions(-)` with
  `create/delete mode` lines) against HEAD (or the parent on amend).
- Nothing-to-commit report: clean / untracked / modified / deleted / both
  variants (status-style blocks) and the empty-repo "Initial commit" variant.

Verification: `phaseB05_crosswise.rs` (registered `phaseB05-crosswise`, 3
tests) — byte-identical stdout/stderr/exit, `rev-parse HEAD`, `cat-file
commit`, `.git/refs/heads/*` and `.git/logs/*` across a multi-step sequence
(initial, `-am`, nothing, `--amend --no-edit`, `--allow-empty`, two `-m`,
`--author`/`--date`, `-F -`), plus all nothing-to-commit variants and the
empty-repo/empty-message cases.

Deferred (documented in FOLLOWUPS): pathspec commits (`commit -- <paths>`),
full interactive editor UX, `--porcelain`/`--dry-run`, `-v` diff output,
hooks, GPG signing, and commit-graph interaction.
