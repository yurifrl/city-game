// Building inspector panel: shown when the placement tool picks a building.
// Lists the server-derived facts for that building and offers demolish — the
// button only requests the command; the server still decides.

import type { AssetDefinitionTS, BuildingTS } from "../protocol/types";

export interface InspectorDeps {
  onDemolish?: (buildingId: string) => void;
}

export interface BuildingSelection {
  building: BuildingTS;
  definition: AssetDefinitionTS | null;
}

export interface BuildingInspector {
  show(selection: BuildingSelection): void;
  clear(): void;
}

export function mountInspector(container: HTMLElement, deps: InspectorDeps = {}): BuildingInspector {
  container.replaceChildren();
  const heading = document.createElement("h3");
  heading.textContent = "Inspector";
  const body = document.createElement("div");
  container.append(heading, body);

  const render = (selection: BuildingSelection | null): void => {
    body.replaceChildren();
    if (!selection) {
      body.textContent = "Click a building to inspect it.";
      return;
    }
    const { building, definition } = selection;
    const rows: Array<[string, string]> = [
      ["Name", definition?.recipe.name ?? `definition ${building.def_id} (missing)`],
      ["Id", building.id],
      ["Cell", `${building.cell.x}, ${building.cell.y}`],
      ["Rotation", `${building.rotation} (${building.rotation * 90}°)`],
      ["Capacity", String(building.derived.capacity)],
      ["Cost", `${building.derived.cost_credits} cr`],
      ["Archetype", definition?.recipe.archetype ?? "unknown"],
    ];
    for (const [label, value] of rows) {
      const row = document.createElement("div");
      row.className = "insp-row";
      const labelEl = document.createElement("span");
      labelEl.textContent = label;
      const valueEl = document.createElement("span");
      valueEl.className = "insp-value";
      valueEl.textContent = value;
      row.append(labelEl, valueEl);
      body.append(row);
    }
    if (deps.onDemolish) {
      const button = document.createElement("button");
      button.className = "demolish";
      button.textContent = "Demolish";
      button.addEventListener("click", () => deps.onDemolish?.(building.id));
      body.append(button);
    }
  };

  render(null);
  return {
    show(selection: BuildingSelection): void {
      render(selection);
    },
    clear(): void {
      render(null);
    },
  };
}
