// Palette command: demolish every building via one authoritative
// `clear_buildings` command and report the count. Result handling is
// command-specific; the server still validates and rejects as usual.
import type { CommandTS } from "../protocol/types";
import { showToast } from "../ui/toast";
import type { CommandFactory } from "./index";

export const clearBuildings: CommandFactory = ({ client, replica }) => ({
  id: "clear-buildings",
  label: "Clear buildings",
  hint: "demolish every building",
  run: () => {
    const command: CommandTS = {
      type: "clear_buildings",
      command_id: replica.nextCommandId(),
    };
    client
      .send(command)
      .then((result) => {
        if (result.result === "cleared_buildings") {
          showToast(`cleared ${result.removed} building${result.removed === 1 ? "" : "s"}`);
        } else if (result.result === "rejected") {
          showToast(result.message, "error");
        }
      })
      .catch((err) => showToast(err instanceof Error ? err.message : String(err), "error"));
  },
});
