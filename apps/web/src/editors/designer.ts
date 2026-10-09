// m1-04 + asset-studio bridge. Unreal-style split: the catalogue is the place
// palette — clicking a card SELECTS it for placement (armed → click the grid).
// Editing lives only in the detail panel, opened via the card's ✎ button or
// double-click; tweaks publish as a NEW definition (published recipes are
// immutable — the server allocates a fresh id per publish). "New asset" opens
// the blank detail. Capacity/cost always come from the server — the client
// never derives them.
import type {
  ArchetypeTS,
  AssetDefinitionTS,
  AssetPartTS,
  RecipeTS,
  WindowStyleTS,
} from "../protocol/types";

export interface DesignerDeps {
  publishRecipe: (recipe: RecipeTS) => Promise<{
    ok: boolean;
    definition?: AssetDefinitionTS;
    errors?: string[];
  }>;
  removeDefinition: (id: string) => Promise<{ ok: boolean; errors?: string[] }>;
  refreshCatalogue: (def: AssetDefinitionTS) => void;
}

const ARCHETYPES: readonly ArchetypeTS[] = ["house", "shop", "office", "tower"];
const WINDOW_STYLES: readonly WindowStyleTS[] = ["grid", "stripes"];
const SEED_MAX = 0xffffffff; // mirrors Rust Recipe.seed (u32)

interface Panel {
  fields: {
    name: HTMLInputElement;
    archetype: HTMLSelectElement;
    width: HTMLInputElement;
    depth: HTMLInputElement;
    floors: HTMLInputElement;
    base: HTMLInputElement;
    roof: HTMLInputElement;
    style: HTMLSelectElement;
    seed: HTMLInputElement;
  };
  partsNote: HTMLElement;
  publish: HTMLButtonElement;
  remove: HTMLButtonElement;
  derived: HTMLElement;
  errors: HTMLElement;
  deps: DesignerDeps;
}

let catalogue: {
  grid: HTMLElement;
  onPlace: (def: AssetDefinitionTS) => void;
  onEdit: (def: AssetDefinitionTS) => void;
} | null = null;

let panel: Panel | null = null;
/** The definition the panel is currently editing (null = blank draft). */
let openDef: AssetDefinitionTS | null = null;
/** Authored parts carried through tweak-and-republish. */
let openParts: AssetPartTS[] = [];

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function clampToRange(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readInt(input: HTMLInputElement, min: number, max: number, fallback: number): number {
  const parsed = Number.parseInt(input.value, 10);
  return Number.isFinite(parsed) ? clampToRange(parsed, min, max) : fallback;
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
  input.value = String(value);
  const label = el("label", undefined, labelText) as HTMLLabelElement;
  label.appendChild(input);
  parent.appendChild(label);
  return input;
}

function colorField(parent: HTMLElement, labelText: string, value: string): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "color";
  input.value = value;
  const label = el("label", undefined, labelText) as HTMLLabelElement;
  label.appendChild(input);
  parent.appendChild(label);
  return input;
}

function selectField<T extends string>(
  parent: HTMLElement,
  labelText: string,
  values: readonly T[],
  value: T,
): HTMLSelectElement {
  const select = document.createElement("select");
  for (const item of values) {
    const option = document.createElement("option");
    option.value = item;
    option.textContent = item;
    if (item === value) option.selected = true;
    select.appendChild(option);
  }
  const label = el("label", undefined, labelText) as HTMLLabelElement;
  label.appendChild(select);
  parent.appendChild(label);
  return select;
}

// ---- iso icon painting ----

/** Scales a #rrggbb color; f>1 lightens toward white, f<1 darkens. */
function shadeHex(hex: string, f: number): string {
  const n = Number.parseInt(hex.slice(1), 16);
  const channel = (v: number): number =>
    Math.max(0, Math.min(255, Math.round(f > 1 ? v + (255 - v) * (f - 1) : v * f)));
  const r = channel((n >> 16) & 255);
  const g = channel((n >> 8) & 255);
  const b = channel(n & 255);
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}

const OUTLINE = "#10131a";

function polygon(
  g: CanvasRenderingContext2D,
  points: Array<[number, number]>,
  fill: string,
): void {
  const first = points[0];
  if (!first) return;
  g.beginPath();
  g.moveTo(first[0], first[1]);
  for (let i = 1; i < points.length; i++) {
    const point = points[i];
    if (point) g.lineTo(point[0], point[1]);
  }
  g.closePath();
  g.fillStyle = fill;
  g.fill();
  g.strokeStyle = OUTLINE;
  g.lineWidth = 1;
  g.stroke();
}

/**
 * Paints a small isometric thumbnail. Parts render as shaded boxes (prisms
 * included); procedural recipes render their mass + roof slab.
 */
