# How to *actually* make a City Builder

- Source: https://www.youtube.com/watch?v=kAD8S5XFaao
- Channel: IndividualKex
- Duration: 1:04
- Retrieved: 2026-11-10, from YouTube auto-generated English captions
- Status: lightly corrected transcript. Caption errors are fixed where the intended term is clear, marked `[sic: ...]`. Unclear audio is not guessed.

This is the whole video. It is a dense list of eight architecture points for a large-scale city builder.

## Transcript

**[00:00]** How to actually make a city builder: the things nobody tells you.

1. **Terrain.** Not how to generate it, that's the easy part. How to manipulate it efficiently. Introducing compute shaders: represent the terrain with a huge matrix. You know who loves huge matrices? Your GPU.
2. **Pathfinding.** Use A\* pathfinding `[sic: captions read "AAR"]`. Don't reinvent the wheel. Pathfinding is the largest computational bottleneck by far, so use a library that's already optimized.
3. **Event-driven architecture.** Keep all your systems separate, with only one-way dependencies. Your core systems, like agents, buildings, jobs and logistics, should have no game objects, no rendering, no audio, just pure C# `[sic: captions read "C"]`. They invoke events that other systems can listen to, to do the actual rendering.
4. **KD-trees.** What is where? Where's the closest house? How many trees are in a five-tile radius? Use a KD-tree.
5. **Schedulers.** A lot of stuff in a city builder is computationally expensive. Use a scheduler to avoid lag spikes.
6. **The plug-in pattern.** Want to add a new building, a new terrain type, a new anything? Define a bunch of interfaces that systems check for to execute the behavior.
7. **The factory pattern.** When you want to make a complex new thing, pass in a factory that the new thing can use to define an arbitrary sequence of behaviors.
8. **Don't give up.**

## Why it is kept here

James (jamessimo) cites this video as the source of his architecture. See [jamessimo-part-6-game-engine.md](jamessimo-part-6-game-engine.md).

Points 1, 3 and 5 line up with this project's constitution in `AGENTS.md`:

- GPU-side terrain.
- A simulation core with no rendering, audio or UI dependencies.
- Bounded multi-rate updates.

Point 3's "no game objects, no rendering, no audio" is the same boundary as "the simulation stays independent of infrastructure". The video is C#/Unity-flavoured and this project is Rust-authoritative, so treat the points as patterns rather than prescriptions.
