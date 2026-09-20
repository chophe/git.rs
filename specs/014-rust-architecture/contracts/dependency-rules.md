# Contract: Dependency Rules

**Feature**: `specs/014-rust-architecture/spec.md` (FR-022) | **Date**: 2026-09-20

## Layer order (arrows point downward only)

```text
surface (git-command, git-cli)
  → access (git-transport, git-protocol, git-credentials, git-hooks, git-worktree, git-compress)
  → language (git-revision, git-pathspec, git-diff, git-merge, git-attributes)
  → store (git-odb, git-refs)
  → mid (git-object, git-core, git-config, git-index, git-commitgraph, git-pretty)
  → foundation (git-hash, git-varint, git-date)
```

`xtask` is automation: it may read the workspace but no runtime crate may depend on it, and production code never depends on test utilities.

## Rules

1. **Acyclic**: the internal path-dependency graph MUST contain zero cycles (verified baseline 2026-09-20: 40 edges, acyclic).
2. **Downward-only**: a component may depend only on its own layer or lower layers. Upward edges (e.g. store → transport, config → discovery) are violations — this is what keeps future fetch (access) from inverting the revision-walking (language/store) arrow.
3. **Composition-root exception**: `git-command` is the only crate allowed broad dependencies; `git-cli` depends on `git-command` only.
4. **Declared use**: each component declares its dependencies in its `Cargo.toml`; undeclared use fails the interface check (no `extern` smuggling, no dev-dependency leakage into production paths).

## Violation semantics

A new edge that creates a cycle or a layer violation MUST fail the dependency gate (`contracts/gates.md`), naming the offending change and the forbidden arrow. Merging such a change requires a ratified spec amendment, not a silent exception.