function drawIcon(canvas: HTMLCanvasElement, def: AssetDefinitionTS): void {
  const g = canvas.getContext("2d");
  if (!g) return;
  const W = canvas.width;
  const H = canvas.height;
  g.clearRect(0, 0, W, H);
  const r = def.recipe;
  const tallest = r.parts.length > 0
    ? Math.max(1, ...r.parts.map((p) => p.y + p.h))
    : r.floors * 3;
  const span = Math.max(
    r.footprint_w + r.footprint_h,
    r.footprint_w + tallest,
    r.footprint_h + tallest,
  );
  const k = Math.min((W - 12) / span, (H - 12) / span);
  const cx = (r.footprint_w - r.footprint_h) / 2;
  const cz = (r.footprint_w + r.footprint_h) / 4;
  const project = (x: number, y: number, z: number): [number, number] => [
    W / 2 + (x - z - cx) * k,
    H / 2 + ((x + z) / 2 - cz) * k - y * k,
  ];
  const box = (
    x: number, y: number, z: number, w: number, h: number, d: number, color: string,
  ): void => {
    polygon(g, [
      project(x + w, y + h, z), project(x + w, y + h, z + d),
      project(x + w, y, z + d), project(x + w, y, z),
    ], shadeHex(color, 0.68));
    polygon(g, [
      project(x, y + h, z + d), project(x + w, y + h, z + d),
      project(x + w, y, z + d), project(x, y, z + d),
    ], shadeHex(color, 0.9));
    polygon(g, [
      project(x, y + h, z), project(x + w, y + h, z),
      project(x + w, y + h, z + d), project(x, y + h, z + d),
    ], shadeHex(color, 1.22));
  };
  const gable = (
    x: number, y: number, z: number, w: number, h: number, d: number, color: string,
  ): void => {
    const hw = w / 2;
    polygon(g, [
      project(x, y, z + d), project(x + w, y, z + d), project(x + hw, y + h, z + d / 2),
    ], shadeHex(color, 0.9));
    polygon(g, [
      project(x + w, y, z), project(x, y, z), project(x + hw, y + h, z + d / 2),
    ], shadeHex(color, 1.12));
    polygon(g, [
      project(x + w, y, z + d), project(x + w, y, z), project(x + hw, y + h, z + d / 2),
    ], shadeHex(color, 0.68));
    polygon(g, [
      project(x, y, z + d), project(x, y, z), project(x + hw, y + h, z + d / 2),
    ], shadeHex(color, 0.6));
  };
  if (r.parts.length > 0) {
    for (const part of [...r.parts].sort((a, b) => a.x + a.z - (b.x + b.z))) {
      if (part.kind === "roof_gable") gable(part.x, part.y, part.z, part.w, part.h, part.d, part.color);
      else box(part.x, part.y, part.z, part.w, part.h, part.d, part.color);
    }
  } else {
    const height = r.floors * 3;
    box(0, 0, 0, r.footprint_w, height, r.footprint_h, r.base_color);
    box(-0.15, height, -0.15, r.footprint_w + 0.3, 0.5, r.footprint_h + 0.3, r.roof_color);
  }
}

// ---- catalogue ----

function metaLine(def: AssetDefinitionTS): string {
  const r = def.recipe;
  const origin = r.parts.length > 0 ? "studio" : r.archetype;
  return `${origin} · ${r.footprint_w}×${r.footprint_h} · ${r.floors} fl · cap ${def.derived.capacity} · ${def.derived.cost_credits} cr`;
}

function buildCard(def: AssetDefinitionTS): HTMLDivElement {
  const card = el("div", "def-card");
  card.dataset.defId = def.id;
  const icon = document.createElement("canvas");
  icon.className = "def-icon";
  icon.width = 152;
  icon.height = 84;
  drawIcon(icon, def);
  card.appendChild(icon);
  card.appendChild(el("div", "name", def.recipe.name));
  card.appendChild(el("div", "meta", metaLine(def)));
  card.title = "Click: select for placement · double-click or ✎: edit";
  card.addEventListener("click", () => catalogue?.onPlace(def));
  card.addEventListener("dblclick", () => catalogue?.onEdit(def));
  const edit = el("button", "card-edit", "✎");
  edit.type = "button";
  edit.title = "Edit this asset";
  edit.addEventListener("click", (event) => {
    event.stopPropagation();
    catalogue?.onEdit(def);
  });
  card.appendChild(edit);
  return card;
}

function catalogueCards(): Iterable<HTMLDivElement> {
  return catalogue?.grid.querySelectorAll<HTMLDivElement>(".def-card") ?? [];
}

function markArmed(id: string | null): void {
  for (const card of catalogueCards()) {
    card.classList.toggle("armed", card.dataset.defId === id);
  }
}

/** Highlights the armed definition's card (or none). */
export function setArmed(id: string | null): void {
  markArmed(id);
}

