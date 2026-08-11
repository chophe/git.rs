# Phase 1 — Loose Objects (Part 1)

## Goal

Read and write loose objects (`objects/xx/yyyy...`) with full format
compatibility and crosswise interop with C git. Enable `hash-object`,
`cat-file`, `mktree`/`commit-tree` on a minimal level.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-odb`: `Odb`, `LooseStore`, `ObjectReader`, `Alternates` | `object-file.c`, `odb.c`, `odb/source-loose.c` |
| `git-object`: `ObjectKind`, header parse/format, `Object` | `object.c`, `blob.c` |
| `git-varint` (port `src/varint.rs` verbatim) | `varint.c` |

## On-disk formats / surfaces

- Loose object path fan-out: `objects/<first-2-hex>/<remaining-hex>`.
- Loose header: `<type> <size>\0` (types: blob, tree, commit, tag), size
  encoded with varint.
- zlib (deflate/inflate) content (use `flate2`; match git's zlib usage — single
  stream, no gzip header).
- `objects/info/alternates` (relative/absolute paths, `#` comments).
- Object ID computation: `<type> <size>\0<content>` hashed with the repo hash
  algorithm.

## Commands enabled

- `git hash-object [-w] [-t <type>] [--stdin]`
- `git cat-file -t / -s / -p / <hash>`
- `git mktree`, `git commit-tree` (minimal, no signing)

## Fully automated test plan

### `git-varint`
- **Unit:** decode/encode round-trip incl. overflow → 0 (port the tests already
  in `src/varint.rs`).
- **Property:** random `u64` round-trips; arbitrary bytes never panic on decode.

### Loose header
- **Unit:** `<type> <size>\0` parse for all four kinds; reject bad type, extra
  NUL, truncated header, absurd size.
- **Property:** random kind + size round-trips through the header formatter.

### Loose read/write (`git-odb`/`LooseStore`)
- **Unit:** write→read round-trip per kind; zlib streaming read of objects
  larger than the internal buffer; perms/umask + `core.sharedRepository`;
  correct fan-out path; refusing to write corrupt sizes.
- **Property:** random blobs 0..64 MiB round-trip; read-back equals original
  bytes (proptest, streaming path).
- **Corruption resilience:** truncated file, garbage header, oversized size,
  wrong directory, missing file → typed errors, never a panic (mirror the
  `t5303` corruption cases as Rust unit tests).

### Crosswise (both directions, automated)
- **C → Rust:** build a fixture repo with C `git hash-object -w` /
  `git commit-tree` / `git mktree`; Rust `cat-file -t/-s/-p` equals C output
  for every object.
- **Rust → C:** Rust `hash-object -w` / `mktree` / `commit-tree` writes; C
  `git cat-file`, `git fsck --strict`, `git count-objects` all pass on the
  resulting repo.

### Alternates
- **Unit:** relative and absolute alternate paths resolve correctly; missing
  alternate is an error, not a panic.
- **Differential:** repos with `objects/info/alternates` → Rust `cat-file`
  equals C for objects reachable via alternates.

### `git-test` additions
- `test-zlib` (inflate/deflate boundaries), `test-sha1`/`test-sha256`
  (already in Phase 0), `test-varint`.

### Fuzz
- Loose-object header parser and reader, corpus seeded from `t5303` and
  `tests/fixtures` corruption cases.

## Gate criteria

- `cargo xtask test` green (unit + doc + proptest).
- Coverage ≥ 90% on git-odb, git-object, git-varint.
- `cargo xtask differential` green (differential + crosswise suites).
- `cargo xtask scoreboard` no regression.
- **t/ scripts pass 100%:** `t1006` (cat-file), `t1007` (hash-object), `t5300`
  (loose half), `t5303` (corruption resilience), `t5400` (commit-tree),
  `t5500` (fetch-pack setup deps), `t1016` subset (needs later phases; skip
  until Phase 9).

## Risks

- Streaming zlib boundary handling (git streams; buffering must not corrupt
  large objects).
- `hash-object -w` must respect `core.sharedRepository` and the object
  directory being absent → created.
- Writing an object must be idempotent (existing object → no error).
