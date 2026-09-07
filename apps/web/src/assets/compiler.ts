// m1-04 owns this file. Contract: docs/contracts/milestone-1.md §Web app.
// Deterministic compilePrototype(def, scene): the whole prototype is derived
// from (recipe, seed) — box mass footprint × floors×3m, facade drawn on a
// canvas DynamicTexture (grid or stripes windows, base_color walls, bounded
// hue jitter from the seed), roof_color flat slab + parapet, seed-driven
// rooftop box. Everything merges into ONE mesh: ≤ ~600 triangles, 1 material,
// no per-window meshes.
import {
  Color3,
  DynamicTexture,
  Mesh,
  MeshBuilder,
  Scene,
  StandardMaterial,
  Vector4,
} from "@babylonjs/core";
import type { ICanvasRenderingContext } from "@babylonjs/core";
import type { AssetDefinitionTS, WindowStyleTS } from "../protocol/types";

export interface CompiledPrototype {
  mesh: Mesh;
  triangleCount: number;
  materialCount: number;
}

const FLOOR_HEIGHT_M = 3;
const WINDOW_SILL_M = 0.8;
const WINDOW_TOP_M = 2.2;
const WINDOW_WIDTH_M = 0.6;
const PLINTH_HEIGHT_M = 0.6;
const SLAB_THICKNESS_M = 0.35;
const SLAB_OVERHANG_M = 0.1;
const PARAPET_THICKNESS_M = 0.25;
const PARAPET_HEIGHT_M = 0.4;
const ROOF_BOX_CLEARANCE_M = 0.35; // parapet thickness + breathing room
const BASE_PPM = 24; // texture pixels per metre, before the canvas-size cap
const MAX_CANVAS_EDGE_PX = 2048;

interface Hsl {
  h: number; // degrees 0..360
  s: number; // percent 0..100
  l: number; // percent 0..100
}

interface FacadeColors {
  wall: string;
  plinth: string;
  window: string;
  frame: string;
  mullion: string;
  roof: string;
  parapet: string;
}

interface FacadeLayout {
  facadeWidthPx: number;
  canvasHeightPx: number;
  patchWidthPx: number;
  ppm: number;
  perimeterM: number;
  floors: number;
  windowStyle: WindowStyleTS;
}

/** mulberry32 — small, fast, fully deterministic given the u32 seed. */
function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function parseHexColor(hex: string): Hsl {
  const match = /^#([0-9a-fA-F]{6})$/.exec(hex);
  const digits = match?.[1];
  if (digits === undefined) throw new Error(`invalid color "${hex}"`);
  const value = Number.parseInt(digits, 16);
  const r = ((value >> 16) & 0xff) / 255;
  const g = ((value >> 8) & 0xff) / 255;
  const b = (value & 0xff) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l: l * 100 };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) * 60;
  else if (max === g) h = ((b - r) / d + 2) * 60;
  else h = ((r - g) / d + 4) * 60;
  return { h, s: s * 100, l: l * 100 };
}

function cssHsl(color: Hsl): string {
  return `hsl(${color.h.toFixed(2)}, ${color.s.toFixed(2)}%, ${color.l.toFixed(2)}%)`;
}

/** Bounded per-building variation so same-palette seeds are not identical. */
function jitterColor(color: Hsl, rng: () => number): Hsl {
  return {
    h: (color.h + (rng() * 2 - 1) * 5 + 360) % 360,
    s: clamp(color.s + (rng() * 2 - 1) * 3, 0, 100),
    l: clamp(color.l + (rng() * 2 - 1) * 3, 2, 98),
  };
}


