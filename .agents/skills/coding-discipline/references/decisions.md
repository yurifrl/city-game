# Decision heuristics

Defaults when the choice is open. Overrule with a written reason, never silently.

## Shape of work
- **Vertical slices.** One thin end-to-end path through real systems beats a broad scaffold. Playable and honest beats complete and fake.
- **Smallest change that proves the behavior.** If a feature needs an architectural shortcut, cut the slice, not the architecture.
- **Boring over clever.** Direct functions, explicit data, narrow APIs. An abstraction needs a current boundary or repeated behavior to exist — one caller is not a boundary.

## When two options
- Pick the one that keeps the domain independent of infrastructure, time, and UI.
- Pick the one with fewer concepts. Deleting code beats refactoring around it.
- Pick the reversible one; record the decision so it can be revisited.
- Measure before optimizing anything. Profile numbers beat intuition and benchmarks-of-strangers alike.

## Correctness postures
- Fail loudly, shed load explicitly. A silent fallback converts a bug into a mystery.
- Validate at the boundary (untrusted input, wire, uploads); trust internally-typed data.
- Deterministic where it pays (explicit ordering, seeded RNG streams); never claim bitwise determinism without proof.
- Concurrency: explicit ordering, bounded queues, one logical writer per state. Last-writer-wins on destructive edits is a bug.

## Delivery
- Done = specified end-to-end behavior plus every acceptance criterion, verified by running it. Not compiling, not "mostly".
- State uncertainty at the claim. `[INFERENCE]` marks anything not directly observed.
- User-reported behavior is ground truth; never re-run their failure to confirm it.
- Ask only what tools cannot answer. Otherwise pick the conservative default and state the choice.
