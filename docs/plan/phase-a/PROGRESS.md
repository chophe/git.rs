# Phase A Implementation Log

Running log of implemented Phase A items ("Deepen What's Routed", see
[00-overview.md](00-overview.md)). Each entry records what was implemented,
where it landed, and how it was verified. Newest entries at the bottom.

| Date | Item | Status |
|---|---|---|
| 2026-08-29 | A2 repo discovery / `--git-dir` / `--work-tree` | DONE |
| 2026-08-29 | A1 sha1dc collision-detecting SHA-1 | planned |
| 2026-08-29 | A3 `cat-file --batch` / `--batch-check` / `%(format)` | planned |
| 2026-08-29 | A4 abbreviation resolution | planned |
| 2026-08-29 | A5 `rev-parse` completion | planned |
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

### A1 — sha1dc collision-detecting SHA-1

Implemented per [01-sha1dc.md](01-sha1dc.md).

### A3 — `cat-file --batch` / `--batch-check` / `%(format)`

Implemented per [03-cat-file-batch.md](03-cat-file-batch.md).

### A4 — abbreviation resolution

Implemented per [04-abbrev-resolution.md](04-abbrev-resolution.md).

### A5 — `rev-parse` completion

Implemented per [05-rev-parse-completion.md](05-rev-parse-completion.md).

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
