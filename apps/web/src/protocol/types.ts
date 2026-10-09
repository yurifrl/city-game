// Hand-maintained TS mirrors of the wire types (docs/contracts/milestone-1.md
// is normative; field names must match crates/protocol exactly, snake_case,
// tagged unions on "type" / "result"). IDs are decimal strings (WireId).
// JSON.parse only ever yields `number`, so u64 fields (tick, cost_credits) are
// typed `number` here — the ts-rs bindings under generated/ use `bigint` and
// would need conversion at the boundary; m1-05 picks one of the two sources.

export interface CellTS {
  x: number;
  y: number;
}

export type ArchetypeTS = "house" | "shop" | "office" | "tower";
export type WindowStyleTS = "grid" | "stripes";

/** One authored piece of an asset (metres, relative to footprint min corner). */
export interface AssetPartTS {
  kind: string;
  x: number;
  y: number;
  z: number;
  w: number;
  h: number;
  d: number;
  color: string;
}

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
  parts: AssetPartTS[];
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

export interface GridTS {
  width: number;
  height: number;
}

export interface WorldTS {
  grid: GridTS;
  tick: number;
  buildings: BuildingTS[];
  definitions: AssetDefinitionTS[];
}

export type RejectCodeTS =
  | "overlap"
  | "out_of_bounds"
  | "unknown_asset"
  | "unknown_building"
  | "duplicate_identity_conflict"
  | "bad_request";

/** client → server intent; `command_id` is client-allocated per session. */
export type CommandTS =
  | {
      type: "place_building";
      command_id: string;
      def_id: string;
      revision: number;
      cell: CellTS;
      rotation: number;
    }
  | {
      type: "demolish_building";
      command_id: string;
      building_id: string;
    };

export interface ClientMessageTS {
  type: "command";
  command: CommandTS;
}

export type EventTS =
  | { type: "building_placed"; building: BuildingTS }
  | { type: "building_demolished"; building_id: string }
  | { type: "definition_published"; definition: AssetDefinitionTS }
  | { type: "definition_removed"; def_id: string };

/** `conflicting_building_id` is omitted on the wire when null. */
export type CommandResultTS =
  | {
      result: "accepted";
      command_id: string;
      building_id: string;
    }
  | {
      result: "rejected";
      command_id: string;
      message: string;
      conflicting_building_id: string | null;
    };

export type ServerMessageTS =
  | { type: "hello"; actor: string; world: WorldTS }
  | { type: "event"; event: EventTS }
  | { type: "command_result"; result: CommandResultTS };
