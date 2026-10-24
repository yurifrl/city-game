// Headless presence board: tracks every connected seat's latest pointer
// intent. Deterministic state machine with no DOM or rendering dependency —
// the render layer (presence/view.ts) reads snapshots from it and maps them
// to meshes.
//
// Latest-wins semantics (per docs/contracts/multiplayer-stream.md §2): the
// most recent input packet per actor defines both the cell and the drag
// state. A packet carrying def_id arms the drag (the placement tool is
// armed), regardless of its kind; a packet without def_id clears it. Input
// order is authoritative, so out-of-order or late packets from a previous
// tool state must not resurrect an older drag.
//
// The board does not know who "we" are — it stores every actor including the
// local one. Filtering out the local ghost is the render layer's job (it is
// the only holder of the local actor id).

import type { PresenceStateTS } from "../protocol/types";

export type PresenceKind = "idle" | "drag";

export interface PresenceEntry {
  actor: string;
  label: string;
  cell: { x: number; y: number };
  kind: PresenceKind;
  /** Definition id while the actor hovers with the placement tool armed. */
  def_id: string | null;
  rotation: number;
}

function cloneEntry(entry: PresenceEntry): PresenceEntry {
  return { ...entry, cell: { ...entry.cell } };
}

export class PresenceBoard {
  private entries = new Map<string, PresenceEntry>();

  /** Applies one presence event; later packets for the same actor win. */
  apply(state: PresenceStateTS): void {
    // Wire truth: drag iff the latest input carried def_id; "idle" packets
    // omit it, so deriving from def_id alone is equivalent and robust.
    const armed = state.def_id !== undefined;
    this.entries.set(state.actor, {
      actor: state.actor,
      label: state.label,
      cell: { ...state.cell },
      kind: armed ? "drag" : "idle",
      def_id: state.def_id ?? null,
      rotation: armed ? (state.rotation ?? 0) : 0,
    });
  }

  /** Removes a seat after player_left; unknown actors are ignored. */
  leave(actor: string): void {
    this.entries.delete(actor);
  }

  /** Registers a seat before its first presence packet, so it can show idle. */
  join(actor: string, label: string): void {
    if (this.entries.has(actor)) return;
    this.entries.set(actor, {
      actor,
      label,
      cell: { x: 0, y: 0 },
      kind: "idle",
      def_id: null,
      rotation: 0,
    });
  }

  get(actor: string): PresenceEntry | null {
    const entry = this.entries.get(actor);
    return entry ? cloneEntry(entry) : null;
  }

  /** Snapshot ordered by join order (insertion order of the map). */
  getAll(): PresenceEntry[] {
    return [...this.entries.values()].map(cloneEntry);
  }

  size(): number {
    return this.entries.size;
  }

  clear(): void {
    this.entries.clear();
  }
}
