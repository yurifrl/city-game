# Making my dream city builder: Part 6 - Game Engine

- Source: https://www.youtube.com/watch?v=eoAfa68OcXk (linked at `t=234s`, which is the section on the IndividualKex video)
- Channel: jamessimo
- Duration: 31:57
- Retrieved: 2026-11-10, from YouTube auto-generated English captions
- Status: curated. This is not a verbatim transcript. Filler, repetition and off-topic personal asides are removed or condensed, and wording is tightened with the meaning kept. Timestamps point to the original. Caption errors are fixed where the intended term is clear.
  - The engine is spelled "Charlotte" in the captions and "Shalott" by the speaker in the middle of the video. The engine's own name is not verified here. Check the link in the video description.

## Summary

James is building a large "gridless" city builder. In this part he explains why he chose a WebGPU/TypeScript engine over Unity, Godot, Unreal and Bevy, and how the CPU/GPU split works for traffic. He then demos procedural building generation on organic parcels, terraforming costs, and a dev console.

## 1. Background and motivation (00:00–04:00)

- He studied game development in the late 2000s and graduated in the early 2010s. He has since worked in startups, SaaS and fintech, and now builds software for large international clients.
- He wants a self-sustaining game. He dislikes microtransactions and DLC that kneecaps the base game.
- The city-builder idea started in 2019 and became serious around 2022. It has produced real prototypes shown in earlier videos, backed by a lot of white-paper research.
- A turning point was the IndividualKex video ([transcript](individualkex-how-to-actually-make-a-city-builder.md)). His takeaways were:
  - compute shaders for terrain;
  - separating rendering from game logic;
  - KD-trees / binary partition trees;
  - "don't give up".

## 2. Engine requirements (04:30–05:30)

- A data-oriented memory layout.
- ECS.
- Extremely fast rendering.
- Live reloading (he comes from web/app development).
- Open source.

## 3. Why not Unity, Godot, Unreal or Bevy (05:00–09:30)

- **Pricing is not the main objection.** He likes Unity and Godot, and says Unity's pricing changes did not upset him because it is a good product that employs many people. His issues "go beyond pricing".
- **Default architecture is scene-coupled.**
  - Citizens, buildings and vehicles each become scene-tree nodes with a transform, a mesh and their own update, with logic bound to the tree.
  - He expects this to be expensive over time, because the engine has to look up and update each object through pointers.
  - He wants the opposite: citizens, buildings and traffic as pure data in tightly packed arrays (struct-of-arrays). The simulation streams through memory sequentially, then hands results to a separate presentation layer.
- Unity and Godot can both do this (Godot as renderer only, Unity DOTS), and City Skylines is built on Unity. But he thinks it is unreasonable to demand this from engines that must serve every game type, and asks how much to pay Unity if he rips out most of its features.
- **Godot + C++ extension:** he built a version of the city builder this way and "did not get along with it".
- **Bevy:** he likes the ECS and open source, but he does not use Rust professionally and is uneasy committing to it yet.

## 4. The chosen stack (10:00–13:00)

- He chose the Charlotte/Shalott WebGPU engine (from IndividualKex, the same person as the 1-minute video).
- The engine's pitch, quoted from its intro:
  - requirements are open source, ECS and fast iteration;
  - it uses TypeScript for instant hot reloading;
  - WebGPU does the heavy lifting, including physics and rendering.
- His stack:
  - **TypeScript** for game code, with some C++ for heavier work.
  - **Svelte** for UI, which comes with the engine.
  - **TypeGPU** to write shaders in TypeScript. It gives automatic data management (byte alignment, buffer indexing) and end-to-end type safety. He has not yet written shaders with it, and his existing shaders predate the engine.
  - **WGSL** as the shader language.

## 5. CPU/GPU traffic architecture (13:00–16:30)

- **The CPU-only problem.** A CPU traffic system has to compute traffic for the whole road network, a common bottleneck. Making it work city-wide means many hacks, single-threaded or with web workers.
- **How his system works:**
  - The traffic system (TypeScript) asks a solver (TypeScript) for road state, such as how many cars are nearby, how busy a road is, and where the queues are.
  - The solver talks to the GPU, which runs his **link transmission model** shader (WGSL).
  - Results return to the solver, then to the traffic system.
  - It runs **twice a second**. The game sends each road's demand to the GPU, which computes every road's traffic at once.
