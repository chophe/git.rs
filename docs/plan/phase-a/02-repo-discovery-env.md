# A2 — `--git-dir` / `--work-tree` / Environment Threading

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §C
("git-command uses std::env::set_current_dir-free design; repo discovery
always starts at CWD — --git-dir/--work-tree CLI overrides are not yet
threaded through commands").

## Goal

Introduce a single shared **repo context** that every command resolves once
and receives explicitly: `GIT_DIR`/`--git-dir`, `GIT_WORK_TREE`/`--work-tree`,
`GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`/`GIT_ALTERNATE_OBJECT_DIRECTORIES`,
and the `--bare`/subdirectory-relative semantics of C git. Do this **before**
any Phase B commands are written so they are born correct.

## Current Rust state (observed)

- `git_core::Repository::discover()` is used directly by commands
  (e.g. `count_objects.rs`), always starting from CWD.
- No command accepts `--git-dir`/`--work-tree`; no `GIT_*` env handling.
- `git-command` deliberately avoids `set_current_dir`; that constraint should
  be preserved (the context struct, not process cwd, carries paths).

## C reference

- `setup.c` (`setup_git_directory_gently`, prefix/worktree logic),
  `environment.c` (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
  `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`,
  `GIT_COMMON_DIR`).
- `builtin/rev-parse.c` documents the resolution rules the port must match
  (`--git-dir`, `--show-toplevel`, `--is-inside-work-tree`,
  `--git-common-dir` — already parsed but not fed from env/CLI overrides).

## Deliverables

1. `git_command::RepoContext` (or an extension of `git_core::Repository`)
   built once in `dispatch()`: git_dir, common_dir, work_tree (Option),
   index file override, object dir + alternates.
2. Global-arg parsing (`--git-dir=...` / `--git-dir ...`, `--work-tree`,
   `--bare`, `-C <dir>`, `-c <name>=<value>`) matching C git's option
   positions (global flags may appear before the command).
3. Thread the context through every routed command module (mechanical but
   wide; `Repository::discover()` call sites become context consumers).
4. Env var support with C precedence order: CLI flag > env var > discovery.

## Sub-tasks (ordered)

1. Write the crosswise tests first: `git --git-dir=X --work-tree=Y status`
   from a subdirectory, `-C`, `GIT_DIR`-only invocations, bare-repo invocations,
   and `rev-parse` reporting the resolved paths — all byte-identical to C.
2. Implement `RepoContext` + resolution precedence; keep CWD untouched.
3. Convert commands one at a time (start with `rev-parse` since it *reports*
   the context, then `status`, `diff`, `cat-file`, the ref family, the ODB
   family); run that command's crosswise suite after each conversion.
4. `-c name=value` config overlay (feed `git-config`'s in-memory layer).
5. Update FOLLOWUPS.md §C item to DONE.

## Test gates

- New crosswise suite (suggest `phaseA02_crosswise.rs`).
- `t/t1500` (rev-parse), `t/t1501` (worktree) relevant cases via the shim.
- All existing suites stay green (this is the regression detector for the
  mechanical refactor).

## Risks / notes

- Purely mechanical but touches every command module; keep the refactor
  commit(s) behavior-preserving and land the *new* flags in a follow-up
  commit so a bisect can separate refactor from feature.
- Alternates (GIT_ALTERNATE_OBJECT_DIRECTORIES) must reach `git_odb::Odb`
  construction, not just path strings.
