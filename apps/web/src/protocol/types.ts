// TS mirrors of the wire types (docs/contracts/milestone-1.md is normative).
// Scaffold ships the data shapes the editors/assets stubs need; m1-03 extends
// this file with Command/Event/ServerMessage/CommandResult mirrors.

export interface CellTS {
  x: number;
  y: number;
}

export type ArchetypeTS = "house" | "shop" | "office" | "tower";
export type WindowStyleTS = "grid" | "stripes";

export interface RecipeTS {
  name: string;
  archetype: ArchetypeTS;
  footprint_w: number;
  footprint_h: number;
  floors: number;
  base_color: string;
  roof_color: string;
  window_style: WindowStyleTS;
  seed: number;
}

export interface DerivedStatsTS {
  capacity: number;
  cost_credits: number;
}

export interface AssetDefinitionTS {
  id: string;
  revision: number;
  recipe: RecipeTS;
  derived: DerivedStatsTS;
}

export interface BuildingTS {
  id: string;
  def_id: string;
  revision: number;
  cell: CellTS;
  rotation: number;
  derived: DerivedStatsTS;
}
