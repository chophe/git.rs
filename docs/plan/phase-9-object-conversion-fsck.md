# Phase 9 — Object Conversion & fsck

## Goal

Implement sha1↔sha256 object conversion (`compatObjectFormat`), the loose
object map (`LMAP` binary format), and full `fsck` (reachability + corruption
reporting). This phase closes the loop on the dual-hash feature that motivated
the original Rust port and makes the whole repository self-checking.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-odb::convert` — `ObjectConvert` (sha1↔sha256 rewrite of tree/commit/tag) | `object-file-convert.c` |
| `git-odb::convert` — signed-content handling (`gpgsig`↔`gpgsig-sha256`) | `object-file-convert.c`, `gpg-interface.c` |
| `git-odb::loosemap` — **LMAP** (port `src/loose.rs` verbatim) | `loose.c` (text format) + `src/loose.rs` (binary LMAP) |
| `git-fsck` — reachability, corruption reporting | `fsck.c`, `reachable.c` |
| `git-odb` — write side: `pack-objects`/`index-pack` (from Phase 2) driving `repack`/`gc` | `builtin/repack.c`, `builtin/pack-objects.c` |

## On-disk formats / surfaces

- `compatObjectFormat` extension + `objects/loose-object-idx`.
- **LMAP** binary format (`LMAP` magic, version, header, per-algorithm
  shortened/full/order tables, trailer checksum) — already specified and tested
  in `src/loose.rs`; port the writer (`ObjectMemoryMap`) and reader
  (`MmapedObjectMap`) and their tests.
- Object conversion rules: tree entries re-hashed, commit/tag headers
  rewritten, signed sections renamed between `gpgsig` and `gpgsig-sha256`,
  grafts applied, `extra-headers` preserved.
- `fsck` message catalog (error/warn classes) and exit semantics.

## Commands enabled

- `git fsck`, `git fsck --strict`
- `git repack` / `git gc` (write side, cruft-aware from Phase 3)
- `git hash-object --literally`, `git index-pack --stdin`
- Dual-hash interop via `rev-parse --output-object-format=` (needs Phase 4 + 7)

## Fully automated test plan

### Unit
- **OID mapping:** map sha1→sha256 and sha256→sha1 for known objects; empty
  tree/blob/null OIDs mapped via reserved entries (port `src/loose.rs` tests).
- **LMAP:** write→read round-trip, short-name-length computation,
  binary-search lookup, padding alignment, trailer validation, corruption
  rejection — port the existing `loose.rs` unit tests verbatim.
- **Object conversion:** tree re-hash, commit/tag header rewrite, signed-section
  rename (`gpgsig`↔`gpgsig-sha256`) preserves/rewrites exactly, unknown object
  types rejected, cycles handled.
- **fsck:** each error/warn class, message text and exit code parity with C;
  corrupt blob/tree/commit/tag detection; dangling vs unreachable vs missing.

### Differential
- **`t1016-compatObjectFormat` is the primary automated differential test** —
  run it against the Rust binary (it already builds both sha1 and sha256 repos,
  signs commits/tags, and cross-verifies OIDs, types, sizes, and content in
  both directions). It must pass 100%.
- **fsck:** run Rust `git fsck` and C `git fsck` on the same deliberately
  corrupted fixture repos; byte-identical output and exit codes.

### Crosswise
- Rust-written `objects/loose-object-idx` (LMAP) read by C, and C-written read
  by Rust (t1016 covers both directions).
- Rust `repack`/`gc` output verified by C `git fsck` and `git gc --verify`.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-odb (convert + loosemap) and git-fsck.
- **t/ scripts pass 100%:** `t1016` (full), `t1450` (fsck), `t1451`,
  `t1452`; full-suite sweep against the committed scoreboard baseline.

## Risks

- Signed-object conversion is security-sensitive: signatures must be
  re-derived/rewritten exactly or verification breaks (t1016 GPG2 cases).
- LMAP must stay byte-compatible with the format validated by the upstream
  Rust tests in `src/loose.rs`.
- fsck message catalog parity matters for tooling that parses `git fsck`
  output.
