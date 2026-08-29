# A3 — `cat-file --batch` / `--batch-check` / `%(format)`

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §A3.

## Goal

Full batch-mode support in `git cat-file`, the single most-used plumbing
command by `t/` scripts. Currently only single-object reads are supported.

## Current Rust state (observed)

- `crates/git-command/src/cat_file.rs` (~152 lines): object reads by id from
  loose + packs via `git_odb::Odb` (the Odb read path already works and is
  cross-verified).
- No batch mode, no `%(format)` handling.

## C reference

- `builtin/cat-file.c` (batch state machine, `batch_option` handling),
  `Documentation/git-cat-file.txt` for the exact output grammar.
- Gate: `t/t1006-cat-file.sh`.

## Deliverables

1. `--batch` protocol: `oid <type> <size>\n` header line, content bytes,
   trailing newline; `missing` line for absent objects (exact C format,
   including `\0`-separated input mode with `-z`).
2. `--batch-check` protocol: `oid <type> <size>\n` only.
3. `--batch-check=%(objectname) %(objecttype) %(objectsize)` and friends:
   the `%(atom)` formatter subset C supports (`%(objectname)`,
   `%(objectsize)`, `%(objectsize:disk)`, `%(deltabase)`, `%(rest)`,
   `%(objecttype)`), including `--batch-all-objects` (ordered iteration over
   loose + packed OIDs) and `--buffer`.
4. Argument forms C accepts: OID, `HEAD`-style refs, `<type> <oid>` pairs,
   `:<path>` / `<rev>:<path>` index paths (needs the abbrev/ref resolution
   helper from A4 — coordinate or sequence after it).

## Sub-tasks (ordered)

1. Freeze the exact output grammar by extracting the relevant expectations
   from `t/t1006` into a crosswise test fixture list (input lines → expected
   byte output, including missing-object and `--allow-unknown-type` cases).
2. Implement the stdin line-reader state machine (with `-z` and `--buffer`
   semantics) over `git_odb::Odb` reads.
3. Implement `%(atom)` formatting; share it later with `for-each-ref` if the
   atom sets overlap (they don't fully — keep it cat-file-local first).
4. `--batch-all-objects`: iterate loose OIDs (sorted, C git sorts via
   `for_each_loose_file_in_objdir`) then packed, deduped, in C's order.
5. Wire `t/t1006` into the scoreboard/shim path.

## Test gates

- `t/t1006-cat-file.sh` through the shim.
- New crosswise suite comparing batch output byte-for-byte on a generated
  repo (`cargo xtask gen-fixtures` output plus pack files).

## Risks / notes

- Whitespace/newline details of the batch protocol are the whole game here;
  the byte-identical crosswise test must come before implementation, not
  after.
- `--batch-check` with `%(rest)` consumes the remainder of the input line —
  easy to get subtly wrong; cover it explicitly.
