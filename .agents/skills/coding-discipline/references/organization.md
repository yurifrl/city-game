# Repo organization and file structure

## Top-level shape

Organize by responsibility and dependency direction — domain code first, applications second, contracts and docs third:

```
crates/            # domain + server libraries (Rust) — the authority
  protocol/        #   wire types shared across languages, one source of truth
  simulation/      #   pure domain rules: no network, no I/O, no wall-clock
  server/          #   infrastructure: HTTP, WS, persistence, the single writer
apps/              # clients and services (TypeScript)
  web/             #   renderer, tools, replica — submits intent, owns no rules
  stream/          #   infra service: seat pool, frame relay
docs/
  contracts/       # NORMATIVE protocols (RFC 2119). Implementation defers to these.
  architecture.md  # decisions and their reasons, kept current with the code
  research.md      # evidence, not claims
test/              # e2e proving the real workflow (real browser, real wire)
```

Adapt the names to the stack (packages vs crates), never the rule: authority lives below, intent flows up.

## Rules

- **One concept, one owner, one home.** Duplicated logic and parallel structures are both bugs. Before adding a file, find its sibling; extend it rather than paralleling it.
- **Dependency direction is the architecture.** Domain never imports infrastructure. A cycle between layers means the boundary is wrong — fix the boundary, don't thread around it.
- **Cohesive modules over layers.** Extract a package only for an enforced dependency boundary, an independent build target, or demonstrated reuse — never for imagined futures.
- **Private by default.** Small public surfaces; name types by domain meaning (`ActorId`, not `StringId`).
- **Contracts are normative.** A `docs/contracts/*.md` with MUST/SHOULD governs the wire and persistence shape. Changing the contract and changing the code are the same change.
- **Decisions live where a new reader finds them.** The architecture doc records decision + deviation + reason; "we later changed X" belongs in the doc, not tribal memory.
- **Tests mirror the runtime shape.** Unit tests beside the code; one e2e over the real stack asserting observable outcomes (wire events, DB rows, rendered pixels).
- **Identity is durable.** Stable domain IDs for anything persisted; array offsets, entity indices, and connection IDs are not IDs.

## Smells that mean reorganize, not patch

- A `utils`/`common`/`shared` dump growing past a handful of true cross-cutters
- Two sources of truth for one fact (cost lives in client *and* server)
- An adapter whose only job is renaming methods
- A module whose behavior can't be guessed from its own contents — config or wiring lives far from use
