# Phase 4 — Object Model & Revision Walking

## Goal

Parse and format the four object kinds (blob, tree, commit, tag), walk trees,
pretty-print commits (including trailers and mailmap), and implement the
revision-walking machinery (`rev-list`, `log`, `show`, path-limited walks,
topological and date ordering, graft/replace).

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-object` — blob/tree/commit/tag parse+format | `blob.c`, `tree.c`, `commit.c`, `tag.c` |
| `git-object` — `TreeWalker`, entry iteration | `tree-walk.c` |
| `git-object` — `Pretty` (format specifiers) | `pretty.c`, `log-tree.c` |
| `git-object` — `Trailer` | `trailer.c` |
| `git-object` — `Mailmap` | `mailmap.c` |
| `git-revision` — `RevWalk` (marking, sorting, path limiting) | `revision.c`, `builtin/rev-list.c` |
| `git-revision` — `ListObjects` (+ filters) | `list-objects.c`, `list-objects-filter*.c` |
| `git-revision` — `Graph` (line graph) | `graph.c`, `decorate.c`, `path-walk.c` |

## On-disk formats / surfaces

- Tree format: `<mode> <name>\0<oid>` entries, mode strings, directory entry
  sorting rules.
- Commit format: header lines, continuation lines, `gpgsig`/`gpgsig-sha256`
  blocks, blank-line separated message, trailers.
- Tag format: object/type/tag/tagger headers + message.
- Pretty format specifiers (`%H %h %s %an %ae %ad %aD %d %N` and friends),
  `--format`/`--pretty`, mailmap rewrite.
- Revision CLI surface: `rev-list` flags, `--topo-order`, `--date-order`,
  `--reverse`, `--first-parent`, `--ancestry-path`, `--parents`, grafts,
  replace refs, path limiting, `--no-walk`, stdin revisions.

## Commands enabled

- `git rev-list`, `git log` (basic), `git show`, `git cat-file -p`
- `git ls-tree` (tree walking)

## Fully automated test plan

### Unit
- **Tree:** parse/serialize round-trip (mode strings, entry names incl.
  non-UTF-8 bytes, dir-entry sorting, tree object ordering per git rules);
  reject malformed entries.
- **Commit:** parse headers + continuation lines + `gpgsig` block + message +
  trailers; serialize round-trip.
- **Tag:** parse/serialize round-trip.
- **Pretty:** every implemented format specifier; `--format` rendering;
  `%ad` with `--date=` variants; mailmap application.
- **RevWalk:** on hand-built small DAGs assert topo order, date order, reverse,
  `--first-parent`, `--ancestry-path`, marking (UNINTERESTING), and path
  limiting exactly.

### Property
- Random nested trees/commits round-trip.
- **Random DAG generator:** `rev-list` output for
  `--topo-order`/`--date-order`/`--reverse`/`--ancestry-path`/`--first-parent`
  equals C byte-for-byte (differential property).
- Random path-limited walks equal C.

### Differential
- `rev-list`, `log`, `show`, `cat-file -p`, `ls-tree` on fixture repos and
  generated DAGs; byte-identical output.

### `git-test` additions
- `test-revision-walking`, `test-reach` (reachability answers vs C).

### Fuzz
- Tree/commit/tag parsers; corpus from `t5303`-style corruption fixtures.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-object, git-revision.
- **t/ scripts pass 100%:** `t6000`–`t6019`, `t6100s`, `t4200s` subset
  (log/format), `t4203` (mailmap), `t3100s` (ls-tree).

## Risks

- Revision-walking ordering and marking semantics are the subtlest correctness
  surface in git — this is why the DAG property tests are mandatory.
- `pretty.c` is ~60K lines; keep the format-specifier matrix explicit and
  differential-tested rather than ported wholesale.
- Non-UTF-8 tree entry names must round-trip byte-exactly.
