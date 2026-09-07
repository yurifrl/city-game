// 96×96 grid rendering: the dark ground plane (also the pointer-pick target
// for cell coordinates), the cell boundary line system, and a one-cell hover
// outline tinted by tool state.

import { Color3, Mesh, MeshBuilder, Scene, StandardMaterial, Vector3 } from "@babylonjs/core";
import type { CellTS, GridTS } from "../protocol/types";

export type HoverMode = "valid" | "invalid" | "neutral";

export interface GridView {
  /** Ground plane covering cell rect [0,width) × [0,height); picking target. */
  ground: Mesh;
  setHover(cell: CellTS | null, mode: HoverMode): void;
  dispose(): void;
}

const LINE_COLOR = Color3.FromHexString("#39445a");
const HOVER_COLORS: Record<HoverMode, Color3> = {
  valid: Color3.FromHexString("#4cc46a"),
  invalid: Color3.FromHexString("#e05548"),
  neutral: Color3.FromHexString("#8fa3c0"),
};

/** Cell under a ground-pick point, or null outside the grid. */
export function cellFromPoint(point: Vector3, grid: GridTS): CellTS | null {
  if (point.x < 0 || point.z < 0 || point.x >= grid.width || point.z >= grid.height) {
    return null;
  }
  return { x: Math.floor(point.x), y: Math.floor(point.z) };
}

export function createGrid(scene: Scene, grid: GridTS): GridView {
  const ground = MeshBuilder.CreateGround("ground", { width: grid.width, height: grid.height }, scene);
  ground.position.x = grid.width / 2;
  ground.position.z = grid.height / 2;
  const groundMaterial = new StandardMaterial("ground-material", scene);
  groundMaterial.diffuseColor = Color3.FromHexString("#1a1f28");
  groundMaterial.specularColor = Color3.Black();
  ground.material = groundMaterial;

  const lines: Vector3[][] = [];
  for (let x = 0; x <= grid.width; x++) {
    lines.push([new Vector3(x, 0.01, 0), new Vector3(x, 0.01, grid.height)]);
  }
  for (let y = 0; y <= grid.height; y++) {
    lines.push([new Vector3(0, 0.01, y), new Vector3(grid.width, 0.01, y)]);
  }
  const gridLines = MeshBuilder.CreateLineSystem("grid-lines", { lines }, scene);
  gridLines.color = LINE_COLOR;
  gridLines.isPickable = false;

  const unitCell = [
    new Vector3(0, 0, 0),
    new Vector3(1, 0, 0),
    new Vector3(1, 0, 1),
    new Vector3(0, 0, 1),
    new Vector3(0, 0, 0),
  ];
  const hover = MeshBuilder.CreateLineSystem("hover-cell", { lines: [unitCell] }, scene);
  hover.color = HOVER_COLORS.neutral.clone();
  hover.isPickable = false;
  hover.position.y = 0.02;
  hover.setEnabled(false);

  return {
    ground,
    setHover(cell: CellTS | null, mode: HoverMode): void {
      if (!cell) {
        hover.setEnabled(false);
        return;
      }
      hover.position.set(cell.x, 0.02, cell.y);
      hover.color = HOVER_COLORS[mode];
      hover.setEnabled(true);
    },
    dispose(): void {
      hover.dispose();
      gridLines.dispose();
      groundMaterial.dispose();
      ground.dispose();
    },
  };
}