function drawFacade(
  ctx: ICanvasRenderingContext,
  layout: FacadeLayout,
  colors: FacadeColors,
): void {
  const { facadeWidthPx, canvasHeightPx, patchWidthPx, ppm, perimeterM, floors, windowStyle } =
    layout;
  const px = (meters: number): number => Math.round(meters * ppm);
  const yPx = (worldY: number): number => canvasHeightPx - px(worldY); // y down on canvas

  ctx.fillStyle = colors.wall;
  ctx.fillRect(0, 0, facadeWidthPx, canvasHeightPx);

  // Darker ground band keeps the silhouette base crisp zoomed out.
  ctx.fillStyle = colors.plinth;
  ctx.fillRect(0, yPx(PLINTH_HEIGHT_M), facadeWidthPx, canvasHeightPx - yPx(PLINTH_HEIGHT_M));

  for (let floor = 0; floor < floors; floor++) {
    const baseY = floor * FLOOR_HEIGHT_M;
    const bandTop = yPx(baseY + WINDOW_TOP_M);
    const bandHeight = px(WINDOW_TOP_M - WINDOW_SILL_M);
    if (windowStyle === "stripes") {
      // Continuous ribbon window per floor, mullions every 2 m.
      ctx.fillStyle = colors.window;
      ctx.fillRect(0, bandTop, facadeWidthPx, bandHeight);
      ctx.fillStyle = colors.mullion;
      const mullionWidth = Math.max(1, px(0.08));
      for (let m = 2; m < perimeterM; m += 2) {
        ctx.fillRect(px(m), bandTop, mullionWidth, bandHeight);
      }
    } else {
      // One dark window per metre cell with a lighter frame for contrast.
      const winWidth = px(WINDOW_WIDTH_M);
      const inset = Math.max(1, px(0.09));
      for (let m = 0; m < perimeterM; m++) {
        const winX = px(m) + Math.floor((ppm - winWidth) / 2);
        ctx.fillStyle = colors.frame;
        ctx.fillRect(winX, bandTop, winWidth, bandHeight);
        ctx.fillStyle = colors.window;
        ctx.fillRect(winX + inset, bandTop + inset, winWidth - 2 * inset, bandHeight - 2 * inset);
      }
      // Subtle floor-slab line keeps storeys legible zoomed out.
      ctx.fillStyle = "rgba(0, 0, 0, 0.18)";
      ctx.fillRect(0, yPx(baseY), facadeWidthPx, 1);
    }
  }

  // Solid patches right of the facade strips: roof slab/box colour on the top
  // half, darker parapet colour on the bottom half.
  const halfHeight = Math.floor(canvasHeightPx / 2);
  ctx.fillStyle = colors.roof;
  ctx.fillRect(facadeWidthPx, 0, patchWidthPx, halfHeight);
  ctx.fillStyle = colors.parapet;
  ctx.fillRect(facadeWidthPx, halfHeight, patchWidthPx, canvasHeightPx - halfHeight);
}

