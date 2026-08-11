# Phase 2 — Packs & idx

## Goal

Read and verify packfiles and their index files, resolve delta chains, and
implement the write side (`index-pack` / `pack-objects`). This is the heart of
the object store and the performance-critical path.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-odb::pack` — `PackReader` (mmap, header/trailer) | `packfile.c` |
| `git-odb::pack` — `PackIndexReader` (v2) | `packfile.c`, `pack.h` |
| `git-odb::pack` — `DeltaResolver` (offset/ref delta, thin packs) | `packfile.c` (delta resolution), `delta.h`, `patch-delta.c` |
| `git-odb::pack` — `PackWriter` (`index-pack`, `pack-objects`) | `pack-write.c`, `pack-objects.c`, `pack-objects.h`, `index-pack.c` (builtin) |
| `git-odb::pack` — `RevIndex` | `pack-revindex.c` |
| `git-odb::pack` — `PackCheck` | `pack-check.c`, `verify-pack.c` (builtin) |

## On-disk formats / surfaces

- Pack v2 header (`PACK`, version 2, object count), trailer (20-byte SHA over
  the whole pack), sorted object entries, delta encoding (offset/ref deltas),
  thin packs (base objects outside the pack), zlib-compressed object data.
- Index v2: magic `\377tOc`, version, fanout (256 × 4-byte), object ids (20 or
  32 bytes), CRCs, 4-byte offsets (+ large-offset extension), 20-byte pack
  checksum trailer.
- Multi-pack index is Phase 3.

## Commands enabled

- `git verify-pack`, `git unpack-objects`, `git index-pack` (write),
  `git pack-objects` (write), `git count-objects -v`, `git cat-file --batch`
  reading from packs.

## Fully automated test plan

### Unit
- **Pack header/trailer:** parse version, count, trailer checksum verification;
  reject bad magic/version/truncation.
- **Index v2:** fanout parse, oid lookup, offset bounds, CRC verification,
  large-offset extension handling.
- **Delta resolution:** offset-delta with a base, ref-delta with lookup,
  delta chains of depth > 1, thin-pack missing-base → graceful error,
  delta apply with overflow-safe bounds.
- **`verify-pack`:** checksum + per-object CRC + delta chain verification,
  matching C `git verify-pack` exit/output semantics.

### Property
- Random object sets → C `git pack-objects` produces a pack → Rust reads all
  object ids and content == the input set.
- Rust `pack-objects` output → C `git index-pack --verify` and
  `git verify-pack` pass.
- `cat-file --batch` over every object in a pack: Rust == C.

### Crosswise (both directions)
- **C → Rust:** pack + idx written by C (via `git repack`/`git pack-objects`)
  read fully by Rust; all objects match.
- **Rust → C:** packs + idx written by Rust verified by C
  (`git index-pack --verify`, `git verify-pack`, `git cat-file`).

### Fuzz
- Pack and idx parsers; corpora seeded from `t5302` (index corruption) and
  `t5313` (pack bounds) fixtures. No panic, defined errors on corruption.

### `git-test` additions
- `test-delta` (delta generate/apply round-trip vs C), `test-pack-deltas`,
  `test-find-pack`.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on the pack/idx/revindex modules.
- **t/ scripts pass 100%:** `t5300`–`t5307`, `t5313`, `t5314`, `t5315`,
  `t5316`, `t5320`, `t5321`, `t5322`.
- Crosswise `verify-pack`/`cat-file --batch` green.

## Risks

- Delta chain depth/ordering and memory blowups on hostile packs (bounds,
  chain-depth limits like C).
- Offset-vs-large-offset and 32-bit overflow in idx reading.
- `pack-objects` write side must match C's object selection and delta
  heuristics to produce byte-compatible packs (or at least verify-compatible
  ones); verify-compatible is the acceptance bar, byte-identical is not
  required.
