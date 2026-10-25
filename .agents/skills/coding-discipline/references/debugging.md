# Evidence-first debugging protocol

Distilled from real failures — a test suite that hung at 180 s four times before the root cause surfaced.

## 1. Reproduce cheaply and deterministically
Before any theory, get the failure to appear on demand. If each run costs minutes, shrink the reproduction first — every later step pays that cost.

## 2. Bound every await before debugging hangs
Unbounded promises hang silently and burn the whole timeout. Wrap raw `open()`/handshake promises in a race with an explicit timeout whose error names the URL or socket. The first bounded failure usually localizes the stall for free.

## 3. Instrument at the stall point, not around it
Step logs between every await in the failing span (`[step] viewer connect` → `[step] viewers open` → …). Buffered test output preserves order across a timeout kill. The last printed step is the stall; reasoning about anything after it is noise.

## 4. Bisect legs with observers
A pipeline client → dispatch → handler → page → wire → consumer: attach a cheap observer at each leg and find the last leg that still sees traffic. Observe real artifacts — outgoing socket frames, dispatch calls, DOM events — not assumptions about them. Hooking `WebSocket.prototype.send` and logging every frame beats inferring from state.

## 5. Differential debugging
When it passes in isolation but fails composed, vary one factor at a time (one seat vs two, fresh server vs loaded, synthetic DOM event vs CDP-injected input). The smallest difference that flips pass/fail contains the bug.

## 6. Treat the driver as guilty until proven innocent
Most "flaky" or "hanging" failures live in the test or driver, not the system:
- **Stale matches.** `.some()` over an accumulative event array matches the first run's event forever. Demand freshness — filter-count deltas ("was 1 accepted result, now 2").
- **Lying types.** A `JSON.stringify(...)` string cast to an object type makes every downstream field `undefined`; `JSON.stringify(NaN)` is `null`, and validators silently drop it. Parse, then narrow.
- **Unresolved promises.** A handshake that only listens for success hangs forever on failure. Every promise needs a reject path whose message names its subject.

## 7. Fix root cause, one at a time
Patch the single confirmed cause, re-run, and let the next failure surface. Batching speculative fixes destroys attribution.

## 8. Prove the fix with the real workflow
The affected workflow runs end to end: real browser, real wire, real persistence — with recorded output or screenshots when the behavior is visual. Tests alone are not proof.

## 9. Clean up like you were never there
Probe files, flood scripts, temp databases, leaked headless processes — remove them after green. Kill leaked seats by matching their distinctive launch flags, never a blanket process-name kill (a blanket kill hits the user's real apps).
