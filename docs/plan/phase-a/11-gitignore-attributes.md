# A11 — `.gitignore` + `.gitattributes` Engine

Parent plan: [00-overview.md](00-overview.md). Hard prerequisite for Phase B
(`add`, `clean`, `status --ignored`, `ls-files --others`) and for A9's
attribute-driven funcname selection. The gap analysis found **zero**
ignore/attribute handling in the port today.

## Goal

A shared crate (suggest `git-attributes`) implementing:
1. `.gitattributes` parsing, attribute lookup (`builtin/check-attr` parity),
   and the `diff=` / `text` / `binary` / `eol` attributes consumed by diff
   and future clean/smudge filters.
2. Ignore machinery: `.gitignore`, `.git/info/exclude`,
   `core.excludesFile`, negative patterns, `**` semantics, per-directory
   stacking, trailing-slash dir-only rules, `!` negation — matching C's
   *precedence and deepest-match* rules exactly.

## Current Rust state (observed)

- Nothing: grep found no `gitignore` reference anywhere in `crates/`.
- `ls-files`/`status`/`diff` have no untracked/ignored awareness beyond
  `status --porcelain` content-comparison.

## C reference

- `dir.c` (the whole ignore engine: `is_excluded`, `add_patterns`,
  `last_matching_pattern`, traversal ordering) — note: C git keeps ignore
  logic in `dir.c`, there is no `ignore.c`.
- `attr.c` (attribute stack: `builtin/` → `info/attributes` → in-tree
  `.gitattributes` per directory → command line `-a`).
- `builtin/check-ignore.c`, `builtin/check-attr.c` (CLI surfaces).
- Gates: `t/t0008-ignores.sh` (ignore semantics, very thorough),
  `t/t0003-attributes.sh`, `t/t2006-check-file-attributes.sh`.

## Deliverables

1. `git-attributes` crate:
   - Pattern compiler shared by both features (C's `wildmatch` is already
     indirectly referenced; port the `WM_PATHNAME` semantics — `*` does not
     cross `/` in pathname mode).
   - Attribute stack lookup API returning C-parity results (`specified`,
     `unset`, `set`, `value:...`).
   - CLI: `git check-attr [-a|--all] attr... -- path...`, `git check-ignore
     [-v] [-n] [--stdin]`.
2. Ignore API: `is_excluded(path, is_dir, per-directory context) -> match
   with source pattern` — the `-v` output of `check-ignore` requires knowing
   which file+line matched.
3. Integration: `status` (`--ignored`, untracked-dir collapse), `ls-files
   --others --exclude-standard`, `diff`/`log` attribute lookups (A9 hook),
   `update-index` honoring existing attr state.

## Sub-tasks (ordered)

1. Port `wildmatch` (`WM_PATHNAME`, `WM_CASEFOLD`) as a standalone module
   with proptests (round-trip against a reference table of C behaviors).
2. Ignore engine + `check-ignore` CLI; `t/t0008` byte-parity.
3. Attributes engine + `check-attr` CLI; `t/t0003`.
4. `status --ignored` / `ls-files --others --exclude-standard` integration
   (completes the consumer side; `status` full rewrite stays in B6 — here
   only the ignore-aware untracked listing).

## Test gates

- `t/t0008-ignores.sh`, `t/t0003-attributes.sh` through the shim.
- Crosswise: identical directory trees with tricky patterns (`**`,
  negations, dir-only, case sensitivity toggles) → identical
  `check-ignore -v` and `status --porcelain --ignored` output.

## Risks / notes

- Pattern precedence bugs are silent (files just don't get ignored) — the
  `t/t0008` corpus is large; port it wholesale rather than sampling.
- `**` semantics in `wildmatch` differ from the Rust `glob` crate — do not
  substitute a library.
