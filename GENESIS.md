# Genesis: a city worth understanding, built together

Status: project definition, not an implemented game. The repository was empty when this document was written. Technical decisions are in [architecture](docs/architecture.md), their evidence in [research](docs/research.md), and the current M2 contract in [docs/contracts/multiplayer-stream.md](docs/contracts/multiplayer-stream.md).

## Why this project exists

Build a browser city simulation whose defining qualities are **simulation depth** and **cooperative multiplayer**. A city should be more than a skyline and a collection of rising bars. Its inhabitants have identities, households, homes, jobs, routines, needs, and eventually relationships and histories. Infrastructure and planning decisions change their lives, and those lives change the city.

The engineering objective is the most understandable, well-organized codebase we can build. Shipping more features does not excuse tangled ownership or rushed architecture. The way to deliver value quickly is to finish small, well-designed playable systems—not to accumulate shortcuts or build a general engine before anyone can place a house.

## Design pillars

1. **Build immediately.** Create a simple building, save it to the catalogue, place it, rotate it, and understand why placement is valid or invalid.
2. **Create the city's assets inside its engine.** Building and terrain designers are first-class tools, sharing the game's contracts and visual pipeline.
3. **A small simulation is already a real simulation.** Even the first residents retain identities and meaningful needs; decorative walkers are not a substitute.
4. **Depth comes from interactions.** Water enables occupancy, occupancy consumes power, work supplies household income, affordability changes housing demand, and unmet needs create consequences.
5. **Build together on one map.** Multiple users can place buildings and edit terrain in the same city. This is cooperation, not competitive factions or a distributed-world MMO.
6. **Show the reason.** Inspect a resident, building, or network to understand what happened and what intervention could help.
7. **Scale detail intelligently.** Simulate people independently of whether their visual representation is visible. Spend geometry on recognizable silhouettes, not invisible detail.

## What “fun quickly” means

The first creative payoff is authoring and placing a building. The first complete city-sim payoff is:

> Build housing → connect a road, electricity, and water → households move in → inspect individual residents → provide jobs and food access → earn enough to maintain services → respond to shortages and growing demand.

Players should encounter a comprehensible problem, make a spatial or economic choice, and observe a consequence within a short session. Examples: a cheap new apartment block overloads the local power supply; a new road makes jobs reachable; a water extension makes a previously empty neighborhood attractive. These are actual state transitions, not scripted congratulatory popups.

**First simulation slice:** housing, persistent residents, water, power, needs, and reasons. **First complete management loop:** add jobs, food access, household budgets, construction costs, operating costs, and city revenue. Until that second boundary, call it a settlement sandbox, not a finished economy.

## Visual direction

The two supplied images are the visual reference: dense oblique/isometric city blocks; tall, varied silhouettes; readable roofs; repeated window grids; dark edge definition; and a colorful, illustrated rather than photorealistic skyline. The images establish appearance, not evidence about the original game's renderer or asset format. Their video text and timestamps are not part of the desired style.

Our implementation choice is **orthographic 3D with a 2.5D presentation**, initially a fixed oblique camera with pan and zoom. Quarter-turn camera views can be added once useful. Buildings have simple geometric masses, stepped setbacks, roof details, and facade textures. Glass is primarily an opaque stylized facade, not thousands of transparent windows. No modeled interiors in the initial scope.

Terrain uses a grid/heightfield with editable elevation, surface materials, shorelines, and buildability. Water supply simulation does not require a physically accurate fluid renderer. Dense-city zoom levels can use simplified meshes or impostors without changing the world simulation.

## Feature inventory

Priority means first meaningful delivery; systems continue to deepen afterward. The roadmap specifies acceptance and dependencies.

