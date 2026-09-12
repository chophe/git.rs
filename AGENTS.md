# AGENTS.md

## What this repository is

Two projects share this repo:

1. **The C git source tree** (upstream git at `v2.55.0-540`, root level: `*.c`, `builtin/`, `t/`, `Documentation/`). It is treated strictly as a **reference implementation and behavior oracle** — read the C to understand behavior, run the C binary and `t/` suite to verify the port.
2. **A standalone pure-Rust rewrite of git** under `crates/` — the active work. **No FFI**: the Rust code never links into C git, and C git is never linked into the Rust build. On-disk formats must stay byte-compatible so both implementations interoperate ("crosswise").

The canonical plan lives in `docs/plan/` (start with `docs/plan/README.md`; `FOLLOWUPS.md` is the actionable backlog). `docs/plan/archive/` holds superseded older drafts (`rust_conversion_plan.md`, `rust_test_template.md`) — don't work from those.

## Critical gotchas

- **The root `Cargo.toml` is a stale leftover** (`gitcore` staticlib, not the workspace). The real workspace is `crates/Cargo.toml`. Run **all cargo commands from `crates/`**, e.g. `cd crates && cargo test --workspace`. Running cargo from the repo root operates on the wrong project.
- **`crates/scoreboard.json` is a committed regression baseline** of the `t/` suite results (run via the shim). Never hand-edit it; `cargo xtask scoreboard` regenerates it and fails on regression. Commit an updated baseline only when you intentionally changed behavior.
- `crates/target/` is the Rust build dir; the C tree also has committed working-tree artifacts (`.o` files, `.depend/`, `bin-wrappers/`, `scalar`, `git`) from a previous `make`. Don't confuse these binaries with the Rust-built `crates/target/debug/git`.
- `watch-and-commit.sh` at the root auto-commits everything with AI-generated messages using `--no-verify`. Do not run it; commits in this repo are crafted deliberately.

## Commands (run from `crates/`)

| Task | Command |
|---|---|
| Build the Rust git binary | `cargo build --workspace` → binary at `crates/target/debug/git` |
| All unit + doc + property tests | `cargo test --workspace` |
| Crosswise suites vs C git | `cargo xtask differential` |
| Regenerate golden fixtures | `cargo xtask gen-fixtures` |
| Run `t/` scoreboard, update baseline | `cargo xtask scoreboard` |

The crosswise suites shell out to **system C git** (default `/usr/bin/git`); they must be byte-identical in stdout/stderr/exit code.

### `scripts/shim-git` dispatcher

A wrapper that routes ported commands (`version`, `hash-object`, `cat-file`, `rev-list`, `status`, `apply`, …) to the Rust binary and everything else to the system git (`RUST_GIT` / `SYSTEM_GIT` env vars override paths). It's how the full `t/` suite runs while the port is incomplete.

**When you port a new command, you must**: add it to the `case` list in `scripts/shim-git`, wire it in `git-command::dispatch`, and add a crosswise suite if needed.

### C-side commands

- Build C git: `make` (or `meson`) at repo root — needed for fresh crosswise oracle binaries.
- Run a C test script: `cd t && ./t1234-name.sh`; the whole suite is the oracle. `GIT_TEST_INSTALLED` can point the suite at a different binary (that's how the shim gets injected).
- There is also `.gitlab-ci.yml` and `.cirrus.yml` for the C side; the Rust side has no CI wiring yet beyond the xtask commands.

## Rust workspace architecture

Crates under `crates/`, in dependency order (mirrors `docs/plan/README.md` phase map):

```
git-hash  git-varint  git-date  git-config      ← Phase 0 foundation
git-core  (strbuf-style primitives, StringBuf port of C strbuf)
git-cli   (binary dispatcher, thin)  git-command (per-command implementations)
git-object git-odb (loose/pack/idx/midx/bitmap) git-commitgraph      ← Phases 1–3
git-revision git-diff git-index git-refs git-merge               ← Phases 4–8
xtask  (automation: test/differential/gen-fixtures/scoreboard)
```

- `git-cli/src/main.rs` is just `git_cli::run(std::env::args())`. `git-cli` knows only about `--version`/usage and delegates every real command to `git_command::dispatch(name, args, out)` (`crates/git-command/src/lib.rs`), which returns `Option<Result<(), CommandError>>`.
- Each command lives in its own module (`cat_file.rs`, `rev_parse.rs`, `status.rs`, …) following the existing pattern; add new ports as a new module + dispatch entry.
- `git-cli/src/lib.rs` pins `VERSION` to the C git version being ported — keep it in sync when rebasing on a new upstream.

## Testing conventions

- **Byte-identical differential tests**: integration tests under `crates/*/tests/` (e.g. `pack_crosswise.rs`, `phase*_crosswise.rs`) run Rust vs C git on identical inputs and assert identical stdout/stderr/exit codes. When porting a command, add a crosswise test and register the suite in `xtask::suites()`.
- **Crosswise on-disk compat**: both directions must hold — C git must validate artifacts the Rust port writes (`git fsck`, `verify-pack`, `commit-graph verify`, `multi-pack-index verify`), and the Rust port must read C-git-produced repos.
- **Property tests** (`proptest`) guard parsers/serializers: no panics on arbitrary input, round-trip invariants.
- Phase "done" gates (from `docs/plan/README.md`): workspace tests green, proptests green, differential byte-identical, crosswise compat, ≥90% `cargo llvm-cov` coverage on the phase's crates, no scoreboard regression, phase's listed `t/` scripts pass through the shim.

## Porting rules

- **C git is the spec.** Match C behavior exactly, including exit codes (usage errors exit `129`, "unknown command" exits `1`) and output formats. When the C source and `t/` tests disagree, `t/` is the oracle.
- Known intentional deviations are tracked in `docs/plan/FOLLOWUPS.md` (e.g. `git-hash` SHA-1 is not yet collision-detecting, `pack-objects` writes non-deltified packs, date handling is UTC-only). Don't silently "fix" these — they're logged backlog items.
- `StringBuf` in `git-core` is a deliberate faithful port of C `strbuf` (see `STRINGBUF_IMPLEMENTATION.md`); use it where the port needs strbuf semantics instead of reaching for `String`/`Vec` rewrites.

## Working on the C tree

If a task touches the C source (root-level `*.c`, `builtin/`, `t/`): follow upstream git conventions (`Documentation/`, coding style in `Documentation/MyFirstContribution.txt`); the C tree at `v2.55.0-540` is a full upstream checkout — read `t/README` for the test harness before writing shell tests.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->
