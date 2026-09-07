// Composition root: builds the engine, camera, grid, replica, network client,
// renderer views, tools and panels, and wires every cross-module data flow:
// hello/events → replica → per-key instances; catalogue/designer → placement;
// client picks → inspector; inspector demolish → command path.

import "./styles.css";
import {
  Color3,
  Color4,
  DirectionalLight,
  HemisphericLight,
  Scene,
  Vector3,
} from "@babylonjs/core";
import { CityClient } from "./connection/client";
import type { ConnectionStatus } from "./connection/client";
import { mountInspector } from "./inspectors/building";
import { compilePrototype } from "./assets/compiler";
import { clearArmed, mountCatalogue, mountDesigner, upsertCatalogueEntry } from "./editors/designer";
import type { DesignerDeps } from "./editors/designer";
import type {
  AssetDefinitionTS,
  BuildingTS,
  CommandTS,
  RecipeTS,
} from "./protocol/types";
import { BuildingViews } from "./renderer/buildings";
import type { BuildingInstance } from "./renderer/buildings";
import { createCamera } from "./renderer/camera";
import { createEngine, backendChip, reflectBackend } from "./renderer/engine";
import type { BackendName } from "./renderer/engine";
import { createGrid } from "./renderer/grid";
import { Ghost } from "./renderer/ghost";
import { PlacementTool } from "./tools/placement";
import { WorldReplica } from "./world/replica";
import { showToast } from "./ui/toast";

function requiredElement(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing #${id} from index.html`);
  return el;
}

const STATUS_LABEL: Record<ConnectionStatus, string> = {
  connecting: "connecting…",
  online: "online",
  offline: "offline (retrying)",
};

async function boot(): Promise<void> {
  const pageCanvas = requiredElement("stage") as HTMLCanvasElement;

  let created;
  try {
    created = await createEngine(pageCanvas);
  } catch (err) {
    console.error("no usable render backend", err);
    backendChip().textContent = "backend: unavailable";
    return;
  }
  const { engine, backend, canvas } = created;

  const scene = new Scene(engine);
  scene.clearColor = new Color4(0.055, 0.065, 0.09, 1);
  const hemi = new HemisphericLight("hemi", new Vector3(0.2, 1, 0.1), scene);
  hemi.intensity = 0.85;
  hemi.groundColor = new Color3(0.12, 0.13, 0.16);
  const sun = new DirectionalLight("sun", new Vector3(-0.5, -1, -0.35), scene);
  sun.intensity = 0.7;

  const replica = new WorldReplica();
  const gridView = createGrid(scene, { width: 96, height: 96 });
  const views = new BuildingViews();
  const ghost = new Ghost(scene);
  createCamera(scene, canvas, new Vector3(48, 0, 48));

  const keyOf = (building: BuildingTS): string => `${building.def_id}@${building.revision}`;
  const keyOfDef = (def: AssetDefinitionTS): string => `${def.id}@${def.revision}`;

  /** Recompute one revision key's instance list from the replica. */
  function refreshInstances(key: string): void {
    const entries: BuildingInstance[] = [];
    for (const building of replica.buildings()) {
      if (keyOf(building) !== key) continue;
      const def = replica.definition(building.def_id);
      if (!def) {
        console.warn(`building ${building.id} references unknown definition ${building.def_id}`);
        continue;
      }
      entries.push({
        id: building.id,
        cell: building.cell,
        rotation: building.rotation,
        footprint_w: def.recipe.footprint_w,
        footprint_h: def.recipe.footprint_h,
      });
    }
    views.setInstances(key, entries);
  }

  /** Compile + register a prototype, then refresh catalogue and instances. */
  function onDefinition(def: AssetDefinitionTS): void {
    const key = keyOfDef(def);
    try {
      const prototype = compilePrototype(def, scene);
      views.registerPrototype(key, prototype.mesh);
      refreshInstances(key);
    } catch (err) {
      console.warn(`prototype compilation failed for ${key}; its buildings will not render`, err);
    }
    try {
      upsertCatalogueEntry(def);
    } catch (err) {
      console.warn("catalogue update skipped (editor module unavailable)", err);
    }
  }

  const inspector = mountInspector(requiredElement("inspector"), {
    onDemolish: (buildingId) => void demolish(buildingId),
  });

  const placement = new PlacementTool({
    scene,
    canvas,
    grid: gridView,
    replica,
    ghost,
    views,
    send: (command) => client.send(command),
    nextCommandId: () => replica.nextCommandId(),
    onSelectBuilding: (id) => {
      if (!id) {
        inspector.clear();
        return;
      }
      const building = replica.building(id);
      if (!building) {
        inspector.clear();
        return;
      }
      inspector.show({ building, definition: replica.definition(building.def_id) ?? null });
    },
    onDisarm: clearArmed,
  });

  async function demolish(buildingId: string): Promise<void> {
    const command: CommandTS = {
      type: "demolish_building",
      command_id: replica.nextCommandId(),
      building_id: buildingId,
    };
    try {
      const result = await client.send(command);
      if (result.result === "accepted") inspector.clear();
      else showToast(result.message, "error");
    } catch (err) {
      showToast(err instanceof Error ? err.message : String(err), "error");
    }
  }

  async function publishRecipe(
    recipe: RecipeTS,
  ): Promise<{ ok: boolean; definition?: AssetDefinitionTS; errors?: string[] }> {
    try {
      const response = await fetch("/api/assets", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(recipe),
      });
      if (response.ok) {
        const definition = (await response.json()) as AssetDefinitionTS;
        return { ok: true, definition };
      }
      const body = (await response.json().catch(() => null)) as { errors?: string[] } | null;
      return { ok: false, errors: body?.errors ?? [`publish failed with HTTP ${response.status}`] };
    } catch (err) {
      return { ok: false, errors: [err instanceof Error ? err.message : String(err)] };
    }
  }

  const designerDeps: DesignerDeps = {
    publishRecipe: (recipe) => publishRecipe(recipe),
    armPlacement: (def) => placement.arm(def),
    refreshCatalogue: (def) => upsertCatalogueEntry(def),
  };

  // Editors are a parallel milestone task (m1-04); until its implementations
  // land, the stubs throw — surface that explicitly instead of dying at boot.
  try {
    mountCatalogue(requiredElement("catalogue"), replica.definitions(), (def) =>
      placement.arm(def),
    );
    mountDesigner(requiredElement("designer"), designerDeps);
  } catch (err) {
    console.warn("editor modules unavailable on this branch (m1-04 pending)", err);
    const catalogue = requiredElement("catalogue");
    const designer = requiredElement("designer");
    catalogue.replaceChildren(notice("Catalogue arrives with the designer task (m1-04)."));
    designer.replaceChildren(notice("Designer arrives with task m1-04."));
  }

  function notice(text: string): HTMLElement {
    const el = document.createElement("p");
    el.className = "notice";
    el.textContent = text;
    return el;
  }

  const client = new CityClient({
    onHello: (world) => {
      replica.applySnapshot(world);
      for (const def of world.definitions) onDefinition(def);
      for (const building of world.buildings) refreshInstances(keyOf(building));
    },
    onEvent: (event) => {
      const diff = replica.applyEvent(event);
      if (event.type === "definition_published" && diff.published) onDefinition(diff.published);
      if (diff.placed) refreshInstances(keyOf(diff.placed));
      if (diff.removed) refreshInstances(keyOf(diff.removed));
    },
    onStatus: (status) => reflectBackend(backend, STATUS_LABEL[status]),
  });

  client.connect();
  engine.runRenderLoop(() => scene.render());
}

void boot();
