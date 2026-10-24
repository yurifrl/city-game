// Rendering layer for remote seats: one cursor marker per remote actor
// positioned on the ground plane at the actor's latest reported cell, a small
// floating label chip, and an armed building ghost when the actor is dragging
// a placement. All positions derive from the presence board's latest-wins
// state; the local seat's own cursor is never rendered here.

import {
  Color3,
  Matrix,
  Mesh,
  MeshBuilder,
  Scene,
  StandardMaterial,
  TransformNode,
  Vector3,
} from "@babylonjs/core";
import type { AbstractEngine, Camera, Observer } from "@babylonjs/core";
import type { AssetDefinitionTS } from "../protocol/types";
import { footprintSize } from "../world/replica";
import { PresenceBoard, type PresenceEntry } from "./board";

export interface PresenceLayerDeps {
  scene: Scene;
  engine: AbstractEngine;
  camera: Camera;
  replica: WorldReplicaLike;
  board: PresenceBoard;
  host: HTMLElement;
  /** Local actor id — its cursor is never rendered here. */
  selfActor: string | null;
}

/** Minimal surface of the world replica the layer needs. */
export interface WorldReplicaLike {
  definition(defId: string): AssetDefinitionTS | undefined;
}

interface RemoteView {
  root: TransformNode;
  dot: Mesh;
  ghost: Mesh;
  material: StandardMaterial;
  ghostMaterial: StandardMaterial;
  label: HTMLDivElement;
  labelText: string;
  lastCell: string | null;
}

const METERS_PER_FLOOR = 3;
const GHOST_MIN_HEIGHT = 1;
const GHOST_ALPHA = 0.45;

/** Stable per-actor color from the actor id (hsl for CSS, Color3 for Babylon). */
function actorColor(actor: string): { css: string; babylon: Color3 } {
  let hash = 0;
  for (let i = 0; i < actor.length; i++) {
    hash = (hash * 31 + actor.charCodeAt(i)) >>> 0;
  }
  const hue = hash % 360;
  return {
    css: `hsl(${hue}, 70%, 65%)`,
    babylon: Color3.FromHSV(hue, 0.7, 0.95),
  };
}

/** Projects a world-space point to CSS percentage coordinates within the canvas. */
function projectToCanvas(
  engine: AbstractEngine,
  camera: Camera,
  world: Vector3,
): { left: string; top: string } {
  const width = engine.getRenderWidth();
  const height = engine.getRenderHeight();
  const p = Vector3.Project(
    world,
    Matrix.Identity(),
    camera.getTransformationMatrix(),
    camera.viewport.toGlobal(width, height),
  );
  return {
    left: `${(p.x / width) * 100}%`,
    top: `${(1 - p.y / height) * 100}%`,
  };
}

export class PresenceLayer {
  private readonly scene: Scene;
  private readonly engine: AbstractEngine;
  private readonly camera: Camera;
  private readonly replica: WorldReplicaLike;
  private readonly board: PresenceBoard;
  private readonly host: HTMLElement;
  private selfActor: string | null;
  private readonly views = new Map<string, RemoteView>();
  private observer: Observer<Scene> | null = null;
  private disposed = false;

  constructor(deps: PresenceLayerDeps) {
    this.scene = deps.scene;
    this.engine = deps.engine;
    this.camera = deps.camera;
    this.replica = deps.replica;
    this.board = deps.board;
    this.host = deps.host;
    this.selfActor = deps.selfActor;
    this.observer = this.scene.onBeforeRenderObservable.add(() => this.renderFrame());
  }

  setSelfActor(actor: string | null): void {
    this.selfActor = actor;
    if (actor !== null) this.renderFrame();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.observer) this.scene.onBeforeRenderObservable.remove(this.observer);
    for (const view of this.views.values()) this.destroyView(view);
    this.views.clear();
  }

  private renderFrame(): void {
    if (this.disposed) return;
    const live = new Set<string>();
    for (const entry of this.board.getAll()) {
      if (entry.actor === this.selfActor) continue;
      live.add(entry.actor);
      const view = this.views.get(entry.actor) ?? this.createView(entry);
      this.updateView(view, entry);
    }
    for (const [actor, view] of this.views) {
      if (!live.has(actor)) {
        this.destroyView(view);
        this.views.delete(actor);
      }
    }
  }

  private createView(entry: PresenceEntry): RemoteView {
    const root = new TransformNode(`presence-${entry.actor}`, this.scene);
    const dot = MeshBuilder.CreateDisc(
      `presence-dot-${entry.actor}`,
      { radius: 0.35, tessellation: 24, sideOrientation: Mesh.DOUBLESIDE },
      this.scene,
    );
    dot.rotation.x = Math.PI / 2;
    dot.isPickable = false;
    dot.parent = root;

    const ghost = MeshBuilder.CreateBox(`presence-ghost-${entry.actor}`, { size: 1 }, this.scene);
    ghost.isPickable = false;
    ghost.setEnabled(false);
    ghost.parent = root;

    const material = new StandardMaterial(`presence-material-${entry.actor}`, this.scene);
    material.emissiveColor = actorColor(entry.actor).babylon;
    material.diffuseColor = Color3.Black();
    material.disableLighting = true;
    material.alpha = 0.85;
    dot.material = material;

    const ghostMaterial = new StandardMaterial(`presence-ghost-material-${entry.actor}`, this.scene);
    ghostMaterial.emissiveColor = actorColor(entry.actor).babylon;
    ghostMaterial.diffuseColor = Color3.Black();
    ghostMaterial.disableLighting = true;
    ghostMaterial.alpha = GHOST_ALPHA;
    ghost.material = ghostMaterial;

    const label = document.createElement("div");
    label.className = "presence-cursor";
    label.style.borderColor = actorColor(entry.actor).css;
    this.host.append(label);

    const view: RemoteView = {
      root,
      dot,
      ghost,
      material,
      ghostMaterial,
      label,
      labelText: "",
      lastCell: null,
    };
    this.views.set(entry.actor, view);
    return view;
  }

  private updateView(view: RemoteView, entry: PresenceEntry): void {
    view.root.position.set(entry.cell.x + 0.5, 0.06, entry.cell.y + 0.5);
    view.dot.rotation.z = -Math.PI / 2;

    const def = entry.def_id === null ? null : this.replica.definition(entry.def_id);
    if (def && entry.kind === "drag") {
      const { w, h } = footprintSize(def.recipe, entry.rotation);
      const height = Math.max(GHOST_MIN_HEIGHT, def.recipe.floors) * METERS_PER_FLOOR;
      view.ghost.scaling.set(w, height, h);
      view.ghost.position.set(entry.cell.x + w / 2, height / 2, entry.cell.y + h / 2);
      view.ghost.setEnabled(true);
    } else {
      view.ghost.setEnabled(false);
    }

    const cellKey = `${entry.cell.x},${entry.cell.y}`;
    if (cellKey !== view.lastCell || view.labelText !== entry.label) {
      view.labelText = entry.label;
      view.lastCell = cellKey;
      const projected = projectToCanvas(this.engine, this.camera, view.root.position);
      view.label.style.left = projected.left;
      view.label.style.top = projected.top;
      view.label.textContent = entry.label;
    }
  }

  private destroyView(view: RemoteView): void {
    view.dot.dispose();
    view.ghost.dispose();
    view.material.dispose();
    view.ghostMaterial.dispose();
    view.root.dispose();
    view.label.remove();
  }
}
