// m1-04 owns this file. This signature is the contract
// (docs/contracts/milestone-1.md); replace the stub body entirely.
import { Mesh, Scene } from "@babylonjs/core";
import type { AssetDefinitionTS } from "../protocol/types";

export interface CompiledPrototype {
  mesh: Mesh;
  triangleCount: number;
  materialCount: number;
}

export function compilePrototype(_def: AssetDefinitionTS, _scene: Scene): CompiledPrototype {
  throw new Error("m1-04 implements compilePrototype");
}
