// Placement tool: armed by the catalogue/designer, previews through the ghost
// snapped to the hovered cell, rotates with R, cancels with ESC, and sends the
// place command on click. A click is only a command when the pointer barely
// moved between down and up — larger drags belong to camera panning. Commands
// stay pending until the server replies; rejections surface as toasts with the
// server's message. Unarmed clicks pick buildings for the inspector.

import type { Scene } from "@babylonjs/core";
import type {
  AssetDefinitionTS,
  CellTS,
  CommandResultTS,
  CommandTS,
  InputMessageTS,
} from "../protocol/types";
import type { BuildingViews } from "../renderer/buildings";
import type { Ghost } from "../renderer/ghost";
import { cellFromPoint } from "../renderer/grid";
import type { GridView } from "../renderer/grid";
import { cellKey } from "../world/replica";
import type { WorldReplica } from "../world/replica";
import { showToast } from "../ui/toast";

export interface PlacementDeps {
  scene: Scene;
  canvas: HTMLCanvasElement;
  grid: GridView;
  replica: WorldReplica;
  ghost: Ghost;
  views: BuildingViews;
  send(command: CommandTS): Promise<CommandResultTS>;
  /** Broadcasts this seat's pointer intent; fire-and-forget (no receipts). */
  sendInput(input: InputMessageTS): void;
  nextCommandId(): string;
  onSelectBuilding(id: string | null): void;
  onArm(def: AssetDefinitionTS): void;
  onDisarm(): void;
}

const CLICK_SLOP_PX = 5;

export class PlacementTool {
  private armed: AssetDefinitionTS | null = null;
  private rotation = 0;
  private hovered: CellTS | null = null;
  private pending = false;
  private downAt: { x: number; y: number } | null = null;

  constructor(private readonly deps: PlacementDeps) {
    deps.canvas.addEventListener("pointermove", this.onPointerMove);
    deps.canvas.addEventListener("pointerdown", this.onPointerDown);
    deps.canvas.addEventListener("pointerup", this.onPointerUp);
    window.addEventListener("keydown", this.onKeyDown);
  }

  arm(def: AssetDefinitionTS): void {
    this.armed = def;
    this.rotation = 0;
    this.pending = false;
    this.deps.onSelectBuilding(null);
    this.deps.onArm(def);
    // First armed packet: the seat is "in drag mode" the moment the tool
    // arms, even before the pointer next moves.
    this.deps.sendInput({
      kind: "move",
      cell: this.hovered ?? { x: 0, y: 0 },
      def_id: def.id,
      rotation: this.rotation,
    });
    this.refreshHover();
  }

  disarm(): void {
    this.armed = null;
    this.deps.onDisarm();
    // Un-arming clears the remote drag state; re-arming after rotation needs
    // the same packet shape, so the rotation is never carried over.
    this.deps.sendInput({ kind: "move", cell: this.hovered ?? { x: 0, y: 0 } });
    this.deps.ghost.hide();
    this.deps.grid.setHover(this.hovered, "neutral");
  }

  dispose(): void {
    this.deps.canvas.removeEventListener("pointerup", this.onPointerUp);
    this.deps.canvas.removeEventListener("pointerdown", this.onPointerDown);
    this.deps.canvas.removeEventListener("pointermove", this.onPointerMove);
    window.removeEventListener("keydown", this.onKeyDown);
  }

  private onPointerMove = (ev: PointerEvent): void => {
    const cell = this.pickCell(ev);
    if (cell && this.hovered && cellKey(cell.x, cell.y) === cellKey(this.hovered.x, this.hovered.y)) {
      return;
    }
    this.hovered = cell;
    if (cell) {
      // Remote seats see this cursor; armed packets carry the tool state so
      // the server derives the drag flag (latest input wins).
      const input: InputMessageTS = { kind: "move", cell };
      if (this.armed) {
        input.def_id = this.armed.id;
        input.rotation = this.rotation;
      }
      this.deps.sendInput(input);
    }
    this.refreshHover();
  };

  private onPointerDown = (ev: PointerEvent): void => {
    this.downAt = { x: ev.clientX, y: ev.clientY };
    const cell = this.pickCell(ev);
    if (cell) {
      // The press brackets the pointer gesture for remote seats; armed
      // packets keep carrying the tool state.
      const input: InputMessageTS = { kind: "down", cell };
      if (this.armed) {
        input.def_id = this.armed.id;
        input.rotation = this.rotation;
      }
      this.deps.sendInput(input);
    }
  };