/** Opens the detail panel: a definition for tweaking, or null for a blank draft. */
export function openDetail(def: AssetDefinitionTS | null): void {
  if (!panel) return;
  openDef = def;
  openParts = def?.recipe.parts ?? [];
  const r = def?.recipe;
  panel.fields.name.value = r?.name ?? "New asset";
  panel.fields.archetype.value = r?.archetype ?? "house";
  panel.fields.width.value = String(r?.footprint_w ?? 3);
  panel.fields.depth.value = String(r?.footprint_h ?? 3);
  panel.fields.floors.value = String(r?.floors ?? 3);
  panel.fields.base.value = r?.base_color ?? "#b5654f";
  panel.fields.roof.value = r?.roof_color ?? "#3a3a3a";
  panel.fields.style.value = r?.window_style ?? "grid";
  panel.fields.seed.value = String(r?.seed ?? 7);
  panel.partsNote.textContent = r
    ? r.parts.length > 0
      ? `${r.parts.length} authored parts from the Asset Studio — geometry is preserved when you publish changes.`
      : "Procedural recipe — geometry derives from the fields above."
    : "Draft: geometry derives from the fields below.";
  panel.remove.hidden = !def;
  panel.derived.hidden = true;
  panel.errors.hidden = true;
}

export function mountCatalogue(
  container: HTMLElement,
  defs: AssetDefinitionTS[],
  onPlace: (def: AssetDefinitionTS) => void,
  onEdit: (def: AssetDefinitionTS) => void,
): void {
  container.replaceChildren();
  const header = el("div", "catalogue-header");
  header.appendChild(el("h3", undefined, "Catalogue"));
  const newButton = el("button", "ghost-button", "+ New");
  newButton.type = "button";
  newButton.title = "Create a new asset from scratch";
  header.appendChild(newButton);
  container.appendChild(header);
  const grid = el("div", "def-grid");
  container.appendChild(grid);
  catalogue = { grid, onPlace, onEdit };

  const sorted = [...defs].sort((a, b) => Number(a.id) - Number(b.id));
  for (const def of sorted) grid.appendChild(buildCard(def));

  newButton.addEventListener("click", () => openDetail(null));
}

export function upsertCatalogueEntry(def: AssetDefinitionTS): void {
  if (!catalogue) return;
  for (const existing of catalogueCards()) {
    if (existing.dataset.defId === def.id) {
      existing.replaceWith(buildCard(def));
      return;
    }
  }
  const card = buildCard(def);
  for (const sibling of catalogueCards()) {
    const siblingId = sibling.dataset.defId;
    if (siblingId !== undefined && Number(siblingId) > Number(def.id)) {
      catalogue.grid.insertBefore(card, sibling);
      return;
    }
  }
  catalogue.grid.appendChild(card);
}

export function removeCatalogueEntry(id: string): void {
  for (const card of catalogueCards()) {
    if (card.dataset.defId === id) card.remove();
  }
}

export function mountDesigner(container: HTMLElement, deps: DesignerDeps): void {
  container.replaceChildren();
  container.appendChild(el("h3", undefined, "Asset details"));

  const form = document.createElement("form");
  container.appendChild(form);

  const nameField = document.createElement("input");
  nameField.type = "text";
  nameField.maxLength = 64;
  nameField.value = "New asset";
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

  const partsNote = el("div", "parts-note");
  form.appendChild(partsNote);

  const actions = el("div", "detail-actions");
  form.appendChild(actions);

  const publish = document.createElement("button");
  publish.type = "submit";
  publish.textContent = "Publish as new";
  actions.appendChild(publish);

  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "ghost-button danger";
  remove.textContent = "Remove";
  remove.hidden = true;
  actions.appendChild(remove);

  const derived = el("div", "derived");
  derived.hidden = true;
  form.appendChild(derived);
  const errors = el("div", "errors");
  errors.hidden = true;
  form.appendChild(errors);

  panel = {
    fields: {
      name: nameField,
      archetype: archetypeField,
      width: widthField,
      depth: depthField,
      floors: floorsField,
      base: baseField,
      roof: roofField,
      style: styleField,
      seed: seedField,
    },
    partsNote,
    publish,
    remove,
    derived,
    errors,
    deps,
  };

  const recipe = (): RecipeTS => ({
    name: panel!.fields.name.value,
    archetype: panel!.fields.archetype.value as ArchetypeTS,
    footprint_w: readInt(panel!.fields.width, 1, 8, 3),
    footprint_h: readInt(panel!.fields.depth, 1, 8, 3),
    floors: readInt(panel!.fields.floors, 1, 40, 3),
    base_color: panel!.fields.base.value,
    roof_color: panel!.fields.roof.value,
    window_style: panel!.fields.style.value as WindowStyleTS,
    seed: readInt(panel!.fields.seed, 0, SEED_MAX, 7),
    parts: openParts,
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void handlePublish();
  });
  remove.addEventListener("click", () => {
    if (openDef) void handleRemove(openDef.id);
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
        openDef = result.definition;
        openParts = result.definition.recipe.parts;
        remove.hidden = false;
        deps.refreshCatalogue(result.definition);
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

  async function handleRemove(id: string): Promise<void> {
    remove.disabled = true;
    errors.hidden = true;
    try {
      const result = await deps.removeDefinition(id);
      if (result.ok) {
        removeCatalogueEntry(id);
        openDetail(null);
      } else {
        errors.textContent = (result.errors ?? ["remove rejected"]).join("\n");
        errors.hidden = false;
      }
    } catch (reason) {
      errors.textContent = `remove failed: ${reason instanceof Error ? reason.message : String(reason)}`;
      errors.hidden = false;
    } finally {
      remove.disabled = false;
    }
  }
}
