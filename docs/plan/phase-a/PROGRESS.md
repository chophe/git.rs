# Phase A Implementation Log

Running log of implemented Phase A items ("Deepen What's Routed", see
[00-overview.md](00-overview.md)). Each entry records what was implemented,
where it landed, and how it was verified. Newest entries at the bottom.

| Date | Item | Status |
|---|---|---|
| 2026-08-29 | A2 repo discovery / `--git-dir` / `--work-tree` | DONE |
| 2026-08-29 | A1 sha1dc collision-detecting SHA-1 | DONE |
| 2026-08-29 | A3 `cat-file --batch` / `--batch-check` / `%(format)` | DONE |
| 2026-08-29 | A4 abbreviation resolution | DONE |
| 2026-08-29 | A5 `rev-parse` completion | DONE (subset) |
| 2026-08-29 | A7 pretty-printing engine | planned |
| 2026-08-29 | A6 `rev-list`/`log` options | planned |
| 2026-08-29 | A8 diff options completion | planned |
| 2026-08-29 | A9 userdiff hunk headers | planned |
| 2026-08-29 | A10 `count-objects -v` close-out | planned |
| 2026-08-29 | A11 `.gitignore` + attributes engine | planned |
| 2026-08-29 | A12 local timezone dates/idents | planned |
| 2026-08-29 | A13 pack delta compression on write | planned |

## Details

### A2 — repo discovery / `--git-dir` / `--work-tree` — DONE

Implemented per [02-repo-discovery-env.md](02-repo-discovery-env.md).

- `git_command::RepoContext` (`crates/git-command/src/lib.rs`): carries the
  effective cwd, `--git-dir`/`--work-tree`/`--common-dir` overrides, `--bare`,
  and `git -c` config overlays. Threaded explicitly through the `Command`
  trait; every command module now takes `ctx` instead of calling
  `Repository::discover()` itself (process cwd is never mutated).
- Global-arg parsing (`RepoContext::from_global_args`): `-C <dir>`
  (cumulative), `-c name[=value]`, `--git-dir[=]`, `--work-tree[=]`,
  `--common-dir[=]`, `--bare`, plus pager passthrough flags; unknown global
  options exit 129 like C git. Wired in `git-cli::run`.
