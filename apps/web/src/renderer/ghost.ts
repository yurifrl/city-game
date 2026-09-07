// Semi-transparent placement preview. A unit box is scaled to the
// definition's effective footprint (odd-rotation swap included) and derived
// height, then tinted green/red by validity. This is a placement aid only —
// the authoritative look comes from compiled prototypes (assets/compiler.ts).

import { Color3, Mesh, MeshBuilder, Scene, StandardMaterial } from "@babylonjs/core";
import type { AssetDefinitionTS, CellTS } from "../protocol/types";
import { footprintSize } from "../world/replica";

const VALID_TINT = Color3.FromHexString("#46c46a");
const INVALID_TINT = Color3.FromHexString("#e05548");
const METERS_PER_FLOOR = 3;

export class Ghost {
  private box: Mesh;
  private material: StandardMaterial;
  private shown = false;

  constructor(scene: Scene) {
    this.box = MeshBuilder.CreateBox("ghost", { size: 1 }, scene);
    this.material = new StandardMaterial("ghost-material", scene);
    this.material.emissiveColor = VALID_TINT.clone();
    this.material.diffuseColor = Color3.Black();
    this.material.disableLighting = true;
    this.material.alpha = 0.45;
    this.box.material = this.material;
    this.box.isPickable = false;
    this.box.setEnabled(false);
  }

  show(def: AssetDefinitionTS, cell: CellTS, rotation: number, valid: boolean): void {
    const { w, h } = footprintSize(def.recipe, rotation);
    const height = Math.max(1, def.recipe.floors) * METERS_PER_FLOOR;
    this.box.scaling.set(w, height, h);
    this.box.position.set(cell.x + w / 2, height / 2, cell.y + h / 2);
    this.material.emissiveColor.copyFrom(valid ? VALID_TINT : INVALID_TINT);
    this.box.setEnabled(true);
    this.shown = true;
  }

  hide(): void {
    if (!this.shown) return;
    this.box.setEnabled(false);
    this.shown = false;
  }

  dispose(): void {
    this.box.dispose();
    this.material.dispose();
  }
}