export function compilePrototype(def: AssetDefinitionTS, scene: Scene): CompiledPrototype {
  const recipe = def.recipe;
  const w = recipe.footprint_w;
  const h = recipe.footprint_h;
  const perimeterM = 2 * (w + h);
  const heightM = recipe.floors * FLOOR_HEIGHT_M;

  const ppm = Math.max(
    4,
    Math.min(BASE_PPM, MAX_CANVAS_EDGE_PX / perimeterM, MAX_CANVAS_EDGE_PX / heightM),
  );
  const px = (meters: number): number => Math.round(meters * ppm);
  const facadeWidthPx = px(perimeterM);
  const patchWidthPx = Math.max(4, px(1));
  const canvasWidthPx = facadeWidthPx + patchWidthPx;
  const canvasHeightPx = px(heightM);

  // Deterministic palette. The PRNG call order fixes the value stream — the
  // rooftop-box draws below depend on it, so do not reorder.
  const rng = mulberry32(recipe.seed >>> 0);
  const wall = jitterColor(parseHexColor(recipe.base_color), rng);
  const roof = jitterColor(parseHexColor(recipe.roof_color), rng);
  const colors: FacadeColors = {
    wall: cssHsl(wall),
    plinth: cssHsl({ h: wall.h, s: wall.s, l: clamp(wall.l - 9, 2, 98) }),
    window: cssHsl({ h: wall.h, s: 16, l: 14 }),
    frame: cssHsl({ h: wall.h, s: 14, l: 38 }),
    mullion: cssHsl({ h: wall.h, s: 10, l: 8 }),
    roof: cssHsl(roof),
    parapet: cssHsl({ h: roof.h, s: roof.s, l: clamp(roof.l - 12, 2, 98) }),
  };

  const texture = new DynamicTexture(
    `tex_${def.id}_r${def.revision}`,
    { width: canvasWidthPx, height: canvasHeightPx },
    scene,
    true,
  );
  texture.anisotropicFilteringLevel = 8;
  drawFacade(
    texture.getContext(),
    {
      facadeWidthPx,
      canvasHeightPx,
      patchWidthPx,
      ppm,
      perimeterM,
      floors: recipe.floors,
      windowStyle: recipe.window_style,
    },
    colors,
  );
  texture.update();

  const material = new StandardMaterial(`mat_${def.id}_r${def.revision}`, scene);
  material.diffuseTexture = texture;
  material.specularColor = new Color3(0.04, 0.04, 0.04);

  // Face UVs (Vector4 = u0, v0, u1, v1). Canvas y grows downward while texture
  // v grows upward: v = 1 - yPx / canvasHeightPx. The four side strips partition
  // the facade atlas in metre widths; top/bottom faces sample the roof patch.
  const uOf = (pxValue: number): number => pxValue / canvasWidthPx;
  const facadeRegion = new Vector4(uOf(facadeWidthPx), 0, 1, 0.5);
  const parapetRegion = new Vector4(uOf(facadeWidthPx), 0.5, 1, 1);
  const strip = (fromM: number, toM: number): Vector4 =>
    new Vector4(uOf(px(fromM)), 0, uOf(px(toM)), 1);

  const massFaceUV = [
    strip(0, w), // front
    strip(w + h, 2 * w + h), // back
    strip(w, w + h), // right
    strip(2 * w + h, perimeterM), // left
    facadeRegion, // top (covered by the roof slab)
    facadeRegion, // bottom (never visible)
  ];

  const box = (
    name: string,
    width: number,
    height: number,
    depth: number,
    x: number,
    y: number,
    z: number,
    faceUV: Vector4[],
  ): Mesh => {
    const part = MeshBuilder.CreateBox(name, { width, height, depth, faceUV }, scene);
    part.position.set(x, y, z);
    part.bakeCurrentTransformIntoVertices();
    return part;
  };

  const parts: Mesh[] = [];
  parts.push(box("mass", w, heightM, h, 0, heightM / 2, 0, massFaceUV));

  const slabWidth = w + 2 * SLAB_OVERHANG_M;
  const slabDepth = h + 2 * SLAB_OVERHANG_M;
  const roofFaceUV = [facadeRegion, facadeRegion, facadeRegion, facadeRegion, facadeRegion, facadeRegion];
  parts.push(
    box(
      "roof-slab",
      slabWidth,
      SLAB_THICKNESS_M,
      slabDepth,
      0,
      heightM + SLAB_THICKNESS_M / 2,
      0,
      roofFaceUV,
    ),
  );

  const parapetY = heightM + SLAB_THICKNESS_M + PARAPET_HEIGHT_M / 2;
  const parapetFaceUV = [parapetRegion, parapetRegion, parapetRegion, parapetRegion, parapetRegion, parapetRegion];
  const faceOffset = slabDepth / 2 - PARAPET_THICKNESS_M / 2;
  const sideOffset = slabWidth / 2 - PARAPET_THICKNESS_M / 2;
  const sideDepth = slabDepth - 2 * PARAPET_THICKNESS_M;
  parts.push(box("parapet-n", slabWidth, PARAPET_HEIGHT_M, PARAPET_THICKNESS_M, 0, parapetY, faceOffset, parapetFaceUV));
  parts.push(box("parapet-s", slabWidth, PARAPET_HEIGHT_M, PARAPET_THICKNESS_M, 0, parapetY, -faceOffset, parapetFaceUV));
  parts.push(box("parapet-e", PARAPET_THICKNESS_M, PARAPET_HEIGHT_M, sideDepth, sideOffset, parapetY, 0, parapetFaceUV));
  parts.push(box("parapet-w", PARAPET_THICKNESS_M, PARAPET_HEIGHT_M, sideDepth, -sideOffset, parapetY, 0, parapetFaceUV));

  // Seed-driven rooftop box (stair bulkhead / vent block) inside the parapet.
  // w,h ≥ 1 keeps the inner span ≥ 0.3 m, so the clamped size always fits.
  const innerWidth = w - 2 * ROOF_BOX_CLEARANCE_M;
  const innerDepth = h - 2 * ROOF_BOX_CLEARANCE_M;
  const boxWidth = Math.min(0.8 + rng() * 0.8, innerWidth);
  const boxDepth = Math.min(0.8 + rng() * 0.8, innerDepth);
  const boxHeight = 0.8 + rng() * 1.0;
  const freeX = Math.max(0, (innerWidth - boxWidth) / 2);
  const freeZ = Math.max(0, (innerDepth - boxDepth) / 2);
  parts.push(
    box(
      "roof-box",
      boxWidth,
      boxHeight,
      boxDepth,
      (rng() * 2 - 1) * freeX,
      heightM + SLAB_THICKNESS_M + boxHeight / 2,
      (rng() * 2 - 1) * freeZ,
      parapetFaceUV,
    ),
  );

  const merged = Mesh.MergeMeshes(parts, true, true, undefined, false, false);
  if (!merged) throw new Error(`compilePrototype: merge failed for definition ${def.id}`);
  merged.name = `prototype_${def.id}_r${def.revision}`;
  merged.material = material;
  return { mesh: merged, triangleCount: merged.getTotalIndices() / 3, materialCount: 1 };
}