| System | First useful capability | Later depth | First delivery |
|---|---|---|---|
| World and camera | Grid, chunk coordinates, orthographic pan/zoom, picking | Wider maps, selectable overlays, alternative views | 1 |
| Building designer | Footprint, height/floors, roof, facade palette, named reusable recipe | Setbacks, modular roofs, landmark parts, variant families, import/export | 1; richer in 6 |
| Asset compilation | Validated recipe → renderable asset + gameplay definition; revisioned catalogue | Automatic LOD/impostors, compression, cost reports, richer recipes | 1; richer in 6 |
| Construction | Preview, snapping, quarter-turn placement, collision/buildability checks, demolition | Upgrades, redevelopment, zoning-assisted growth | 1; richer in 5–7 |
| Terrain designer | Paint surfaces, raise/lower/flatten, save terrain, reject invalid occupied edits | Shores, slopes, terrain presets, larger brushes | 2; richer in 6 |
| Shared-map multiplayer | Two or more clients edit one server-owned world; presence and clear conflicts | More concurrent editors, viewer roles, larger edit regions | 2 |
| Persistence | Save/reload authoritative world and pinned assets | Checkpoints, recovery verification, explicit version migrations | 1–2 |
| Roads and access | Connected road graph and building entrances | Capacity, travel time, congestion, transit, freight | 3; richer in 5–7 |
| Electricity | Generation, connections, demand, capacity, shortages, diagnostic overlay | Line bottlenecks, storage, different generation tradeoffs | 3; richer in 7 |
| Water | Source/pump, network, demand, source limits, shortages, overlay | Storage, sewage, contamination, treatment | 3; richer in 7 |
| Housing | Explicit household slots, occupancy, suitability, unmet housing demand | Rent pressure, density, household composition, neighborhood quality | 4–5 |
| Individuals | Stable identity, household/home, life stage, current activity, needs | Skills, preferences, health, relationships, memories, aging | 4; richer in 7 |
| Daily life | Sleep/rest, food/water needs, shelter and comfort, explainable activity choice | Schedules, leisure, education, care, social interactions | 4–5; richer in 7 |
| Jobs and businesses | Persistent job assignment, wages, basic workplaces | Skill matching, productivity, production chains, business failure | 5; richer in 7 |
| Food and goods | Explicit external supplier, shop inventory, purchasing and consumption | Local production, freight, inventories, supply-chain disruptions | 5; richer in 7 |
| Economy | Household budget, housing costs, construction/maintenance, city revenue | Policies, land value, taxation tradeoffs, specialization | 5; richer in 7 |
| Demand and migration | People unable to find suitable housing; incoming/leaving households with reasons | Demography, preference-driven migration, regional pressures | 4–5; richer in 7 |
| Explainability | Inspect building, citizen, network and rejected action | Causal histories, trends, neighborhood comparisons | Every slice |
| Services and environment | Consequences of basic service failure | Sewage, pollution, waste, healthcare, education, fire response | 7 |
| Emergent stories | Remember consequential changes such as relocation or job loss | Relationships and memories influence later decisions | 7 |
| Performance tools | Reproducible representative city scenes, resource and timing counters | Load suites, large worlds and observer scaling | Every slice; scale gate in 8 |

## Individuals are not decorations

Start with few needs, but make them causal. A resident retains a home and later a workplace; the next available building is not a replacement identity. A household occupies explicit capacity. Needs change over simulation time. Activities can satisfy needs only if the required resource or service is accessible. An inaccessible workplace affects attendance and income; no water affects suitability and well-being.

Distinguish physiological needs (food, drinking water, rest), environmental/service conditions (shelter, electricity, access), and later personal needs (social contact, leisure, preferences). One generic happiness number must not replace these causes. Early slices may model limited need categories, but the supported ones must have real outcomes.

Housing demand should be explainable from households seeking suitable, reachable, affordable housing. Supply and occupancy must agree. Migration represents named arrivals/departures with recorded causes, not a magic population-growth timer. Before jobs and affordability exist, explicitly expose the narrower eligibility rules instead of pretending to simulate them.

Dwarf Fortress-level depth is the long-term ambition: interconnected material/social systems that generate stories. It is not a commitment to build anatomy, world history, fully simulated interiors, or every fluid particle before the first useful city.

## Building and terrain designers

**Building workflow:** choose footprint → shape the mass and roof → select facade pattern and palette → attach entrances and an approved gameplay archetype → preview at several zoom levels → validate → publish a revision → place it in the live map.

Authoring should let someone create a useful variant without writing code. Shared geometry, palette and texture variation should allow a varied skyline without a unique material and draw call for every building. The editor must show invalid entrances, footprint mismatch, excessive geometry/materials, and incompatible gameplay dimensions before publication.

**Terrain workflow:** select a brush → preview its affected cells → edit elevation/material → inspect buildability and shoreline → apply an authoritative transaction → persist. Height edits must update construction validity and affected connectivity. They must not silently bury occupied buildings or sever networks without reporting consequences.

An editable recipe is source; a published revision is immutable; a placed building is a world entity referencing that revision. Editing the source does not silently change the capacity or appearance of every existing building in a shared world. Explicit upgrades are separate commands.

## Multiplayer deliberately stays simple

The cooperative experience is simple; its consistency still needs care. One server owns the map, simulation clock and shared treasury. Edits from all players enter the same ordered command path. Conflicting placement produces one accepted result and a clear rejection, not divergent cities. A reconnecting client restores current authoritative state. Shared pause/speed changes require the owner's permission.

Start with invited collaborators and owner/editor permissions. No PvP, player-owned competing economies, territory warfare, peer-to-peer authority, client lockstep, or world sharding. Two clients are the first proof, not the eventual concurrency ceiling. Permission and asset-validation boundaries exist before exposing a server publicly.

## Scope discipline

Do not begin with a general-purpose engine, a custom GPU renderer, an unrestricted scripting language, photorealistic graphics, full traffic AI, every city service, or planet-scale maps. Do not postpone the integrated designers or multiplayer foundations until after building a large solo game.

Prefer a few complete interactions over many inert systems. Every added system must answer: what choice does it create, what existing system does it affect, how can the player see the cause, who owns its state, and what does it cost to simulate?
