// Engine bootstrap: WebGPU when the browser exposes navigator.gpu and engine
// creation actually succeeds, otherwise WebGL2. The chosen backend is mirrored
// into the #backend-chip element so the running renderer is always visible.
// The canvas actually used is returned, because a failed WebGPU init can bind
// a "webgpu" context to the original element and block WebGL2 on it.

import { AbstractEngine, Engine, WebGPUEngine } from "@babylonjs/core";

export type BackendName = "webgpu" | "webgl2" | "webgl1";

export function backendChip(): HTMLElement {
  const el = document.getElementById("backend-chip");
  if (!el) throw new Error("#backend-chip missing from index.html");
  return el;
}

export function reflectBackend(backend: BackendName, status?: string): void {
  backendChip().textContent = status
    ? `backend: ${backend} · ${status}`
    : `backend: ${backend}`;
}

export interface CreatedEngine {
  engine: AbstractEngine;
  backend: BackendName;
  canvas: HTMLCanvasElement;
}

export async function createEngine(pageCanvas: HTMLCanvasElement): Promise<CreatedEngine> {
  let canvas = pageCanvas;
  if ("gpu" in navigator && navigator.gpu) {
    try {
      const engine = new WebGPUEngine(canvas, {
        antialias: true,
        adaptToDeviceRatio: true,
      });
      await engine.initAsync();
      reflectBackend("webgpu");
      return { engine, backend: "webgpu", canvas };
    } catch (err) {
      console.warn("WebGPU engine creation failed, falling back to WebGL2", err);
    }
    // A failed WebGPU init may have bound a "webgpu" context to the canvas,
    // which would block creating a WebGL2 context on the same element.
    // Continue on a fresh element and hand that one back to the caller.
    const fallback = canvas.cloneNode(false) as HTMLCanvasElement;
    canvas.replaceWith(fallback);
    canvas = fallback;
  }

  const engine = new Engine(canvas, true, {
    adaptToDeviceRatio: true,
    stencil: true,
  });
  const backend: BackendName = engine.webGLVersion >= 2 ? "webgl2" : "webgl1";
  reflectBackend(backend);
  return { engine, backend, canvas };
}
