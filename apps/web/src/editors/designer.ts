// m1-04 owns this file. These signatures are the contract
// (docs/contracts/milestone-1.md); replace the stub bodies entirely.
import type { AssetDefinitionTS, RecipeTS } from "../protocol/types";

export interface DesignerDeps {
  publishRecipe(
    recipe: RecipeTS,
  ): Promise<{ ok: boolean; definition?: AssetDefinitionTS; errors?: string[] }>;
  armPlacement(def: AssetDefinitionTS): void;
  refreshCatalogue(def: AssetDefinitionTS): void;
}

export function mountDesigner(_container: HTMLElement, _deps: DesignerDeps): void {
  throw new Error("m1-04 implements mountDesigner");
}

export function mountCatalogue(
  _container: HTMLElement,
  _defs: AssetDefinitionTS[],
  _onArm: (def: AssetDefinitionTS) => void,
): void {
  throw new Error("m1-04 implements mountCatalogue");
}

export function upsertCatalogueEntry(_def: AssetDefinitionTS): void {
  throw new Error("m1-04 implements upsertCatalogueEntry");
}
