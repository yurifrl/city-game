// m1-04 owns this file. Contract: docs/contracts/milestone-1.md §Web app.
// Building designer form (name, archetype, footprint, floors, colors, window
// style, seed → recipe JSON → deps.publishRecipe) and the asset catalogue
// (list definitions, click to arm, upsert on definition_published).
// Capacity/cost always come from the server — the client never derives them.
import type {
  ArchetypeTS,
  AssetDefinitionTS,
  RecipeTS,
  WindowStyleTS,
} from "../protocol/types";

export interface DesignerDeps {
  publishRecipe(
    recipe: RecipeTS,
  ): Promise<{ ok: boolean; definition?: AssetDefinitionTS; errors?: string[] }>;
  armPlacement(def: AssetDefinitionTS): void;
  refreshCatalogue(def: AssetDefinitionTS): void;
}

const ARCHETYPES: readonly ArchetypeTS[] = ["house", "shop", "office", "tower"];
const WINDOW_STYLES: readonly WindowStyleTS[] = ["grid", "stripes"];
const SEED_MAX = 0xffffffff; // mirrors Rust Recipe.seed (u32)

interface CatalogueState {
  container: HTMLElement;
  onArm: (def: AssetDefinitionTS) => void;
}

// Module-level because upsertCatalogueEntry() must reach the catalogue mounted
// earlier without a handle. mountCatalogue() is the single writer.
let catalogue: CatalogueState | null = null;

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== undefined) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function clampToRange(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readInt(input: HTMLInputElement, min: number, max: number, fallback: number): number {
  const value = Math.trunc(input.valueAsNumber);
  if (!Number.isFinite(value)) return fallback;
  return clampToRange(value, min, max);
}

function numberField(
  parent: HTMLElement,
  labelText: string,
  min: number,
  max: number,
  value: number,
): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "number";
  input.min = String(min);
  input.max = String(max);
  input.step = "1";
  input.value = String(value);
  parent.append(el("label", undefined, labelText), input);
  return input;
}

function colorField(parent: HTMLElement, labelText: string, value: string): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "color";
  input.value = value;
  parent.append(el("label", undefined, labelText), input);
  return input;
}

function selectField<T extends string>(
  parent: HTMLElement,
  labelText: string,
  values: readonly T[],
  value: T,
): HTMLSelectElement {
  const select = document.createElement("select");
  for (const option of values) {
    const node = document.createElement("option");
    node.value = option;
    node.textContent = option;
    select.appendChild(node);
  }
  select.value = value;
  parent.append(el("label", undefined, labelText), select);
  return select;
}

/** Single source of the card look for build and refresh — keeps them in lockstep. */
function cardChildren(def: AssetDefinitionTS): HTMLDivElement[] {
  const recipe = def.recipe;
  const meta = `${recipe.archetype} · ${recipe.footprint_w}×${recipe.footprint_h} · ${recipe.floors} fl · cap ${def.derived.capacity} · ${def.derived.cost_credits} cr`;
  return [el("div", "name", recipe.name), el("div", "meta", meta)];
}

function buildCard(def: AssetDefinitionTS): HTMLDivElement {
  const card = el("div", "def-card");
  card.dataset.defId = def.id;
  card.replaceChildren(...cardChildren(def));
  card.addEventListener("click", () => {
    markArmed(def.id);
    catalogue?.onArm(def);
  });
  return card;
}

function catalogueCards(): Iterable<HTMLDivElement> {
  return catalogue?.container.querySelectorAll<HTMLDivElement>(".def-card") ?? [];
}

function markArmed(id: string | null): void {
  for (const card of catalogueCards()) {
    card.classList.toggle("armed", card.dataset.defId === id);
  }
}

/** Clears the armed highlight (ESC / tool disarm). */
export function clearArmed(): void {
  markArmed(null);
}

export function mountCatalogue(
  container: HTMLElement,
  defs: AssetDefinitionTS[],
  onArm: (def: AssetDefinitionTS) => void,
): void {
  container.replaceChildren();
  container.appendChild(el("h3", undefined, "Catalogue"));
  catalogue = { container, onArm };

  const sorted = [...defs].sort((a, b) => Number(a.id) - Number(b.id));
  for (const def of sorted) container.appendChild(buildCard(def));
}