  private onPointerUp = (ev: PointerEvent): void => {
    const down = this.downAt;
    this.downAt = null;
    if (down) {
      const cell = this.pickCell(ev);
      if (cell) this.deps.sendInput({ kind: "up", cell });
    }
    if (!down) return;
    const movedPx = Math.hypot(ev.clientX - down.x, ev.clientY - down.y);
    if (movedPx > CLICK_SLOP_PX) return; // camera drag, not a click
    this.onClick(ev);
  };

  private onKeyDown = (ev: KeyboardEvent): void => {
    // Never capture keys typed into catalogue/designer form fields.
    if (ev.target instanceof HTMLInputElement || ev.target instanceof HTMLSelectElement || ev.target instanceof HTMLTextAreaElement) {
      return;
    }
    if ((ev.key === "r" || ev.key === "R") && this.armed) {
      this.rotation = (this.rotation + 1) % 4;
      if (this.hovered) {
        this.deps.sendInput({
          kind: "move",
          cell: this.hovered,
          def_id: this.armed.id,
          rotation: this.rotation,
        });
      }
      this.refreshHover();
      return;
    }
    if (ev.key === "Escape") {
      this.disarm();
      this.deps.onSelectBuilding(null);
    }
  };

  private onClick(ev: PointerEvent): void {
    if (this.armed && this.hovered) {
      this.tryPlace();
      return;
    }
    this.selectAt(ev);
  }

  private tryPlace(): void {
    const def = this.armed;
    const cell = this.hovered;
    if (!def || !cell || this.pending || !this.validAt(cell)) return;
    this.pending = true;
    const command: CommandTS = {
      type: "place_building",
      command_id: this.deps.nextCommandId(),
      def_id: def.id,
      revision: def.revision,
      cell,
      rotation: this.rotation,
    };
    this.deps
      .send(command)
      .then((result) => {
        this.pending = false;
        if (result.result === "rejected") showToast(result.message, "error");
      })
      .catch((err: unknown) => {
        this.pending = false;
        showToast(err instanceof Error ? err.message : String(err), "error");
      });
  }

  private selectAt(ev: PointerEvent): void {
    const point = this.localPoint(ev);
    // Prefer a direct hit on a rendered building, then fall back to the
    // authoritative cell index (also works before prototypes compile).
    const buildingPick = this.deps.scene.pick(point.x, point.y, (mesh) =>
      this.deps.views.isPrototype(mesh),
    );
    const fromMesh = buildingPick ? this.deps.views.resolveBuildingId(buildingPick) : null;
    if (fromMesh) {
      this.deps.onSelectBuilding(fromMesh);
      return;
    }
    const groundPick = this.deps.scene.pick(point.x, point.y, (mesh) => mesh === this.deps.grid.ground);
    const cell = groundPick?.pickedPoint ? cellFromPoint(groundPick.pickedPoint, this.deps.replica.grid) : null;
    const building = cell ? this.deps.replica.buildingAt(cell) : undefined;
    this.deps.onSelectBuilding(building?.id ?? null);
  }

  private pickCell(ev: PointerEvent): CellTS | null {
    const point = this.localPoint(ev);
    const pick = this.deps.scene.pick(point.x, point.y, (mesh) => mesh === this.deps.grid.ground);
    return pick?.pickedPoint ? cellFromPoint(pick.pickedPoint, this.deps.replica.grid) : null;
  }

  private localPoint(ev: PointerEvent): { x: number; y: number } {
    const rect = this.deps.canvas.getBoundingClientRect();
    return { x: ev.clientX - rect.left, y: ev.clientY - rect.top };
  }

  private validAt(cell: CellTS): boolean {
    const def = this.armed;
    if (!def) return false;
    const cells = this.deps.replica.footprintCells(def.id, cell, this.rotation);
    if (cells.length === 0) return false; // unknown definition
    return cells.every((c) => this.deps.replica.inBounds(c) && !this.deps.replica.buildingAt(c));
  }

  private refreshHover(): void {
    if (!this.hovered) {
      this.deps.ghost.hide();
      this.deps.grid.setHover(null, "neutral");
      return;
    }
    if (!this.armed) {
      this.deps.ghost.hide();
      this.deps.grid.setHover(this.hovered, "neutral");
      return;
    }
    const valid = this.validAt(this.hovered);
    this.deps.ghost.show(this.armed, this.hovered, this.rotation, valid);
    this.deps.grid.setHover(this.hovered, valid ? "valid" : "invalid");
  }
}
