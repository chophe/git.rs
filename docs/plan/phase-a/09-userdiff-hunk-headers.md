# A9 — Function-Context Hunk Headers (userdiff)

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §D
Phase 5 "Remaining": "Function-context hunk headers (userdiff drivers) —
unified output matches git only for text without them".

## Goal

Make `@@ ... @@ <function context>` lines byte-identical to C git by porting
the userdiff driver table and the xdiff context-emission logic. This is the
last known gap in the plain-text unified renderer.

## Current Rust state (observed)

- `git-diff` renders unified hunks without function context; the phase-5
  crosswise suite passes only because test inputs avoid languages with
  default drivers.

## C reference

- `userdiff.c` (the builtin driver table: `PATTERNS(...)` entries for ~40
  languages plus `IPATTERN` variants; each driver has a funcname regex and
  word-regex), `xdiff/xemit.c` (`xdl_emit_diff`, where the funcname line is
  produced via `xdl_emit_hunk_hdr` and the driver's regex match over the
  pre-context), `xdiff-interface.c` (`ff_regexp` — the funcname matching
  callback).
- Gate: `t/t4018-diff-funcname.sh` (this suite has ~500 cases — it is the
  definitive oracle), `t/t4034-diff-words.sh` only if word-regexes are also
  ported (they belong to word-diff in A8; out of scope here).

## Deliverables

1. Builtin driver table ported to Rust data (name, funcname regex,
   `WORD_REGEXP`, flags like `!`-negation semantics). Regexes use
   POSIX-ERE-with-POSIX-Extended-syntax quirks (xdiff uses `regexec` with
   `REG_NEWLINE`); the chosen Rust regex engine must handle these — validate
   early, and if not, use a POSIX-ERE engine.
2. Hunk-header emission: after each hunk, walk back to the last line matched
   by the driver's funcname regex in the *pre-image* and emit its
   first-line content, truncated per C's rules; blank/`@@` only when no
   driver applies.
3. `--no-prefix`/`-p<n>`/diff attributes interplay: driver selection via
   `.gitattributes diff=<name>` (depends on A11's attributes engine — if A11
   hasn't landed, support the builtin extension-mapping only and note the
   gap) and `diff=<name>` config.
4. `funcname` config override (`diff.<driver>.xfuncname` / `.funcname`).

## Sub-tasks (ordered)

1. Extract `t/t4018` case list as the fixture set (its `funcname/*` test
   files are inputs; expectations are per-language).
2. Port the driver table with a build-time check that every C driver has a
   Rust counterpart (parse the C source in xtask or hand-verify with a
   checklist test).
3. Implement `ff_regexp`-equivalent matching + `xemit` header emission.
4. Wire attributes-based driver selection once A11 exists.

## Test gates

- `t/t4018-diff-funcname.sh` through the shim — the target is 100% of its
  cases, it is self-contained.
- Existing phase-5 crosswise suites must remain byte-identical.

## Risks / notes

- POSIX ERE vs Rust `regex` crate differences (backreference-free but
  leftmost-longest vs leftmost-first matching) can flip funcname matches on
  driver regexes that use alternation; verify with the `t/t4018` corpus
  before committing to a regex engine.
