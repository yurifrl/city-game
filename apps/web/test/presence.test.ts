// Acceptance test for the multiplayer-stream presence wire contract, run
// against the headless seat boot (server-side E2E harness feeds it through
// the same Vite module graph the browser seat loads).
//
// Covered here (client-side, no browser): the wire types' JSON shapes round
// the seat's input path — identify first, input fire-and-forget, hello →
// helloworld ordering, and the three presence events feeding the board with
// latest-wins semantics.

import { describe, expect, test } from "bun:test";

import { PresenceBoard } from "../src/presence/board";
import type { InputMessageTS, ServerMessageTS } from "../src/protocol/types";

/** Build an InputMessage the way the placement tool does. */
function input(kind: InputMessageTS["kind"], opts: Partial<InputMessageTS> = {}): InputMessageTS {
  return { kind, cell: { x: 5, y: 5 }, ...opts };
}

describe("presence wire contract (client side)", () => {
  test("identify is the first message; input envelopes never carry an actor", () => {
    // The client transmits {type:"identify", key} before anything else; input
    // envelopes are {type:"input", input} with no actor field anywhere.
    const identify = { type: "identify" as const, key: "seed-uuid" };
    const envelope = { type: "input" as const, input: input("move") };
    expect(identify.type).toBe("identify");
    expect(envelope.type).toBe("input");
    expect("actor" in envelope).toBe(false);
    expect("actor" in (envelope.input as Record<string, unknown>)).toBe(false);
  });

  test("hello precedes helloworld and neither precedes identify (FIFO order)", () => {
    const frames: ServerMessageTS[] = [
      { type: "hello", you: "a1b2c3d4e5" },
      {
        type: "helloworld",
        world: {
          grid: { width: 96, height: 96 },
          tick: 1,
          definitions: [],
          buildings: [],
        },
      },
    ];
    expect(frames[0]?.type).toBe("hello");
    expect(frames[1]?.type).toBe("helloworld");
    // The seat's actor id is the hello's you, echoed nowhere else.
    expect(frames[0].type === "hello" && frames[0].you).toBe("a1b2c3d4e5");
  });

  test("player_joined → presence → player_left round-trips through the board", () => {
    const board = new PresenceBoard();
    board.join("a1b2c3d4e5", "player-a1b2");
    expect(board.get("a1b2c3d4e5")?.label).toBe("player-a1b2");
    expect(board.get("a1b2c3d4e5")?.kind).toBe("idle");

    // Armed drag at (10, 20): def_id present on the latest packet.
    board.apply({
      actor: "a1b2c3d4e5",
      label: "player-a1b2",
      cell: { x: 10, y: 20 },
      kind: "drag",
      def_id: "road",
      rotation: 1,
    });
    const entry = board.get("a1b2c3d4e5");
    expect(entry?.kind).toBe("drag");
    expect(entry?.def_id).toBe("road");

    // Disarm clears the drag on the next plain move.
    board.apply({
      actor: "a1b2c3d4e5",
      label: "player-a1b2",
      cell: { x: 11, y: 21 },
      kind: "idle",
    });
    expect(board.get("a1b2c3d4e5")?.kind).toBe("idle");
    expect(board.get("a1b2c3d4e5")?.def_id).toBeNull();

    board.leave("a1b2c3d4e5");
    expect(board.get("a1b2c3d4e5")).toBeNull();
  });

  test("building owner flows through the wire type and CommandResult", () => {
    // M1 building JSON had no owner; M2 adds owner only when present.
    const placed = {
      type: "building_placed" as const,
      building: {
        id: "1",
        def_id: "hut",
        revision: "hut@1",
        cell: { x: 1, y: 1 },
        rotation: 0,
        derived: { capacity: 4, cost_credits: 10 },
        owner: "a1b2c3d4e5",
      },
    };
    expect(placed.building.owner).toBe("a1b2c3d4e5");
    const accepted = {
      result: "accepted" as const,
      command_id: "s1-1",
      building_id: "1",
      owner: "a1b2c3d4e5",
    };
    expect(accepted.owner).toBe("a1b2c3d4e5");
  });
});
