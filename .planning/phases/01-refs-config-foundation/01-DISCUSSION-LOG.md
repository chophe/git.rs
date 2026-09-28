# Phase 1: Refs & Config Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-09-28
**Phase:** 1-Refs & Config Foundation
**Areas discussed:** Reflog surface, Transaction safety, Config scope, Includes & scopes

---

## Reflog surface

| Option | Description | Selected |
|--------|-------------|----------|
| Show+log only (Recommended) | show + log read path only; expire/delete deferred | |
| Full with expire | Full surface incl expire --expire=now/--all and delete | ✓ |
| You decide | Let researcher/planner pick subcommand split from t1410 gates | |

**User's choice:** Full with expire
**Notes:** Q1 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| All commands log (Recommended) | Every ref mutation logs from day one incl branch/tag/update-ref/clone | ✓ |
| Core four first | Only checkout/commit/reset + update-ref log now | |
| You decide | Let planner decide logging order from t1410 | |

**User's choice:** All commands log
**Notes:** Q2 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact gating (Recommended) | HEAD always, branch only when logallrefupdates allows | ✓ |
| Always log both | Always log both HEAD and branch | |
| You decide | Let planner follow C source + t1410 | |

**User's choice:** C-exact gating
**Notes:** Q3 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C defaults (Recommended) | C defaults honored via config (gc.reflogExpire etc) | ✓ |
| Explicit only | Expire only via explicit flags | |
| You decide | Let planner pick from C source | |

**User's choice:** C defaults
**Notes:** Q4 of 4 in area.

---

## Transaction safety

| Option | Description | Selected |
|--------|-------------|----------|
| Atomic C-exact (Recommended) | All-or-nothing with C-exact lock/transaction errors | ✓ |
| Best-effort | Per-ref apply | |
| You decide | Let planner follow t1400/t3210 | |

**User's choice:** Atomic C-exact
**Notes:** Q1 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| --stdin full (Recommended) | Full --stdin with create/update/delete/verify lines | ✓ |
| Single-ref only | Single-ref update-ref only in Phase 1 | |
| You decide | Let planner split from t1400 | |

**User's choice:** --stdin full
**Notes:** Q2 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| Include packed write (Recommended) | Lock files + atomic rename + packed-refs write in Phase 1 | ✓ |
| Loose only | Loose-ref locking only | |
| You decide | Let researcher check current packed-refs writer | |

**User's choice:** Include packed write
**Notes:** Q3 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact errors (Recommended) | Byte-exact lock-failure messages and exits | ✓ |
| Simplified text | Simplified errors; same exit codes | |
| You decide | Let planner match C stderr | |

**User's choice:** C-exact errors
**Notes:** Q4 of 4 in area.

---

## Config scope

| Option | Description | Selected |
|--------|-------------|----------|
| Full matrix (Recommended) | get/set/unset/list/add/replace-all/remove-section/show-origin/type | ✓ |
| Basics only | Only get/set/unset/list now | |
| You decide | Let planner split from t1300 | |

**User's choice:** Full matrix
**Notes:** Q1 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| All scopes (Recommended) | System/global/local/worktree with C precedence | ✓ |
| Local+global | Local + global only in Phase 1 | |
| You decide | Let planner follow C lookup order | |

**User's choice:** All scopes
**Notes:** Q2 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact (Recommended) | Multivar + --null/--fixed-value/--get-urlmatch byte-exact | ✓ |
| Single-value | Single-value get/set only | |
| You decide | Let planner check t1300 multivar cases | |

**User's choice:** C-exact
**Notes:** Q3 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact errors (Recommended) | Invalid files fail with C-exact fatal text and 128 | ✓ |
| Lenient | Lenient parse; same codes, looser text | |
| You decide | Let planner match C stderr | |

**User's choice:** C-exact errors
**Notes:** Q4 of 4 in area.

---

## Includes & scopes

| Option | Description | Selected |
|--------|-------------|----------|
| Full includeIf (Recommended) | include.path + includeIf conditional all honored | ✓ |
| Plain only | Plain include.path only | |
| You decide | Let researcher check ConfigSet include support | |

**User's choice:** Full includeIf
**Notes:** Q1 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact paths (Recommended) | Relative includes resolve from including file; ~ expansion | ✓ |
| Absolute only | Absolute paths only in Phase 1 | |
| You decide | Let planner follow C path logic | |

**User's choice:** C-exact paths
**Notes:** Q2 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C precedence (Recommended) | CLI -c and GIT_CONFIG_COUNT beat files; system→global→local→worktree | ✓ |
| Simplified | Simplified: CLI beats local only | |
| You decide | Let planner lock from C source | |

**User's choice:** C precedence
**Notes:** Q3 of 4 in area.

| Option | Description | Selected |
|--------|-------------|----------|
| C-exact guard (Recommended) | Detect include cycles and fail C-exactly with depth cap | ✓ |
| No guard | No cycle handling in Phase 1 | |
| You decide | Let planner check C limit | |

**User's choice:** C-exact guard
**Notes:** Q4 of 4 in area.

---

## the agent's Discretion

None — user made concrete selections on all 16 questions.

## Deferred Ideas

None — discussion stayed within phase scope.