- Env support: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`,
  `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`
  (precedence CLI > env > discovery).
- `git-core`: `Repository` gained `index_file`, `object_dir`, `alternates`,
  `git_dir_specified` fields; explicit `GIT_DIR` overrides are validated
  (`not a git repository: '<v>'`, exit 128) like C git.
- `git-odb`: object-dir override + alternates (env and
  `objects/info/alternates`) reach `LooseStore` and `Odb` reads/packs.
- `git-config`: `set`/`set_in`/`set_cli` for the `-c` overlay layer.
- `rev-parse`: implemented `--show-toplevel`, `--is-inside-work-tree`; made
  `--git-dir`/`--git-common-dir` match C git's relative/absolute/verbatim
  path rendering.
- Verification: new crosswise suite `phaseA02_crosswise.rs` (registered in
  `xtask::suites()` as `phaseA02-crosswise`) — byte-identical vs system git
  for flag/env/`-C` forms; full workspace tests green; scoreboard baseline
  updated with the new passing suite; FOLLOWUPS.md §C item marked DONE.

### A1 — sha1dc collision-detecting SHA-1 — DONE

Implemented per [01-sha1dc.md](01-sha1dc.md).

- `crates/git-hash/src/sha1dc.rs`: pure-Rust port of the vendored
  `sha1dc/` C sources — streaming update/finalize, per-step state capture,
  the 32 disturbance vectors (generated from `ubc_check.c`), the
  unavoidable-bitcondition mask, and the recompression check
  (`sha1_recompression_step` via loop form). The C trailing per-DV exact
  refinement block is omitted (it only clears mask bits, a performance
  filter); detection is driven by the recompression check itself.
- `CryptoHasher::Sha1` now uses sha1dc; `is_safe()` returns true for SHA-1
  (negative test inverted). New `finalize_checked`/`finalize_oid_checked`
  error path: colliding input yields
  `SHA-1 appears to be part of a collision attack: <digest>` (C git's die
  message); plain `finalize` keeps C git's default safe-hash=0 digest.
- Detection wired through `git-object::try_compute_id`, loose-object writes
  (`git-odb` `OdbError::Collision`), and `hash-object`.
- Verification: SHAttered PDF unit test (detected, `38762cf7...` reported),
  proptest sha1dc == standard SHA-1 on arbitrary inputs, new crosswise suite
  `phaseA01_crosswise.rs` (registered as `phaseA01-crosswise`) byte-identical
  vs system git; full workspace tests green; scoreboard baseline updated;
  FOLLOWUPS.md §A1 marked DONE.

### A3 — `cat-file --batch` / `--batch-check` / `%(format)` — DONE

Implemented per [03-cat-file-batch.md](03-cat-file-batch.md).

- `%(atom)` formatter: `%(objectname)`, `%(objecttype)`, `%(objectsize)`,
  `%(objectsize:disk)`, `%(deltabase)`, `%(rest)`; unknown atoms fail like
  C git. Default formats match `--batch` / `--batch-check`.
- `--batch-all-objects`: sorted, deduped iteration over loose + packed OIDs.
- `-z` (NUL-terminated records) and `--buffer` accepted; `%(rest)` splits
  the input record at the first space only when the format uses it.
- New `git_odb::Odb::disk_info` supplies on-disk size (loose file size or
  packed entry span) and delta base (Ofs/Ref delta resolution).
- `resolve_arg` gained `<rev>:<path>` support (commit → tree → path walk),
  used by cat-file and shared with other commands.
- Verification: crosswise suite `phaseA03_crosswise.rs` (registered as
  `phaseA03-crosswise`) compares byte-identical output for loose and packed
  repos, formats, `-z`, `%(rest)`, all-objects; full workspace tests green;
  scoreboard updated.

### A4 — abbreviation resolution — DONE

Implemented per [04-abbrev-resolution.md](04-abbrev-resolution.md).

- `git_revision::Resolver` (`crates/git-revision/src/resolve.rs`): full hex
  → refname candidates → abbreviated hex (>=4 chars) over loose fanout dirs
  and sorted pack indexes; ambiguity error reproduces C git's exact text
  (`error: short object ID <pfx> is ambiguous` + the generic
  ambiguous-argument die, exit 128).
- `<rev>~<n>` / `<rev>^<n>` peels and `<rev>:<path>` tree walks; `HEAD~0`
  parity confirmed.
- `git-command::resolve_arg` delegates to the resolver, so `rev-list`,
  `log`, `commit-tree` (full-length requirement dropped, FOLLOWUPS closed)
  and friends share one resolution path with byte-identical diagnostics;
  `rev-parse` echoes unresolved args like C git.
- `git-cli` now buffers stdout so stdout/stderr ordering matches C git when
  streams are merged.
- Verification: crosswise suite `phaseA04_crosswise.rs` (registered as
  `phaseA04-crosswise`) covering abbreviations at many lengths, peels,
  rev:path, too-short/unknown/verify errors and a genuinely ambiguous
  prefix; full workspace tests green; scoreboard updated.

### A5 — `rev-parse` completion — DONE (core subset)

Implemented per [05-rev-parse-completion.md](05-rev-parse-completion.md).

- Range syntax `A..B` (prints B, ^A) and `A...B` (prints B, A, ^merge-base
  via first-parent ancestry), empty sides default to HEAD.
- Output shaping: `--symbolic`, `--symbolic-full-name`, `--short[=n]`,
  `--abbrev-ref` (improved), `--sq`, `--sq-quote` (C-exact quoting);
  unrecognized options are echoed verbatim like C git's passthrough.
- Verification: `--verify` with `--quiet` (silent exit 1) and
  `--default=<arg>` fallback.
- Repo shape: `--is-bare-repository`, `--is-shallow-repository`,
  `--show-prefix` / `--show-cdup` (correct from subdirectories),
  `--absolute-git-dir`, `--shared-index-path` (prints nothing like C in
  plain repos), `--local-env-vars` (C's `local_repo_env` list).
- Deferred: `--parseopt`, `@{upstream}`/`@{push}`/`@{-N}` expansions and
  date-based `@{...}` (needs the reflog item; resolution errors currently
  fall out as the generic ambiguous-argument message).` -- noted as
  remaining Phase A follow-ups.

### A7 — pretty-printing engine

Implemented per [07-pretty-engine.md](07-pretty-engine.md).

### A6 — `rev-list` / `log` options

Implemented per [06-rev-list-log-options.md](06-rev-list-log-options.md).

### A8 — diff options completion

Implemented per [08-diff-options.md](08-diff-options.md).

### A9 — userdiff hunk headers

Implemented per [09-userdiff-hunk-headers.md](09-userdiff-hunk-headers.md).

### A10 — `count-objects -v` close-out

Verified per [10-count-objects-v.md](10-count-objects-v.md).

### A11 — `.gitignore` + attributes engine

Implemented per [11-gitignore-attributes.md](11-gitignore-attributes.md).

### A12 — local timezone dates/idents

Implemented per [12-local-timezone-dates.md](12-local-timezone-dates.md).

### A13 — pack delta compression on write

Implemented per [13-pack-delta-compression.md](13-pack-delta-compression.md).