export function upsertCatalogueEntry(def: AssetDefinitionTS): void {
  // No catalogue mounted yet: hello.definitions mounts it before any publish
  // can arrive, so there is nothing to render into.
  if (!catalogue) return;
  for (const existing of catalogueCards()) {
    if (existing.dataset.defId === def.id) {
      existing.replaceChildren(...cardChildren(def));
      return;
    }
  }
  const card = buildCard(def);
  for (const sibling of catalogueCards()) {
    const siblingId = sibling.dataset.defId;
    if (siblingId !== undefined && Number(siblingId) > Number(def.id)) {
      catalogue.container.insertBefore(card, sibling);
      return;
    }
  }
  catalogue.container.appendChild(card);
}

export function mountDesigner(container: HTMLElement, deps: DesignerDeps): void {
  container.replaceChildren();
  container.appendChild(el("h3", undefined, "Designer"));

  const form = document.createElement("form");
  container.appendChild(form);

  const nameField = document.createElement("input");
  nameField.type = "text";
  nameField.maxLength = 64;
  nameField.value = "New Building";
  form.append(el("label", undefined, "Name"), nameField);

  const archetypeField = selectField(form, "Archetype", ARCHETYPES, "house");
  const rowDims = el("div", "row");
  form.appendChild(rowDims);
  const widthField = numberField(rowDims, "Width", 1, 8, 3);
  const depthField = numberField(rowDims, "Depth", 1, 8, 3);
  const floorsField = numberField(form, "Floors", 1, 40, 3);
  const rowColors = el("div", "row");
  form.appendChild(rowColors);
  const baseField = colorField(rowColors, "Walls", "#b5654f");
  const roofField = colorField(rowColors, "Roof", "#3a3a3a");
  const styleField = selectField(form, "Window style", WINDOW_STYLES, "grid");
  const seedField = numberField(form, "Seed", 0, SEED_MAX, 7);

  const preview = el("pre", "meta");
  form.appendChild(preview);

  const publish = document.createElement("button");
  publish.type = "submit";
  publish.textContent = "Publish";
  form.appendChild(publish);

  const derived = el("div", "derived");
  derived.hidden = true;
  form.appendChild(derived);
  const errors = el("div", "errors");
  errors.hidden = true;
  form.appendChild(errors);

  const recipe = (): RecipeTS => ({
    name: nameField.value,
    archetype: archetypeField.value as ArchetypeTS,
    footprint_w: readInt(widthField, 1, 8, 3),
    footprint_h: readInt(depthField, 1, 8, 3),
    floors: readInt(floorsField, 1, 40, 3),
    base_color: baseField.value,
    roof_color: roofField.value,
    window_style: styleField.value as WindowStyleTS,
    seed: readInt(seedField, 0, SEED_MAX, 7),
  });

  const showPreview = (): void => {
    preview.textContent = JSON.stringify(recipe(), null, 2);
  };
  form.addEventListener("input", showPreview);
  showPreview();

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void handlePublish();
  });

  async function handlePublish(): Promise<void> {
    if (publish.disabled) return;
    publish.disabled = true;
    derived.hidden = true;
    errors.hidden = true;
    try {
      const result = await deps.publishRecipe(recipe());
      if (result.ok && result.definition) {
        derived.textContent = `def ${result.definition.id} · capacity ${result.definition.derived.capacity} · cost ${result.definition.derived.cost_credits} cr`;
        derived.hidden = false;
        deps.refreshCatalogue(result.definition);
        markArmed(result.definition.id);
        deps.armPlacement(result.definition);
      } else {
        errors.textContent = (result.errors ?? ["publish rejected"]).join("\n");
        errors.hidden = false;
      }
    } catch (reason) {
      errors.textContent = `publish failed: ${reason instanceof Error ? reason.message : String(reason)}`;
      errors.hidden = false;
    } finally {
      publish.disabled = false;
    }
  }
}
