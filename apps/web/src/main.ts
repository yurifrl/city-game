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
import {
  mountCatalogue,
  mountDesigner,
  openDetail,
  removeCatalogueEntry,
  setArmed,
  upsertCatalogueEntry,
} from "./editors/designer";
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
import { mountCommandPalette } from "./commands";
import { WorldReplica } from "./world/replica";
import { showToast } from "./ui/toast";
import { PresenceBoard } from "./presence/board";
import { PresenceLayer } from "./presence/view";

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
  const presenceBoard = new PresenceBoard();
  const gridView = createGrid(scene, { width: 96, height: 96 });
  const views = new BuildingViews();
  const ghost = new Ghost(scene);
  const cameraRig = createCamera(scene, canvas, new Vector3(48, 0, 48));
  const presenceLayer = new PresenceLayer({
    scene,
    engine,
    camera: cameraRig.camera,
    replica,
    board: presenceBoard,
    host: requiredElement("presence-overlay"),
    selfActor: null,
  });
  const identityChip = requiredElement("identity-chip");

  // Browser-console debug handle for E2E tests and diagnostics, not a game surface.
  (window as unknown as Record<string, unknown>).__city = {
    replica,
    scene,
    camera: cameraRig.camera,
  };

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
    sendInput: (input) => client.sendInput(input),
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
    onArm: (def) => setArmed(def.id),
    onDisarm: () => setArmed(null),
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
      else if (result.result === "rejected") showToast(result.message, "error");
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

  const removeDefinition = async (
    id: string,
  ): Promise<{ ok: boolean; errors?: string[] }> => {
    try {
      const response = await fetch(`/api/assets/${id}`, { method: "DELETE" });
      if (response.ok) return { ok: true };
      const body = (await response.json().catch(() => null)) as { errors?: string[] } | null;
      return { ok: false, errors: body?.errors ?? [`remove failed with HTTP ${response.status}`] };
    } catch (err) {
      return { ok: false, errors: [err instanceof Error ? err.message : String(err)] };
    }
  };

  const designerDeps: DesignerDeps = {
    publishRecipe: (recipe) => publishRecipe(recipe),
    removeDefinition,
    refreshCatalogue: (def) => upsertCatalogueEntry(def),
  };

  mountCatalogue(
    requiredElement("catalogue"),
    replica.definitions(),
    (def) => {
      placement.arm(def);
      showToast(`placing ${def.recipe.name} — click the grid · R rotate · Esc cancel`, "info");
    },
    (def) => openDetail(def),
  );
  mountDesigner(requiredElement("designer"), designerDeps);

  const client = new CityClient({
    onIdentified: (you) => {
      presenceLayer.setSelfActor(you);
      identityChip.textContent = `you: ${you.slice(0, 4)}`;
    },
    onHello: (world) => {
      presenceBoard.clear();
      replica.applySnapshot(world);
      for (const def of world.definitions) onDefinition(def);
      for (const building of world.buildings) refreshInstances(keyOf(building));
      // Seat boot readiness for the E2E harness: the authoritative snapshot
      // is applied, so the screencast now shows the real world, not a blank
      // scene. Must fire after applySnapshot, before first paint settles.
      (window as unknown as Record<string, unknown>).__cityReady = true;
    },
    onEvent: (event) => {
      switch (event.type) {
        case "player_joined":
          presenceBoard.join(event.actor, event.label);
          return;
        case "player_left":
          presenceBoard.leave(event.actor);
          return;
        case "presence":
          presenceBoard.apply(event.presence);
          return;
        default:
          break;
      }
      const diff = replica.applyEvent(event);
      if (event.type === "definition_published" && diff.published) onDefinition(diff.published);
      if (event.type === "definition_removed" && diff.removedDef) {
        removeCatalogueEntry(diff.removedDef);
        setArmed(null);
        showToast(`asset ${diff.removedDef} removed`, "info");
      }
      if (diff.placed) refreshInstances(keyOf(diff.placed));
      if (diff.removed) refreshInstances(keyOf(diff.removed));
      for (const cleared of diff.cleared ?? []) refreshInstances(keyOf(cleared));
    },
    onStatus: (status) => reflectBackend(backend, STATUS_LABEL[status]),
  });

  mountCommandPalette({ client, replica });

  client.connect();
  engine.runRenderLoop(() => scene.render());
}

void boot();
