# Phase A Handoff: State, Remaining Work, and Instructions for the Next Agent

Last updated: end of the A6 commit (`b5c44b02ef`); A8 work in progress in the working tree.

## Repo ground rules (from AGENTS.md)

- Run **all** cargo commands from `crates/` (`cd crates`). The root `Cargo.toml`
  is a stale leftover; running cargo from the repo root is wrong.
- C git is the spec; `t/` is the oracle; byte-identical stdout/stderr/exit
  codes are the acceptance test.
- An **external watcher** (watch-and-commit / a formatting hook) rewrites
  files in this workspace (you will see "file has been modified since last
  read"). Re-read before editing; after writing, verify the bytes landed
  (plain `git diff`).
  **Escape gotcha:** this environment historically mangled `\n` escapes in
  Python heredocs. When editing Rust string/char literals, prefer `sed`/
  byte-level Python (e.g. `bytes([92,110])`) or write the whole file with a
  single `write` tool call, then confirm with `git diff` before building.
- Each item commits separately with `<git_commits>` conventions.

## Completed and committed (all with crosswise suites in `xtask::suites()`, scoreboard updated)

| Item | Commit | Notes |
|---|---|---|
| A2 repo discovery / env threading | `6fe5d152e4` (+watcher commits) | RepoContext everywhere, `--git-dir` etc. |
| A1 sha1dc | `97bf4a36e3` | SHAttered detection, hash-object parity |
| A3 cat-file batch | `f190e10c49` | `%(atom)`, `--batch-all-objects`, `-z`, rev:path |
| A4 abbreviation resolution | `1c53275cd8` | `git_revision::Resolver`, peels, ambiguity text |
| A5 rev-parse | `c27d6c5f52` | ranges, symbolic, shape flags, sq, env vars |
| A12 local timezone | `151b04a961` | TZif reader, DST-correct idents, calendar months |
| A10 count-objects | `e3145f6846` | garbage semantics, -H, usage errors |
| A7 pretty engine | `14e0ac4337` | git-pretty crate; log driven by it |
| A6 rev-list/log options | `b5c44b02ef` | rev_info walker; topo/date order, filters, ranges, --objects, path limits |

`cargo test --workspace`: **55 test binaries, all green** (before the A8
work-tree edits; re-verify after finishing A8).

## IN PROGRESS: A8 diff engine completion (uncommitted working tree)

Target file: `docs/plan/phase-a/08-diff-options.md`. Much is implemented and
byte-identical vs system git on the /tmp fixture matrix, listed below.

### What is already done in the working tree (verify, don't redo)

- `git-diff/src/unified.rs`: `render_unified_ctx` with `-U<n>`, the
  `\ No newline at end of file` marker, section-context in `@@` headers
  (last kept line before the hunk, e.g. `@@ -2 +2 @@ a`), and zero-count
  hunk ranges (`@@ -3,0 +4 @@`). Unit tests present.
- `git-diff/src/tree.rs`: `Change` gained `old_path`, `new_path`, `score`;
  `MAX_SCORE = 60000`.
- `git-command/src/patch.rs`: rewrote the renderers — `render_stat`
  (exact C `show_stats`: 80-col width, scale_linear, Bin handling,
  number_width=3 for binaries), `render_shortstat`, `render_numstat`
  (binary rows as `-\t-\tpath`), `render_name_line`, `render_raw`
  (abbreviated 7-char oids, `dirty` new-side as `0000000`), `render_summary`,
  `render_change_patch_ctx` (rename blocks, binary footers, mode-change
  lines, `-U`). `BlobSource` bridges odb + synthetic blobs. `mode6`
  zero-pads modes to 6 digits (`40000`→`040000`) and renders absent modes
  as `000000`.
- `git-command/src/diff.rs`: rewrote with option parsing (`--stat`,
  `--shortstat`, `--numstat`, `--name-only`, `--name-status`, `--raw`,
  `--summary`, `--patch-with-stat`, `-U<n>`, `-s`, `--cached/--staged`,
  `-M/--find-renames[=n]`, `--no-renames`, `--diff-filter=`,
  `--exit-code/--quiet`, `--no-index`), sources (worktree synthetic tree
  built from `git-index` + stat-match reuse of index oids; index tree from
  flat entries; HEAD/tree resolution via `resolve_arg`), delta blobs kept
  in an `extra` map, rename detection (exact pass + line-similarity pass),
  and path limiting.
- `git-index`: added `Default` impl for `Index`.
- Crosswise suite `crates/git-command/tests/phaseA08_crosswise.rs`
  (fixture with no-EOL modify, exact rename, binary, staged add/delete).
  **Verified byte-identical on the matrix below** (run before registering):
  `diff HEAD`, `--stat`, `--numstat`, `--shortstat`, `--name-only`,
  `--name-status`, `--raw`, `--summary`, `--cached`, `--no-renames`,
  `--find-renames=90`, `-U0`, `-U1`, `-U3`, `HEAD -- f.txt`, `HEAD -- bin.dat`,
  `--diff-filter=M`, `--diff-filter=R`, `--exit-code`, plain `diff`,
  `--patch-with-stat`. `diff HEAD -U0` hunk headers byte-identical.

### What must STILL be done to close A8 (in dependency order)

1. **Finish `diff-tree --exit-code`** (`crates/git-command/src/diff_tree.rs`):
   recognize `--exit-code`/`--quiet`; after emitting changes, return
   `CommandError::silent(1)` when changes exist and 0 otherwise (like
   `diff.rs::finish`). Also port `diff-tree` raw rendering to use the new
   `patch::render_raw` (abbreviated 7-char oids per system git) — confirm
   with `git diff-tree HEAD~1 HEAD` in a dirty repo. Do NOT break the
   existing `phase5_crosswise.rs` expectations (it compares raw lines
   with FULL 40-char oids on its fixture — check whether that suite expects
   full or abbreviated; if it expects full, keep `diff-tree` raw full and
   only add `--exit-code`).
2. **Register the A8 suite** in `xtask/src/main.rs::suites()` as
   `phaseA08-crosswise` and re-run `cargo run -p xtask -- scoreboard`
   (from `crates/`). Run `cargo test --workspace` — ensure 55+ suites green.
3. **Remove debug leftovers** in the working tree before committing:
   - `crates/git-command/examples/dbgdiff.rs` (and the dir if empty)
   - `crates/git-index/examples/dbg.rs` (and the dir if empty)
   - any `DIFF_DEBUG` eprintln blocks in `git-command/src/diff.rs`
4. **Commit A8** (conventional message describing what it enables), update
   `docs/plan/phase-a/PROGRESS.md` (mark A8 DONE with implemented scope),
   add the deferred list (word-diff, --color, --patience/--histogram
   algorithms, --dirstat, whitespace family, --relative, -S/-G pickaxe,
   rename/copy heuristic is similarity-approximation not C's
   diffcore-rename algorithm, stat width hard-coded 80 like non-tty C).
5. **Known A8 deviations to record in FOLLOWUPS**: exact rename scoring
   differs from diffcore-rename for edited renames; `--diff-filter`
   lowercase = exclude implemented; `status` field for `T` (mode change)
   classification.

## REMAINING PHASE A ITEMS (do next, same rigor)

### A9 — userdiff hunk headers (`docs/plan/phase-a/09-userdiff-hunk-headers.md`)

Hunk headers: the `@@ ... @@` suffix now uses the last context line (done in
A8). Extend with the builtin userdiff drivers (C `userdiff.c`): known
paths/extensions get function-name regexes (e.g. `*.c` uses
`^[A-Za-z_].*[^:)]*$`) that select the nearest preceding matching line from
the OLD side instead of "last context line". Gates: crosswise suite with a
fixture of `.c`/`.py`/`.md` files; `t/t4018-*.sh` concepts (funcname
patterns live in `userdiff.c`). Keep default (no driver) behavior
byte-identical to today.

### A11 — `.gitignore` + attributes engine (`docs/plan/phase-a/11-gitignore-attributes.md`)

New crate (suggest `git-ignore` or put in `git-core`):
- `.gitignore` parsing per C `dir.c` `parse_ignore_file` (patterns,
  unanchored vs anchored, `!` negation, trailing `/` dir-only, `**`).
- `git-check-ignore`-style match (walk dirs bottom-up, LAST matching
  pattern wins, negation) and `git status --ignored` groundwork.
- `.gitattributes` minimal subset: `-diff`, `binary`,
  `diff=<driver>`/`diff=<funcname>` piping into A9.
- Gates: crosswise suites vs `git check-ignore` on crafted trees.

### A13 — pack delta compression on write (`docs/plan/phase-a/13-pack-delta-compression.md`)

`git pack-objects` currently writes non-deltified packs (recorded
deviations). Implement delta candidate selection (window), `delta.c`
delta encoding (`create_delta` semantics), OFS/REF delta entries, and
`git verify-pack`/`git index-pack` compatibility (C git must verify our
packs). Crosswise: `git pack-objects <obj-list>` from our binary vs C,
then `git verify-pack -v` both. Big item; split commit by (a) delta
encoder CRATE, (b) pack writer integration, (c) crosswise suite.

## General do-this-every-time checklist

1. Read the item's plan doc in `docs/plan/phase-a/` first; it names gates.
2. Probe system git (`/usr/bin/git`) behavior before coding; write the
   crosswise test first when possible (style: `crates/git-command/tests/
   phaseA0X_crosswise.rs`).
3. After each change: `cd crates && cargo build --workspace` (0 errors),
   `cargo test --workspace` (all green), the item's new crosswise test,
   `cargo run -p xtask -- scoreboard` (writes baseline; fails on regression).
4. `git add -A` + commit per item; update `docs/plan/phase-a/PROGRESS.md`
   and `docs/plan/FOLLOWUPS.md` (`**DONE (AX)**` style entries already used).
5. Never edit `crates/scoreboard.json` by hand; never run
   `watch-and-commit.sh`; do not commit from the repo root (use `crates/`).

## Suggested next-agent prompt (copy/paste)

"Work through the Phase A remaining items of the pure-Rust git port at
/Users/ali/dev/rust/git.rs, A8-finish first then A9, A11, A13 in that
order. Read docs/plan/phase-a/README + the per-item docs (08, 09, 11, 13),
and docs/plan/phase-a/HANDOFF.md for exact state. A8: finish diff-tree
--exit-code, register phaseA08-crosswise in xtask::suites(), delete the
debug example files, run cargo test --workspace and the scoreboard from
crates/, commit A8 and mark PROGRESS.md; then implement A9 (userdiff hunk
headers over A8's hunk-context), A11 (.gitignore + attributes with
check-ignore crosswise parity), A13 (delta encoding in pack-objects
verified by C git) — each with its own crosswise suite registered in
xtask, gates green (cargo test --workspace + scoreboard no-regression),
committed separately, and PROGRESS.md/FOLLOWUPS.md updated. Follow the
handoff's escape gotcha (prefer byte-level Python/sed for \n edits) and
the per-commit checklist."