- **Hard wall between CPU and GPU.**
  - The game side does not run in lockstep with the GPU. If the GPU is busy it may skip frames.
  - The solver can return nothing or stale data. This is acceptable at 2 Hz.
  - Cars are not agents. They do not need to know where they are going. They only need the road's busyness, and cars are drawn on top of that.

## 6. Why he was secretive about the engine (16:30–19:30)

- The engine is very new, and he wanted to be sure about it first.
- He wanted to avoid debates in the comments about web technology. Some people assume it is WebGL, which is a different, older API from WebGPU.
- The game will **not** be a web game. It will ship as a bundled EXE or Mac DMG.
- He treats the project like a client project: documenting everything, running automated tests, and drawing diagrams, so he understands and controls the technology.
- He accepts the risk that the technology choice is wrong. Everything compiles to assembly eventually, and shader computation is already close to the hardware, so he could change language or engine later.
- The project name is "Ashkelon". The game's final name is undecided. Ashkelon is the port city where the word "shalott" was first documented.

## 7. Procedural generation demo (20:00–27:30)

- **Relief shading:** he exaggerates the terrain normals on slopes so the terrain is easier to read. Before and after are compared in the video.
- **Dev console:** `world.load` loads a saved world and `world.save` saves one. `build.all` runs the building generation pass over drawn parcels.
- **Parcels subdivide automatically** after being drawn.
- **Row (terraced) housing** is generated, and it follows curved roads drawn with the freehand tool.
- **High street / commercial blocks** are inspired by Barcelona superblocks:
  - each building gets its own plot of land;
  - courtyards are generated;
  - commercial areas are concreted over;
  - a commercial rule leaves side and back space for parking, and removing the rule would centre the building.
- **Residential blocks** become European-style apartment blocks rather than individual homes.
- **Weird shapes are handled:** he deliberately drew a triangular parcel and an odd-shaped plot, and the building conforms to the land. Parcel generation still has known issues. A second procedural pass is planned, and he will cover procedural generation in the next video.
- **Foundations:** buildings used to float on uneven ground. They now get extended foundations, which can also read as plinths or wooden pillars for cheaper housing.
- **Generation works on organic parcels.** It does not require a grid.

## 8. Terraforming and city policy (26:00–28:00)

- Low-density houses conform to the land and are not terraformed.
- A large factory terraforms, with retaining walls. The **developer pays the terraforming cost** when the building is made.
- This opens a policy lever for the city: an additional land-impact charge for terraforming. It raises revenue and discourages factories from tearing up areas.
- If terraforming is made too expensive, the factory may never be built, or may be built much smaller.
- For now, the test rule is that a factory always tries to build on all the land it is given.

## 9. Dev console and cheat codes (28:00–29:00)

- It is a real console, with `help` listing every command.
- Commands are easy to add and trigger real developer functions.
- It has a dev tier and an end-user tier. The dev tier is hidden from players, and some commands will be unlisted cheat codes.

## 10. Slopes, zoning, camera and roadmap (29:00–32:00)

- **Slopes:** on sharp slopes, buildings are smaller because they cannot use all the land. The generator picks a build site within the city's rules.
- **Zoning:** rules are currently hard-coded. A custom zoning system is planned.
- **Orthographic camera:** it exists but is "extremely broken". He still needs to understand clipping planes. Terrain chunk loading and unloading is visible through it.
- **Missing tools:** he misses Unity/Godot's in-app debugging tools.
- **Plan:**
  - Build the gameplay mechanics and a closed gameplay loop before sharing a playable build, so testers do not report problems he already knows about.
  - The next video covers procedural generation.
  - The one after that covers UI, designed in Figma.

## Relevance to this project

Comparison with `AGENTS.md` and `docs/architecture.md`:

| Idea in the video | Relation to this project |
| --- | --- |
| Data as packed arrays, simulation separate from presentation | Same direction as the dense-storage and domain/render separation rules. |
| GPU link-transmission traffic at 2 Hz, with stale results acceptable | Fits the bounded multi-rate update rule. Note that this project says the Rust simulation owns results, so a GPU-computed result that affects gameplay would need to be authoritative on the server. |
| Cars as an aggregate flow, with drawn cars as visuals only | Matches the project's "crowd marker is not the people" distinction only if the underlying people remain persistent individuals. Here cars are purely visual. |
| Developer pays terraforming cost, city sets policy | A candidate for "expose causes" and an economy/construction rule. It is not a commitment. |
| Procedural parcel subdivision and building generation | Relevant to the integrated building designers. It is not adopted. |
| Console commands for dev tools | Compare with the project's rule that administrative tools go through the same ordered command path. |
