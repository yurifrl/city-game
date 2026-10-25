---
name: coding-discipline
description: Use when writing, organizing, or reviewing production code; when debugging hangs, timeouts, silent failures, or tests that pass suspiciously; when deciding where new files, modules, or packages live; and before claiming any change complete.
---

# Coding Discipline

## Overview

Five habits, applied in order: **read before write, prove before fix, bound everything, one owner per concept, leave no trace.** Code that cannot show evidence of working has not worked. Every concept has one owner; every await has a deadline; every fix ends with the scaffolding gone.

## Directives

Precedence: this skill expands the always-on discipline rules in `AGENTS.md` (global and repo). Repo constitutions (`AGENTS.md` in a project, `docs/architecture.md`) win for project-specific boundaries; this skill wins only where they are silent. Never silently weaken an AGENTS.md rule to satisfy this skill.

### Evidence before action
- Read the relevant code before editing anything. Never patch a guess.
- Reproduce a bug before fixing it. A fix without a failing state is a hypothesis.
- Verify with the real affected workflow — real browser, real wire, real database — not typechecks or mocks alone.
- Report only what you exercised. Mark inferences `[INFERENCE]`.

### Bound everything
- Every await, queue, loop, and wait has a bound. An unbounded promise is the prime suspect for any hang.
- Slow consumers never block producers: bounded mailboxes; drop or shed explicitly; never block the writer.
- A sleep is never synchronization. Wait on the observable event ("the wire carried the event"), not the clock.

### One owner, one source of truth
- One concept → one owner → one home. Before creating anything, find the existing implementation and reuse or extend it.
- The domain never imports infrastructure (HTTP, DB, UI, wall-clock). Infrastructure depends on the domain, never the reverse.
- Name things by domain meaning, not by layer or pattern (`AssetCatalogue`, not `DataManager`).

### Explicit states and honest types
- Model failures as states the reader can see. No silent fallbacks, no swallowed errors, no defaults that hide absence.
- A type that lies is worse than no type. If a value is a string, type it string — parse, then narrow.
- Validation may reject, never silently drop: a dropped frame or dropped input must be observable somewhere.

### Assertions observe fresh outcomes
- Assert on observable outcomes (wire events, DB rows, rendered pixels), never internal wiring or incidental text.
- Require per-item freshness: each input acknowledged by its own matching output. A counter delta ("was 1 accepted receipt, now 2") beats `.some()` over an accumulative array — stale matches pass forever.
- Keep focused regressions for plausible bugs: duplicate edits, double spending, stale state, save/reload.

### Clean cutover, no trace
- Replace: migrate every caller, delete the old path. No shims, no dead code, no `TODO: implement`.
- After green: remove probe scripts, temp fixtures, leaked processes. The tree should look like the feature was always there.
- Docs move with contracts: change a boundary → update the decision doc in the same change.

## Quick reference

| Situation | Move |
|---|---|
| Hang, timeout, no output | Find the unbounded await; instrument the exact stall point with step logs |
| Passes alone, fails composed | Differential: vary one factor at a time; probe legs in isolation |
| "Fixed" but can't say why | No root cause = not fixed. Keep bisecting with observers |
| New module — where? | Nearest existing sibling that can own it; new package only for a current enforced boundary — one consumer is not a boundary |
| Test passed suspiciously fast | Stale match, wrong target, or asserted wiring instead of behavior |

## Red flags — stop and re-check

- Writing code before reading what's there
- "It should work" without a run
- Catch-and-continue on an unexpected error
- An abstraction with one caller and no boundary
- A test that never could have failed
- Leftover probe files, temp dirs, orphan processes

## References

- [references/debugging.md](references/debugging.md) — the evidence-first debugging protocol
- [references/organization.md](references/organization.md) — repo layout, module organization, where things live
- [references/decisions.md](references/decisions.md) — decision heuristics and tradeoff defaults
