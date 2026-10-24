// Unit tests for the headless presence board. These cover the wire-truth
// semantics from docs/contracts/multiplayer-stream.md §2: latest input per
// actor wins, drag follows the presence of def_id on that latest input,
// player_left prunes, and the board stores every actor (self-filtering is
// the render layer's job).

import { describe, expect, test } from "bun:test";

import type { PresenceStateTS } from "../src/protocol/types";
import { PresenceBoard } from "../src/presence/board";

function presence(
  overrides: Partial<Omit<PresenceStateTS, "kind"> & { kind: PresenceStateTS["kind"] }> = {},
): PresenceStateTS {
  return {
    actor: "aaaa-bbbb",
    label: "player-aaaa",
    cell: { x: 3, y: 4 },
    kind: "idle",
    ...overrides,
  };
}

describe("PresenceBoard", () => {
  test("apply creates an idle entry from a bare presence packet", () => {
    const board = new PresenceBoard();
    board.apply(presence());

    const entry = board.get("aaaa-bbbb");
    expect(entry).toEqual({
      actor: "aaaa-bbbb",
      label: "player-aaaa",
      cell: { x: 3, y: 4 },
      kind: "idle",
      def_id: null,
      rotation: 0,
    });
  });

  test("apply upserts: later packet replaces cell, label, and armed state", () => {
    const board = new PresenceBoard();
    board.apply(presence());
    board.apply(
      presence({
        label: "player-zzzz",
        cell: { x: 10, y: 20 },
        def_id: "def-1",
        rotation: 2,
      }),
    );

    expect(board.size()).toBe(1);
    expect(board.get("aaaa-bbbb")).toMatchObject({
      label: "player-zzzz",
      cell: { x: 10, y: 20 },
      kind: "drag",
      def_id: "def-1",
      rotation: 2,
    });
  });

  test("armed move sets drag; disarm move clears it (latest-wins)", () => {
    const board = new PresenceBoard();
    board.apply(presence({ def_id: "def-1" }));
    expect(board.get("aaaa-bbbb")?.kind).toBe("drag");

    board.apply(presence()); // un-armed move after the armed one
    expect(board.get("aaaa-bbbb")?.kind).toBe("idle");
    expect(board.get("aaaa-bbbb")?.def_id).toBeNull();
    expect(board.get("aaaa-bbbb")?.rotation).toBe(0);
  });

  test("drag derives from def_id presence, not from the kind label", () => {
    // The server normalizes pointer down/up to idle|drag before broadcast,
    // so the board only ever sees those two; def_id alone decides drag.
    const board = new PresenceBoard();
    board.apply(presence({ kind: "idle", def_id: "def-9" }));
    expect(board.get("aaaa-bbbb")?.kind).toBe("drag");

    board.apply(presence({ kind: "idle" }));
    expect(board.get("aaaa-bbbb")?.kind).toBe("idle");
    expect(board.get("aaaa-bbbb")?.def_id).toBeNull();
  });

  test("rotation defaults to 0 when armed packet omits it", () => {
    const board = new PresenceBoard();
    board.apply(presence({ def_id: "def-1" }));
    expect(board.get("aaaa-bbbb")?.rotation).toBe(0);
  });

  test("latest input per actor wins; other actors are untouched", () => {
    const board = new PresenceBoard();
    board.apply(presence({ actor: "a1", label: "player-a1" }));
    board.apply(presence({ actor: "a2", label: "player-a2" }));
    board.apply(presence({ actor: "a1", cell: { x: 9, y: 9 }, def_id: "d" }));

    expect(board.get("a1")?.cell).toEqual({ x: 9, y: 9 });
    expect(board.get("a1")?.kind).toBe("drag");
    expect(board.get("a2")?.cell).toEqual({ x: 3, y: 4 });
    expect(board.get("a2")?.kind).toBe("idle");
  });

  test("join inserts an idle seat without overwriting existing state", () => {
    const board = new PresenceBoard();
    board.join("a1", "player-a1");
    expect(board.get("a1")).toMatchObject({ kind: "idle", cell: { x: 0, y: 0 } });

    board.apply(presence({ actor: "a1", cell: { x: 5, y: 6 } }));
    board.join("a1", "player-a1"); // duplicate join must not reset the seat
    expect(board.get("a1")?.cell).toEqual({ x: 5, y: 6 });
  });

  test("leave prunes and ignores unknown actors", () => {
    const board = new PresenceBoard();
    board.apply(presence({ actor: "a1" }));
    board.apply(presence({ actor: "a2" }));
    board.leave("a1");
    board.leave("ghost-actor");

    expect(board.get("a1")).toBeNull();
    expect(board.size()).toBe(1);
  });

  test("getAll returns a join-order snapshot decoupled from board state", () => {
    const board = new PresenceBoard();
    board.join("a1", "player-a1");
    board.apply(presence({ actor: "a2" }));

    const snapshot = board.getAll();
    expect(snapshot.map((entry) => entry.actor)).toEqual(["a1", "a2"]);

    // Mutating the snapshot must not leak into the board.
    snapshot[0]!.cell.x = 999;
    snapshot.pop();
    expect(board.get("a1")?.cell.x).toBe(0);
    expect(board.size()).toBe(2);
  });

  test("board stores every actor including the local seat (no self-filter)", () => {
    const board = new PresenceBoard();
    board.apply(presence({ actor: "local-seat" }));
    board.apply(presence({ actor: "remote-seat" }));

    expect(board.size()).toBe(2);
    expect(board.get("local-seat")).not.toBeNull();
  });

  test("clear empties the board (used on reconnect helloworld)", () => {
    const board = new PresenceBoard();
    board.apply(presence({ actor: "a1" }));
    board.clear();

    expect(board.size()).toBe(0);
    expect(board.getAll()).toEqual([]);
  });

  test("get returns null for unknown actors", () => {
    const board = new PresenceBoard();
    expect(board.get("nobody")).toBeNull();
  });
});
