// Command registry: every palette command is one file in this folder
// exporting a `CommandFactory` that receives the shared side-effect context.
// This index composes them for the palette, which stays presentation-only.
// Adding a command = adding a file here + one entry in the list below.
import { CommandPalette, type PaletteCommand } from "../ui/palette";
import type { CityClient } from "../connection/client";
import type { WorldReplica } from "../world/replica";
import { clearBuildings } from "./clear-buildings";

export interface CommandContext {
  client: CityClient;
  replica: WorldReplica;
}

export type CommandFactory = (ctx: CommandContext) => PaletteCommand;

export function mountCommandPalette(ctx: CommandContext): CommandPalette {
  return new CommandPalette([clearBuildings(ctx)]);
}
