// Client-side replica of the authoritative world. Holds definitions and
// buildings exactly as the wire delivers them, plus a cell→building index for
// fast occupancy queries. Never mutated ad hoc — only through applySnapshot /
// applyEvent, which the composition root feeds from the connection client.

import type {
  AssetDefinitionTS,
  BuildingTS,
  CellTS,
  EventTS,
  WorldTS,
} from "../protocol/types";

export const GRID_WIDTH = 96;
export const GRID_HEIGHT = 96;

/** What a single applied event changed, so callers refresh precisely. */
export interface ReplicaDiff {
  placed?: BuildingTS;
  removed?: BuildingTS;
  published?: AssetDefinitionTS;
}

/** Effective footprint dims: odd quarter turns swap width/height. */
export function footprintSize(
  recipe: { footprint_w: number; footprint_h: number },
  rotation: number,
): { w: number; h: number } {
  const swap = (rotation & 1) === 1;
  return {
    w: swap ? recipe.footprint_h : recipe.footprint_w,
    h: swap ? recipe.footprint_w : recipe.footprint_h,
  };
}

/**
 * Canonical key for the cell→building index. Single formatting point: every
 * set/get/delete on the index must agree on it (call sites in this file).
 */
export function cellKey(x: number, y: number): string {
  return `${x},${y}`;
}

export class WorldReplica {
  private defs = new Map<string, AssetDefinitionTS>();
  private buildingsById = new Map<string, BuildingTS>();
  /** "x,y" → building id occupying that cell. */
  private cells = new Map<string, string>();
  private gridDims = { width: GRID_WIDTH, height: GRID_HEIGHT };
  private currentTick = 0;
  private commandCounter = 0;
  // Session-scoped prefix keeps command ids unique across server restarts, so
  // persisted receipts can never collide with a fresh session's ids.
  private readonly session = Math.random().toString(36).slice(2, 10);

  applySnapshot(world: WorldTS): void {
    this.defs.clear();
    this.buildingsById.clear();
    this.cells.clear();
    this.gridDims = { width: world.grid.width, height: world.grid.height };
    this.currentTick = world.tick;
    for (const def of world.definitions) this.defs.set(def.id, def);
    for (const building of world.buildings) {
      this.buildingsById.set(building.id, building);
      for (const cell of this.buildingCells(building)) {
        this.cells.set(cellKey(cell.x, cell.y), building.id);
      }
    }
  }

  applyEvent(event: EventTS): ReplicaDiff {
    switch (event.type) {
      case "building_placed": {
        const building = event.building;
        this.buildingsById.set(building.id, building);
        for (const cell of this.buildingCells(building)) {
          this.cells.set(cellKey(cell.x, cell.y), building.id);
        }
        return { placed: building };
      }
      case "building_demolished": {
        const building = this.buildingsById.get(event.building_id);
        this.buildingsById.delete(event.building_id);
        if (building) {
          for (const cell of this.buildingCells(building)) {
            this.cells.delete(cellKey(cell.x, cell.y));
          }
        }
        return { removed: building };
      }
      case "definition_published":
        this.defs.set(event.definition.id, event.definition);
        return { published: event.definition };
    }
  }

  definition(id: string): AssetDefinitionTS | undefined {
    return this.defs.get(id);
  }

  definitions(): AssetDefinitionTS[] {
    return [...this.defs.values()].sort((a, b) =>
      a.id.localeCompare(b.id, undefined, { numeric: true }),
    );
  }

  building(id: string): BuildingTS | undefined {
    return this.buildingsById.get(id);
  }

  buildings(): BuildingTS[] {
    return [...this.buildingsById.values()];
  }

  buildingAt(cell: CellTS): BuildingTS | undefined {
    const id = this.cells.get(cellKey(cell.x, cell.y));
    return id ? this.buildingsById.get(id) : undefined;
  }

  inBounds(cell: CellTS): boolean {
    return (
      cell.x >= 0 &&
      cell.y >= 0 &&
      cell.x < this.gridDims.width &&
      cell.y < this.gridDims.height
    );
  }

  get grid(): { width: number; height: number } {
    return this.gridDims;
  }

  /**
   * World-space cells covered by `defId` anchored at `cell` with `rotation`
   * quarter turns clockwise. Empty when the definition is unknown.
   */
  footprintCells(defId: string, cell: CellTS, rotation: number): CellTS[] {
    const def = this.defs.get(defId);
    if (!def) return [];
    const { w, h } = footprintSize(def.recipe, rotation);
    const cells: CellTS[] = [];
    for (let dy = 0; dy < h; dy++) {
      for (let dx = 0; dx < w; dx++) {
        cells.push({ x: cell.x + dx, y: cell.y + dy });
      }
    }
    return cells;
  }

  nextCommandId(): string {
    this.commandCounter += 1;
    return `${this.session}-${this.commandCounter}`;
  }

  private buildingCells(building: BuildingTS): CellTS[] {
    return this.footprintCells(building.def_id, building.cell, building.rotation);
  }
}
