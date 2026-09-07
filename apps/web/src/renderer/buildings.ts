// Thin-instance renderer for buildings. One prototype mesh per published
// asset revision (key `${defId}@${revision}`, compiled by assets/compiler.ts);
// every building of that revision renders as a thin instance of it. Instance
// entries are buffered per key, so prototype registration may happen before or
// after instances arrive. Entity↔instance maps support picking and removal.
//
// Prototype convention (mirrors docs/contracts/milestone-1.md footprint
// anchoring): the compiled mesh has the footprint's minimum cell at the local
// origin, footprint spanning +x/+z from there, base at y=0.

import { AbstractMesh, Matrix, Mesh } from "@babylonjs/core";
import type { PickingInfo } from "@babylonjs/core";
import type { CellTS } from "../protocol/types";

export interface BuildingInstance {
  id: string;
  cell: CellTS;
  rotation: number;
  footprint_w: number;
  footprint_h: number;
}

/** World transform for one instance, including the odd-rotation dim swap. */
function instanceMatrix(entry: BuildingInstance): Matrix {
  const swap = (entry.rotation & 1) === 1;
  const w = swap ? entry.footprint_h : entry.footprint_w;
  const h = swap ? entry.footprint_w : entry.footprint_h;
  const toLocalCenter = Matrix.Translation(-entry.footprint_w / 2, 0, -entry.footprint_h / 2);
  const rotate = Matrix.RotationY((entry.rotation * Math.PI) / 2);
  const toWorldCenter = Matrix.Translation(entry.cell.x + w / 2, 0, entry.cell.y + h / 2);
  return toLocalCenter.multiply(rotate).multiply(toWorldCenter);
}

export class BuildingViews {
  private meshes = new Map<string, Mesh>();
  private keyByMeshId = new Map<number, string>();
  private entriesByKey = new Map<string, BuildingInstance[]>();
  private idsByKey = new Map<string, string[]>();
  private idToEntity = new Map<string, { key: string; index: number }>();

  registerPrototype(key: string, mesh: Mesh): void {
    const previous = this.meshes.get(key);
    if (previous && previous !== mesh) {
      this.keyByMeshId.delete(previous.uniqueId);
      previous.dispose();
    }
    this.meshes.set(key, mesh);
    this.keyByMeshId.set(mesh.uniqueId, key);
    mesh.thinInstanceEnablePicking = true;
    const buffered = this.entriesByKey.get(key);
    if (buffered) this.setInstances(key, buffered);
  }

  setInstances(key: string, entries: BuildingInstance[]): void {
    this.entriesByKey.set(key, entries);
    const mesh = this.meshes.get(key);
    if (!mesh) return;
    const data = new Float32Array(entries.length * 16);
    entries.forEach((entry, i) => instanceMatrix(entry).copyToArray(data, i * 16));
    mesh.thinInstanceSetBuffer("matrix", data, 16, false);
    mesh.thinInstanceCount = entries.length;
    if (entries.length > 0) mesh.thinInstanceRefreshBoundingInfo(true);
    mesh.setEnabled(entries.length > 0);
    this.idsByKey.set(key, entries.map((entry) => entry.id));
    this.reindex();
  }

  removeEntity(id: string): void {
    const entity = this.idToEntity.get(id);
    if (!entity) return;
    const entries = (this.entriesByKey.get(entity.key) ?? []).filter(
      (entry) => entry.id !== id,
    );
    this.setInstances(entity.key, entries);
  }

  /** Building id rendered under a picking hit on a prototype mesh, if any. */
  resolveBuildingId(pick: PickingInfo): string | null {
    if (!pick.hit || !pick.pickedMesh) return null;
    const key = this.keyByMeshId.get(pick.pickedMesh.uniqueId);
    if (!key) return null;
    const thin = (pick as PickingInfo & { thinInstanceIndex?: number }).thinInstanceIndex;
    if (thin === undefined || thin < 0) return null;
    return this.idsByKey.get(key)?.[thin] ?? null;
  }

  isPrototype(mesh: AbstractMesh): boolean {
    return this.keyByMeshId.has(mesh.uniqueId);
  }

  private reindex(): void {
    this.idToEntity.clear();
    for (const [key, ids] of this.idsByKey) {
      ids.forEach((id, index) => this.idToEntity.set(id, { key, index }));
    }
  }
}
